//! L1 パターンの検証関数群 (設計 §7.4)。
//!
//! いずれも「マッチした部分文字列 (置換対象の capture group)」を受け取り
//! true/false を返す純関数。誤検出率を下げるための追加フィルタであり、
//! ここで false を返しても他ルールとの重複や L2 での再検出は妨げない。

use std::sync::LazyLock;

const BASE62_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Luhn アルゴリズム (クレジットカード番号)。
pub fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut d = d;
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum.is_multiple_of(10)
}

/// Shannon エントロピー (bit/byte)。
pub fn shannon_entropy(s: &str) -> f64 {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let len = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

fn base62_encode_padded(mut n: u32, width: usize) -> String {
    if n == 0 {
        return "0".repeat(width);
    }
    let mut chars = Vec::new();
    while n > 0 {
        chars.push(BASE62_ALPHABET[(n % 62) as usize]);
        n /= 62;
    }
    while chars.len() < width {
        chars.push(b'0');
    }
    chars.reverse();
    String::from_utf8(chars).expect("base62 alphabet is ASCII")
}

/// GitHub PAT のチェックサム (`gh[pousr]_` + 30文字payload + 6文字CRC32チェックサム、base62)。
/// classic 形式 (payload長が36でない = チェックサムを持たない) は検証不能として true を返す
/// (偽陰性を避ける)。
pub fn github_pat_checksum_valid(full_match: &str) -> bool {
    let Some(us_idx) = full_match.find('_') else {
        return true;
    };
    let payload = &full_match[us_idx + 1..];
    if payload.len() != 36 {
        return true;
    }
    let (_body, checksum) = payload.split_at(30);
    let signed_part = &full_match[..us_idx + 1 + 30];
    let crc = crc32fast::hash(signed_part.as_bytes());
    base62_encode_padded(crc, 6) == checksum
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut lut = [255u8; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .iter()
        .enumerate()
    {
        lut[*c as usize] = i as u8;
    }
    let mut bits: u32 = 0;
    let mut nbits = 0u32;
    let mut out = Vec::new();
    for b in s.bytes() {
        let v = lut[b as usize];
        if v == 255 {
            return None;
        }
        bits = (bits << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Some(out)
}

/// JWT: header セグメントが有効な JSON オブジェクトへデコードできるか。
pub fn jwt_header_valid(full_match: &str) -> bool {
    let Some(header_b64) = full_match.split('.').next() else {
        return false;
    };
    let Some(bytes) = base64url_decode(header_b64) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map(|v| v.is_object())
        .unwrap_or(false)
}

static RUST_TYPE_ANNOTATION_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^(?:Option|Vec|Box|Result|HashMap|HashSet|BTreeMap|BTreeSet|Arc|Rc|RefCell|Cell)<[^,]*>$|^&?(?:String|str|bool|char|u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64)$",
    )
    .expect("valid regex")
});

/// `struct Foo { field: Option<String>, }` のようなRust型注釈らしい値か
/// (`bearer_ctx` がフィールド名中の "token"/"key" にマッチし、直後の型を
/// 値として誤検出するのを防ぐ)。
pub fn looks_like_rust_type_annotation(s: &str) -> bool {
    RUST_TYPE_ANNOTATION_RE.is_match(s)
}

/// bearer_ctx: 前後の文脈語で拾った値のエントロピーが十分高く、かつ
/// Rust の型注釈 (`Option<String>` 等) ではないか。
pub fn high_entropy_value(s: &str) -> bool {
    shannon_entropy(s) >= 3.0 && !looks_like_rust_type_annotation(s)
}

/// aws_secret: 40文字のキャプチャ値がランダムらしい (エントロピー) か。
pub fn aws_secret_entropy(s: &str) -> bool {
    shannon_entropy(s) >= 3.5
}

const BUILTIN_NON_SECRET_DOMAINS: &[&str] = &[
    "example.com",
    "example.org",
    "example.net",
    "test.com",
    "localhost",
];

/// email: ドキュメント例・テスト用の定型ドメインを既定で除外する。
pub fn email_domain_not_example(email: &str) -> bool {
    match email.rsplit_once('@') {
        Some((_, domain)) => !BUILTIN_NON_SECRET_DOMAINS
            .iter()
            .any(|d| domain.eq_ignore_ascii_case(d)),
        None => true,
    }
}

/// 日本の電話番号: ハイフン区切りを除いた桁数が現実的な範囲か。
pub fn phone_jp_digit_count(s: &str) -> bool {
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    (9..=11).contains(&digits)
}

/// E.164: 総桁数が仕様範囲内 (国番号含め最大15桁)。
pub fn phone_e164_digit_count(s: &str) -> bool {
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    (8..=15).contains(&digits)
}

/// 米国 SSN の無効レンジ除外 (area=000/666/900-999, group=00, serial=0000 は無効)。
pub fn ssn_valid_range(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let (Ok(area), Ok(group), Ok(serial)) = (
        parts[0].parse::<u32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) else {
        return false;
    };
    area != 0 && area != 666 && area < 900 && group != 0 && serial != 0
}

/// IBAN mod-97 チェック (ISO 7064)。
pub fn iban_mod97_valid(iban: &str) -> bool {
    let iban: String = iban.chars().filter(|c| !c.is_whitespace()).collect();
    if iban.len() < 15 || iban.len() > 34 {
        return false;
    }
    let chars: Vec<char> = iban.chars().collect();
    if chars.len() < 4 {
        return false;
    }
    let rearranged: String = chars[4..].iter().chain(chars[0..4].iter()).collect();
    let mut remainder: u64 = 0;
    for c in rearranged.chars() {
        if let Some(d) = c.to_digit(10) {
            remainder = (remainder * 10 + d as u64) % 97;
        } else if c.is_ascii_alphabetic() {
            let v = c.to_ascii_uppercase() as u64 - 'A' as u64 + 10;
            for digit in v.to_string().chars() {
                let d = digit.to_digit(10).unwrap();
                remainder = (remainder * 10 + d as u64) % 97;
            }
        } else {
            return false;
        }
    }
    remainder == 1
}

/// マイナンバー簡易チェックディジット。
///
/// 一般に公開されているアルゴリズム記述に基づく実装であり、政府公式リファレンス
/// 実装との厳密な一致は未検証。あくまで「ランダムな12桁数字」を除外するための
/// 追加フィルタとして扱う (§7.4 「検査用数字」の実装上の簡略化)。
pub fn mynumber_checkdigit_plausible(s: &str) -> bool {
    if s.len() != 12 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let digits: Vec<u32> = s.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let body = &digits[0..11];
    let mut sum = 0u32;
    for (idx, &d) in body.iter().enumerate() {
        let position_from_right = (11 - idx) as u32; // 11..1
        let weight = if position_from_right <= 6 {
            position_from_right + 1
        } else {
            position_from_right - 5
        };
        sum += d * weight;
    }
    let r = sum % 11;
    let check = if r <= 1 { 0 } else { 11 - r };
    check == digits[11]
}

static UUID_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .expect("valid regex")
});

