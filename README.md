# AI Gateway

[![Rust](https://img.shields.io/badge/rust-1.85%2B-blue?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![CI](https://github.com/MihirMohapatra/ai-gateway/actions/workflows/ci.yml/badge.svg)](https://github.com/MihirMohapatra/ai-gateway/.github/workflows/rust.yml)

A **unified API gateway** for multiple LLM providers (OpenAI, Anthropic, and more) with built-in resilience, caching, observability, and hot-reloadable configuration.

---

## Features

- **Multi-provider routing** — Route requests to OpenAI, Anthropic, or custom providers by model name
- **Streaming SSE** — Proxy streaming responses directly to clients via Server-Sent Events
- **Resilience** — Retry with exponential backoff, circuit breaker pattern, rate-limit handling
- **Semantic caching** — SHA-256 hashed requests cached in Redis or local in-memory store
- **Cost metering** — Track prompt/completion tokens per API key with pluggable backends (console, Postgres)
- **Content guardrails** — Block/require regex patterns on request/response, max length enforcement
- **Response normalization** — All provider responses mapped to a unified `GatewayResponse` schema
- **Observability** — Prometheus metrics (`/metrics`), structured JSON logs, OpenTelemetry tracing, per-request spans
- **Hot-reload config** — Update `gateway.toml` at runtime; routing rules reload without restart
- **Authentication** — JWT-based auth layer, rate limiting via token bucket

---

## Architecture

```
<img src="docs/architecture.svg" alt="AI Gateway Architecture" width="800">

### Workspace layout

| Crate | Purpose |
|---|---|
| `gateway-core` | HTTP server, routes, metrics, TLS — the binary |
| `adapters` | `ProviderAdapter` trait + wrappers (retry, circuit breaker, caching, metering, guardrails) + OpenAI/Anthropic implementations |
| `middleware` | Auth, rate limiting, telemetry, tracing layers |
| `cache` | `RedisCache` and `LocalCache` backends |
| `config` | TOML config parser, env overrides, hot-reload file watcher |

---

## Quick start

### Prerequisites

- Rust 1.85+
- (Optional) Redis — for production caching
- (Optional) Postgres — for cost metering

### 1. Clone & build

```bash
git clone https://github.com/MihirMohapatra/ai-gateway.git
cd ai-gateway
cargo build --release
```

### 2. Configure

Copy the example config and set your API keys:

```bash
cp gateway.toml gateway.toml
# Edit gateway.toml with your keys, or use env vars:
export GATEWAY__PROVIDERS__OPENAI__API_KEY="sk-..."
export GATEWAY__PROVIDERS__ANTHROPIC__API_KEY="sk-ant-..."
```

### 3. Run

```bash
# from the project root (reads gateway.toml by default):
cargo run --release

# or specify a custom config path:
GATEWAY_CONFIG=/etc/ai-gateway/prod.toml cargo run --release
```

### 4. Test

```bash
# Health check
curl http://localhost:3000/health

# Chat completion
curl -X POST http://localhost:3000/chat \
  -H "Authorization: Bearer $(your-jwt)" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Streaming chat
curl -N -X POST http://localhost:3000/chat/stream \
  -H "Authorization: Bearer $(your-jwt)" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Tell me a story"}]
  }'

# Metrics
curl http://localhost:3000/metrics

# Root
curl http://localhost:3000/
```

---

## Configuration

The gateway is configured via `gateway.toml` with environment variable overrides.

### Example `gateway.toml`

```toml
[server]
host = "0.0.0.0"
port = 3000

[auth]
jwt_secret = "change-me-in-production"
enabled = true

[providers.openai]
api_key = "sk-..."
base_url = "https://api.openai.com"

[providers.anthropic]
api_key = "sk-ant-..."
base_url = "https://api.anthropic.com"

[cache]
redis_url = "redis://localhost:6379"
ttl_secs = 3600

[guardrails]
max_input_chars = 100000
max_output_chars = 100000
blocked_patterns = []
required_patterns = []

[metering]
database_url = "postgres://user:pass@localhost:5432/ai_gateway"

[routing]
default_provider = "openai"
```

### Environment variable overrides

All config fields can be overridden with `GATEWAY__SECTION__KEY` format:

| Env var | Overrides |
|---|---|
| `GATEWAY__SERVER__HOST` | `server.host` |
| `GATEWAY__SERVER__PORT` | `server.port` |
| `GATEWAY__AUTH__JWT_SECRET` | `auth.jwt_secret` |
| `GATEWAY__PROVIDERS__OPENAI__API_KEY` | `providers.openai.api_key` |
| `GATEWAY__PROVIDERS__OPENAI__BASE_URL` | `providers.openai.base_url` |
| `GATEWAY__PROVIDERS__ANTHROPIC__API_KEY` | `providers.anthropic.api_key` |
| `GATEWAY__PROVIDERS__ANTHROPIC__BASE_URL` | `providers.anthropic.base_url` |
| `GATEWAY__CACHE__REDIS_URL` | `cache.redis_url` |
| `GATEWAY__METERING__DATABASE_URL` | `metering.database_url` |

### Hot reload

Edit `gateway.toml` at runtime — the gateway detects file changes via `notify` and swaps the routing pipeline atomically. In-flight requests complete with the old configuration; new requests use the updated one.

---

## API endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/` | Root health check |
| `GET` | `/health` | Health check |
| `GET` | `/metrics` | Prometheus metrics |
| `POST` | `/chat` | Chat completion (non-streaming) |
| `POST` | `/chat/stream` | Chat completion (SSE streaming) |

### POST /chat

**Request body:**
```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": false,
  "max_tokens": 1024,
  "temperature": 0.7
}
```

**Response (`GatewayResponse`):**
```json
{
  "id": "chatcmpl-...",
  "model": "gpt-4",
  "provider": "openai",
  "content": "Hello! How can I help you today?",
  "finish_reason": "stop",
  "usage": {
    "prompt_tokens": 25,
    "completion_tokens": 9,
    "total_tokens": 34
  },
  "latency_ms": 1234,
  "cached": false,
  "created": 1700000000
}
```

---

## Deployment

### Docker

```bash
# Build
docker build -t ai-gateway .

# Run with Redis + Postgres
docker-compose up
```

The `Dockerfile` uses a multi-stage build:

1. **Builder** stage — `rust:bookworm` compiles the release binary
2. **Runtime** stage — `debian:bookworm-slim` with only `ca-certificates` (final image ≈ 20 MB)

### docker-compose

```bash
# Set your keys
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export JWT_SECRET="your-secret"

# Start all services
docker-compose up -d

# View logs
docker-compose logs -f gateway
```

The compose file includes:
- `gateway` — the AI Gateway on port 3000
- `redis:7-alpine` — caching backend (with health check)
- `postgres:16-alpine` — metering database (with health check)

---

## Prometheus metrics

Available at `GET /metrics`:

| Metric | Type | Labels | Description |
|---|---|---|---|
| `ai_gateway_requests_total` | Counter | `provider`, `model`, `status` | Total requests |
| `ai_gateway_request_duration_seconds` | Histogram | `provider`, `model` | Latency (buckets: 5ms–10s) |
| `ai_gateway_tokens_total` | Counter | `provider`, `type` | Token usage (prompt/completion) |
| `ai_gateway_cache_operations_total` | Counter | `type` | Cache hits/misses |
| `ai_gateway_errors_total` | Counter | `provider`, `error_type` | Error count |
| `ai_gateway_requests_in_flight` | Gauge | — | Active requests |

A **Grafana dashboard** template is available at [`grafana/dashboard.json`](grafana/dashboard.json) with panels for request rate, latency p50/p95/p99, error ratio, token usage, cache hit ratio, and estimated cost/hour.

---

## Development

### Running tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p adapters
cargo test -p gateway-core

# With logging
RUST_LOG=debug cargo test
```

### Project phases

| Phase | Feature | Branch |
|---|---|---|
| 1 | Project scaffold, workspace | `main` |
| 2 | Request ingestion, `ChatCompletionRequest` | `phase-2-ingestion` |
| 3 | Middleware pipeline (auth, rate limit, telemetry) | `phase-3-middleware` |
| 4 | Multi-provider routing, retry, circuit breaker | `phase-4-routing` |
| 5 | `ProviderAdapter` trait, streaming SSE | `phase-5-adapters` |
| 6 | Semantic caching, cost metering, guardrails, response normalization | `phase-6` |
| 7 | Prometheus metrics, tracing spans, JSON logs, Grafana dashboard | `phase-7` |
| 8 | `gateway.toml` config, env overrides, hot reload, Docker, docker-compose | `phase-8` |

### Code structure

```
crates/
├── adapters/          # ProviderAdapter trait, OpenAI/Anthropic clients, wrappers
├── cache/             # RedisCache and LocalCache backends
├── config/            # gateway.toml parser, env overrides, hot reload
├── gateway-core/      # Binary: axum server, routes, metrics, TLS
└── middleware/         # Auth, rate limit, telemetry, tracing layers
```

### Key dependencies

- **axum** — HTTP framework
- **tower / tower-http** — Middleware pipeline, tracing
- **reqwest** — HTTP client for provider APIs
- **redis** — Caching backend
- **sqlx** — Postgres metering backend
- **prometheus** — Metrics exposition
- **opentelemetry** — Distributed tracing (OTLP)
- **notify** — File watching for hot reload
- **sha2** — Cache key hashing
- **jsonwebtoken** — JWT authentication
- **governor** — Rate limiting

---

## License

MIT License — see [LICENSE](LICENSE) for details.
