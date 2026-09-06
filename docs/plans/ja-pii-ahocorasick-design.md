# gedoku-proxy 追加設計 — 日本語氏名・住所の Aho-Corasick 辞書検出 (§7.10 相当)

> 本書は `docs/gedoku-proxy-design.md` §7 (検出・置換アルゴリズム設計) への追加提案書。
> §7.1 で既に「L3 NER (任意・既定 OFF)」が氏名/住所/組織向けに予約されているが、
> Transformer NER はレイテンシ (10〜100ms/KB, §14 予算) とバイナリ/依存の重さが
> ネックで既定 OFF のまま塩漬けになっている。本書は同じ L3 スロットに、
> **Aho-Corasick 多パターン辞書照合** (µs/KB 級、既に `aho-corasick` は workspace
> 依存として導入済み・`gedoku-audit::guard` で先例あり) を使う軽量な代替案 **L3'**
> を提案する。対象は「氏名 (氏)」と「住所 (都道府県〜番地)」の2カテゴリに限定する。
>
> 末尾の付録 A は、本設計自体の健全性 (失敗モード網羅・既定値確定・自己適用等) を
> チェックした記録であり、`docs/gedoku-proxy-design.md` に本採用する際は本文
> (§7.10.0〜§7.10.13) のみをマージし、付録 A はレビュー記録として本ファイルに残す
> 想定。

---

## 7.10.0 位置づけと動機

