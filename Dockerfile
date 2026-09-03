# 計画 P5-T5: 単一バイナリ配布 (musl静的リンク)。
# `ldd` で動的リンク依存が最小限であることを確認するのが DoD。
FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl -p gedoku-server

FROM scratch
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/gedoku-proxy /gedoku-proxy
ENTRYPOINT ["/gedoku-proxy"]
