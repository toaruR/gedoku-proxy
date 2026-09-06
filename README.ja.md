# gedoku-proxy（毒抜きLLMプロキシ）

[English](README.md)

### エージェントの通信を解毒する

APIキーやメールアドレスを型の分かるプレースホルダへ自動置換してからアップストリームへ送り、レスポンスで元に戻す透過プロキシ。`ANTHROPIC_BASE_URL` に差し込むだけ。Rust / axum 製。

> 詳細設計は [docs/gedoku-proxy-design.md](docs/gedoku-proxy-design.md)、仕様は [docs/specification.md](docs/specification.md) を参照。

## ステータス

**P0〜P5 完了**: 透過プロキシ骨格、構造保存JSONウォーカー、PII/シークレット検出・置換、ストリーミング（SSE）復元、監査ログ、設定/デプロイをすべて実装済みでCIにも組み込み済み。残存する既知のギャップ（Bedrock/Vertex向けAWS event-streamフレーミング、290秒keepaliveソークテスト、dylintカスタムlint、K8s実クラスタ検証）は[docs/known-gaps.md](docs/known-gaps.md)を参照。

## 構成

```
src/
  gedoku-core/           設定(Config)・エラー型
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
