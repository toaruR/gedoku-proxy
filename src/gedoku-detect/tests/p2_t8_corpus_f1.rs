//! P2-T8 完了ハーネス: 合成秘密コーパス300件+非秘密コーパス300件でのF1/誤検出率レポート。
//!
//! 各カテゴリの正例は、対応する validator (luhn/mod97/checksum/checkdigit 等) を
//! 実際に満たす値をここで生成時に総当たりで計算して埋め込む(validators.rs はどれも
//! pub なので本物のアルゴリズムをそのまま呼び出せる)。負例は「正規表現の形は似ているが
//! validator が弾く値」「既定 OFF ルール (private IP/home path) への言及」「無害な文章」
//! の3系統で構成する。

use gedoku_detect::{validators, DetectionEngine};

fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

fn rand_string(seed: u64, len: usize, charset: &[u8]) -> String {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        let v = lcg_next(&mut state);
        let idx = ((v >> 33) as usize) % charset.len();
        s.push(charset[idx] as char);
    }
    s
}

fn rand_digits(seed: u64, len: usize) -> String {
    rand_string(seed, len, b"0123456789")
}

const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const UPPER_ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const BASE64ISH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const JWT_SAFE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn base62_encode_padded(mut n: u32, width: usize) -> String {
    if n == 0 {
        return "0".repeat(width);
    }
    let mut chars = Vec::new();
    while n > 0 {
        chars.push(BASE62[(n % 62) as usize]);
        n /= 62;
    }
    while chars.len() < width {
        chars.push(b'0');
    }
    chars.reverse();
    String::from_utf8(chars).unwrap()
}

fn valid_card_number(seed: u64) -> String {
    let prefix = rand_digits(seed, 15);
    for d in 0..10u32 {
        let candidate = format!("{prefix}{d}");
        if validators::luhn_valid(&candidate) {
            return candidate;
        }
    }
    unreachable!("luhn check digit always exists")
}

fn valid_mynumber(seed: u64) -> String {
    let body = rand_digits(seed, 11);
    for d in 0..10u32 {
        let candidate = format!("{body}{d}");
        if validators::mynumber_checkdigit_plausible(&candidate) {
            return candidate;
        }
    }
    unreachable!("mynumber check digit always exists")
}

fn valid_iban(seed: u64) -> String {
    let bban = format!("WEST{}", rand_digits(seed, 14));
    for d in 0..100u32 {
        let candidate = format!("GB{d:02}{bban}");
        if validators::iban_mod97_valid(&candidate) {
            return candidate;
        }
    }
    unreachable!("iban check digits always exist within 00..99")
}

fn valid_github_pat(seed: u64) -> String {
    let body = rand_string(seed, 30, ALNUM);
    let signed = format!("ghp_{body}");
    let crc = crc32fast::hash(signed.as_bytes());
    format!("{signed}{}", base62_encode_padded(crc, 6))
}

const JWT_HEADER_HS256: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

fn valid_jwt(seed: u64) -> String {
    let payload = rand_string(seed, 20, JWT_SAFE);
    let sig = rand_string(seed.wrapping_add(1), 10, JWT_SAFE);
    format!("{JWT_HEADER_HS256}.{payload}.{sig}")
}

fn seed_for(cat_idx: u64, i: u64) -> u64 {
    cat_idx * 100_003 + i * 7919 + 1
}

