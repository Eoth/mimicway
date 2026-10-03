[Français](README.fr.md)

# Mimicway

[![CI](https://github.com/Eoth/mimicway/actions/workflows/ci.yml/badge.svg)](https://github.com/Eoth/mimicway/actions/workflows/ci.yml) [![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/Eoth/mimicway/badge)](https://scorecard.dev/viewer/?uri=github.com/Eoth/mimicway)

Mock or proxy any HTTP API, one rule at a time, from a web UI. A single self-contained binary: no database, no agent, no cloud account.

Mimicway sits between the application you test and the services it calls. Each service you declare gets its own URL namespace on Mimicway; every request that reaches it is either answered by a rule you wrote (fixed, templated or scripted response, optional latency and errors) or forwarded to the real backend. Everything is changed live from the UI or the REST API, without restarting anything.

- **Mock and proxy side by side**: a whole service, or a single rule, can switch between a mocked answer and the real backend. Mock only the calls you need; let the rest through.
- **Rules that read the request**: conditions on path parameters, query, headers, JSON body, XML body (XPath, SOAP) and form fields, combined with AND/OR.
- **Responses that look real**: templates with request values and fake data, a visual JSON/XML builder, optional Rhai scripts for computed or deterministic values, chaos mode for latency and errors.
- **From real traffic to mocks**: observe what a proxied service actually returns and accept suggested rules instead of writing them by hand.
- **Built to be audited**: no telemetry, four documented outbound flows, local-only by default, no `unsafe` code, dependencies checked in CI. See [Trust at a glance](#trust-at-a-glance).

> Mimicway was called lightMock until version 0.1. Upgrading keeps your data, URLs and settings; [MIGRATING.md](MIGRATING.md) lists the few names to update.

The interface is available in English and French; adding a language is one file (see [Translations](#translations)).

![The list of services in the Mimicway UI](docs/en/screenshots/home-service-list.png)

## Trust at a glance

Mimicway is meant for corporate networks where every outbound flow has to be justified. The short version:

| Question | Answer |
|---|---|
| Does it phone home? | No. There is no telemetry, no update check and no vendor server. |
| What does it connect to? | Only what you configure: the backend of a proxied service, the TCP check of that backend when you click "Test", your Keycloak if authentication is on, your Kafka brokers if that feature is compiled in and enabled. |
| Who can reach it? | By default it listens on `127.0.0.1` only. Containers opt in to every interface with `BIND_ADDRESS=0.0.0.0`. |
| Can a web page drive it? | No. The management API refuses cross-site writes, foreign CORS origins and, when local-only, requests addressed to a non-loopback name (DNS rebinding). |
| Does it keep secrets from the traffic? | No. `Authorization`, cookies and API-key headers are replaced by `[redacted]` in logs, observation and suggestions; URLs lose their credentials. |
| Can a script escape? | Rhai scripts have no file, network or `eval` access and are bounded in operations, string size and depth. |
| Unsafe code? | `#![forbid(unsafe_code)]` outside tests. |
| Supply chain? | `Cargo.lock` and `package-lock.json` committed, CI actions and base images pinned by digest; `cargo-deny` (advisories, licenses, sources), `npm audit`, Trivy and gitleaks run in CI. Releases ship SBOMs, build provenance attestations and a signed image. |

Details, with the code that backs each claim: [security model](docs/en/security.md). A guided path through the code for a reviewer, with commands to check the claims yourself: [REVIEWING.md](REVIEWING.md). Reporting a vulnerability: [SECURITY.md](SECURITY.md).

## Quick start

### With Docker

```bash
docker run --rm -p 7342:7342 -v mimicway-data:/data ghcr.io/eoth/mimicway
```

The image is published for amd64 and arm64 with each release, signed and with its build provenance ([verifying a release](SECURITY.md#verifying-a-release)). To build it yourself instead: `docker build -t mimicway .`.

### Binary

Each [release](https://github.com/Eoth/mimicway/releases) has a single-file binary for Linux (x86_64 and arm64, static), macOS (Intel and Apple Silicon) and Windows, with the UI inside. Unpack it and run `./mimicway`.

Open <http://localhost:7342>. The interface needs Chrome or Edge 111, Firefox 114, Safari 16.4 or later (browsers from March 2023 on).

### From source

Requirements: Rust 1.85+ (edition 2024), Node.js 22.12+ with npm. Bootstrap scripts install them: [Windows](scripts/bootstrap-windows.ps1), [Linux/macOS](scripts/bootstrap-linux.sh).

```bash
cd frontend && npm ci && npm run build && cd ..
cargo build --release
./target/release/mimicway          # mimicway.exe on Windows
```

Building the UI first embeds it in the binary: `target/release/mimicway` is then complete on its own, and keeps its data in `./data` by default (see [Configuration](#configuration)).

### Your first mock

Create a service named `demo` with one rule, then call it:

```bash
curl -X POST http://localhost:7342/api/services \
  -H "Content-Type: application/json" \
  -d '{
    "name": "demo",
    "listen_path": "/v1/users/{id}",
    "real_target_url": "",
    "is_mocked": true,
    "rewrite_directory_urls": false,
    "group_name": null,
    "wsdl_mode": "auto",
    "rules": [{
      "name": "hello",
      "method": "GET",
      "sub_path": null,
      "action": "mock",
      "conditions": { "all_of": [], "any_of": [] },
      "response": {
        "status": 200,
        "headers": [{ "name": "Content-Type", "value": "application/json" }],
        "body": [{ "type": "Template", "template": "{\"id\":\"{{path.id}}\",\"name\":\"{{fake.FirstName}}\"}" }]
      }
    }]
  }'

curl http://localhost:7342/demo/v1/users/42
# {"id":"42","name":"..."}
```

The same service can be built in the UI in a minute; on an empty instance, the home screen's **Load an example** button creates a `users-api` service to try (`GET /users-api/users/42`). For a fuller picture, import [examples/devops-toolchain.json](examples/devops-toolchain.json) (GitLab, Jenkins, Wiki.js and Keycloak mocks) from the UI's Import button, or with `curl -X PUT http://localhost:7342/api/config -H "Content-Type: application/json" --data @examples/devops-toolchain.json`.

## Concepts

**Service.** A named entry point. It is exposed under `/{name}/{listen_path}`, or `/{group_code}/{name}/{listen_path}` when it belongs to a group. An empty `listen_path` catches everything under `/{name}/`. When a request is forwarded, the `/{group_code}/{name}` prefix is removed and the rest is appended to `real_target_url`.

**Rule.** A service holds an ordered list of rules. Each rule has its own HTTP method, an optional `sub_path`, conditions, and an action: answer with a mock, or forward to the backend. The first matching rule wins; when none matches, Mimicway answers 404 and the request log records it.

**Mock / proxy switch.** With `is_mocked` off, a service forwards every request to `real_target_url` and its rules wait. With it on, the rules answer; `action: proxy` on a rule forwards only the requests that rule matches (partial mock). A service without a target is purely mocked.

| Service | listen_path | Rule | URL to call |
|---|---|---|---|
| `insee` | `/v4/sirene/{siret}` | `GET` | `GET /insee/v4/sirene/44306184100047` |
| `accounts` | `/login` | `POST` | `POST /accounts/login` |
| `users` | *(empty)* | `GET`, sub_path `/{id}` | `GET /users/42` |

**Group.** Services can be gathered in a group: shown together in the UI, prefixed by the group's 5-character code in URLs, and, when authentication is on, managed by the group's admins and members.

## Features

| Area | What you get | Guide |
|---|---|---|
| Services and routing | URL namespace per service, REST or SOAP (WSDL requests can bypass the mock), directory URL rewriting | [Services](docs/en/services.md) |
| Groups | Shared URL prefix, per-group admins and members | [Groups](docs/en/groups.md) |
| Matching rules | Method, sub-path, conditions on path, query, headers, JSON, XML/XPath, form; AND/OR | [Matching rules](docs/en/matching-rules.md) |
| Responses | Templates, visual JSON/XML builder, fake data, chaos mode (latency, error rate) | [Responses and templates](docs/en/responses-and-templates.md) |
| Scripts | Up to three sandboxed Rhai blocks per rule, deterministic values per seed, dates, JSON/XML helpers | [Rhai scripts](docs/en/rhai-scripts.md) |
| Rule tester and conflicts | Replay a draft rule against a captured request with a per-condition verdict; warning when a new rule overlaps another | [Rule tester](docs/en/rule-tester-and-conflicts.md) |
| Request log | The last 200 requests: mode, matching rule (or none), status, and the captured request for the rule tester | [Request log](docs/en/request-log.md) |
| Traffic observation | Watch a proxied service and turn what it really returns into rules | [Traffic observation](docs/en/traffic-observation.md) |
| Availability check | A TCP connection test to the backend, never an HTTP request | [Availability check](docs/en/availability-check.md) |
| Backups | Automatic rotation before every change, one-click restore | [Backups](docs/en/backups-and-restore.md) |
| Administration | Import, export, reset, dark mode | [Administration](docs/en/administration.md) |
| Authentication | Optional Keycloak login, roles per group, super-admins | [Authentication](docs/en/authentication.md) |
| Kafka (optional) | The same rules applied to Kafka messages, with a message log and a simulator | [Kafka](docs/en/kafka-messaging.md) |
| Raw TCP (optional) | Fixed answers to binary protocols matched by prefix or regex | [Optional features](#optional-features) |

All guides: [docs/en/index.md](docs/en/index.md).

## Configuration

Everything is set through environment variables; none is required.

| Variable | Default | Purpose |
|---|---|---|
| `PORT` | `7342` | HTTP port. |
| `BIND_ADDRESS` | `127.0.0.1` | Interface to listen on. `0.0.0.0` (or `::`) accepts remote connections; the container image sets it. |
| `DATA_PATH` | `./data` | Directory of `mock-config.yaml` and its `backups/`. |
| `STATIC_DIR` | *(the UI embedded in the binary)* | A directory to serve the UI from instead, for UI development or a customized UI. A binary built without the UI reads `./frontend/dist`. |
| `RUST_LOG` | `mimicway=info` | Log filter, for example `mimicway=debug`. |
| `BACKUP_MAX_COUNT` | `5` | Backups kept in `{DATA_PATH}/backups/` before rotation. |
| `API_BASE_URL` | *(empty)* | Where the UI calls the API when they are served from different origins; see [Split UI and API](#split-ui-and-api). |
| `CORS_ALLOWED_ORIGINS` | *(empty)* | Comma-separated origins allowed to call the management API from a browser, besides the UI's own origin. Needed only with `API_BASE_URL`. Mocked services always answer any origin. |
| `PROXY_READ_TIMEOUT_SECS` | `120` | Longest silence accepted from a proxied backend between two reads; past it the client gets 504. |
| `REDACT_HEADERS` | *(empty)* | Extra header names (comma-separated) whose values are hidden in logs, observation and suggestions, on top of `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`, `X-Auth-Token`, `X-Amz-Security-Token`. |
| `REQUEST_LOG_MAX_BODY_SIZE` | `16384` | Bytes of each body kept in the request log. |
| `TRAFFIC_OBSERVATION_MAX_BODY_SIZE` | `16384` | Bytes of each body kept by traffic observation. |
| `TRAFFIC_OBSERVATION_MAX_BUFFER_SIZE` | `10485760` | Largest request or response body (by its `Content-Length`) that traffic observation buffers to capture an exchange; larger or unsized bodies are streamed and not observed. |
| `TRAFFIC_OBSERVATION_MAX_KEYS` | `200` | Distinct request shapes observed per service. |
| `TRAFFIC_OBSERVATION_SAMPLES_PER_KEY` | `8` | Exchanges kept per request shape. |
| `TRAFFIC_OBSERVATION_MIN_SAMPLES` | `3` | Exchanges needed before a rule is suggested. |
| `AUTH_ENABLED` | `false` | Turns Keycloak authentication on. `KEYCLOAK_URL`, `KEYCLOAK_REALM` and `KEYCLOAK_CLIENT_ID` then become required; the server refuses to start without them. |
| `KEYCLOAK_URL` | *(empty)* | Base URL Mimicway uses to reach Keycloak. |
| `KEYCLOAK_REALM` | *(empty)* | Realm. |
| `KEYCLOAK_CLIENT_ID` | *(empty)* | Client the tokens must be issued for (`azp` or `aud`). |
| `KEYCLOAK_ISSUER` | *(realm URL)* | Expected `iss` of tokens, when Keycloak issues them under another URL than `KEYCLOAK_URL` (behind a proxy, for instance). |
| `SUPER_ADMINS` | *(empty)* | Comma-separated user names with full rights, including reset and restore. |
| `SHOW_RESET_BUTTON` | `false` | Shows the reset button when authentication is off. Display only: the server decides who may reset. |
| `KAFKA_ENABLED` | `false` | Starts the Kafka consumer (binary built with `messaging-kafka`). |
| `KAFKA_BROKERS` | *(empty)* | Comma-separated brokers. |
| `KAFKA_CONSUMER_GROUP` | `mimicway` | Consumer group. |
| `KAFKA_LISTEN_TOPIC` | *(empty)* | Topic consumed. |
| `KAFKA_REPLY_TOPIC` | *(empty)* | Topic the mocked reply is published to; nothing is published when empty. |
| `MESSAGE_LOG_TTL_MS` | `86400000` | Retention of the Kafka message log. |
| `MESSAGE_LOG_MAX_BODY_SIZE` | `16384` | Bytes of each message kept in that log. |
| `TCP_MOCK_MAX_MESSAGE_SIZE` | `16384` | Largest message read by a raw TCP mock (binary built with `tcp-mock`). |

An invalid value (a malformed `BIND_ADDRESS`, authentication on without its Keycloak settings) stops the server at startup with a message saying which variable to fix, and exit code 2.

## Management API

All under `/api`, JSON in and out. Error messages follow the request's `Accept-Language` (English by default, French available). With authentication on, every route except `/api/health`, `/api/auth/status`, `/api/auth/login` and `/api/auth/validate` needs a bearer token; see [REVIEWING.md](REVIEWING.md#authorization-matrix) for who may call what.

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/health` | Liveness probe. |
| GET, PUT | `/api/config` | Whole configuration: read (filtered to what the caller may see), replace. |
| DELETE | `/api/config/reset` | Remove every service and group (super-admin; a protected backup is kept). |
| GET | `/api/config/backups` | List backups (super-admin). |
| POST | `/api/config/restore/{file}` | Restore a backup (super-admin). |
| GET, POST | `/api/services` | List, create ungrouped services. |
| GET, PUT, DELETE | `/api/services/{name}` | Read, replace, delete an ungrouped service. |
| GET, PUT, DELETE | `/api/groups/{group}/services/{name}` | The same for a service of a group. |
| PUT | `…/{name}/toggle` | Switch mock / proxy. |
| POST | `…/{name}/ping` | TCP availability check of the backend. |
| PUT | `…/{name}/rules/reorder` | Reorder rules. |
| POST, DELETE | `…/{name}/observe` | Start, stop traffic observation. |
| GET | `…/{name}/suggestions` | Rules suggested from observed traffic. |
| GET | `/api/observation/status` | Services under observation. |
| POST | `/api/rule-test` | Test a draft rule against a captured request. |
| POST | `/api/rule-conflicts` | Overlaps between a draft rule and the service's other rules. |
| POST | `/api/script/validate` | Compile a Rhai script. |
| GET | `/api/logs` | Request log (`?limit=`, filtered to what the caller may see). |
| GET, POST | `/api/groups` | List, create groups. |
| GET, PUT, DELETE | `/api/groups/{name}` | Read, update, delete a group. |
| PUT | `/api/groups/{name}/members` | Set a group's admins and members. |
| POST | `/api/auth/login`, `/api/auth/validate` | Keycloak login, token check. |
| GET | `/api/auth/me`, `/api/auth/status` | Current user, whether authentication is on. |
| GET, POST | `/api/messaging/status`, `/api/messaging/logs`, `/api/messaging/simulate` | Kafka (feature `messaging-kafka`). |
| GET, POST, PUT, DELETE | `/api/tcp/status`, `/api/tcp/services[/{name}]` | Raw TCP mocks (feature `tcp-mock`). |

`…/{name}` stands for `/api/services/{name}` or `/api/groups/{group}/services/{name}`. A service of a group is only reachable through the second form.

## Templates and scripts

A response body is made of fragments. In a `Template` fragment, single braces are literal (plain JSON or XML), and `{{ }}` holds an expression:

| Expression | Example result |
|---|---|
| `{{path.siret}}`, `{{query.page}}`, `{{header.x-request-id}}` | Values from the request |
| `{{body.customer/id}}`, `{{xpath.Envelope/Body/order/id}}` | A value of a JSON body (JSON pointer) or of an XML/SOAP body (path without namespace prefixes) |
| `{{path.siret \| first(9)}}` | `443061841` |
| `{{fake.Email}}` | Fake data |
| `{{uuid}}`, `{{now_ms}}`, `{{now_iso}}`, `{{now_epoch}}`, `{{seq}}` | Generated values |
| `{{script.total}}` | A value returned by the rule's script |

Pipes: `lower`, `upper`, `trim`, `capitalize`, `first(n)`, `last(n)`, `substr(start, len)`, `default("x")`, `replace("a", "b")`, `prepend("x")`, `append("x")`, `length`.

Fake data kinds: `FirstName`, `LastName`, `Email`, `CompanyName`, `StreetName`, `DatePast`, `DateFuture`, `TimestampMs`, `BoolRandom`, `LoremSentence`, and French formats (`PhoneNumberFR`, `CityFR`, `PostcodeFR`, `FullAddressFR`, `CountryFR`, `IbanFR`, `Siren`, `Siret`). The visual builder adds integers in a range (`Integer{min,max}`).

A rule can also run up to three [Rhai](https://rhai.rs) scripts (`pre_script`, `script`, `post_script`) that read the request and return a value or a map used by the template. Besides the language itself they get `random_int`, `seeded_int`, `seeded_pick` (same input, same answer), `uuid`, `fake`, `now_ms`, `now_iso`, `year`, `date_now`, `date_past`, `date_future`, `parse_date`, `parse_json`, `to_json`, `parse_xml_items` and `xml_element`. Scripts run in a sandbox: no file or network access, no `eval`, at most 10,000 operations, 1 MB strings, 1,000-item arrays, 32 call levels. Full reference with examples: [Rhai scripts](docs/en/rhai-scripts.md).

## Deployment

### Container

The [Dockerfile](Dockerfile) builds the UI, embeds it in the binary, and copies that single file into an otherwise empty image (`scratch`: no operating system, no shell), running as an unprivileged user (uid 1000). The image listens on every interface (`BIND_ADDRESS=0.0.0.0`) and keeps its data in `/data`: mount a volume there.

```yaml
# compose.yaml
services:
  mimicway:
    build: .
    ports: ["7342:7342"]
    volumes: ["mimicway-data:/data"]
volumes:
  mimicway-data:
```

### Kubernetes

[k8s/](k8s/) holds a Kustomize base (a single-replica Deployment with a read-only root file system, no privilege escalation and every capability dropped, a PersistentVolumeClaim, a Service) and two overlays to route traffic to it: a standard `Ingress`, or Gloo Edge. See [k8s/README.md](k8s/README.md).

```bash
cd k8s/ingress
kustomize edit set image mimicway=<registry>/mimicway:<version>
kubectl create namespace mimicway
kubectl apply -k .
```

Mimicway must be served from the root of a host, and by a single replica: the configuration lives in one file on one volume.

### Split UI and API

By default the UI calls the API on its own origin (`/api/...`), which works whenever one Mimicway serves both. If your ingress routes the API to another origin, set on the instance that serves the UI:

```bash
API_BASE_URL=https://mimicway-api.example.com
```

and on the instance that serves the API:

```bash
CORS_ALLOWED_ORIGINS=https://mimicway.example.com
```

The UI reads `API_BASE_URL` at startup from `GET /runtime-config.json`, so one image serves every environment without a rebuild. Check it with `curl https://mimicway.example.com/runtime-config.json`.

### Behind a proxy

Mimicway does not terminate TLS; put it behind your usual reverse proxy or ingress. When authentication is on, use HTTPS end to end for the UI, since it sends passwords to `/api/auth/login`.

## Backups

Before each change, the previous `mock-config.yaml` is copied to `{DATA_PATH}/backups/` (the last `BACKUP_MAX_COUNT` are kept). A reset also writes a copy to `backups/protected/`, kept 30 days whatever the rotation. Changes apply in memory at once and reach the disk in the background; on `SIGTERM` or Ctrl+C the server finishes writing before it exits.

Restore from the UI (Backups button, super-admins), or by hand: stop Mimicway, copy a backup over `mock-config.yaml`, start it again.

## Optional features

Both are compiled out by default: their code and dependencies are not in the standard binary.

- **Kafka** (`cargo build --release --features messaging-kafka`, needs cmake, a C toolchain and, on Linux, the libcurl headers (`libcurl4-openssl-dev`) to build librdkafka): consumes `KAFKA_LISTEN_TOPIC`, matches each message with the same rules and templates as HTTP (scripts excepted), optionally publishes the reply, keeps a message log and offers a simulator in the UI. See [Kafka](docs/en/kafka-messaging.md).
- **Raw TCP** (`--features tcp-mock`): listens on the ports you declare, on the same interface as HTTP (`BIND_ADDRESS`), and answers each message with fixed bytes, chosen by hexadecimal prefix or regex. No relay mode: a TCP mock never connects anywhere. Configuration lives in `{DATA_PATH}/tcp-config.yaml`.

## Development

```bash
cargo test                               # Rust unit and integration tests
cargo test --features tcp-mock
cargo clippy --all-targets -- -D warnings
cd frontend && npm test                  # Vitest
cd frontend && npm run test:e2e          # Playwright, against a running Mimicway on :7342
```

For UI work with hot reload, run the binary, then `cd frontend && npm run dev` and open <http://localhost:5173>. `build.rs` embeds `frontend/dist` when it exists; `cargo build` without a built UI gives a binary that serves `STATIC_DIR` (`./frontend/dist` by default).

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs formatting, clippy and tests with each feature, the UI tests and build, the end-to-end suite against the real binary, `cargo-deny`, `npm audit`, an image build scanned by Trivy, and a secret scan; each job runs when a file it checks changes, and a change to the documentation runs the documentation checks and the secret scan only.

Code map:

```
src/
  main.rs       startup, configuration errors, graceful shutdown
  server/       Axum router, management API, interception, browser guard, logs, observation, suggestions
  engine/       matcher, proxy, template renderer, Rhai sandbox
  auth/         Keycloak client (JWKS validation), middleware, roles
  store/        YAML persistence, write-behind, backups
  models/       configuration schema
  i18n.rs       server message catalogues
  messaging/    Kafka (feature messaging-kafka)
  tcp/          raw TCP mocks (feature tcp-mock)
frontend/       Svelte 5 UI, Vitest and Playwright tests
k8s/            Kubernetes manifests
examples/       configurations ready to import
```

### Translations

English is the source language: every message is written once, in English, in the code. Each other language is one catalogue keyed by the English text: [frontend/src/locales/fr.json](frontend/src/locales/fr.json) for the UI, [src/locales/fr.json](src/locales/fr.json) for API messages. Tests extract every message from the code and fail when a catalogue misses one, keeps an unused one or loses a placeholder; a pseudo-locale run fails on any text left untranslated in the UI. To add a language, add both catalogues and register the language in `frontend/src/lib/i18n.svelte.js` and `src/i18n.rs`. The documentation exists in English (`docs/en/`, `README.md`) and French (`docs/fr/`, `README.fr.md`), with the same pages, headings and screenshots in each language; CI checks it.

## Troubleshooting

| Symptom | Fix |
|---|---|
| A mock answers 404 | The URL must start with the service name (`/{name}/...`), and with the group code for a grouped service. The service page shows the exact URL. |
| A method or sub-path is not mocked | Methods belong to rules: add a rule for that method and sub-path. |
| The UI shows a blank page | The binary was built before the UI: build the UI, then the binary again, or point `STATIC_DIR` to `frontend/dist` (an absolute path on Windows). The startup log says where the UI comes from. |
| Port 7342 already in use | Set `PORT`, or stop the other process. |
| Other machines cannot reach Mimicway | It listens on `127.0.0.1` by default: set `BIND_ADDRESS=0.0.0.0`. |
| 403 "Cross-site request refused" | The UI is served from another origin: add it to `CORS_ALLOWED_ORIGINS`. |
| 403 "this Mimicway only listens on the local machine" | Call it through `localhost` or `127.0.0.1`, or listen on another interface. |
| Login refused although the password is right | Keycloak issues tokens under another URL than `KEYCLOAK_URL`, or for another client: set `KEYCLOAK_ISSUER`, check `KEYCLOAK_CLIENT_ID` (the server log names the cause). |
| `link.exe not found` on Windows | Install the Visual Studio Build Tools with the C++ workload. |
| Kafka build fails on Windows with a path length error | Set `CARGO_TARGET_DIR` to a short path such as `C:\lm-target`. |

## Project

- [ROADMAP.md](ROADMAP.md): what is planned, and in which order.
- [MIGRATING.md](MIGRATING.md): upgrading an installation from lightMock.
- [CHANGELOG.md](CHANGELOG.md): changes by release.
- [SECURITY.md](SECURITY.md): supported versions and private vulnerability reporting.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to propose a change, run the checks and add a language; [code of conduct](CODE_OF_CONDUCT.md).
- License: [MIT](LICENSE).
