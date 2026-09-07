<!-- spec-doc:last-reviewed-commit=b00b89583c709295b7c83a9861c706d7ffc97dd2 reviewed-at=2026-09-07 -->

# 仕様書

<!--
このファイルは /spec-doc コマンドが git 差分から自動で追記・更新する。
- 先頭のマーカー行は /spec-doc が「前回どのコミットまで確認したか」を記録するためのもの。
  手で編集しない（初回はマーカーなしのままでもよく、その場合 /spec-doc が初回モードで動く）
- 構成: 対象（機能 / モジュール / API / 設定 など）ごとに節を分ける
- 実装手順ではなく「何が・どう振る舞うか」（仕様・挙動・設計意図）を書く
-->

## 1. システム概要と基本動作

`gedoku-proxy` は、LLM（Anthropic Claude API等）を利用するエージェントとアップストリームサーバーの間に配置される透過型プロキシ（リバースプロキシ）である。

エージェントが外部LLMに送信するプロンプト等のテキストから、APIキー・認証情報・個人識別情報（メールアドレス、電話番号、マイナンバー、クレジットカード番号、日本語の氏名・住所など）を検出し、暗号化・型情報付きプレースホルダ（例: `⟦GDK:SECRET:4XYZ...⟧`）へ自動置換（解毒）する。LLMからのレスポンスを受信した際は、プロキシ内部の暗号化ストアを参照し、プレースホルダを透過的に元のテキストへ復元（毒戻し）してエージェントへ返却する。

エージェント側は環境変数 `ANTHROPIC_BASE_URL` をプロキシのアドレスに向けるだけで、コード変更なしに機密情報の漏洩を防ぐことができる。

```
[Agent / Client]
       │
       ▼ (1) 生のプロンプト送信 (機密情報・氏名等を含む)
[gedoku-proxy]
       ├─► 検出エンジン: 機密情報・PIIを検出
       ├─► マップストア: 原文を暗号化保存し、プレースホルダへ置換
       ▼ (2) 解毒済みプロンプト (⟦GDK:...⟧) 転送
[Upstream LLM (api.anthropic.com)]
       │
       ▼ (3) プレースホルダを含むレスポンス (SSEストリーム / JSON)
[gedoku-proxy]
       ├─► ストリームスキャナ: プレースホルダを検出
       ├─► マップストア: 原文を復号して元の文字列に復元
       ▼ (4) 完全に復元されたレスポンスを返却
[Agent / Client]
```

---

## 2. プロキシ・ルーティング仕様 (`gedoku-server`)

### 2.1 待ち受けとプロキシ転送
- デフォルトで `127.0.0.1:18787` で待ち受け、`https://api.anthropic.com` へリクエストをフォワードする。
- プロセス起動時にOSレベルのメモリダンプ硬化処理 `harden_process_best_effort()` を実行し、Linux環境では `RLIMIT_CORE=0` および `prctl(PR_SET_DUMPABLE, 0)` を設定して異常終了時のコアダンプファイル経由のメモリ漏洩を防止する。

### 2.2 サポートエンドポイント
- **`POST /v1/messages`**
  Anthropic Messages API のメインエンドポイント。リクエストボディのJSONを検査・解毒し、アップストリームへ転送する。ストリーミング（SSE）レスポンスおよび非ストリーミングレスポンスの双方で復元処理を行う。
- **`POST /v1/complete`**
  レガシーな補完API。`/v1/messages` と同様に透過解毒・復元転送を行う。
- **`GET /v1/models`**
  モデル一覧取得エンドポイント。`models_passthrough` 設定が `true` の場合はアップストリームへ透過転送し、`false`（既定）の場合はプロキシ内部でダミーのモデル一覧レスポンスを即時返却する。
- **`GET /healthz`**
  プロキシ自身のヘルスチェック用。常に HTTP 200 OK (`{"status":"ok"}`) を返す。

