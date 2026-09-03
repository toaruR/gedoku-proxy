//! tail-hold スキャナ (設計 §12, 計画 P3-T3/T4)。
//!
//! SSE チャンク境界をまたいで分割されたプレースホルダー `⟦GDK:CATEGORY:ID⟧` を
//! 検出し、`resolve` コールバックで原文へ復元する。バイト列は状態機械で1回だけ
//! 走査し、確定できない末尾は次の `feed` 呼び出しまで保留する (最大
//! `MAX_HOLD_BYTES`、それ以上は生バイト列として確定出力する)。
//!
//! `regex-automata` の低レベル DFA API は使わず、固定文法専用の手書き状態機械で
//! 実装した (計画 RISK-1 のコンティンジェンシー: 固定文法1種類のみなので状態機械の
//! 方が単純かつ検証しやすい)。

use crate::sentinel::{
    is_category_char, is_category_start, is_id_char, CLOSE, ID_LEN, MAX_CATEGORY_LEN,
    MAX_HOLD_BYTES, OPEN, PREFIX,
};

#[derive(Default)]
pub struct TailHoldScanner {
    hold: Vec<u8>,
}

enum ParseResult {
    /// span 先頭からの確定長。`id_start..id_end` が ID 部分の範囲。
    Complete {
        len: usize,
        id_start: usize,
        id_end: usize,
    },
    /// span 先頭から `len` バイトまでで不一致が確定した (プレースホルダーではない)。
    Invalid { len: usize },
    /// 現時点のバイト列だけでは判定できない (続きを待つ)。
    Incomplete,
}

enum MatchOutcome {
    Complete,
    Incomplete,
    Mismatch(usize),
}

impl TailHoldScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_pending(&self) -> bool {
        !self.hold.is_empty()
    }

    /// 新しいチャンクを取り込み、確定した出力バイト列を返す。
    /// `resolve(id) -> Option<original>` が `None` を返す (未知/失効 ID) 場合は
    /// フェイルオープンでスキャンした生バイト列をそのまま出力する。
    pub fn feed(&mut self, chunk: &[u8], resolve: &dyn Fn(&str) -> Option<String>) -> Vec<u8> {
        let mut buf = std::mem::take(&mut self.hold);
        buf.extend_from_slice(chunk);

        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0usize;

        while i < buf.len() {
            let Some(rel_pos) = find_open(&buf[i..]) else {
                let keep_from = i + partial_open_suffix_start(&buf[i..]);
                out.extend_from_slice(&buf[i..keep_from]);
                self.hold = buf[keep_from..].to_vec();
                return out;
            };
            let pos = i + rel_pos;
            out.extend_from_slice(&buf[i..pos]);

            match try_parse_placeholder(&buf[pos..]) {
                ParseResult::Complete {
                    len,
                    id_start,
                    id_end,
                } => {
                    let id = std::str::from_utf8(&buf[pos + id_start..pos + id_end]).unwrap_or("");
                    match resolve(id) {
                        Some(original) => out.extend_from_slice(original.as_bytes()),
                        None => out.extend_from_slice(&buf[pos..pos + len]),
                    }
                    i = pos + len;
                }
                ParseResult::Invalid { len } => {
                    out.extend_from_slice(&buf[pos..pos + len]);
                    i = pos + len;
                }
                ParseResult::Incomplete => {
                    let held = &buf[pos..];
                    if held.len() > MAX_HOLD_BYTES {
                        // 文法上ここまで来て Incomplete のまま上限を超えることはない
                        // (安全弁): プレースホルダーとして成立しえないので生のまま出す。
                        out.extend_from_slice(held);
                        i = buf.len();
                    } else {
                        self.hold = held.to_vec();
                        return out;
                    }
                }
            }
        }

        self.hold.clear();
        out
    }

    /// 保留中のバイト列を無条件で生のまま出力し状態をリセットする。
    /// ストリーム終端、および 50ms タイムアウト時 (呼び出し側の責務) の両方で使う。
    pub fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.hold)
    }
}

fn find_open(data: &[u8]) -> Option<usize> {
    if data.len() < OPEN.len() {
        return None;
    }
    data.windows(OPEN.len()).position(|w| w == OPEN)
}

/// `data` の末尾が OPEN マーカーの先頭部分と一致する最長の位置を返す
/// (そこから先を次チャンクへ持ち越す)。一致がなければ `data.len()`。
fn partial_open_suffix_start(data: &[u8]) -> usize {
    let max_check = (OPEN.len() - 1).min(data.len());
    for k in (1..=max_check).rev() {
        let suffix = &data[data.len() - k..];
        if OPEN.starts_with(suffix) {
            return data.len() - k;
        }
    }
    data.len()
}

