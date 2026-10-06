# Architecture

How Mimicway is built: its parts, the path a request takes through them, and the properties the design keeps. It is written for contributors and reviewers; [REVIEWING.md](REVIEWING.md) gives a reading order along the trust boundaries, and the [security model](docs/en/security.md) lists what is exposed and every outbound flow.

## One process

Mimicway is one Rust program (`src/`, built with Cargo) that also serves its web interface (`frontend/`, Svelte 5, built with Vite) on the same HTTP port. There is no database, no agent and no second process: the configuration lives in memory and in one YAML file under `DATA_PATH`. `build.rs` embeds the built interface in the binary, so one file is a complete installation.

```
 browser (interface, /api)          applications under test (/{service}/...)
            \                                   /
             v                                 v
   HTTP port: CORS > language > security headers > browser guard > authentication > interception
             |                                 |
    management API (server/api.rs)    service traffic (server/intercept.rs)
             |                                 |
    validation, authorization         matcher > mock (renderer, templates, scripts)
             |                                 |      or proxy (engine/proxy.rs) ----> real backend
             v                                 v
    store: snapshot in memory,        request log, traffic observation (memory only)
    YAML file and backups on disk
```

## Components

| Part | Where | Role |
|---|---|---|
| Startup | `src/main.rs` | Reads the environment, loads the store, builds the router and serves it; on SIGTERM, stops taking requests and writes the pending changes before exiting. |
| Router | `src/server/mod.rs` | The middleware, in the order a request meets them: CORS, the language of the request (`i18n.rs`), security headers and the browser guard (`browser_guard.rs`), authentication (`auth/middleware.rs`), then interception of service traffic; behind them, `/runtime-config.json`, the management API under `/api` and the interface files (`ui_files.rs`). |
| Management API | `src/server/api.rs` | Every `/api` endpoint, each with its authorization check; `validation.rs` checks names, reserved routes, paths and expressions before anything is stored. |
| Service traffic | `src/server/intercept.rs` | Finds the service from the path (`/{name}/...` or `/{group code}/{name}/...`), asks the matcher for the first rule that applies, then renders the mock response or forwards the request. |
| Engine | `src/engine/` | `matcher.rs` evaluates conditions on paths, query, headers, JSON, XML and forms; `renderer.rs` and `template.rs` build responses (fake data, chaos); `script.rs` runs Rhai scripts in a sandbox; `proxy.rs` is the HTTP client of proxying and of the availability check (`server/ping.rs`). |
| Store | `src/store/` | The configuration (services and groups): reads take a snapshot, changes swap in a modified copy, and a background task writes the YAML file; backups before each overwrite. |
| Authentication | `src/auth/` | Local validation of Keycloak tokens against the realm's keys (`keycloak.rs`), group permissions (`mod.rs`). Off unless `AUTH_ENABLED=true`. |
| Observation and suggestions | `src/server/observation.rs`, `suggestion.rs` | On demand, bounded copies of what a proxied service exchanges, and the rules suggested from them; memory only. |
| Request log | `src/server/request_log.rs`, `redaction.rs` | The last 200 requests, in memory, credential headers replaced by `[redacted]`. |
| Settings | `src/settings.rs`, the `*_in` functions next to each setting | Reading environment variables through a lookup, so that tests pass their own values. |
| Messages | `src/i18n.rs`, `src/locales/` | Server messages in the request's language. |
| Kafka (optional) | `src/messaging/` | Consumer, publisher and message log; compiled only with the `messaging-kafka` feature. |
| Raw TCP mocks (optional) | `src/tcp/` | Listeners on their own ports, mock only, with their own file `tcp-config.yaml`; compiled only with the `tcp-mock` feature. |
| Interface | `frontend/src/` | `App.svelte` and the components of `lib/components/`; `lib/api.js` is the one client of the management API; `lib/i18n.svelte.js` and `locales/` translate; `tokens.css` and `app.css` hold the design system ([frontend/design-system.md](frontend/design-system.md)). |

## A request to a service

1. The middleware stack checks nothing that belongs to the application under test: service traffic needs no Mimicway token and keeps its own headers.
2. `intercept.rs` resolves the service from the first path segments. A service in proxy mode forwards the request as it comes and streams the backend's response back; the steps below apply to a mocked service.
3. The body is buffered (10 MiB at most) and the matcher tries the service's rules in order: method, sub-path, then the conditions (every one of `all_of`, and one of `any_of` when it has any). The first rule that matches answers.
4. The rule renders its response from its template, through its scripts and chaos settings when it has some, or forwards the request when its action is `proxy`. Without a matching rule, the service answers 404.
5. The request log records the outcome; when a user observes a service in proxy mode, a bounded copy of each exchange is kept for suggestions.

## A change through the management API

1. Authentication (when enabled) identifies the user; the handler checks that user's right on the service or group concerned.
2. The new configuration is validated against the current one under the store's write lock (`try_update`), so two changes cannot interleave.
3. Under the same lock, the previous file is copied to `backups/` (rotated, `BACKUP_MAX_COUNT`) and the new snapshot is swapped in: every later request sees it at once.
4. The YAML file is written by the background writer (temporary file, then rename), never by the request.

## Properties the design keeps

- **No request waits on disk.** Reads use the current snapshot, writes go through a bounded queue (64 jobs) to one writer task.
- **Memory is bounded.** Request log, observation, message log and queues all have fixed or configurable limits; bodies kept for display are truncated.
- **Three places open a connection.** `engine/proxy.rs` (proxy and availability check), `auth/keycloak.rs` and `messaging/`; `tcp/` only listens.
- **No `unsafe` Rust**, tests included, and no code generation besides the table of embedded interface files that `build.rs` writes.
- **One source of truth per message**: an English sentence written where it is used, translated by catalogues that tests keep complete.

## Tests

| Kind | Where | Run with |
|---|---|---|
| Rust unit and router tests | Below `#[cfg(test)]` in each file, or a sibling `tests.rs` | `cargo test`, `cargo test --features tcp-mock` |
| Rust integration test | `tests/graceful_shutdown.rs` | `cargo test` |
| Interface unit and property tests | `frontend/src/tests/` | `npm test` |
| End-to-end tests | `frontend/e2e/` | `npm run test:e2e`, against a running binary |
| Fuzz targets | `fuzz/`, entry points in `src/fuzzing.rs` | `cargo +nightly fuzz run <target>` |
| Repository checks | `scripts/*.test.mjs` | `node --test scripts/*.test.mjs` |