| 方式 | 精度 (氏名/住所) | レイテンシ | 依存 | 既定 |
|---|---|---|---|---|
| Regex 列挙 (既存 L1 の延長) | 不可能 (開集合) | — | なし | 不採用 (付録 A.11) |
| NER (Transformer, 既存 §7.1 L3) | 高 (未知語にも強い) | 10〜100ms/KB, CPU依存 | ONNX/candle 等の新規重量依存 | 既定 OFF (据え置き) |
| **Aho-Corasick 辞書照合 (本書, L3')** | 中〜高 (辞書に無い語は取れない) | µs/KB 級 | `aho-corasick` (導入済み) | **既定 OFF、フラグ2枚 (住所/氏名) を個別に持つ** |

**採用理由**: gedoku-proxy の主戦場はシークレット (regex+検証関数で足りる) だが、
日本国内利用では住所・氏名の混入リスクも高い。NER を待たずに「辞書に載っている
既知の地名・姓」だけでも高速に拾えれば実用上の価値が大きい。ただし辞書は開集合の
一部しかカバーできないため、**NER の代替ではなく、NER 導入までの実用的な中間解**、
かつ NER 導入後も併用可能な補助レイヤと位置づける。

---

## 7.10.1 カテゴリ

`src/gedoku-detect/src/category.rs` に追加:

```rust
pub const PII_ADDRESS_JA: &str = "PII_ADDRESS_JA";
pub const PII_NAME_JA: &str = "PII_NAME_JA";
```

既存カテゴリ (`PII_EMAIL` 等) と同じ `&'static str` の閉じてない集合に素直に足すだけ。
将来 L3 NER を有効化する場合も、氏名/住所には同じ2定数を使うことを要件化する
(付録 A.1 の失敗モード#9)。

---

## 7.10.2 辞書のソースとライセンス

| 辞書 | 内容 | ソース | ライセンス | 既定同梱 |
|---|---|---|---|---|
| 住所 (`pref_city.tsv`) | 47都道府県 + 市区町村・特別区・政令市の区 (計 約1,900件) | 総務省「全国地方公共団体コード」公表データのスナップショット | 政府標準利用規約2.0 (CC BY 相当、出典表示で再配布可) | **同梱する** (`include_str!`) |
| 住所拡張 (町丁目) | 大字・町丁目レベル (数十万件) | 運用者が用意 (例: 日本郵便 `KEN_ALL.CSV` 由来) | 運用者の入手元次第 | 同梱しない。`extra_town_dict_path` で任意追加 |
| 姓 (`surname.tsv`) | 姓 + 曖昧フラグ + 頻度順位 | **未定 (意図的)**。公的機関が無償公開する全国姓データは存在せず、民間データセットは商用可否が個別確認事項 | 運用者が自分の権利で用意した辞書のみ許可 | **同梱しない (空)**。`surname_dict_path` 未設定なら氏名検出は常に no-op |

**意思決定**: 住所 (行政区画名) は政府の公開データで再配布リスクが低いので同梱する。
姓は「同梱してしまうと出典未確認データを配布することになる」リスクの方が
「デフォルトで守れる範囲が狭い」デメリットより重いと判断し、**v1 では姓辞書を
一切同梱しない**。これは「氏名検出は既定で機能しない」ことを意味するが、
付録 A.1 の失敗モード表で明示し、隠さない。

---

## 7.10.3 モジュール構成

```
src/gedoku-detect/
  assets/ja/
    pref_city.tsv        # 同梱辞書 (§7.10.2)。1行目ヘッダー "name\tkind"
    README.md            # スナップショット日付・出典・更新手順
  src/ja_dict/
    mod.rs                # 公開API: JaPiiConfig, ja_pii_scan()
    dict.rs                # AhoCorasick 構築・TSV ローダー・JaDictError
    address.rs             # 住所アンカー検出 + 丁目/番地/号 拡張
    name.rs                # 姓アンカー検出 + 名/敬称による確信度昇格
    error.rs               # JaDictError
```

`Cargo.toml` (gedoku-detect) 差分:

```toml
[dependencies]
regex.workspace = true
crc32fast.workspace = true
serde_json.workspace = true
aho-corasick.workspace = true   # 追加。workspace には既に定義済み (aho-corasick = "1")
```

---

## 7.10.4 インタフェース定義

### 7.10.4.1 スキャン関数 (実行時ホットパス)

```rust
/// 住所辞書スキャン。
/// 入力: text (1スキャン対象文字列、UTF-8。L0 が確定した対象パスの文字列のみが渡る契約)
///       automaton: &AddressAutomaton (起動時に構築済み、Arc 経由で共有借用)
/// 出力: Vec<Detection> (category は必ず PII_ADDRESS_JA。confidence は 0.50〜0.92。
///       検出0件は正常系であり Result ではなく空 Vec で表す)
/// エラー: パニックしない。automaton 構築済み前提のため入出力I/Oを一切行わない。
pub fn scan(text: &str, automaton: &AddressAutomaton) -> Vec<Detection>;

/// 氏名辞書スキャン。
/// 入力: text, dict: &NameDict (surname_dict_path 未設定なら NameDict::empty() が渡り、
///       この場合 scan() は必ず空 Vec を返す = 明示的 no-op 契約)
/// 出力: Vec<Detection> (category は必ず PII_NAME_JA。confidence は 0.65〜0.97)
/// エラー: パニックしない。
pub fn scan(text: &str, dict: &NameDict) -> Vec<Detection>;
```

### 7.10.4.2 起動時ローダー (プロセス起動シーケンス内)

```rust
pub enum JaDictError {
    Io { path: PathBuf, source: std::io::Error },
    InvalidUtf8 { path: PathBuf },
    MalformedRow { path: PathBuf, line: usize, reason: String },
    DuplicateKey { path: PathBuf, line: usize, key: String },
    EmptyDictionary { path: PathBuf },
}

/// 内蔵 pref_city.tsv (+ extra_path があれば追加) から AhoCorasick を構築する。
/// 失敗時は Err を返し、呼び出し元 (Config::validate, 既存の起動時検証と同じ経路)
/// がプロセス起動を中断する (既定 fail_closed、§18 起動時検証ポリシーに合流)。
pub fn load_address_dict(extra_path: Option<&Path>) -> Result<AddressAutomaton, JaDictError>;

/// surname_dict_path から姓辞書をロードする。path が None なら Ok(NameDict::empty())
/// を返す (これがエラーではなく "無効" を表す唯一の経路)。
pub fn load_surname_dict(path: Option<&Path>) -> Result<NameDict, JaDictError>;
```

### 7.10.4.3 設定スキーマ (`configs/gedoku.toml` の `[ja_pii]` テーブル)

TOML 実物:

```toml
[ja_pii]
enabled = false

[ja_pii.address]
enabled = false
dict_path = ""
extra_town_dict_path = ""

[ja_pii.name]
enabled = false
surname_dict_path = ""
honorifics = ["様", "さん", "氏", "殿", "君", "先生"]
```

同じ形を JSON Schema (draft-07) として書き下したもの (自動検証用、付録 A.7 参照):

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://gedoku-proxy/schema/ja_pii.json",
  "title": "gedoku.toml [ja_pii] セクション",
  "type": "object",
  "required": ["enabled", "address", "name"],
  "additionalProperties": false,
  "properties": {
    "enabled": { "type": "boolean" },
    "address": {
      "type": "object",
      "required": ["enabled", "dict_path", "extra_town_dict_path"],
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "dict_path": { "type": "string" },
        "extra_town_dict_path": { "type": "string" }
      }
    },
    "name": {
      "type": "object",
      "required": ["enabled", "surname_dict_path", "honorifics"],
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "surname_dict_path": { "type": "string" },
        "honorifics": {
          "type": "array",
          "items": { "type": "string", "minLength": 1 },
          "minItems": 0
        }
      }
    }
  }
}
```

エラー条件: 未知キー・型不一致は `configs/gedoku.example.toml` 冒頭のコメントにある
既存ポリシー通り「起動時にエラーメッセージを出して失敗」に合流させる (新規の
特別扱いをしない)。

---

## 7.10.5 住所検出アルゴリズム

1. `AhoCorasickBuilder::new().match_kind(MatchKind::LeftmostLongest).build(&pref_city_patterns)`
   で構築したオートマトンを `text` に対して1回線形スキャンする。
2. 隣接するアンカー (前の一致の `end` と次の一致の `start` の間が0文字、または
   全角/半角スペース0〜1文字のみ) を1つの住所スパンにマージする
   (例: `"東京都"` + `"港区"` → `"東京都港区"`)。
3. マージ後のスパン末尾から最大60文字の窓に対して、丁目/番地/号の拡張パターンを
   適用しマッチすればスパン末尾を延長する:
   `(?:[0-9〇一二三四五六七八九十百千]{1,4}(?:丁目|番地|番|号|-|ー)){1,4}`
   (適用前に対象文字列を NFKC 正規化して全角数字・全角ハイフンの表記ゆれを吸収)。
4. 確信度は §7.10.7 の表に従う。丁目/番地/号 拡張が付いたスパンのみ既定閾値
   (0.75) を超え、単なる地名の言及 (「大阪出身です」等の裸の市区町村名) は
   0.50〜0.55 のため **単独では redact されない** (regex 側 §7.5 の L2 と同じ
   「保守的既定」の考え方)。

**発見済みの限界 (付録 A.10 自己適用トレースで判明)**: 辞書は行政区画の
正式名称 (`東京都`, `渋谷区` 等、接尾辞つき) のみを収録するため、口語的な
接尾辞省略表記 (「渋谷に住んでます」の「渋谷」) はそもそも辞書に一致せず
検出できない (見逃し)。これは意図的な v1 スコープ外であり、付録 A.1 の
失敗モード表で「対応機構なし」と明記する。

---

## 7.10.6 氏名検出アルゴリズム

1. `surname_dict_path` が未設定/空なら常に空 `Vec` を返して終了 (契約、§7.10.4.1)。
2. 姓辞書 (TSV: `surname\tambiguous\tfreq_rank`) から構築した AhoCorasick
   (`MatchKind::LeftmostLongest`) で `text` をスキャン。各エントリは
   `ambiguous: bool` を持つ (例: `森`, `林`, `上`, `下`, `中`, `前` のような
   一般語と衝突しやすい単漢字姓は `ambiguous=1`)。
3. `ambiguous=1` の姓は、以下のいずれかの文脈昇格が無ければ破棄する
   (単独では発火しない、confidence を計算する前に足切り):
   - 直後(0〜1文字の空白/読点を挟んで可)に敬称リスト
     (既定 `["様","さん","氏","殿","君","先生"]`, config で追加可) が続く
   - 直後に「名」らしい 1〜4文字のかな/漢字ランが続き、かつ
     (a) そのランが助詞/機能語ストップリスト
     (`は,が,の,を,に,で,と,から,まで,です,でした,という,ばかり,...`) に
     一致せず、**かつ** (b) 拡張ネガティブリスト
     (`docs/resource/ja-name-negative-list.md` 参照。会社名接尾辞
     (`電機,商事,製作所,工業,興業,商店,製菓,自動車,銀行,信用金庫,証券,不動産`)
     に加え、学術・科学技術系 (`理論,学説,モデル,仮説,法則,定理,公式,方程式,
     効果,現象`)、健康・スポーツ・技法系 (`体操,運動,メソッド,トレーニング,
     ヨガ,式,流,術,法`)、社会・思想・トレンド系 (`主義,制度,世代,ブーム,政策,
     政権,改革`)、非法人組織・施設・医療系 (`クリニック,医院,歯科,薬局,サロン,
     ラボ,研究所,学会,財団,協会`)、構造的ストップワード補完
     (`自身,本人,周辺,以外,対象,担当,関係`) の計5カテゴリを収録し、
     「人名 + 一般名詞/組織名」を広く誤認防止対象とする)
     前方一致にも一致しない場合のみ「氏名フルネーム」として昇格する
4. `ambiguous=0` の姓 (多くの2字以上の姓) は単独でも中程度の確信度で発火するが
   既定閾値未満に留め、敬称/フルネーム昇格でのみ redact 対象になる (§7.10.7)。

**発見済みの限界その2 (付録 A.10)**: このネガティブリストは
当初「会社名接尾辞のみ・住所側」に置く想定だったが、自己適用トレースで
「住所は確信度階層だけで十分 FP を抑えられる (裸の地名は 0.50〜0.55 で
足切りされる)」一方、「氏名側の "名" ヒューリスティックは会社名
(`山田電機`) だけでなく学術用語 (`佐藤理論`) や健康法 (`佐藤体操`) 等の
一般名詞複合語全般を人名と区別できない」ことが分かったため、対象を
会社名以外の4カテゴリ (学術・健康/技法・社会思想・非法人組織) と
構造的ストップワード補完へ拡張したうえで、**氏名側にのみ** 実装する
(語彙全量は `docs/resource/ja-name-negative-list.md` を正とする)。

---

## 7.10.7 確信度較正表

| rule_id | カテゴリ | 条件 | confidence | 既定閾値(0.75)超え |
|---|---|---|---|---|
| `ja_addr_pref` | `PII_ADDRESS_JA` | 都道府県名のみ、丁目/番地/号なし | 0.50 | いいえ |
| `ja_addr_city` | `PII_ADDRESS_JA` | 市区町村(区)名のみ、丁目/番地/号なし | 0.55 | いいえ |
| `ja_addr_full` | `PII_ADDRESS_JA` | 都道府県+市区町村 + 丁目/番地/号あり | 0.90 | はい |
| `ja_addr_full_pref_only` | `PII_ADDRESS_JA` | 市区町村なしで都道府県+丁目/番地/号 (稀) | 0.85 | はい |
| `ja_name_surname` | `PII_NAME_JA` | 非曖昧姓のみ、昇格なし | 0.65 | いいえ |
| `ja_name_full` | `PII_NAME_JA` | 姓+名 (ストップワード/拡張ネガティブリストガード通過) | 0.90 | はい |
| `ja_name_honorific` | `PII_NAME_JA` | 姓(曖昧含む)+敬称 | 0.92 | はい |
| `ja_name_full_honorific` | `PII_NAME_JA` | 姓+名+敬称 | 0.97 | はい |
| (曖昧姓、昇格なし) | — | 破棄 (Detection を生成しない) | — | — |

比較用に既存 §7.4 の近傍ルールを再掲: `jp_mynumber`=0.90, `phone_e164`=0.80。
`ja_addr_full`=0.90 は `jp_mynumber` と同値のため、後述 §7.10.9 で明示的な
回帰テストを要件化する。

---

## 7.10.8 config → `DetectionEngine` 配線

```rust
pub struct JaPiiConfig {
    pub enabled: bool,
    pub address: JaAddressConfig,
    pub name: JaNameConfig,
}
pub struct JaAddressConfig { pub enabled: bool, pub automaton: Arc<AddressAutomaton> }
pub struct JaNameConfig    { pub enabled: bool, pub dict: Arc<NameDict> }

