# Reviewing Mimicway

This guide is for the engineer asked to approve Mimicway before it runs in their company. It gives a reading order that follows the trust boundaries, the authorization matrix of the API, and commands that check each claim instead of taking it on trust. The security model itself (what is exposed, outbound flows, defaults) is described in [docs/security.md](docs/en/security.md).

## Facts that bound the review

- One Rust binary, no database, no code generation, and no `unsafe` code, tests included (`#![forbid(unsafe_code)]` in `src/lib.rs` and `src/main.rs`). Its one build script, `build.rs`, lists the files of the built UI so that `include_bytes!` embeds them; it reads nothing else, downloads nothing and runs nothing.
- The UI is a static Svelte bundle embedded in the binary and served by `src/server/ui_files.rs`, which answers only the exact files of that bundle; its one runtime dependency is Svelte, whose runtime the compiler puts in the bundle; the rest of `frontend/package.json` is build and test tools.
- Optional features are off by default and not compiled unless requested: `messaging-kafka` (Kafka, pulls `rdkafka` and C code) and `tcp-mock` (raw TCP mocks, no extra dependency).
- Every dependency comes from crates.io or npm, is pinned by a committed lockfile and checked in CI for advisories and licenses.

## Reading order

Read along the path a request takes, from the outside in. Production code lives above the `#[cfg(test)]` line of each file; tests are below it or in a sibling `tests.rs`.

| Order | File | What to check |
|---|---|---|
| 1 | `src/main.rs` | Configuration from the environment, bind address (loopback by default), graceful shutdown on SIGTERM. |
| 2 | `src/server/mod.rs`, `src/server/ui_files.rs`, `build.rs` | Router and middleware order: CORS, security headers, browser guard, authentication, service interception, API routes; the UI files embedded at build time and served by exact name. |
| 3 | `src/server/browser_guard.rs` | CORS per path, refusal of cross-site writes, DNS-rebinding guard, response headers. |
| 4 | `src/auth/middleware.rs`, `src/auth/keycloak.rs` | Which paths require a token; local JWT validation (algorithms, issuer, expiry, client); key-set refresh limits. |
| 5 | `src/server/api.rs` | Authorization of each endpoint (matrix below); validation of every mutation, done under the store's write lock (`try_update`). |
| 6 | `src/server/validation.rs` | Names, reserved routes, dot segments, regular expressions, backup file names (path traversal). |
| 7 | `src/server/intercept.rs`, `src/engine/proxy.rs` | Service traffic: matching, mock rendering, proxying (the only outbound HTTP), timeouts, hop-by-hop headers. |
| 8 | `src/engine/script.rs` | Rhai sandbox: limits, disabled module loading and `eval`, native functions. |
| 9 | `src/engine/template.rs`, `src/engine/renderer.rs`, `src/engine/matcher.rs`, `src/engine/regex_cache.rs` | Response templates, chaos injection, condition evaluation on untrusted input. |
| 10 | `src/store/mod.rs` | Persistence: atomic writes, backups, restore. |
| 11 | `src/server/redaction.rs`, `src/server/request_log.rs`, `src/server/observation.rs`, `src/server/suggestion.rs` | What is kept from traffic, and how credentials are removed. |
| 12 | `src/tcp/`, `src/messaging/` | Only if you enable these features. |

## Authorization matrix

With `AUTH_ENABLED=false` every caller is an anonymous super-admin: this mode is meant for a single developer, which is why the binary listens on loopback by default. With authentication enabled:

| Endpoint | Allowed |
|---|---|
| `GET /api/health`, `GET /api/auth/status`, `POST /api/auth/login`, `POST /api/auth/validate` | Anyone (no token) |
| `GET /api/auth/me`, `POST /api/script/validate`, `POST /api/rule-test`, `POST /api/rule-conflicts` | Any authenticated user (the last three are stateless) |
| `GET /api/config`, `GET /api/services`, `GET /api/logs`, `GET /api/observation/status` | Any authenticated user, filtered to what they can access |
| `PUT /api/config`, `DELETE /api/config/reset`, `GET /api/config/backups`, `POST /api/config/restore/:file` | Super-admins |
| `POST /api/services` | Super-admins anywhere; group admins inside their group |
| `GET`, `PUT` a service; toggle, ping, reorder rules, observe, suggestions | Members and admins of the service's group, super-admins. Moving a service to another group also requires the right to create it there |
| `DELETE` a service | Super-admins |
| `GET /api/groups` | Groups the user belongs to (all for super-admins) |
| `POST /api/groups` | Any authenticated user, who becomes its admin |
| `GET /api/groups/:name` | Its members and admins, super-admins |
| `PUT`, `DELETE /api/groups/:name`, `PUT /api/groups/:name/members` | Its admins, super-admins |
| `/api/tcp/...` (feature `tcp-mock`) | Reading: any authenticated user; changes: super-admins |
| `/api/messaging/...` (feature `messaging-kafka`) | Status: any authenticated user; log and simulation: super-admins |

The tests in `src/server/api/authz_tests.rs` exercise this matrix through the real router with real signed tokens.

## Checking the claims yourself

```bash
# Everything the CI runs (see .github/workflows/ci.yml)
cargo fmt --check
cargo clippy --all-targets --features tcp-mock -- -D warnings
cargo test --features tcp-mock
cargo deny check                       # advisories, licenses, sources
(cd frontend && npm ci && npm test && npm audit)

# Where can the process open an outbound connection?
grep -rn "reqwest::Client\|Client::builder\|TcpStream::connect\|rdkafka" src --include=*.rs

# Where does it touch the file system?
grep -rn "std::fs::\|tokio::fs::" src --include=*.rs

# Any unsafe code, tests included? (the attributes that forbid it, comments and a CSP keyword match)
grep -rn "unsafe" src --include=*.rs

# Which environment variables does it read?
grep -rhn "env::var(\"" src --include=*.rs | grep -o "\"[A-Z_]*\"" | sort -u
```

## Direct dependencies and why

| Crate | Purpose |
|---|---|
| `axum`, `tower`, `tower-http`, `http`, `http-body-util`, `tokio`, `futures-util` | HTTP server, middleware (CORS, static files), async runtime |
| `reqwest` (rustls, no OpenSSL) | Proxy and Keycloak client |
| `jsonwebtoken` | Access token validation |
| `serde`, `serde_json`, `serde_yaml` | API payloads and configuration file |
| `quick-xml` | XPath-like conditions and XML helpers on request bodies |
| `regex` | Regex conditions (linear-time engine, no catastrophic backtracking) |
| `rhai` | Sandboxed rule scripts |
| `fastrand`, `uuid` | Fake data, sequence and request ids |
| `url` | URL parsing and form decoding |
| `tracing`, `tracing-subscriber` | Logs |
| `rdkafka` (optional) | Kafka |

Development only: `proptest` (property tests of the suggestion engine), `ring` and `base64` (signing tokens for the fake Keycloak realm of the tests).

## Out of scope of the code

TLS termination, rate limiting and network egress control are left to the deployment (ingress, service mesh, network policies); [docs/security.md](docs/en/security.md) lists what to configure.