### 2.3 認証転送モード (`auth_mode`)
アップストリームへの資格情報転送方式を3種類から選択可能:
1. **`passthrough`**（既定）: クライアントが提示した `x-api-key` または `Authorization` ヘッダーをそのままアップストリームへ転送する。
2. **`replace`**: クライアントから渡された資格情報を破棄し、プロキシに設定された `upstream_api_key` または `upstream_auth_token` で強制的に上書きして転送する。
3. **`augment`**: クライアントが資格情報を指定していない場合のみ、プロキシ側の設定値で補完する。

### 2.4 リクエスト制限・通信仕様
- **最大リクエストサイズ**: `max_request_bytes`（既定 32MiB）。
  - 超過時の動作: ADR-9 に基づき、サイズ超過時は fail-open（解毒処理を行わずそのままアップストリームへ転送）してプロキシが原因で通信全体が遮断されるのを防ぐ。
- **検出エラー時の挙動 (`on_detect_error`)**:
  - `fail_closed`（既定）: 検出処理中に予期せぬ内部エラーが発生した場合、アップストリームへの転送を中断しエラーを返す。
  - `fail_open`: 検出失敗時でも未解毒のまま転送を継続する。
- **適用モード (`mode`)**:
  - `enforce`（既定）: 検出された機密情報をプレースホルダへ置換する。
  - `observe`: 検出処理と監査ログ出力・デバッグダンプのみを行い、本文の置換は行わずそのまま転送する（導入前検証用）。
- **接続タイムアウト**: `upstream_connect_timeout_secs`（既定 30秒）。
- **`Accept-Encoding`**: デフォルトで `identity` を送信し、レスポンスがgzip等で圧縮されてストリーム復元処理が阻害されるのを回避する。

---

## 3. 構造保存 JSON ウォーカー (`gedoku-jsonwalk`)

Anthropic API の JSON リクエスト構造を維持しながら、検査対象フィールドのみを正確に置換する。

- **対象フィールド**:
  - `system`: トップレベルのシステムプロンプト文字列、または配列ブロック。
  - `messages[*].content`: ユーザーおよびアシスタントのメッセージ本文。
    - 単純な文字列形式: `"content": "..."`
    - ブロック配列形式: `[{"type": "text", "text": "..."}, {"type": "image", ...}, {"type": "tool_use", ...}, {"type": "tool_result", ...}]`
- **非改変保証**:
  - テキスト以外の浮動小数点数値の桁・精度表現（serde_json の arbitrary_precision 考慮）や未定義キー、配列の順序をそのまま保持して再直列化する。

---

## 4. 検出エンジン仕様 (`gedoku-detect`)

正規表現ルール、情報量エントロピー検知、各種チェックサムバリデータ、および Aho-Corasick 辞書走査を組み合わせた4層アーキテクチャ。

### 4.1 検出カテゴリ (`Category`)
- `SECRET`: 各種APIキー、トークン、秘密鍵、パスワード
- `PII_EMAIL`: メールアドレス
- `PII_PHONE`: 電話番号（国際E.164、国内固定・携帯）
- `PII_CARD`: クレジットカード番号（Luhnチェック必須）
- `PII_ID`: 日本の個人番号（マイナンバー。チェックディジット必須）、米SSNなど
- `PII_ACCOUNT`: IBANなど口座情報
- `NET`: IPv4, IPv6 アドレス
- `PII_PATH`: ローカルファイルパス
- `PII_ADDRESS_JA`: 日本語の住所
- `PII_NAME_JA`: 日本語の氏名

### 4.2 検出レイヤー構造
1. **L1 (正規表現ルール & 文脈判定)**:
   - プレフィックス付きシークレット（`sk-ant-...`, `ghp_...`, `aws_access_key_id`, JWTトークン等）。
   - 代入文脈判定（`bearer_ctx`）: `api_key = "..."`, `token: "..."` などの前置キーワードに続く高エントロピー文字列を検出。素の環境変数形式、JSON形式（`"key": "value"`）、Python辞書形式（`'key': 'value'`）に対応。Rustの構造体型注釈（`token: Option<String>` 等）を誤検出しないための除外ガードを内蔵。
2. **L2 (エントロピー検知)**:
   - 文字列の Shannon エントロピーを計算し、ランダム性の高いトークンを検出。
   - Git commit SHA, UUID などの既知の非機密ランダム文字列はノイズ除外フィルタで弾く。
