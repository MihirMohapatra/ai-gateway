# syntax=docker/dockerfile:1
FROM rust:bookworm AS builder

WORKDIR /app
COPY . .

RUN cargo build --release --bin gateway-core

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/gateway-core /app/gateway-core
COPY gateway.toml /app/gateway.toml

EXPOSE 3000

ENTRYPOINT ["/app/gateway-core"]
