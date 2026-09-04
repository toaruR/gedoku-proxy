# gedoku-proxy

[日本語](README.ja.md)

### Detoxify what you send your agent

A transparent proxy that auto-redacts API keys and emails into type-aware placeholders before they reach the upstream, then restores them in the response. Just point `ANTHROPIC_BASE_URL` at it. Rust / axum.

> See [docs/gedoku-proxy-design.md](docs/gedoku-proxy-design.md) for the detailed design and [docs/specification.md](docs/specification.md) for the spec.

## Status

**P0–P5 complete**: transparent proxy skeleton, structure-preserving JSON walking, PII/secret detection & substitution, streaming (SSE) restoration, audit logging, and config/deployment are all implemented and covered by CI. Known remaining gaps (Bedrock/Vertex AWS event-stream framing, a 290s keepalive soak test, dylint custom lints, and real K8s cluster validation) are tracked in [docs/known-gaps.md](docs/known-gaps.md).

## Layout

```
src/
  gedoku-core/           config (Config), error types
  gedoku-server/         axum handlers / bin crate gedoku-proxy
  gedoku-mock-upstream/  fake Anthropic server for testing
```

## Build & Run

```bash
cargo build --workspace
cargo run -p gedoku-server --bin gedoku-proxy
```

Listens on `127.0.0.1:18787` by default and forwards to `https://api.anthropic.com`.

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:18787
export ANTHROPIC_AUTH_TOKEN=sk-...   # reuse your existing credentials
```

## Config File

Settings can also come from a TOML file instead of environment variables. Resolution order (first match wins):

1. `--config <path>` / `-c <path>` CLI argument
2. `GEDOKU_CONFIG_FILE` environment variable
3. `configs/gedoku.toml`, if it exists in the current directory
4. Environment variables (see below), each falling back to its default

```bash
cargo run -p gedoku-server --bin gedoku-proxy -- --config configs/gedoku.toml
```

See [configs/gedoku.example.toml](configs/gedoku.example.toml) for all available keys. Unknown keys or type mismatches make the process fail to start.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `GEDOKU_CONFIG_FILE` | none | Path to a TOML config file (see Config File above) |
| `GEDOKU_LISTEN_ADDR` | `127.0.0.1:18787` | Listen address |
| `GEDOKU_UPSTREAM_BASE_URL` | `https://api.anthropic.com` | Upstream base URL |
| `GEDOKU_UPSTREAM_AUTH_MODE` | `passthrough` | Credential forwarding mode: `passthrough` / `replace` / `augment` |
| `GEDOKU_UPSTREAM_AUTH_TOKEN` | none | Token used in `replace`/`augment` mode |
| `GEDOKU_UPSTREAM_API_KEY` | none | Same, for `x-api-key` |
| `GEDOKU_EXTRA_UPSTREAM_HEADERS` | none | Extra headers, format: `Name: Value, Name2: Value2` |
| `GEDOKU_UPSTREAM_ACCEPT_ENCODING` | `identity` | `Accept-Encoding` sent to upstream |
| `GEDOKU_MAX_REQUEST_BYTES` | `33554432` (32MiB) | Max request body size |
| `GEDOKU_MODELS_PASSTHROUGH` | `false` | Whether to pass `/v1/models` through to upstream |
| `GEDOKU_UPSTREAM_CONNECT_TIMEOUT_SECS` | `30` | Upstream connect timeout |

## Testing

```bash
cargo test --workspace -j 1
cargo clippy --workspace --all-targets -j 1
```

> On Windows, running `cargo test --workspace` in parallel can fail with a paging file error (os error 1455). Use `-j 1` to work around it.

## License

MIT