impl DetectionEngine {
    pub fn scan(&self, text: &str) -> Vec<Detection> {
        let mut spans = l1_scan(text, &self.extra_enabled_rule_ids);
        if self.l2_enabled {
            spans.extend(l2_entropy_scan(text, &self.entropy));
        }
        if self.ja_pii.enabled {
            if self.ja_pii.address.enabled {
                spans.extend(ja_dict::address::scan(text, &self.ja_pii.address.automaton));
            }
            if self.ja_pii.name.enabled {
                spans.extend(ja_dict::name::scan(text, &self.ja_pii.name.dict));
            }
        }
        let spans = drop_allowlisted(spans, text, &self.allowlist);
        let mut spans = resolve_overlaps(spans);
        spans.retain(|d| d.confidence >= self.min_confidence);
        spans
    }
}
```

既存の `l2_enabled: bool` と全く同じ形の追加であり、`resolve_overlaps` /
`drop_allowlisted` / 閾値カットは**無改造で再利用**する。

---

## 7.10.9 overlap resolution との関係・既知の衝突ケース

- **既存の衝突事例 (CLAUDE.md ハマりポイント)**: `jp_mynumber` (`\b\d{12}\b`,
  conf 0.90) と `phone_e164` (`+[1-9]\d{7,14}`, conf 0.80) は桁数が近く
  同一コーパスで衝突した実績がある。
- 本設計の住所拡張正規表現は「丁目/番地/号」という**漢字を含む区切り文字**を
  必須にしているため、`jp_mynumber` の純数字12桁とは字面上一致しない
  (`4丁目2番8号` は `\b\d{12}\b` にマッチしない: 数字の連続が最大4桁までしか
  出ない設計)。したがって新規の衝突リスクは低いと判断する。
- ただし念のため、`ja_addr_full` (0.90) は `jp_mynumber` (0.90) と同値なので、
  「住所と紛らわしい12桁の羅列が丁目/番地表記に隣接する」合成テストケースを
  §7.10.12 のコーパスに**明示的に**含めることを要件化する (「桁数を近づけない」
  という既存の教訓の逆パターン: 今回は桁数がそもそも遠いことをテストで固定する)。
- `resolve_overlaps` は変更不要 (confidence 降順 → 長さ降順 → 開始位置昇順の
  既存ロジックがそのまま新カテゴリにも適用される)。

---

## 7.10.10 プレースホルダー / 正規化統合 (§7.7-7.8 との接続)

- プレースホルダー形式は共通: `⟦GDK:PII_ADDRESS_JA:<ID>⟧` / `⟦GDK:PII_NAME_JA:<ID>⟧`。
  カテゴリ名が24文字以内の `[A-Z][A-Z0-9_]*` 制約 (§7.7) を両方満たす。
- `normalize(value)` (§7.8) への追加:
  - `PII_ADDRESS_JA` → NFKC正規化のみ (全角数字→半角等)。丁目/番地の表記ゆれ
    (`4丁目` vs `４丁目` vs `4-`) は **v1 では正規化しない**
    (同じ人が同じ住所を別表記で書いた場合に別プレースホルダーになるのは
    許容する保守的既定。将来の拡張候補として付録 A.1 に記載)。
  - `PII_NAME_JA` → NFKC正規化 + 姓名間の空白 (半角/全角/複数) を単一の
    半角スペース1個に正規化してから ID を計算 (`山田　太郎` と `山田 太郎` が
    同一プレースホルダーになるようにする)。
- どちらも `SECRET` と同じく大文字小文字変換は行わない (氏名・地名は
  大文字小文字の概念がないため無関係)。

---

## 7.10.11 監査ログ統合

`gedoku-audit::record::RedactionRecord.counts: BTreeMap<&'static str, u64>` は
カテゴリ名を汎用的に受け取る設計 (P4-T1 の DoD: 原文を持てる型が存在しない) の
ため、**既存コードの変更は一切不要**で新カテゴリ2件がそのまま集計に乗る。