3. **L3 (バリデータ検証)**:
   - クレジットカード: Luhnアルゴリズムでチェックサムが一致するもののみを `PII_CARD` と判定。
   - マイナンバー: 12桁の数字に対して総務省制定のモジュラス11・チェックディジット計算を行い、一致するもののみを `PII_ID` と判定。
   - メールアドレス: `example.com`, `test.com` 等のドキュメント用予約ドメインを検出対象から除外。
4. **L4 (日本語PII Aho-Corasick辞書・構文走査)**:
   - 詳細は第5節を参照。

### 4.3 確信度評価と競合解決 (`overlap`)
複数のルールが同一または重複するスパンにマッチした場合、以下のルールで単一の検出を採用する:
1. **確信度（Confidence）比較**: 定義済みの確信度較正表に基づき、確信度が高い検出を優先（例: マイナンバー(0.90) > 12桁電話番号(0.80)）。
2. **最長一致**: 確信度が同点の場合は、マッチした文字列長がより長いスパンを優先。
3. **出現順ソート**: 重複しないスパン同士は出現位置（start index）順で保持。

---

## 5. 日本語PII解毒仕様 (`ja_pii`: 氏名・住所)

日本語特有の表記ゆれ、文脈、敬称、地名構造を高速に解析するため、Aho-Corasick アルゴリズムによるオートマトン照合と字種遷移スキャンを統合。誤検知（False Positive）リスクへの配慮から、**デフォルトではマスタースイッチが無効（オプトイン）** となっている。

### 5.1 住所解毒 (`ja_pii.address`)
- **内蔵自治体辞書**:
  全都道府県および市区町村（約1,900自治体、政令指定都市の区を含む）をバイナリ内蔵の `assets/ja/pref_city.tsv` に収録。起動時に Aho-Corasick オートマトンへビルドされる。
- **走査と境界判定**:
  1. 都道府県・市区町村プレフィックスを辞書で高速照合。
  2. 直後に続く町丁目、大字、番地、号、地番、建物名、部屋番号を、漢数字・全角数字・半角数字の正規化（NFKC）、ハイフンバリアントの字種遷移でスキャン。
  3. 番地・号まで含む完全な住所表記（`CONF_ADDR_FULL`: 0.90）を特定し、`⟦GDK:PII_ADDRESS_JA:<ID>⟧` に置換。
- **過剰解毒（誤検知）防止**:
  文脈上単に言及されただけの都道府県名や市区町村名（例: 「東京都の天気」「大阪市に行く」など、番地以降が続かないもの）は `CONF_ADDR_PREF` (0.50) / `CONF_ADDR_CITY` (0.55) と低確信度に留め、デフォルトの置換閾値（0.80）未満として置換対象外にする。
- **拡張辞書**:
  必要に応じて `extra_town_dict_path` にTSVファイルを指定し、町丁目・小字の辞書を追加可能。

### 5.2 氏名解毒 (`ja_pii.name`)
- **外部姓辞書の利用**:
  著作権およびバイナリフットプリント保護のため、一般的な姓データはバイナリに同梱しない。利用時はユーザーが用意したTSVファイル（`surname_dict_path`）をロードして動作する（未指定時は氏名走査は自動的に no-op となる）。
- **敬称およびフルネーム認識**:
  1. **敬称付き姓**: 姓辞書にマッチした単語の直後に敬称（`honorifics`: 既定 `["様", "さん", "氏", "殿", "君", "先生"]`）が続く場合（例: 「田中様」「佐藤さん」）、高確信度（`CONF_NAME_HONORIFIC`: 0.92）で検出。
  2. **フルネーム**: 姓の直後に2〜3文字の漢字（名）が続くパターン（空白区切り含む）を認識し、高確信度（`CONF_NAME_FULL`: 0.90）で検出。
  3. **敬称付きフルネーム**: 姓＋名＋敬称（例: 「山田太郎様」）は最高確信度（`CONF_NAME_FULL_HONORIFIC`: 0.97）で検出。
