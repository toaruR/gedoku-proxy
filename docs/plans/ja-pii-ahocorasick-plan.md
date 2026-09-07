# 日本語PII Aho-Corasick辞書検出 実装計画

- 上流設計: [docs/plans/ja-pii-ahocorasick-design.md](./ja-pii-ahocorasick-design.md)(FINAL確定)
- strict-goal plan session: `rl_01M1V2RM4C848WB816R9FK5SHV` (round3, FINAL, weighted_mean=9, 8/8 criteria合格)
- artifact digest: `sha256:46b52f4b459555b87e057684ddd6cf73c7e1cd52292ca3339ea052c640524487`

## 概要

docs/plans/ja-pii-ahocorasick-design.md(FINAL確定設計)に基づき、日本語住所/氏名のAho-Corasick辞書検出(L3')をgedoku-detectに実装する。カテゴリ定数追加→依存追加→辞書アセット準備→ja_dictローダー(fail_closed)→address/name検出アルゴリズム→[ja_pii]設定スキーマ→DetectionEngine配線→confidence較正表とjp_mynumber衝突回帰テスト→プレースホルダー/正規化統合→監査ログ(受理/拒否対称)→§7.10.12単体テスト+F1コーパス→docs/gedoku-proxy-design.mdへの本文マージ、の順に13タスクへ分解する。外部データ整備に依存するT003(辞書アセット)は前提が最も不確実なため依存関係が許す最前段に置き、後戻りコストを最小化する順序にした。

## 前提

1. `docs/resource/ja-name-negative-list.md` および `docs/resource/last_name_org.csv` は既にリポジトリに用意済みであり、T006の拡張ネガティブリスト/テスト用ダミー姓データの原資として利用できる。
2. `src/gedoku-core/src/lib.rs` の Config/FileConfig(deny_unknown_fields)拡張パターンを `[ja_pii]` セクションにもそのまま適用できる。
3. 総務省「全国地方公共団体コード」相当の都道府県・市区町村データはT003内で新規に整備し、他タスクの前提とする。
4. 設計書の7.10.0(位置づけと動機)と付録A(A.1〜A.11、設計プロセス健全性チェック)は実装対象の決定を含まない設計プロセスの検証記録であるため、対応する実装タスクを設けない(実装不要)。
5. T003(辞書アセット追加)とT013(ドキュメントマージ)はRustコード変更を伴わずビルドに影響しないため、それ以外の全11タスクはverifyにcargo build/cargo testを含み、各タスク完了時点でビルド可能・テストgreenであることを保証する。

## タスク一覧

### T001: category.rs へ PII_ADDRESS_JA / PII_NAME_JA 定数を追加

- **intent**: 設計§7.10.1に従い、既存のPII_EMAIL等と同じ形式でカテゴリ定数を2つ追加し、以降の全タスクが参照する共通シンボルを確定させる。
- **design_refs**: `PII_ADDRESS_JA`
- **depends_on**: なし
- **changes**: `src/gedoku-detect/src/category.rs` (modify) — PII_ADDRESS_JA/PII_NAME_JA定数追加
- **acceptance**:
  - category::PII_ADDRESS_JAとcategory::PII_NAME_JAが&'static strの公開定数として存在する
  - 既存定数(PII_EMAIL等)と同じ宣言スタイル(裸のconst、doc commentなし)である
- **verify**:
  - `cargo build -p gedoku-detect` (exit 0)
  - `grep -c PII_ADDRESS_JA src/gedoku-detect/src/category.rs` (exit 0)
- **estimate_rounds**: 1

### T002: gedoku-detect の Cargo.toml に aho-corasick 依存を追加

- **intent**: 設計§7.10.3の依存差分に従い、workspaceに既に定義済みのaho-corasickをgedoku-detectクレートの依存として有効化する。
- **design_refs**: `aho-corasick.workspace = true`
- **depends_on**: なし
- **changes**: `src/gedoku-detect/Cargo.toml` (modify) — aho-corasick.workspace = true を追加
- **acceptance**:
  - gedoku-detect/Cargo.tomlの[dependencies]にaho-corasick.workspace = trueが追加されている
  - cargo build --workspace が成功する(新規重量依存の追加によるビルド破壊がない)
- **verify**:
  - `cargo build --workspace` (exit 0)
- **estimate_rounds**: 1

### T003: 住所辞書アセット pref_city.tsv と README.md を整備

- **intent**: 設計§7.10.2/§7.10.3に従い、47都道府県+市区町村・特別区・政令市の区を収録したTSV辞書と、出典・スナップショット日付・更新手順を記載したREADMEを新規追加する。
- **design_refs**: `assets/ja/pref_city.tsv`
- **depends_on**: なし
- **changes**:
  - `src/gedoku-detect/assets/ja/pref_city.tsv` (add) — 1行目ヘッダー name\tkind、kindはpref|city|ward
  - `src/gedoku-detect/assets/ja/README.md` (add) — 出典・スナップショット日付・更新手順
- **acceptance**:
  - pref_city.tsvの1行目が name\tkind であり、以降の各行がname/kind(pref|city|ward)の2列TSVである
  - kind=prefの行数がちょうど47件(全都道府県)であり、kind=cityまたはkind=wardの行数合計が150件以上である
  - README.mdに取得元データの出典とスナップショット日付が明記されている
- **verify**:
  - `head -1 src/gedoku-detect/assets/ja/pref_city.tsv` (exit 0)
  - `test -f src/gedoku-detect/assets/ja/README.md` (exit 0)
  - `test "$(awk -F'\t' '$2=="pref"' src/gedoku-detect/assets/ja/pref_city.tsv | wc -l)" = "47" && test "$(awk -F'\t' '$2!="pref"' src/gedoku-detect/assets/ja/pref_city.tsv | wc -l)" -ge 150` (exit 0)
- **estimate_rounds**: 2

### T004: ja_dict モジュール骨格(JaDictError, TSVローダー, AddressAutomaton/NameDict構築)

- **intent**: 設計§7.10.3/§7.10.4.2に従い、ja_dictモジュールの土台(dict.rs, error.rs, mod.rs)を作成し、load_address_dict/load_surname_dictのfail_closedローダーを実装する。以降のaddress/name検出タスクが依存する共通基盤。
- **design_refs**: `pub fn load_address_dict`, `pub fn load_surname_dict`
- **depends_on**: T002, T003
- **changes**:
  - `src/gedoku-detect/src/ja_dict/mod.rs` (add)
  - `src/gedoku-detect/src/ja_dict/error.rs` (add)
  - `src/gedoku-detect/src/ja_dict/dict.rs` (add)
  - `src/gedoku-detect/src/lib.rs` (modify) — pub mod ja_dict; 宣言を追加
- **acceptance**:
  - load_address_dict(None)がpref_city.tsvからAddressAutomatonを構築しOkを返す
  - load_surname_dict(None)がOk(NameDict::empty())を返す(未設定=no-op契約)
  - 列数不一致・重複キーを含む不正なTSVはJaDictError::MalformedRow/DuplicateKeyでErrを返しパニックしない
- **verify**:
  - `cargo test -p gedoku-detect ja_dict::dict::` (exit 0)
  - `cargo test -p gedoku-detect invalid_surname_tsv_fails_closed_at_startup` (exit 0)
- **estimate_rounds**: 2

### T005: ja_dict::address 住所検出アルゴリズムの実装

- **intent**: 設計§7.10.4.1/§7.10.5に従い、AddressAutomatonによるアンカー検出・隣接マージ・丁目/番地/号拡張・NFKC正規化を実装し、confidence 0.50〜0.92の範囲でDetectionを返すscan()を完成させる。
- **design_refs**: `ja_dict::address`, `住所検出アルゴリズム`
- **depends_on**: T001, T004
- **changes**: `src/gedoku-detect/src/ja_dict/address.rs` (add)
- **acceptance**:
  - 「東京都港区芝公園4丁目2番8号」からja_addr_full(confidence>=0.90)を検出する
  - 丁目/番地/号を伴わない裸の市区町村名はconfidence 0.50〜0.55に留まり既定閾値(0.75)未満である
  - 全角数字・全角ハイフンを含む丁目/番地表記もNFKC正規化により検出できる
- **verify**:
  - `cargo test -p gedoku-detect ja_addr_full_detected_with_banchi` (exit 0)
  - `cargo test -p gedoku-detect bare_city_mention_not_redacted_by_default_engine` (exit 0)
- **estimate_rounds**: 3

### T006: ja_dict::name 氏名検出アルゴリズムと拡張ネガティブリストの実装

- **intent**: 設計§7.10.4.1/§7.10.6に従い、姓辞書スキャン・曖昧姓の敬称/フルネーム昇格ガード・docs/resource/ja-name-negative-list.md記載の5カテゴリ拡張ネガティブリストによる誤検出防止を実装する。
- **design_refs**: `ja_dict::name`, `氏名検出アルゴリズム`, `docs/resource/ja-name-negative-list.md`
- **depends_on**: T001, T004
- **changes**: `src/gedoku-detect/src/ja_dict/name.rs` (add)
- **acceptance**:
  - 「森の中を歩いた」のような曖昧姓単独の言及ではDetectionを生成しない
  - 「森様にご確認ください」でja_name_honorific(confidence>=0.92)を検出する
  - 山田電機/佐藤理論/佐藤体操/田中主義/山本クリニックの5パターン全てでja_name_fullへの誤昇格が起きない
  - surname_dict_path未設定(NameDict::empty())時はscan()が常に空Vecを返す
- **verify**:
  - `cargo test -p gedoku-detect ambiguous_surname` (exit 0)
  - `cargo test -p gedoku-detect negative_list_guards_against_false_full_name` (exit 0)
  - `cargo test -p gedoku-detect name_scan_is_noop_when_surname_dict_path_unset` (exit 0)
- **estimate_rounds**: 3

### T007: [ja_pii] 設定スキーマの Config/FileConfig 拡張

- **intent**: 設計§7.10.4.3に従い、[ja_pii]テーブル(enabled, address.*, name.*)をFileConfig(deny_unknown_fields)へ追加し、JaPiiConfig/JaAddressConfig/JaNameConfigへ変換する処理とJSON Schema(ja_pii.schema.json)を用意する。
- **design_refs**: `[ja_pii]`, `surname_dict_path`, `ja_pii.schema.json`
- **depends_on**: T004
- **changes**:
  - `src/gedoku-core/src/lib.rs` (modify) — FileConfigへ[ja_pii]セクション追加、Config::from_toml_str/from_envの分岐拡張
  - `configs/gedoku.example.toml` (modify) — [ja_pii]セクションのコメント付きサンプル追加
  - `docs/schema/ja_pii.schema.json` (add)
- **acceptance**:
  - [ja_pii]セクション省略時は既定値(enabled=false等、§7.10.13)で起動する
  - 未知キーまたは型不一致を含む[ja_pii]セクションは起動時にErrを返す(fail_closed、deny_unknown_fields)
  - ja_pii.schema.jsonがdraft-07として妥当であり、configs/gedoku.example.tomlの[ja_pii]セクションと整合する
- **verify**:
  - `cargo test -p gedoku-core ja_pii` (exit 0)
  - `node -e "JSON.parse(require('fs').readFileSync('docs/schema/ja_pii.schema.json','utf8'))"` (exit 0) — schema JSONの構文妥当性を確認(本リポジトリはRust workspaceでnode依存を持たないためajv等の外部検証器は使わない)
- **estimate_rounds**: 2

### T008: DetectionEngine への ja_pii 配線

- **intent**: 設計§7.10.8/付録A.5に従い、ja_pii.enabled/address.enabled/name.enabledの3ビット8状態に応じてaddress::scan/name::scanを呼び出す分岐をDetectionEngine::scanへ追加し、既存のdrop_allowlisted/resolve_overlaps/閾値カットを無改造で再利用する。
- **design_refs**: `impl DetectionEngine`, `ja_pii.enabled`
- **depends_on**: T005, T006, T007
- **changes**: `src/gedoku-detect/src/engine.rs` (modify) — JaPiiConfigフィールド追加とscan()内の条件分岐追加
- **acceptance**:
  - 付録A.5の8状態表(E/A/S)通りにaddress::scan/name::scanの呼び出し有無が決まる
  - E=0の全状態でaddress.enabled/name.enabledの値に関わらず両scanが呼ばれず、既存の警告ログが出る
  - 既存のdrop_allowlisted/resolve_overlaps/min_confidenceカットのコードが変更されずに新カテゴリへも適用される
- **verify**:
  - `cargo test -p gedoku-detect engine::` (exit 0)
- **estimate_rounds**: 2

### T009: confidence較正表の定数化とjp_mynumber衝突回帰テスト

- **intent**: 設計§7.10.7/§7.10.9/付録A.4対抗策#2に従い、confidence値をコード定数として固定し、本文の表と定数を突き合わせるCIテスト(ja_pii_confidence_table_matches_doc)、および丁目/番地表記とjp_mynumberの桁数衝突回帰テストを追加する。
- **design_refs**: `確信度較正表`, `ja_pii_confidence_table_matches_doc`, `addr_full_does_not_collide_with_jp_mynumber_regex`
- **depends_on**: T005, T006
- **changes**:
  - `src/gedoku-detect/src/ja_dict/mod.rs` (modify) — confidence定数をpub(crate)で一元管理
  - `src/gedoku-detect/tests/ja_pii_confidence_table.rs` (add)
- **acceptance**:
  - ja_pii_confidence_table_matches_docが§7.10.7の表(0.50〜0.97、9行)と実装定数の不一致を検知して失敗する
  - addr_full_does_not_collide_with_jp_mynumber_regexが「港区123456789012丁目1番1号」でjp_mynumberとの誤混線がないことを確認する
- **verify**:
  - `cargo test -p gedoku-detect ja_pii_confidence_table_matches_doc` (exit 0)
  - `cargo test -p gedoku-detect addr_full_does_not_collide_with_jp_mynumber_regex` (exit 0)
- **estimate_rounds**: 2

### T010: プレースホルダー生成と normalize() への PII_ADDRESS_JA/PII_NAME_JA 統合

- **intent**: 設計§7.10.10に従い、既存のプレースホルダー形式(⟦GDK:category:ID⟧)とnormalize(value)関数へ新カテゴリ2種の正規化ルール(住所=NFKCのみ、氏名=NFKC+空白単一化)を追加する。
- **design_refs**: `⟦GDK:PII_ADDRESS_JA:<ID>⟧`, `normalize(value)`
- **depends_on**: T008
- **changes**: `src/gedoku-detect/src/detection.rs` (modify) — normalize()にPII_ADDRESS_JA/PII_NAME_JA分岐を追加
- **acceptance**:
  - PII_ADDRESS_JAの正規化はNFKCのみで、丁目/番地の表記ゆれ(4丁目 vs 4-)は別プレースホルダーのままである
  - PII_NAME_JAの正規化は姓名間の空白(半角/全角/複数)を単一の半角スペースへ統一し、「山田　太郎」と「山田 太郎」が同一プレースホルダーIDになる
- **verify**:
  - `cargo test -p gedoku-detect normalize_ja_name_collapses_whitespace` (exit 0)
- **estimate_rounds**: 2

### T011: 監査ログ統合(ja_pii_dict_loaded / ja_pii_dict_load_rejected)

- **intent**: 設計§7.10.11に従い、辞書ロード成功時のja_pii_dict_loadedイベントと、JaDictErrorの全バリアントに対する拒否時のja_pii_dict_load_rejectedイベントを、同一target(gedoku_audit)配下の対称なログとして起動シーケンスに追加する。
- **design_refs**: `ja_pii_dict_loaded`, `ja_pii_dict_load_rejected`
- **depends_on**: T004
- **changes**: `src/gedoku-detect/src/ja_dict/dict.rs` (modify) — load_address_dict/load_surname_dictの成功/失敗経路にtracing::info!/error!を追加
- **acceptance**:
  - 辞書ロード成功時にtarget=gedoku_audit, event=ja_pii_dict_loadedが構造化フィールド(address_entries等)付きで出力される
  - JaDictErrorの全バリアント(Io/InvalidUtf8/MalformedRow/DuplicateKey/EmptyDictionary)でtarget=gedoku_audit, event=ja_pii_dict_load_rejectedが出力される
- **verify**:
  - `cargo test -p gedoku-detect ja_pii_dict_load_rejected` (exit 0)
- **estimate_rounds**: 2

### T012: §7.10.12 単体テスト一式とF1コーパス(ホールドアウト10%)の追加

- **intent**: 設計§7.10.12に従い、P2-T8方式(validator逆算で正例/負例を自動生成しF1をCIゲートにする)を踏襲したコーパステストを新規追加し、学習側・ホールドアウト側双方でF1>=0.90をCIゲートとする。
- **design_refs**: `7.10.12`
- **depends_on**: T008, T009, T010, T011
- **changes**: `src/gedoku-detect/tests/ja_pii_corpus_f1.rs` (add)
- **acceptance**:
  - §7.10.12記載の8個の#[test]関数(ja_addr_full_detected_with_banchi等)が全てgreenである
  - 自動生成コーパスの学習側F1が0.90以上である
  - 辞書に含めないホールドアウト10%側のF1も0.90以上であり、丸暗記による乖離が無いことを確認する
- **verify**:
  - `cargo test -p gedoku-detect --test ja_pii_corpus_f1` (exit 0)
- **estimate_rounds**: 3

### T013: docs/gedoku-proxy-design.md への本文マージと既定値整合確認

- **intent**: 設計冒頭の想定に従い、docs/plans/ja-pii-ahocorasick-design.mdの§7.10.0〜§7.10.13本文のみをdocs/gedoku-proxy-design.md §7へマージし(付録Aはレビュー記録として元ファイルに残す)、既定値一覧とconfigs/gedoku.example.tomlの実際の既定値との整合を最終確認する。
- **design_refs**: `docs/gedoku-proxy-design.md`, `7.10.13`
- **depends_on**: T012
- **changes**: `docs/gedoku-proxy-design.md` (modify) — §7に7.10.0〜7.10.13を追記、付録Aは転記しない
- **acceptance**:
  - docs/gedoku-proxy-design.mdの§7に7.10.0〜7.10.13の本文が追記されている
  - §7.10.13の既定値一覧とconfigs/gedoku.example.tomlの[ja_pii]セクションの実際の既定値が一致する
- **verify**:
  - `grep -q '7.10.13' docs/gedoku-proxy-design.md` (exit 0)
- **estimate_rounds**: 1

## 依存関係順(トポロジカルソート)

T001, T002, T003, T004, T005, T006, T007, T008, T009, T010, T011, T012, T013