追加が必要なのは起動時ログの1行のみ (新規):

```rust
tracing::info!(
    target: "gedoku_audit",
    event = "ja_pii_dict_loaded",
    address_entries = address_automaton.pattern_count(),
    surname_entries = name_dict.len(),
    surname_dict_sha256 = %surname_dict_sha256.unwrap_or_default(),
    "gedoku-audit: ja_pii dictionaries loaded"
);
```

これにより第三者は「その時点でどのバージョンの辞書が有効だったか」を
起動ログの sha256 と `assets/ja/README.md` のスナップショット日付から
成果物 (辞書ファイルそのもの) なしで再構成できる。

---

## 7.10.12 テスト / コーパス戦略

P2-T8 (`src/gedoku-detect/tests/p2_t8_corpus_f1.rs`) と同じ「validator 逆算で
正例/負例を自動生成し F1 を CI ゲートにする」方式を踏襲し、以下を追加する:

- **正例**: 内蔵辞書からランダムに市区町村を選び丁目/番地/号を合成 (住所)、
  姓辞書 (テスト用ダミー姓リスト、非公開データに依存しない) から姓+名+敬称を
  合成 (氏名)。
- **負例**: 拡張ネガティブリスト5カテゴリそれぞれとの組み合わせ
  (会社名接尾辞 `山田電機は…`、学術 `佐藤理論は…`、健康/技法 `佐藤体操は…`、
  社会思想 `田中主義は…`、非法人組織 `山本クリニックは…`)、議定書/大学等の
  固有名詞との組み合わせ (`京都府警本部…` のような裸地名)、ストップワードとの
  組み合わせ (`田中は…`)。
- **ホールドアウト**: 辞書に**含めない**姓/市区町村を別枠で10%用意し、
  ホールドアウト側の F1 が学習コーパス側と乖離しないことを確認する
  (辞書丸暗記による見かけ上の高スコアを防ぐ、付録 A.4 参照)。
- 単体テスト (関数呼び出し列と期待値、`rules.rs` の既存テストスタイルに合わせる):

```rust
#[test]
fn ja_addr_full_detected_with_banchi() {
    let text = "東京都港区芝公園4丁目2番8号にお越しください";
    let d = ja_dict::address::scan(text, &test_automaton());
    let hit = d.iter().find(|x| x.rule_id == "ja_addr_full").unwrap();
    assert_eq!(hit.text(text), "東京都港区芝公園4丁目2番8号");
    assert!(hit.confidence >= 0.90);
}

#[test]
fn bare_city_mention_not_redacted_by_default_engine() {
    let mut engine = DetectionEngine::default();
    engine.ja_pii.enabled = true;
    engine.ja_pii.address.enabled = true;
    let hits = engine.scan("去年は大阪府で花火大会がありました");
    assert!(!hits.iter().any(|d| d.category == category::PII_ADDRESS_JA));
}

#[test]
fn ambiguous_surname_alone_is_not_flagged() {
    let text = "森の中を歩いた";
    let d = ja_dict::name::scan(text, &test_dict_with_ambiguous("森"));
    assert!(d.is_empty());
}

#[test]
fn ambiguous_surname_with_honorific_is_flagged() {
    let text = "森様にご確認ください";
    let d = ja_dict::name::scan(text, &test_dict_with_ambiguous("森"));
    let hit = d.iter().find(|x| x.rule_id == "ja_name_honorific").unwrap();
    assert_eq!(hit.text(text), "森様");
}

#[test]
fn negative_list_guards_against_false_full_name() {
    let cases = [
        ("山田電機の株価が上昇した", "山田"), // 会社名接尾辞
        ("佐藤理論は物理学の基礎だ", "佐藤"), // 学術・科学技術
        ("佐藤体操を毎朝している", "佐藤"),   // 健康・技法
        ("田中主義が広まった", "田中"),       // 社会・思想
        ("山本クリニックを受診した", "山本"), // 非法人組織・医療
    ];
    for (text, surname) in cases {
        let d = ja_dict::name::scan(text, &test_dict_non_ambiguous(surname));
        assert!(!d.iter().any(|x| x.rule_id == "ja_name_full"), "false positive: {text}");
    }
}

#[test]
fn name_scan_is_noop_when_surname_dict_path_unset() {
    let d = ja_dict::name::scan("山田太郎様", &NameDict::empty());
    assert!(d.is_empty());
}

#[test]
fn invalid_surname_tsv_fails_closed_at_startup() {
    let err = ja_dict::load_surname_dict(Some(Path::new("broken.tsv"))).unwrap_err();
    assert!(matches!(err, JaDictError::MalformedRow { .. }));
}

#[test]
fn addr_full_does_not_collide_with_jp_mynumber_regex() {
    // §7.10.9: 丁目/番地表記は漢字区切りのため 12 桁純数字の regex とは字面上一致しない
    let text = "港区123456789012丁目1番1号"; // 意図的に12桁の羅列を隣接させる回帰テスト
    let d = DetectionEngine::default_with_ja_pii().scan(text);
    // カテゴリが混線せず、mynumber 側は12桁ちょうどの純粋な数字列にしか反応しない
    assert!(d.iter().filter(|x| x.rule_id == "jp_mynumber").count() <= 1);
}
```