fn try_parse_placeholder(span: &[u8]) -> ParseResult {
    // span[0..OPEN.len()] == OPEN は呼び出し側 (find_open) が保証済み。
    let mut off = OPEN.len();

    match match_literal(span, off, PREFIX) {
        MatchOutcome::Complete => off += PREFIX.len(),
        MatchOutcome::Incomplete => return ParseResult::Incomplete,
        MatchOutcome::Mismatch(at) => return ParseResult::Invalid { len: at },
    }

    let cat_start = off;
    if off >= span.len() {
        return ParseResult::Incomplete;
    }
    if !is_category_start(span[off]) {
        return ParseResult::Invalid { len: off + 1 };
    }
    off += 1;
    loop {
        if off - cat_start >= MAX_CATEGORY_LEN {
            return ParseResult::Invalid { len: off };
        }
        if off >= span.len() {
            return ParseResult::Incomplete;
        }
        if span[off] == b':' {
            break;
        }
        if !is_category_char(span[off]) {
            return ParseResult::Invalid { len: off + 1 };
        }
        off += 1;
    }
    off += 1; // ':' を消費

    let id_start = off;
    let id_end = id_start + ID_LEN;
    if span.len() < id_end {
        return match span[id_start..].iter().position(|&b| !is_id_char(b)) {
            Some(bad) => ParseResult::Invalid {
                len: id_start + bad + 1,
            },
            None => ParseResult::Incomplete,
        };
    }
    if let Some(bad) = span[id_start..id_end].iter().position(|&b| !is_id_char(b)) {
        return ParseResult::Invalid {
            len: id_start + bad + 1,
        };
    }
    off = id_end;

    match match_literal(span, off, CLOSE) {
        MatchOutcome::Complete => ParseResult::Complete {
            len: off + CLOSE.len(),
            id_start,
            id_end,
        },
        MatchOutcome::Incomplete => ParseResult::Incomplete,
        MatchOutcome::Mismatch(at) => ParseResult::Invalid { len: at },
    }
}

fn match_literal(span: &[u8], off: usize, literal: &[u8]) -> MatchOutcome {
    for (k, &want) in literal.iter().enumerate() {
        let idx = off + k;
        if idx >= span.len() {
            return MatchOutcome::Incomplete;
        }
        if span[idx] != want {
            return MatchOutcome::Mismatch(idx + 1);
        }
    }
    MatchOutcome::Complete
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver_map(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |id: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| v.to_string())
        }
    }

    fn placeholder(id: &str) -> String {
        format!("\u{27E6}GDK:SECRET:{id}\u{27E7}")
    }

    #[test]
    fn resolves_placeholder_fed_in_one_chunk() {
        let id = "5QK2ZP31XY";
        let resolve = resolver_map(&[("5QK2ZP31XY", "sk-ant-original")]);
        let mut scanner = TailHoldScanner::new();
        let input = format!("before {} after", placeholder(id));
        let out = scanner.feed(input.as_bytes(), &resolve);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "before sk-ant-original after"
        );
    }

    #[test]
    fn unresolvable_id_passes_through_unchanged() {
        let resolve = resolver_map(&[]);
        let mut scanner = TailHoldScanner::new();
        let input = placeholder("5QK2ZP31XY");
        let out = scanner.feed(input.as_bytes(), &resolve);
        assert_eq!(String::from_utf8(out).unwrap(), input);
    }

    #[test]
    fn plain_text_without_sentinel_is_untouched() {
        let resolve = resolver_map(&[]);
        let mut scanner = TailHoldScanner::new();
        let out = scanner.feed("hello, world! no sentinel here.".as_bytes(), &resolve);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "hello, world! no sentinel here."
        );
    }

    #[test]
    fn resolves_when_split_at_every_byte_boundary() {
        let id = "5QK2ZP31XY";
        let resolve = resolver_map(&[("5QK2ZP31XY", "sk-ant-original")]);
        let input = format!("pre-{}-post", placeholder(id));
        let bytes = input.as_bytes();

        for split in 0..=bytes.len() {
            let mut scanner = TailHoldScanner::new();
            let mut out = scanner.feed(&bytes[..split], &resolve);
            out.extend(scanner.feed(&bytes[split..], &resolve));
            out.extend(scanner.flush());
            assert_eq!(
                String::from_utf8(out).unwrap(),
                "pre-sk-ant-original-post",
                "failed at split={split}"
            );
        }
    }

    #[test]
    fn multiple_placeholders_in_one_chunk_all_resolve() {
        let resolve = resolver_map(&[("AAAAAAAAAA", "one"), ("BBBBBBBBBB", "two")]);
        let mut scanner = TailHoldScanner::new();
        let input = format!(
            "{} and {}",
            placeholder("AAAAAAAAAA"),
            placeholder("BBBBBBBBBB")
        );
        let out = scanner.feed(input.as_bytes(), &resolve);
        assert_eq!(String::from_utf8(out).unwrap(), "one and two");
    }

    #[test]
    fn invalid_prefix_after_open_marker_is_passed_through() {
        let resolve = resolver_map(&[]);
        let mut scanner = TailHoldScanner::new();
        // OPEN の直後が "GDK:" ではないので即座に不一致確定する。
        let input = "\u{27E6}not-a-placeholder\u{27E7}";
        let out = scanner.feed(input.as_bytes(), &resolve);
        assert_eq!(String::from_utf8(out).unwrap(), input);
    }

    #[test]
    fn flush_emits_stale_incomplete_hold_as_literal() {
        let resolve = resolver_map(&[("5QK2ZP31XY", "sk-ant-original")]);
        let mut scanner = TailHoldScanner::new();
        // OPEN + "GDK:SECRET:" までで打ち切り、ID/CLOSE が来ないままにする。
        let partial = "\u{27E6}GDK:SECRET:";
        let out = scanner.feed(partial.as_bytes(), &resolve);
        assert!(out.is_empty(), "should be held pending");
        let flushed = scanner.flush();
        assert_eq!(String::from_utf8(flushed).unwrap(), partial);
    }

    #[test]
    fn category_exceeding_max_length_is_invalid_not_stuck_incomplete() {
        let resolve = resolver_map(&[]);
        let mut scanner = TailHoldScanner::new();
        let long_category = "A".repeat(30);
        let input = format!("\u{27E6}GDK:{long_category}:5QK2ZP31XY\u{27E7}");
        let out = scanner.feed(input.as_bytes(), &resolve);
        assert_eq!(String::from_utf8(out).unwrap(), input);
    }
}
