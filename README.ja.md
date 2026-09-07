# gedoku-proxy（毒抜きLLMプロキシ）

[English](README.md)

### エージェントの通信を解毒する

APIキーやメールアドレス、日本語の氏名・住所（オプトイン）を型の分かるプレースホルダへ自動置換してからアップストリームへ送り、レスポンスで元に戻す透過プロキシ。`ANTHROPIC_BASE_URL` に差し込むだけ。Rust / axum 製。

> 詳細設計は [docs/gedoku-proxy-design.md](docs/gedoku-proxy-design.md)、仕様は [docs/specification.md](docs/specification.md) を参照。

## ステータス

**P0〜P5 完了**: 透過プロキシ骨格、構造保存JSONウォーカー、PII/シークレット検出・置換、ストリーミング（SSE）復元、監査ログ、設定/デプロイをすべて実装済みでCIにも組み込み済み。また、Aho-Corasickに基づく日本語PII（氏名・住所）検出・解毒エンジンをサポートしています。残存する既知のギャップ（Bedrock/Vertex向けAWS event-streamフレーミング、290秒keepaliveソークテスト、dylintカスタムlint、K8s実クラスタ検証）は[docs/known-gaps.md](docs/known-gaps.md)を参照。

## 構成

```
src/
  gedoku-core/           設定(Config)・エラー型
  gedoku-detect/         シークレット/PII/日本語氏名・住所検出エンジン
  gedoku-mapstore/       暗号化マッピングストア・プレースホルダ生成
  gedoku-stream/         SSEストリーミング復元スキャナ
  gedoku-jsonwalk/       構造保存JSONウォーカー
  gedoku-audit/          HMAC監査ロギング
  gedoku-server/         axumハンドラ / binクレート gedoku-proxy
  gedoku-mock-upstream/  テスト用の偽Anthropicサーバ
```

## ビルド・実行

```bash
cargo build --workspace
cargo run -p gedoku-server --bin gedoku-proxy
```

デフォルトで `127.0.0.1:18787` で待ち受け、`https://api.anthropic.com` へ転送する。

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:18787
export ANTHROPIC_AUTH_TOKEN=sk-...   # 既存の資格情報をそのまま使用
```

## 設定ファイル

環境変数の代わりにTOMLファイルでも設定できる。解決順（先に見つかったものが優先）:

1. `--config <path>` / `-c <path>` 起動引数
2. `GEDOKU_CONFIG_FILE` 環境変数
3. カレントディレクトリに `configs/gedoku.toml` があればそれ
4. 環境変数群（下表、未指定項目は既定値）

```bash
cargo run -p gedoku-server --bin gedoku-proxy -- --config configs/gedoku.toml
```

設定可能な全項目は [configs/gedoku.example.toml](configs/gedoku.example.toml) を参照。未知キーや型不一致がある場合は起動に失敗する。

## 日本語PII解毒（氏名・住所）

Aho-Corasick アルゴリズムとヒューリスティック判定を組み合わせ、日本語の氏名・住所を高速に検出しプレースホルダへ置換（レスポンス時に透過復元）します。誤検知（False Positive）リスクを避けるため、**デフォルトでは無効（オプトイン）** になっています。有効化するには `configs/gedoku.toml` にて設定します。

### 住所解毒 (`ja_pii.address`)
- **内蔵辞書**: 全国の都道府県・市区町村（約1,900自治体）を内蔵バイナリに同梱。
- **完全住所検出**: 「東京都渋谷区...」のような自治体名に続く町丁目・番地・号・建物名・部屋番号を正規表現および字種遷移でスキャンし、`⟦GDK:PII_ADDRESS_JA:...⟧` へ置換。
- **誤検知低減**: 一般的な地名の言及（番地を伴わない「大阪市」「東京都」単体など）は意図的に置換対象外とし、過剰な解毒を防ぎます。
- **拡張**: 必要に応じて `extra_town_dict_path` で町丁目辞書を追加可能。

### 氏名解毒 (`ja_pii.name`)
- **辞書指定**: ライセンス・データ容量の観点から姓データは同梱していません。利用時はユーザー側でTSV形式の姓辞書を用意し、`surname_dict_path` に指定します。
- **高精度判定**: 敬称（`honorifics`: `様`・`さん`・`氏`・`先生` など）を伴う呼びかけやフルネーム（姓＋名）パターンを認識し、`⟦GDK:PII_NAME_JA:...⟧` へ置換。
- **ネガティブリストガード**: 一般名詞と重複しやすい姓の単独出現による誤検知を抑制。

### 設定例 (`configs/gedoku.toml`)

```toml
[ja_pii]
enabled = true

[ja_pii.address]
enabled = true
# extra_town_dict_path = "path/to/towns.tsv"  # 追加町丁目辞書（任意）

[ja_pii.name]
enabled = true
surname_dict_path = "path/to/surnames.tsv"    # 姓辞書（TSV形式、必須）
honorifics = ["様", "さん", "氏", "殿", "君", "先生"]  # 検出対象敬称
```

## 環境変数

| 変数 | 既定値 | 説明 |
|---|---|---|
| `GEDOKU_CONFIG_FILE` | なし | TOML設定ファイルのパス（上記「設定ファイル」参照） |
| `GEDOKU_LISTEN_ADDR` | `127.0.0.1:18787` | 待ち受けアドレス |
| `GEDOKU_UPSTREAM_BASE_URL` | `https://api.anthropic.com` | 転送先ベースURL |
| `GEDOKU_UPSTREAM_AUTH_MODE` | `passthrough` | 資格情報転送モード: `passthrough` / `replace` / `augment` |
| `GEDOKU_UPSTREAM_AUTH_TOKEN` | なし | `replace`/`augment` 時に使うトークン |
| `GEDOKU_UPSTREAM_API_KEY` | なし | 同上（`x-api-key`用） |
| `GEDOKU_EXTRA_UPSTREAM_HEADERS` | なし | 追加ヘッダー。`Name: Value, Name2: Value2` 形式 |
| `GEDOKU_UPSTREAM_ACCEPT_ENCODING` | `identity` | アップストリームへの `Accept-Encoding` |
| `GEDOKU_MAX_REQUEST_BYTES` | `33554432`（32MiB） | リクエストボディ上限 |
| `GEDOKU_MODELS_PASSTHROUGH` | `false` | `/v1/models` をアップストリームへ到達させるか |
| `GEDOKU_UPSTREAM_CONNECT_TIMEOUT_SECS` | `30` | アップストリーム接続タイムアウト |

## テスト

```bash
cargo test --workspace -j 1
cargo clippy --workspace --all-targets -j 1
```

> Windows環境では `cargo test --workspace` を並列実行するとページングファイル不足（os error 1455）で失敗することがある。`-j 1` で回避する。

## ライセンス

MIT