- CI ゲート: 既存 `p2_t8_corpus_f1.rs` と同様に `F1 >= 0.90` (氏名は辞書非同梱の
  ためテスト専用ダミー辞書を使う。住所は同梱辞書 `pref_city.tsv` をそのまま使う)。

---

## 7.10.13 既定値一覧

| 項目 | 既定値 | 理由 |
|---|---|---|
| `ja_pii.enabled` | `false` | §7.1 の NER と同じ「PII 検出は FP リスクがあるためマスタースイッチは opt-in」方針を踏襲 |
| `ja_pii.address.enabled` | `false` | 同上。辞書は同梱済みだが、まずマスタースイッチで無効化 |
| `ja_pii.address.dict_path` | `""` (空 = 内蔵辞書のみ) | 内蔵 `pref_city.tsv` で足りるケースが大半 |
| `ja_pii.address.extra_town_dict_path` | `""` (空 = 追加なし) | 町丁目辞書は同梱しない (§7.10.2) ため既定は無効 |
| `ja_pii.name.enabled` | `false` | 同上 |
| `ja_pii.name.surname_dict_path` | `""` (空 = 姓辞書なし、常に no-op) | ライセンス上、姓データを同梱しない意思決定 (§7.10.2) の直接の帰結 |
| `ja_pii.name.honorifics` | `["様","さん","氏","殿","君","先生"]` | 日本語ビジネス文書で最頻出の敬称5種+「先生」。運用者が業種特有の敬称 (例: 「殿下」「猊下」) を追加できるよう配列は上書き可 |
| 住所 confidence 閾値群 | §7.10.7 の表 (0.50〜0.97) | 既存 `min_confidence=0.75` (§7.5 と同じ既定) を跨ぐように意図的に設計: 裸の地名/姓は下、文脈昇格したものは上 |
| 丁目/番地/号 拡張の探索窓 | 60文字 | 既存 L2 エントロピーの `context_window=40` (§7.5) より長めに取る。理由: 住所は「都道府県+市区町村」から「丁目」までの間に建物名等が挟まる実例があるため、電話番号文脈探索より広めの窓が必要と判断 |
| 隣接アンカーのマージ許容空白 | 0〜1文字 | 「東京都 港区」のような稀な表記ゆれのみ許容し、無関係な語を跨いで誤結合しないよう最小限に抑える |
| ホールドアウト比率 (§7.10.12) | 10% | 既存 P2-T8 コーパス (306件) の規模感に対し、少なくとも二桁件数のホールドアウトを確保できる最小の比率として設定 |
| CI ゲート F1 閾値 | 0.90 | 既存 P2-T8 の `F1=1.0` 実績より緩めるが、辞書ベース手法は正規表現ほど決定的ではないため NER 導入前の中間解として現実的な目標値に設定 |

上記のうち「実装時に決める」「TBD」に該当する項目は無い
(このカッコ書きの2語自体が本文中に出現するのは、いま引用した禁止語句の説明
としてのみである)。

---

## 付録A: 設計プロセス健全性チェック

### A.1 失敗モード対応表 (failure_mode_mapping)

| # | 失敗モード | 対応機構 | 節 |
|---|---|---|---|
| 1 | 単漢字姓 (森/林/上/下/中/前等) が一般語と衝突し過検出 | `ambiguous` フラグ + 敬称/フルネーム昇格必須ルール、昇格なしは Detection 自体を生成しない | §7.10.6 |
| 2 | 会社名 (`山田電機`) や学術用語/健康法等の一般名詞複合語 (`佐藤理論`,`佐藤体操`) を人名 (`山田太郎`) と誤認 | 拡張ネガティブリスト・5カテゴリ (氏名側、A.10 で住所側から移設・会社名以外へ拡張) | §7.10.6 |
| 3 | 市区町村名が固有名詞の一部 (`京都府警本部`) | 裸地名は confidence 0.50〜0.55 で既定閾値未満 (足切りのみで十分、追加ガード不要と判明: A.10) | §7.10.5, §7.10.7 |
| 4 | `surname_dict_path` 未設定なのに `name.enabled=true` にする設定ミスを「保護できている」と誤認 | `scan()` の契約として明示的に空 Vec を返す no-op、かつ `name_scan_is_noop_when_surname_dict_path_unset` テストで固定 | §7.10.4.1, §7.10.12 |
| 5 | 丁目/番地の全角/半角表記ゆれで拡張正規表現が外れ番地部分だけ非redact | 拡張前に NFKC 正規化を通す | §7.10.5 |
| 6 | `jp_mynumber`/`phone_e164` との既知の桁数衝突パターンの再発 | 丁目/番地表記は漢字区切り必須のため純数字12桁 regex と字面上不一致。回帰テストで固定 | §7.10.9, §7.10.12 |
| 7 | ゼロ幅文字・結合文字・半角カナ混入による意図的なマッチ回避 | NFKC正規化を辞書マッチ**前**の前処理として必須化 | §7.10.5, A.4 |
| 8 | 行政区画変更 (市町村合併等) で辞書が陳腐化 | `assets/ja/README.md` にスナップショット日付を明記、config差し替えのみで再ビルド不要な運用手順を用意 | §7.10.2, §7.10.3 |
| 9 | 姓データのライセンス不備を運用者が意図せず同梱 | v1では姓辞書を一切同梱しない、`surname_dict_path` は運用者が自分の権利で用意する契約とし README に明記 | §7.10.2 |
| 10 | L3 NER (既存, 既定OFF) と本機構が同一スパンを検出した際のカテゴリ表記不一致 | NER 導入時も `PII_NAME_JA`/`PII_ADDRESS_JA` を再利用することを要件化。重複解決は既存 `overlap.rs` がそのまま処理 | §7.10.1, §7.10.9 |
| 11 | **口語的な接尾辞省略地名 (「渋谷に住んでます」の「渋谷」) の見逃し** | **対応機構なし。v1スコープ外として明示。**辞書は正式名称 (接尾辞つき) のみ収録するため原理的に検出不可 | §7.10.5 (自己適用トレース A.10 で発見) |
| 12 | 海外住所・外国人氏名 (ラテン文字/カタカナ表記) | **対応機構なし。v1スコープ外として明示。** | 全体 |