/// 正例コーパス。各要素は (期待カテゴリ, テキスト)。
fn positive_corpus() -> Vec<(&'static str, String)> {
    use gedoku_detect::category::*;
    let mut out = Vec::new();
    const N: u64 = 17; // 18カテゴリ * 17 = 306件

    for i in 0..N {
        out.push((
            SECRET,
            format!(
                "here is the key: sk-ant-{} please keep it safe",
                rand_string(seed_for(0, i), 30, JWT_SAFE)
            ),
        ));
        out.push((
            SECRET,
            format!(
                "openai key rotated to sk-{} today",
                rand_string(seed_for(1, i), 28, ALNUM)
            ),
        ));
        out.push((
            SECRET,
            format!(
                "old access key id was AKIA{}, please deactivate",
                rand_string(seed_for(2, i), 16, UPPER_ALNUM)
            ),
        ));
        out.push((
            SECRET,
            format!(
                "aws_secret_access_key=\"{}\"",
                rand_string(seed_for(3, i), 40, BASE64ISH)
            ),
        ));
        out.push((
            SECRET,
            format!("token leaked: {}", valid_github_pat(seed_for(4, i))),
        ));
        out.push((
            SECRET,
            format!(
                "slack bot token xoxb-{}",
                rand_string(seed_for(5, i), 20, ALNUM)
            ),
        ));
        out.push((
            SECRET,
            format!(
                r#"service account json: {{"type": "service_account", "private_key": "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n"}}"#,
                rand_string(seed_for(6, i), 40, BASE64ISH)
            ),
        ));
        out.push((
            SECRET,
            format!(
                "-----BEGIN RSA PRIVATE KEY-----\n{}\n{}\n-----END RSA PRIVATE KEY-----",
                rand_string(seed_for(7, i), 44, BASE64ISH),
                rand_string(seed_for(7, i + 1000), 44, BASE64ISH)
            ),
        ));
        out.push((
            SECRET,
            format!("auth header was {}", valid_jwt(seed_for(8, i))),
        ));
        out.push((
            SECRET,
            format!("export API_KEY={}", rand_string(seed_for(9, i), 20, ALNUM)),
        ));
        out.push((
            SECRET,
            format!(
                "connect via postgres://admin:{}@db-{i}.internal:5432/app",
                rand_string(seed_for(10, i), 16, ALNUM)
            ),
        ));
        out.push((
            PII_EMAIL,
            format!("contact jane.doe{i}@realcorp-{i}.io for details"),
        ));
        out.push((
            PII_PHONE,
            format!(
                "call me at 090-{:04}-{:04}",
                (1000 + i * 37) % 10000,
                (2000 + i * 53) % 10000
            ),
        ));
        out.push((
            // 総桁数をわざと12桁からずらす (jp_mynumber の `\b\d{12}\b` と桁数が
            // 一致すると、稀にチェックディジットが偶然一致して overlap 解決で
            // 信頼度の高い jp_mynumber 側が採用されてしまうため)。
            PII_PHONE,
            format!("reach +81{:09}", (100_000_000 + i * 9377) % 1_000_000_000),
        ));
        out.push((
            PII_CARD,
            format!("card on file: {}", valid_card_number(seed_for(13, i))),
        ));
        out.push((
            PII_ID,
            format!("my number is {}", valid_mynumber(seed_for(14, i))),
        ));
        out.push((
            PII_ID,
            format!(
                "ssn on record: {:03}-{:02}-{:04}",
                100 + i * 3,
                10 + i,
                1000 + i * 11
            ),
        ));
        out.push((
            PII_ACCOUNT,
            format!("wire to iban {}", valid_iban(seed_for(17, i))),
        ));
    }

    out
}