/// L2 用ノイズ除外: git SHA (40/64桁 hex) や UUID は既定で除外する (§7.5)。
pub fn looks_like_known_noise(token: &str) -> bool {
    if UUID_RE.is_match(token) {
        return true;
    }
    let is_hex = !token.is_empty() && token.bytes().all(|b| b.is_ascii_hexdigit());
    is_hex && (token.len() == 40 || token.len() == 64)
}

static CONTEXT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(key|token|secret|password|passwd|bearer|auth)\s*[:=]\s*['"]?$"#)
        .expect("valid regex")
});

/// L2: 候補直前 `window` バイトに代入文脈語 (KEY= / "token": / Bearer など) があるか。
pub fn in_assignment_context(text: &str, start: usize, window: usize) -> bool {
    let mut ws = start.saturating_sub(window);
    while ws < start && !text.is_char_boundary(ws) {
        ws += 1;
    }
    CONTEXT_RE.is_match(&text[ws..start])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn_accepts_known_valid_number_rejects_flipped_digit() {
        assert!(luhn_valid("4111111111111111"));
        assert!(!luhn_valid("4111111111111112"));
    }

    #[test]
    fn shannon_entropy_low_for_repeated_char_high_for_random_look() {
        assert!(shannon_entropy("aaaaaaaaaaaa") < 1.0);
        assert!(shannon_entropy("aZ3$kQ9!mP2@wX7^") > 3.0);
    }

    #[test]
    fn github_pat_checksum_round_trips() {
        let payload_prefix = "ghp_";
        let body: String = "a".repeat(30);
        let signed = format!("{payload_prefix}{body}");
        let crc = crc32fast::hash(signed.as_bytes());
        let checksum = base62_encode_padded(crc, 6);
        let full = format!("{signed}{checksum}");
        assert!(github_pat_checksum_valid(&full));
        let mut tampered = full.clone();
        tampered.replace_range(4..5, if &tampered[4..5] == "a" { "b" } else { "a" });
        assert!(!github_pat_checksum_valid(&tampered));
    }

    #[test]
    fn classic_length_github_token_is_not_rejected() {
        let classic = format!("ghp_{}", "x".repeat(36 + 4)); // != 36 payload len
        assert!(github_pat_checksum_valid(&classic));
    }

    #[test]
    fn jwt_header_valid_detects_real_header_and_rejects_garbage() {
        let header = base64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let token = format!("{header}.payload.sig");
        assert!(jwt_header_valid(&token));
        assert!(!jwt_header_valid("not-a-jwt.payload.sig"));
    }

    fn base64url_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            let chars = [
                ALPHABET[((n >> 18) & 0x3f) as usize],
                ALPHABET[((n >> 12) & 0x3f) as usize],
                ALPHABET[((n >> 6) & 0x3f) as usize],
                ALPHABET[(n & 0x3f) as usize],
            ];
            let take = match chunk.len() {
                1 => 2,
                2 => 3,
                _ => 4,
            };
            out.push_str(std::str::from_utf8(&chars[..take]).unwrap());
        }
        out
    }

    #[test]
    fn email_domain_excludes_documentation_examples() {
        assert!(!email_domain_not_example("a@example.com"));
        assert!(email_domain_not_example("a@realcompany.io"));
    }

    #[test]
    fn ssn_valid_range_rejects_known_invalid_areas() {
        assert!(ssn_valid_range("123-45-6789"));
        assert!(!ssn_valid_range("000-45-6789"));
        assert!(!ssn_valid_range("666-45-6789"));
        assert!(!ssn_valid_range("901-45-6789"));
    }

    #[test]
    fn iban_mod97_validates_and_rejects_tampered() {
        // GB82 WEST 1234 5698 7654 32 は ISO 7064 の一般的な例。
        assert!(iban_mod97_valid("GB82WEST12345698765432"));
        assert!(!iban_mod97_valid("GB82WEST12345698765433"));
    }

    #[test]
    fn mynumber_checkdigit_is_self_consistent() {
        let body = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1];
        let mut sum = 0u32;
        for (idx, &d) in body.iter().enumerate() {
            let position_from_right = (11 - idx) as u32;
            let weight = if position_from_right <= 6 {
                position_from_right + 1
            } else {
                position_from_right - 5
            };
            sum += d * weight;
        }
        let r = sum % 11;
        let check = if r <= 1 { 0 } else { 11 - r };
        let full: String = body
            .iter()
            .chain(std::iter::once(&check))
            .map(|d| std::char::from_digit(*d, 10).unwrap())
            .collect();
        assert!(mynumber_checkdigit_plausible(&full));
    }

    #[test]
    fn looks_like_known_noise_excludes_uuid_and_git_sha() {
        assert!(looks_like_known_noise(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(looks_like_known_noise(&"a".repeat(40)));
        assert!(!looks_like_known_noise("ZZthisIsNotHexOrUuidZZ12345"));
    }

    #[test]
    fn high_entropy_value_rejects_rust_type_annotations() {
        assert!(!high_entropy_value("Option<String>"));
        assert!(!high_entropy_value("Vec<String>"));
        assert!(!high_entropy_value("HashMap<String>"));
        assert!(high_entropy_value("aZ3kQ9mP2wX7bN4h"));
    }

    #[test]
    fn assignment_context_detects_trailing_key_equals() {
        let text = "export API_KEY=abcd1234efgh5678";
        let candidate_start = text.find("abcd1234efgh5678").unwrap();
        assert!(in_assignment_context(text, candidate_start, 40));
        let text2 = "just some prose abcd1234efgh5678 nothing special";
        let candidate_start2 = text2.find("abcd1234efgh5678").unwrap();
        assert!(!in_assignment_context(text2, candidate_start2, 40));
    }
}