- **ネガティブリストガード**:
  日常用語・一般名詞と重複しやすい姓（「森」「林」「清水」「日向」など）が単独で出現した場合は、敬称や名が伴わない限り検出しないガード機構を備える。

---

## 6. マップストアと暗号化・プレースホルダ (`gedoku-mapstore`)

置換された機密情報の原文を安全に保持し、レスポンス時に逆引きするためのセキュアストア。

### 6.1 プレースホルダ形式
- **Unicode形式**（既定）: `⟦GDK:<CATEGORY>:<ID>⟧`
  LLMが文章として誤認・改変しにくいよう、数学用特殊二重角括弧（U+27E6, U+27E7）を採用。
- **ASCII形式**（環境変数 `GEDOKU_SENTINEL_ASCII=1` 等）: `<<GDK:<CATEGORY>:<ID>>>`
  Unicode括弧を正常に扱えないクライアント環境向けのフォールバック。

### 6.2 決定論的ID生成
- 同一の入力値（同一カテゴリ・同一レルム）に対しては、常に同じ ID を生成する。これにより、エージェントが複数回同じ変数や名前を参照した場合でも、LLM側で同一のプレースホルダとして一貫した推論が可能となる。
- ID は Crockford's Base32 文字列で構成され、読みやすさとURLセーフ性を両立。

### 6.3 レルム（Realm）分離と暗号化
- **レルム分離戦略 (`realm_strategy`)**:
  - `credential`（既定）: クライアントのAPIキー等のハッシュ値からレルムIDを導出し、クライアントごとに分離されたマッピングストアを保持する（マルチテナント安全性）。
  - `global`: プロセス全体で単一のレルムを共有する。
  - `ephemeral`: リクエスト毎に使い捨てのレルムを生成する。
- **二重鍵導出 (HKDF-SHA256)**:
  プロセス起動時に乱数生成されるマスターキーから、レルムごとに独立した「Pepper鍵」と「AES-256-GCM暗号化鍵」をHKDFにより分離導出。メモリダンプや漏洩時でも別レルムのデータ復号を防止する。

---

## 7. レスポンス復元スキャナ (`gedoku-stream`)

LLMからの出力テキストに含まれる `⟦GDK:<CATEGORY>:<ID>⟧` をスキャンし、元の値に復元する。

### 7.1 ストリーミング復元（SSE / Server-Sent Events）
- LLMのストリーミングレスポンスでは、プレースホルダ文字列が複数チャンクに分断されて送信される場合がある（例: チャンク1末尾が `⟦GDK:PII_NA`, チャンク2先頭が `ME_JA:3K9...⟧`）。
- **`TailHoldScanner`（テールホールドスキャナ）**:
  プレースホルダの開始マーカー `⟦GDK:` を検知した際、終端マーカー `⟧` を受信するまで該当文字列のフラッシュを一時保留（ホールド）し、次チャンクと結合して完全なIDが揃った時点で一括して復号・置換してクライアントへ流す。
- プレースホルダとして不完全なままタイムアウトまたはストリーム終端に達した場合は、安全にバッファの内容をそのまま出力する。

### 7.2 非ストリーミング復元
- レスポンス全体のJSON（`content[*].text` 等）に対してテキスト走査を行い、検出された全プレースホルダを暗号化ストアから逆引きして元データへ一括置換する。

---

## 8. 監査ログ仕様 (`gedoku-audit`)

機密情報の保護要件を満たしながら、検出・置換のトレーサビリティを確保するゼロナレッジ監査ロギング。

- **ゼロナレッジ設計**:
  監査ログには、機密情報や個人情報の**生テキストは一切記録しない**。記録されるのは、イベント種別、タイムスタンプ、レルム識別子（ハッシュ）、検出カテゴリ、プレースホルダID、確信度スコア、置換前後のおおよその文字数のみ。
- **改ざん検知署名**:
  各ログエントリには HMAC-SHA256 署名を付与可能。
- **主なイベント種別**:
  - `redaction_applied`: リクエスト本文への置換適用
  - `restoration_applied`: レスポンス本文への復元適用
  - `ja_pii_dict_loaded`: 日本語PII辞書のロード完了（収録件数記録）
  - `ja_pii_dict_load_rejected`: 日本語PII辞書ロード失敗・拒否（fail-closed）
