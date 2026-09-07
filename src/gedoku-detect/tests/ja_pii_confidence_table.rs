//! T009: 設計§7.10.7の確信度較正表とコード定数の突き合わせ、および
//! §7.10.9で懸念されたjp_mynumber(12桁数字)との桁数衝突回帰テスト。

use gedoku_detect::category;
use gedoku_detect::engine::DetectionEngine;
use gedoku_detect::ja_dict::{address, load_address_dict, load_surname_dict, name, NameDict};

/// 設計§7.10.7の表 (rule_id, confidence) を8行そのまま列挙する。
/// コード側の定数が変わった場合にこのテストが検知できるよう、
/// 定数は再エクスポートせず `ja_dict::address::scan`/`name::scan` の
/// 実出力から confidence を読み取って突き合わせる (定数を直接importして
/// 定数同士を比較する自己参照テストにしない)。
#[test]
fn ja_pii_confidence_table_matches_doc() {
    let addr_dict = load_address_dict(None).expect("builtin address dict must load");

    let addr_pref = address::scan("北海道が広いです", &addr_dict);
    let pref_hit = addr_pref
        .iter()
        .find(|d| d.rule_id == "ja_addr_pref")
        .expect("ja_addr_pref must fire on pref-only text");
    assert_eq!(pref_hit.confidence, 0.50);

    let addr_city = address::scan("去年は横浜市で花火大会がありました", &addr_dict);
    let city_hit = addr_city
        .iter()
        .find(|d| d.rule_id == "ja_addr_city")
        .expect("ja_addr_city must fire on city-only text");
    assert_eq!(city_hit.confidence, 0.55);

    let addr_full = address::scan("東京都港区芝公園4丁目2番8号", &addr_dict);
    let full_hit = addr_full
        .iter()
        .find(|d| d.rule_id == "ja_addr_full")
        .expect("ja_addr_full must fire on full address text");
    assert_eq!(full_hit.confidence, 0.90);

    // pref_only (市区町村なし) は builtin 辞書に県名のみの短い住所を作れないため、
    // 都道府県名の直後に丁目/番地/号が続く合成文字列で確認する。
    let addr_pref_only = address::scan("北海道1丁目1番1号", &addr_dict);
    let pref_only_hit = addr_pref_only
        .iter()
        .find(|d| d.rule_id == "ja_addr_full_pref_only")
        .expect("ja_addr_full_pref_only must fire when city is absent");
    assert_eq!(pref_only_hit.confidence, 0.85);

    let name_surname_dict = test_dict("森", false);
    let name_surname = name::scan("森の中を歩いた", &name_surname_dict);
    let surname_hit = name_surname
        .iter()
        .find(|d| d.rule_id == "ja_name_surname")
        .expect("ja_name_surname must fire on bare non-ambiguous surname");
    assert_eq!(surname_hit.confidence, 0.65);

    let name_full_dict = test_dict("山田", false);
    let name_full = name::scan("山田太郎について話した", &name_full_dict);
    let full_hit = name_full
        .iter()
        .find(|d| d.rule_id == "ja_name_full")
        .expect("ja_name_full must fire on surname+given name");
    assert_eq!(full_hit.confidence, 0.90);

    let name_honorific_dict = test_dict("森", true);
    let name_honorific = name::scan("森様にご確認ください", &name_honorific_dict);
    let honorific_hit = name_honorific
        .iter()
        .find(|d| d.rule_id == "ja_name_honorific")
        .expect("ja_name_honorific must fire even for ambiguous surname + honorific");
    assert_eq!(honorific_hit.confidence, 0.92);

    let name_full_honorific_dict = test_dict("山田", false);
    let name_full_honorific = name::scan("山田太郎様にご連絡ください", &name_full_honorific_dict);
    let full_honorific_hit = name_full_honorific
        .iter()
        .find(|d| d.rule_id == "ja_name_full_honorific")
        .expect("ja_name_full_honorific must fire on surname+given name+honorific");
    assert_eq!(full_honorific_hit.confidence, 0.97);
}

/// §7.10.9: 住所拡張(丁目/番地/号を必須とする漢字混じり表記)は純粋な12桁数字を
/// 要求する`jp_mynumber`(`\b\d{12}\b`)とは字面上一致せず衝突しないことを確認する。
#[test]
fn addr_full_does_not_collide_with_jp_mynumber_regex() {
    let mut engine = DetectionEngine::default();
    engine.ja_pii.enabled = true;
    engine.ja_pii.address.enabled = true;

    let text = "港区123456789012丁目1番1号";
    let hits = engine.scan(text);

    // jp_mynumber (L1, PII_ID) は12桁ちょうどの数字連続にのみマッチするが、
    // 住所拡張正規表現は数字が最大4桁までしか連続しない設計のため、
    // 同一スパンで両ルールが同時にヒットすることはない。
    let mynumber_hits: Vec<_> = hits.iter().filter(|d| d.rule_id == "jp_mynumber").collect();
    let addr_hits: Vec<_> = hits
        .iter()
        .filter(|d| d.category == category::PII_ADDRESS_JA)
        .collect();
    for m in &mynumber_hits {
        for a in &addr_hits {
            assert!(
                m.end <= a.start || a.end <= m.start,
                "jp_mynumber and ja_addr_* spans overlap: {m:?} vs {a:?}"
            );
        }
    }
}

fn test_dict(surname: &str, ambiguous: bool) -> NameDict {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gedoku-detect-confidence-table-test-{}-{}",
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("surname.tsv");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "surname\tambiguous\tfreq_rank").unwrap();
    writeln!(f, "{surname}\t{}\t1", if ambiguous { 1 } else { 0 }).unwrap();
    load_surname_dict(Some(&path)).expect("test surname tsv must load")
}