/// 負例コーパス。いずれも `DetectionEngine::default()` では検出されてはならない。
fn negative_corpus() -> Vec<String> {
    let mut out = Vec::new();
    const N: u64 = 12;

    for i in 0..N {
        // 正規表現の形は似ているが validator が弾くべきもの。
        let mut bad_card = valid_card_number(seed_for(100, i));
        let last = bad_card.pop().unwrap();
        let flipped = std::char::from_digit((last.to_digit(10).unwrap() + 1) % 10, 10).unwrap();
        bad_card.push(flipped);
        out.push(format!("card typo maybe: {bad_card}"));

        out.push(format!(
            "example contact: person{i}@example.com (docs only)"
        ));

        out.push(format!(
            "ssn invalid area: 666-{:02}-{:04}",
            10 + i,
            1000 + i * 11
        ));
        out.push(format!(
            "ssn invalid area: 901-{:02}-{:04}",
            10 + i,
            2000 + i * 11
        ));

        let mut bad_mynumber = valid_mynumber(seed_for(101, i));
        let last = bad_mynumber.pop().unwrap();
        let flipped = std::char::from_digit((last.to_digit(10).unwrap() + 1) % 10, 10).unwrap();
        bad_mynumber.push(flipped);
        out.push(format!("reference id {bad_mynumber}"));

        let good_iban = valid_iban(seed_for(102, i));
        let country = &good_iban[0..2];
        let check: u32 = good_iban[2..4].parse().unwrap();
        let rest = &good_iban[4..];
        let bad_check = (check + 1) % 100;
        out.push(format!("iban typo: {country}{bad_check:02}{rest}"));

        let mut bad_pat = valid_github_pat(seed_for(103, i));
        bad_pat.replace_range(4..5, if &bad_pat[4..5] == "a" { "b" } else { "a" });
        out.push(format!("stale token {bad_pat}"));

        out.push(format!("not a real jwt: auth.token.value.number{i}"));

        out.push(format!("password=aaaaaaaa{i:02} (default, please change)"));

        out.push(format!("access key fragment AKIA{i:04} is incomplete"));

        // 既定 OFF ルール (opt-in しない限り検出されない)。
        out.push(format!("internal server at 10.0.{i}.5 responded slowly"));
        out.push(format!("logs are under /home/deploy{i}/app.log today"));
    }

    // 無害な文章 (数値・記号を含むが秘密情報の形をしていないもの)。
    let filler = [
        "the deployment finished in {i} minutes without incident",
        "please review PR #{i} before the release cut",
        "we measured {i} percent improvement in latency",
        "meeting moved to room {i}, same time as usual",
        "the cache hit rate stayed above {i}0 percent all week",
        "issue {i} was closed as a duplicate",
        "build number {i} passed all checks",
        "the quarterly report has {i} pages this time",
        "server restarted {i} times during the maintenance window",
        "the changelog now has {i} entries for this release",
        "we onboarded {i} new users this month",
        "the retry budget allows {i} attempts before giving up",
        "config version {i} rolled out to all regions",
        "the test suite now covers {i} more edge cases",
        "sprint {i} planning starts on monday",
    ];
    for (idx, tmpl) in filler.iter().enumerate() {
        for i in 0..14u64 {
            out.push(tmpl.replace("{i}", &(idx as u64 * 14 + i).to_string()));
        }
    }

    out
}

#[test]
fn p2_t8_corpus_f1_report() {
    let engine = DetectionEngine::default();
    let positives = positive_corpus();
    let negatives = negative_corpus();
    assert!(
        positives.len() >= 300,
        "positive corpus must have >=300 items, got {}",
        positives.len()
    );
    assert!(
        negatives.len() >= 300,
        "negative corpus must have >=300 items, got {}",
        negatives.len()
    );

    let mut true_positive = 0usize;
    let mut false_negative = 0usize;
    for (expected_category, text) in &positives {
        let hits = engine.scan(text);
        if hits.iter().any(|d| d.category == *expected_category) {
            true_positive += 1;
        } else {
            false_negative += 1;
            eprintln!("MISS[{expected_category}]: {text}");
        }
    }

    let mut false_positive_texts = 0usize;
    let mut false_positive_detections = 0usize;
    for text in &negatives {
        let hits = engine.scan(text);
        if !hits.is_empty() {
            false_positive_texts += 1;
            false_positive_detections += hits.len();
            eprintln!("FALSE POSITIVE: {text} -> {hits:?}");
        }
    }

    let recall = true_positive as f64 / positives.len() as f64;
    let false_positive_rate = false_positive_texts as f64 / negatives.len() as f64;
    let precision =
        true_positive as f64 / (true_positive + false_positive_detections).max(1) as f64;
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    println!(
        "P2-T8 corpus report: N_pos={} N_neg={} TP={} FN={} FP_texts={} FP_detections={} \
         recall={recall:.4} precision={precision:.4} f1={f1:.4} fp_rate={false_positive_rate:.4}",
        positives.len(),
        negatives.len(),
        true_positive,
        false_negative,
        false_positive_texts,
        false_positive_detections,
    );

    assert!(recall >= 0.98, "recall too low: {recall:.4}");
    assert!(
        false_positive_rate <= 0.02,
        "false positive rate too high: {false_positive_rate:.4}"
    );
    assert!(f1 >= 0.98, "f1 too low: {f1:.4}");
}