- **置換デバッグダンプ (`debug_dump_replacements`)**:
  開発・テスト環境向けのオプション。有効時、原文を含めずに「カテゴリ」「プレースホルダID」「前後のコンテキスト単語」のみを `gedoku-replacements-debug.log` に出力し、意図しない誤検知の調査を支援する。

---

## 9. 設定・環境変数仕様 (`gedoku-core`)

設定の読み込み順序は以下の優先順位に従う（最初に見つかったものが優先）:
1. コマンドライン引数: `--config <path>` または `-c <path>`
2. 環境変数: `GEDOKU_CONFIG_FILE`
3. プロセスのカレントディレクトリに存在する `configs/gedoku.toml`
4. 個別の環境変数およびデフォルト値

### 9.1 設定項目一覧 (`configs/gedoku.toml`)

```toml
# プロキシ待ち受けアドレス
listen_addr = "127.0.0.1:18787"

# 転送先アップストリームURL
upstream_base_url = "https://api.anthropic.com"

# 認証転送モード: "passthrough" | "replace" | "augment"
auth_mode = "passthrough"
# upstream_auth_token = "sk-ant-..."
# upstream_api_key = "sk-ant-..."

# 通信設定
upstream_accept_encoding = "identity"
max_request_bytes = 33554432
models_passthrough = false
upstream_connect_timeout_secs = 30

# 障害時動作: "fail_closed" | "fail_open"
on_detect_error = "fail_closed"

# レルム戦略: "credential" | "global" | "ephemeral"
realm_strategy = "credential"

# 動作モード: "enforce" | "observe"
mode = "enforce"

# デバッグ用置換ログ出力
debug_dump_replacements = false

# アップストリーム追加ヘッダー
[[extra_upstream_headers]]
name = "x-proxy-by"
value = "gedoku"

# 日本語PII検出 (オプトイン)
[ja_pii]
enabled = false

[ja_pii.address]
enabled = false
dict_path = ""             # 内蔵 pref_city.tsv を利用 (空文字)
extra_town_dict_path = ""  # 追加町丁目辞書 (TSV形式)

[ja_pii.name]
enabled = false
surname_dict_path = ""     # 姓辞書 (TSV形式、指定必須)
honorifics = ["様", "さん", "氏", "殿", "君", "先生"]
```

### 9.2 環境変数対応表

| 環境変数 | 対応TOML設定 | 既定値 | 説明 |
|---|---|---|---|
| `GEDOKU_CONFIG_FILE` | - | なし | TOML設定ファイルのパス |
| `GEDOKU_LISTEN_ADDR` | `listen_addr` | `127.0.0.1:18787` | 待ち受けアドレス |
| `GEDOKU_UPSTREAM_BASE_URL` | `upstream_base_url` | `https://api.anthropic.com` | 転送先ベースURL |
| `GEDOKU_UPSTREAM_AUTH_MODE` | `auth_mode` | `passthrough` | 認証転送モード |
| `GEDOKU_UPSTREAM_AUTH_TOKEN` | `upstream_auth_token` | なし | 転送用Bearerトークン |
| `GEDOKU_UPSTREAM_API_KEY` | `upstream_api_key` | なし | 転送用x-api-key |
| `GEDOKU_EXTRA_UPSTREAM_HEADERS` | `extra_upstream_headers` | なし | 追加ヘッダー (`K: V, K2: V2`) |
| `GEDOKU_UPSTREAM_ACCEPT_ENCODING` | `upstream_accept_encoding`| `identity` | 送信Accept-Encoding |
| `GEDOKU_MAX_REQUEST_BYTES` | `max_request_bytes` | `33554432` | リクエストサイズ上限 |
| `GEDOKU_MODELS_PASSTHROUGH` | `models_passthrough` | `false` | /v1/models の透過可否 |
| `GEDOKU_UPSTREAM_CONNECT_TIMEOUT_SECS` | `upstream_connect_timeout_secs` | `30` | 接続タイムアウト(秒) |