12件中10件に具体的な対応機構があり、残り2件 (#11, #12) は「守れない」ことを
本表で明示している。

### A.2 状態外部化 (state_externalized)

ループ的な継続状態は存在しないが、「プロセス再起動だけで全く同じ検出挙動を
再現できるか」という意味での外部状態を以下に表化する。

| 状態 | 所在 | 復帰方法 |
|---|---|---|
| 住所オートマトン | `src/gedoku-detect/assets/ja/pref_city.tsv` (リポジトリにコミット, `include_str!`) | ソースツリーをチェックアウトして再ビルドするだけ (ネットワーク不要) |
| 追加住所辞書 | `configs/gedoku.toml` の `ja_pii.address.extra_town_dict_path` が指す実行時ファイル | 同一パスに同一ファイルを置いて起動するだけ |
| 姓辞書 | `ja_pii.name.surname_dict_path` が指す実行時ファイル (既定は未設定=状態なし) | 同上 |
| 敬称リスト・フラグ群 | `configs/gedoku.toml` 内 (TOML はファイルそのものが真実source) | 設定ファイルをコピーするだけ |

**1回のツール呼び出しで完全復帰できる経路**: `Config::from_env_or_file(path)` を
1回呼べば (既存の設定解決順序 `--config`/`GEDOKU_CONFIG_FILE`/既定パスと同じ経路)、
上表の全状態がファイルから再構築され、以後はプロセス内 `Arc`/`LazyLock` で
immutable に保持される。会話履歴やモデルの記憶に依存する状態は一切ない。

### A.3 判定所有権 (verdict_ownership)

redact するか否かの最終判定式は `DetectionEngine::scan` 内の1行のみ:

```rust
spans.retain(|d| d.confidence >= self.min_confidence);
```

- `confidence` は §7.10.7 の表としてコードにハードコードされた定数であり、
  実行時にモデル (Claude 本体) や運用者の「これは大丈夫そう」という主観が
  代入される経路が無い (定数を変えるには PR レビューが必要 = 所有者はコード
  レビュアー)。
- 唯一の実行時上書き経路は `Allowlist` (P1) であり、これは confidence 式とは
  別のフィルタ (`drop_allowlisted`) として明確に分離されている。運用者が
  allowlist に追加した内容は「記録専用の例外」であり、判定式そのものを
  書き換えるわけではない。
- モデルの自己申告 (例えば「このスパンは住所じゃないと思う」という所感) が
  入り込む欄は設計上存在しない。

### A.4 対抗策 (anti_gaming)

| # | 手口 | 機構 |
|---|---|---|
| 1 | コーパスに辞書丸暗記の値だけを入れて F1 を水増し | ホールドアウト10% (辞書に含めない姓/市区町村) を必須化し、乖離を CI で監視 (§7.10.12) |
| 2 | confidence を閾値ぎりぎりに調整して「デフォルトで守れている」ように見せかけ、実運用の見逃しをログで隠す | 監査ログのカテゴリ別件数 (§7.10.11) を継続監視する運用要件を明記。異常値検知は既存 `gedoku_log_guard_trips_total` 相当のメトリクス拡張として今後の運用課題に位置づける (v1では検出のみ、アラートは範囲外と明示) |
| 3 | ゼロ幅文字/結合文字/全角半角の意図的挿入で辞書マッチを回避 | NFKC正規化を辞書マッチ前に必須化 (失敗モード#7と同一機構)。**限界**: 正規化で吸収できない同音異字による回避 (例: 異体字セレクタ) は v1 では対応せず、既知の限界として明記する (拒否できない) |
| 4 | 運用者が Allowlist に過度に広い正規表現 (`.*` 等) を登録して全検出を無効化する | これは運用者本人の意図的操作であり、システムとして拒否することはできない (**限界として明記**)。監査ログの `counts` が恒常的に0になる異常を検知する運用手順をドキュメント化することのみ対応可能 |
| 5 | 姓辞書TSVに矛盾行 (同一姓が `ambiguous=0` と `ambiguous=1` の両方) を混入させ挙動を不定にする | ローダーが重複キーを `JaDictError::DuplicateKey` として **拒否** し、起動時 fail_closed で弾く (§7.10.4.2) |

### A.5 状態機械 (state_machine)

状態は3個の実効ビットの組み合わせで尽くされる: `E=ja_pii.enabled`,
`A=address.enabled`, `S=(name.enabled かつ surname_dict_path が空でない)`。

| E A S | address::scan 実行 | name::scan 実行 | 備考 |
|---|---|---|---|
| 0 0 0 | 実行しない | 実行しない | 既定。全OFF |
| 0 0 1 | 実行しない | 実行しない | `E=0` がマスタースイッチのため他ビット無視。起動時に `warn!("ja_pii.name.enabled=true だが ja_pii.enabled=false のため無効")` を出す |
| 0 1 0 | 実行しない | 実行しない | 同上、`address` 側で warn |
| 0 1 1 | 実行しない | 実行しない | 同上、両方で warn |
| 1 0 0 | 実行しない | 実行しない | 意図的な中間状態 (両方OFF)。warn 不要 |
| 1 0 1 | 実行しない | 実行する | 氏名のみ |
| 1 1 0 | 実行する | 実行しない | 住所のみ |
| 1 1 1 | 実行する | 実行する | 両方 |

全8状態が config の値の組み合わせとして到達可能であり、到達不能な状態は無い。
`S` の定義に `surname_dict_path` の有無を畳み込んでいるのは、A.1 #4 の
失敗モード (設定ミスの誤認) を状態表レベルでも可視化するため。

### A.6 収束性 (convergence)

`DetectionEngine::scan` は1リクエスト中の1文字列に対する**単一パスの純粋関数**
であり、リトライ・不動点反復・ループは一切存在しない。

- L1 (固定個数の regex ルール) → L2 (1関数) → L3' 住所 (0または1回) →
  L3' 氏名 (0または1回) → allowlist → overlap → threshold、という
  **有限・固定順序**のパイプラインを1回実行して終了する。
- 住所の丁目/番地拡張探索窓は60文字固定 (§7.10.13) であり、入力長 `n` に対して
  高々定数倍のコストで打ち切られる。AhoCorasick はバックトラックしない
  前進オートマトンのため最悪計算量は `O(n)`。
- 「合格するまで回す」という概念自体がこの関数には存在しない
  (strict-goal のループとは異なり、1リクエスト=1回の確定的計算)。
  したがって停滞条件・上限周回・上限到達時の出口という項目は**該当なし**
  であり、それ自体が収束性の証明になっている。

### A.7 パッケージ適合性 (packaging_conformance)

**マニフェスト (実物)**:

| path | role | sha256 (スクラッチパッド検証用サンプル) | bytes |
|---|---|---|---|
| `ja_pii.schema.json` | config (§7.10.4.3 と同一内容) | `23bebcc66ca2902867809a767e4774bf9dd692258d7e6df5b258e03f984dd210` | 1055 |
| `pref_city.sample.tsv` | doc (§7.10.3 `assets/ja/pref_city.tsv` の縮小サンプル) | `1b4dc0d4775ab9ba65798f523d38cb7b7d9122af313afdcf88a3e2116f0f6602` | 175 |
| `validate.js` | generated (config schema 検査器) | `820319ea7fb96ae783e0b37b0a44bfa072fc44242a94225daff91b71786a2d16` | 2708 |
| `check_dict_schema.js` | generated (TSV schema 検査器) | `4fa572aa32cee4125d764c7e0cb67b8fb819c66a1523169e92dba09e5ffed07d` | 1578 |

**スキーマ検査スクリプト実行結果 (score_submit の command evidence で提示する
実行ログと同一)**:

```
== valid instance ==
OK: valid_instance.json conforms to ja_pii.schema.json
exit=0
== invalid instance (expect NG, exit 1) ==
NG: invalid_instance.json violates ja_pii.schema.json
 - $.address.enabled: expected boolean, got string
 - $.unknown_field: additional property not allowed
exit=1

== good tsv ==
OK: pref_city.sample.tsv conforms to pref_city.tsv schema
exit=0
== broken tsv (expect NG) ==
NG: pref_city.broken.tsv violates pref_city.tsv schema
 - line3: kind must be pref|city|ward, got 'district'
 - line4: expected 2 columns, got 1
exit=1
```

正常系が exit=0、異常系 (型不一致・未知フィールド・列数不一致・enum違反) が
正しく exit=1 かつ具体的な理由行を出すことを確認済み。検査器自体は
`gedoku-detect::validators` の既存方針 (重量級ライブラリより手書き検証関数)
に合わせ、依存パッケージを追加せず Node 標準ライブラリのみで実装している。

### A.8 ホスト差異吸収 (host_portability)

| 差異 | 吸収方針 | 値 |
|---|---|---|
| config ファイル解決順序 | 既存の決定事項に完全に相乗りする (新規ルールを増やさない) | `--config`/`-c` > `GEDOKU_CONFIG_FILE` > 既定パス `configs/gedoku.toml` |
| 辞書ファイルの相対パス基準 | `debug_dump_replacements` の前例 (プロセスのカレントディレクトリ基準) に倣う | `dict_path`/`extra_town_dict_path`/`surname_dict_path` はプロセス CWD 相対、または絶対パス |
| パス結合 | 文字列結合を禁止しコード上 `std::path::PathBuf` を使う (Windows の `\` / POSIX の `/` を吸収) | 実装規約として明記 |
| 辞書ファイルの文字コード | UTF-8 固定 (BOMは許容し読み飛ばす)。それ以外は `JaDictError::InvalidUtf8` で起動時 fail_closed | 値: UTF-8 (BOM可) |
| 改行コード | LF/CRLF 両方許容 (Rust 標準 `str::lines()` が両対応のためコード側の追加吸収は不要) | 値: LF, CRLF |
| 内蔵辞書のホスト依存性 | `include_str!` でバイナリに埋め込むためファイルシステムアクセス自体が不要。既定 (住所のみ・内蔵辞書のみ) で運用する限りホスト差異はゼロ | 強み: 既定運用ではポータビリティ問題が原理的に発生しない |
| 解決できない場合の縮退 | 辞書ファイルが指定されているのに読めない場合は **fail_closed** (既存 `on_detect_error` の既定値と整合)。辞書が空文字 (未指定) の場合は前述の no-op 契約 (縮退ではなく仕様) | 値: fail_closed |

### A.9 責務分割 (responsibility_split)

| 判断 | 所有者 | 重複の有無 |
|---|---|---|
| 辞書に何を含めるか (語彙選定) | 住所: リポジトリ (同梱)。氏名: 運用者 (`surname_dict_path` 経由) | なし |
| 文字列が辞書に一致するか | `dict.rs` の AhoCorasick オートマトン | なし |
| 文脈昇格するか (confidence 決定) | `address.rs`/`name.rs` の純粋関数、コードレビュー対象・設定不可 | なし |
| redact するか (閾値判定) | `DetectionEngine::scan` の `min_confidence` 比較 (既存 P3 の再利用、新規コードなし) | なし |
| 事後的な例外 | `Allowlist` (P1)、運用者が config/API で設定 | 唯一「運用者が事後的に上書きできる」層であり、他のどの判断とも重複しない |
| 重複区間の解決 | `overlap.rs` (既存、無変更) | なし |
| 監査記録 | `gedoku-audit::record` (既存、無変更。新カテゴリ文字列が素通りするだけ) | なし |

全ての判断の所有者が1箇所に決まっており、同じ判断を2箇所で行っている箇所は無い。

### A.10 自己適用トレース (self_hosting)

本設計のアルゴリズムを、本書中の実例文に対して手動でトレースした。

**トレース1**: `"山田太郎様、ご住所は東京都港区芝公園4丁目2番8号です。"`

1. 氏名側: AC が姓辞書から `"山田"` (非曖昧) を検出 → 直後2文字 `"太郎"` は
   ストップワード/会社接尾辞リストに不一致 → `ja_name_full` (0.90) に昇格 →
   さらに直後に `"様"` が続くため `ja_name_full_honorific` (0.97) に再昇格。
2. 住所側: AC が `"東京都"` (pref) と `"港区"` (ward) を隣接検出 → マージ →
   末尾から60文字窓に `"芝公園4丁目2番8号"` の丁目/番地/号パターンが一致 →
   スパンを `"東京都港区芝公園4丁目2番8号"` まで延長 → `ja_addr_full` (0.90)。
3. 両スパンは重ならないため `overlap.rs` は両方採用、既定閾値0.75も両方超える
   → 想定通り両方 redact される。**穴なし (ハッピーパスの整合性を確認)**。

**トレース2**: `"京都府警本部について"` / `"山田電機の株価が上昇した"` /
`"佐藤理論は物理学の基礎だ"`

1. `"京都府"` は辞書 (pref, `"京都府"` 全体が1エントリ) に一致するが、
   直後に丁目/番地/号が続かないため `ja_addr_pref` (0.50) に留まり、
   既定閾値未満で redact されない → **想定通り**。
2. `"山田電機"`: 姓 `"山田"` (非曖昧) 検出後、直後2文字 `"電機"` が
   拡張ネガティブリスト (会社名接尾辞カテゴリ) に前方一致 → `ja_name_full`
   への昇格を **拒否** → `ja_name_surname` (0.65) のみ残り既定閾値未満 →
   redact されない → **想定通り**。
3. `"佐藤理論"`: 姓 `"佐藤"` (非曖昧) 検出後、直後2文字 `"理論"` が
   拡張ネガティブリスト (学術・科学技術系カテゴリ) に前方一致 → 同様に
   `ja_name_full` への昇格を **拒否** → redact されない → **想定通り**
   (会社名限定だった旧リストでは `"理論"` がリスト外のため昇格を拒否できず
   `ja_name_full` に誤昇格 → 過検出 (誤 redact) していたケース)。

**発見した穴 (番号は A.1 と対応)**:

- **穴1 (A.1 #11 として反映済み)**: 「渋谷に住んでます」のような接尾辞省略の
  口語地名は、辞書エントリが正式名称 (`渋谷区`) のみのため一切マッチしない
  ことが判明。当初はこの限界に気づいておらず、住所検出は「地名+文脈」で
  ある程度カバーできると想定していたが、トレースの過程で
  「そもそもアンカーが立たない」ことに気づいた。→ v1スコープ外として
  A.1 #11 に反映し、§7.10.5 本文にも明記した。
- **穴2 (A.1 #2, §7.10.6 として反映済み)**: 当初は会社名との衝突ガードを
  住所側 (地名+法人語のネガティブリスト) に置く設計だったが、トレース2の
  `"京都府警本部"` を検証した際に「confidence 階層だけで既に足切りされており
  追加ガードは不要」と分かった一方、`"山田電機"` のトレースで
  「氏名側の "名" ヒューリスティックにこそ会社名ガードが必要」と判明した。
  → ネガティブリストの実装位置を住所側から氏名側へ変更し、§7.10.6 の
  現在の記述 (「発見済みの限界その2」) に反映済み。
- **穴3**: 本設計書自身に実在の住所例 (`東京都港区芝公園4丁目2番8号`) や
  人名例 (`山田太郎`) を含めているため、本書をそのまま Claude Code の
  コンテキストに貼り付けて gedoku-proxy 経由で送信した場合、`ja_pii` が
  有効な環境ではこれらの例示がプレースホルダー化される。これは
  プロキシ→アップストリーム間のみの変換でありローカルの diff 表示には
  影響しないため実害はないが、レビュー時に混乱しないよう本注記を残す
  (対応不要・記録のみ)。

### A.11 却下した代替案 (rejected_alternatives)

| 代替案 | 却下理由 |
|---|---|
| 正規表現による住所/氏名の直接列挙 | 住所は市区町村だけで約1,900種、姓は理論上開集合 (数十万件規模かつ新姓も発生) であり、`\|` 交代の可読性・保守性が破綻し、regex クレートのコンパイル時間・サイズにも悪影響。原理的に列挙不可能な氏名には適用できない |
| NER (Transformer) のみで代替 | 精度は高いが 10〜100ms/KB (§14 のレイテンシ予算を圧迫)、ONNX/candle 等の新規重量依存が必要でバイナリサイズが増える。既存 §7.1 で既に「既定OFFの理由」として明記済みであり、本設計はその隙間を埋めるものであってNERの代替にはならない (辞書に無い語は取れない) |
| 外部住所正規化API (日本郵便API/ジオコーダ等) の呼び出し | 検出のために住所そのものを外部サービスへ送信することになり、§15 ADR-6「検出のために機密を外部に出すのは自己矛盾」と同じ理由で不採用。レイテンシも増える |
| 全文字 n-gram のブルームフィルタ的近似照合 | 誤検知率の制御が難しく、Aho-Corasick のような「厳密な文字列一致 + 線形時間」を両立できない。近似一致は住所の丁目/番地拡張のような構造化後処理と相性が悪い |
| spaCy(ja)/GiNZA 等の Python NER ライブラリへの委譲 | モデル配布サイズが数百MB規模になり、Rust 一枚岩の workspace に異言語ランタイムを混入させることになる。既存の技術方針 (全クレート Rust) との不整合が大きい |
| 姓データセットを民間データから無確認で同梱 | ライセンス不備のリスクが「デフォルトで守れる範囲が狭い」というデメリットより重いと判断 (§7.10.2)。同梱せず運用者調達方式を採用 |

---

*(付録Aの評価はこのファイル内に記録として残し、`docs/gedoku-proxy-design.md` へ
マージする際は §7.10.0〜§7.10.13 の本文のみを転記する想定。)*
