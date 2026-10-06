[Français](../fr/security.md)

# Security model

This page is meant for whoever has to decide whether Mimicway may run in their environment: what it exposes, what it talks to, what it trusts, what it protects and what it leaves to the deployment. Every statement below is enforced by code and covered by tests; the file and test names are given so that each one can be checked.

## In one paragraph

Mimicway is a single process with no database, no telemetry and no outbound traffic of its own. It listens on one HTTP port (and optionally on raw TCP ports you configure), serves its UI and management API there, and answers the traffic of the services you define: from rules (mock) or by forwarding to the backend you configured (proxy). It only calls out to that backend, to Keycloak if you enable authentication, and to Kafka if you build and enable it.

## What it exposes

| Surface | Path or port | Who is expected to call it |
|---|---|---|
| Management API | `/api/...` | The UI and your automation. Protected by Keycloak tokens when `AUTH_ENABLED=true`. |
| UI shell | `/`, `/index.html`, `/assets/...`, `/runtime-config.json` | Browsers. Public on purpose: it shows the login screen. |
| Service traffic | `/{service}/...`, `/{group code}/{service}/...` | The applications under test. Never requires a Mimicway token: it carries the applications' own credentials. |
| Raw TCP mocks | ports you configure, on the `BIND_ADDRESS` interface (`tcp-mock` feature, off by default) | Clients of binary protocols. |

By default the binary listens on `127.0.0.1` only (`BIND_ADDRESS`). The container image sets `0.0.0.0`, the container network being the boundary there.

## Every outbound network flow

There is no other one: no telemetry, no update check, no hard-coded host. Outbound calls are made in `src/engine/proxy.rs` (proxy and ping), `src/auth/keycloak.rs` (Keycloak) and `src/messaging/` (Kafka, feature-gated); `src/tcp/` only listens.

| Flow | Trigger | Destination | Bounds |
|---|---|---|---|
| Availability ping | A user clicks "test the target" | `real_target_url` of that service | TCP connection only, no HTTP request, no TLS handshake; 3 s timeout; result cached 2 minutes |
| Proxy | A request matches a service in proxy mode, or a rule with `action: proxy` | `real_target_url` of that service, never another host | 10 s to connect, 120 s of silence at most (`PROXY_READ_TIMEOUT_SECS`); redirects are not followed; paths with `.`/`..` segments are refused before forwarding |
| Keycloak | `AUTH_ENABLED=true` only | `KEYCLOAK_URL` | Login, token refresh, realm key set (cached 5 minutes); 5 s to connect, 10 s overall |
| Kafka | Built with `--features messaging-kafka` **and** `KAFKA_ENABLED=true` | `KAFKA_BROKERS` | Consumer and producer on the configured topics |

## Who is trusted with what

**The operator** (environment variables, container, network) is fully trusted.

**Configuration editors** (UI and API users) define responses, proxy targets and scripts. Without authentication, anyone who can reach the management API is an editor with every right: that is the intended mode for a developer's machine, which is why the binary only listens on loopback by default. With `AUTH_ENABLED=true`, editors are Keycloak users and rights follow groups: a group admin manages the group's services and members, a member edits its services, and only super-admins (`SUPER_ADMINS`) manage ungrouped services, reset the configuration or restore backups. These boundaries are checked on every endpoint, including moves between groups, the full configuration export and the request log (`src/server/api.rs`, tests in `src/server/api/authz_tests.rs`).

Because an editor chooses proxy targets, Mimicway will send HTTP requests to any host an editor configures: that is what a proxy is. Decide who may edit (authentication) and restrict where the process may connect (network policy) accordingly.

**Rule scripts** (Rhai) are written by editors and run on every matching request, in a sandbox: at most 10,000 operations, bounded call depth, string, array and map sizes, no file access (`import` resolves nothing), no `eval`, no network, and output of `print`/`debug` sent to the debug log. The native functions never panic, whatever their arguments (`src/engine/script.rs`, tests next to it).

**Service traffic** (paths, headers, bodies sent by the applications under test) is untrusted input: bodies are limited to 10 MiB, XML is parsed without DTDs or entity expansion, regular expressions of the rules are compiled once with a size limit, and path parameters are decoded as UTF-8.

**Proxied backends** are untrusted too: their responses are streamed back as they come; only traffic observation, when a user enables it for a service, buffers bounded copies.

**Other websites open in a browser** must not be able to drive a Mimicway reachable from that browser (`src/server/browser_guard.rs`):
- the management API answers cross-origin calls only for the origins listed in `CORS_ALLOWED_ORIGINS` (none by default);
- state-changing API requests that the browser marks as cross-site are refused unless their origin is listed;
- when listening on loopback only, the API answers only requests addressed to `localhost`, `*.localhost`, `127.0.0.0/8` or `::1`, which defeats DNS rebinding;
- the UI and the API are served with a content security policy (no inline script, no framing), `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`. Responses of mocked and proxied services are left untouched.

## Authentication

When enabled, Mimicway validates Keycloak access tokens locally against the realm's published keys: asymmetric signature (RSA, RSA-PSS, ECDSA, EdDSA; never HMAC or `none`), issuer (`KEYCLOAK_ISSUER` if tokens carry another URL than `KEYCLOAK_URL`), expiry, and the client the token was issued to (`azp` or `aud` must name `KEYCLOAK_CLIENT_ID`). Keycloak is never asked to validate a token on Mimicway's behalf. An unknown key id refreshes the key set at most once every 30 seconds. Error responses carry no internal detail. Tests run against a fake realm signing real tokens (`src/auth/keycloak/tests.rs`).

The login form uses Keycloak's password grant. Replacing it with the authorization code flow with PKCE, and supporting any OpenID Connect provider, are planned (see [ROADMAP.md](../../ROADMAP.md)).

## Data

- **Configuration**: one YAML file in `DATA_PATH` (`mock-config.yaml`), written atomically, with rotated backups in `backups/` and a protected backup before each reset (kept 30 days). It holds what editors type: avoid putting real credentials in mock responses or scripts.
- **Request log**: the last 200 requests, in memory only, bodies truncated to 16 KiB. Credential headers (`Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`, `X-Auth-Token`, `X-Amz-Security-Token`, plus `REDACT_HEADERS`) are stored as `[redacted]`, and the same applies to traffic observation; suggested rules never copy them. With authentication, users only see the entries of the services they can access.
- **Logs** (stdout): no request bodies, no credentials; proxy URLs lose their `user:password@` part.

## Supply chain and build

- `Cargo.lock` and `frontend/package-lock.json` are committed: builds resolve exactly the same dependency graph.
- CI fails on any known vulnerability of a locked dependency (RustSec through cargo deny, and OSV, which also holds GitHub's advisories, for every lock file), on a yanked crate, on a license outside a permissive allow-list and on a dependency from outside crates.io ([deny.toml](../../deny.toml)), on npm advisories (high severity and above for the packages the UI ships, critical for the build and test tools), on critical or high vulnerabilities of the container image (Trivy) and on committed secrets (gitleaks).
- The server's parsers and matchers of untrusted input (request conditions, sub-paths, XML paths, templates, raw TCP hexadecimal, configuration imports) are fuzzed by ClusterFuzzLite, five minutes on each pull request that changes them and an hour every week; the UI's own parsers have property tests.
- CodeQL analyses the server (Rust), the interface (JavaScript) and the CI workflows whenever they change, the workflows on every pull request, and all three weekly; its alerts are listed in the repository's Security tab.
- The CI's third-party actions are pinned to commit SHAs, and Dependabot proposes their updates along with those of the crates, npm packages and base images.
- The code contains no `unsafe` Rust, tests included (`#![forbid(unsafe_code)]`).
- The image holds the static binary and its data directory, nothing else (`FROM scratch`): no operating system, shell or system package, so nothing in it but Mimicway's own code for a vulnerability to sit in. It runs as a non-root user (uid 1000); the Kubernetes manifests add a read-only root filesystem, no privilege escalation and no Linux capability.
- Releases are built from the tagged commit by `.github/workflows/release.yml`: every archive and SBOM is published with its keyless signature and the build provenance of all of them (Sigstore, the workflow's identity, no long-lived key), the release carries a CycloneDX SBOM for the Rust crates and one for the UI's shipped packages, and the image is signed and attested the same way. [SECURITY.md](../../SECURITY.md#verifying-a-release) gives the verification commands.
- The Linux binaries and their archives are reproducible: the release workflow builds each one twice, on two machines, and publishes nothing unless both give the same bytes, and anyone can rebuild them from the tag to compare ([SECURITY.md](../../SECURITY.md#rebuilding-a-release)).
- The base images of both Dockerfiles are pinned by digest.

## Hardening checklist

1. Shared instance: set `AUTH_ENABLED=true` with Keycloak and list `SUPER_ADMINS`.
2. Keep the default loopback binding on workstations; in a cluster, expose Mimicway through your ingress with TLS (Mimicway itself serves plain HTTP).
3. Restrict egress (Kubernetes `NetworkPolicy`, firewall) to the backends you actually proxy and to Keycloak.
4. Set `CORS_ALLOWED_ORIGINS` only if the UI is served from another origin than the API.
5. Add your own credential header names to `REDACT_HEADERS`.
6. Use the provided image and manifests, or reproduce their restrictions (non-root, read-only root filesystem, no capabilities).

## Known limits

- No built-in TLS, rate limiting or audit trail of configuration changes (the backups keep previous versions; an audit log is planned).
- Without authentication, every user is a super-admin. This is intended for local use only.
- Authentication supports Keycloak only for now.

## Why these guarantees hold

The sections above state the facts. This one ties them into an argument, the project's assurance case: what Mimicway promises, against whom, where trust changes hands, and why its design and its code keep each promise.

### Security requirements

1. Mimicway opens no connection that its operator or an editor did not configure.
2. Without authentication, only the local machine reaches the management API.
3. With authentication, a user reaches only the services and groups their roles allow, on every endpoint.
4. A web page open in a browser cannot drive a Mimicway that this browser can reach.
5. The traffic of the applications under test and the answers of proxied backends cannot crash the server, exhaust its memory, run code or reach its files.
6. Credentials seen in traffic are not kept in logs, observations or suggestions.
7. An accepted change of configuration is never lost, and the previous configuration can be restored.

### Threat model

| Who | What they could try | What stops them |
|---|---|---|
| A website that a developer visits while Mimicway runs on their machine | Cross-site requests to the management API, DNS rebinding | The browser guard: CORS allow-list, refusal of cross-site writes, host check when listening on loopback |
| Anyone on a network the port is exposed to | Reading or changing the mocks | Loopback by default; authentication with Keycloak; TLS at the ingress |
| An authenticated user | Reaching another group's services, or administration | Authorization on every endpoint, tested through the real router with signed tokens |
| An editor, trusted to configure | Escaping from a rule script | The Rhai sandbox: no file, network, `import` or `eval`, bounded operations, sizes and depth |
| The applications under test, a proxied backend | Oversized or malformed input, path traversal, resource exhaustion | Body limit, typed parsing, XML without DTD, size-limited regular expressions, dot segments refused, timeouts, fuzzed parsers |
| A compromised dependency or CI action | Malicious code or a known vulnerability in the build | Lock files, actions and images pinned by digest, cargo deny, OSV, npm audit, Trivy, Dependabot, CodeQL |

The operator (environment, container, network) is trusted. Editors are trusted to choose proxy targets: forwarding to the host an editor configured is what a proxy does, so restricting egress belongs to the deployment (point 3 of the hardening checklist).

### Trust boundaries

- **The HTTP port.** Everything that crosses it is untrusted: management requests are authenticated and authorized when authentication is on, and service traffic is data for the rules, never an instruction to Mimicway.
- **The browser's origin.** The management API answers another origin only when `CORS_ALLOWED_ORIGINS` lists it.
- **Roles.** Anonymous caller, authenticated user, group member, group admin, super-admin; the [authorization matrix](../../REVIEWING.md#authorization-matrix) says which one each endpoint needs.
- **The script sandbox.** Rhai code written by editors runs inside the process, without access to anything outside its sandbox.
- **Outbound answers.** Proxied backends and Keycloak are untrusted: their answers are streamed back or parsed as data, within timeouts.
- **The file system.** Mimicway writes under `DATA_PATH` only, to file names it generates or checks against their pattern.

### Secure design principles

| Principle | How it applies |
|---|---|
| Economy of mechanism | One process, no database, one module for every outbound HTTP call, one store for the configuration. |
| Fail-safe defaults | Loopback only, no CORS origin, redirects not followed. With `AUTH_ENABLED=true` but its Keycloak settings missing, Mimicway refuses to start; when Keycloak cannot be reached, protected routes refuse the request. |
| Complete mediation | Every management endpoint but the four public ones checks the caller's right on each request, moves between groups and the full configuration export included. |
| Open design | The code and this model are public; protection rests on Keycloak's keys and on the network policy, not on secrecy. |
| Separation of privilege | Resetting, restoring backups and managing ungrouped services need the super-admin role; a group admin manages their own group only. |
| Least privilege | The image runs as a non-root user, on a read-only root filesystem, without Linux capabilities; CI jobs get read-only tokens, and write access only where a job needs it. |
| Least common mechanism | State kept per service (sequence counters, observations), and shared views (configuration, request log) filtered per user. |
| Psychological acceptability | The safe setup needs no configuration on a workstation; refusals say why, in the user's language. |
| Limited attack surface | One port, no telemetry, optional features not compiled unless asked for, interface files served by exact name. |
| Input validation with allowlists | Names match `[A-Za-z0-9_-]+` and avoid reserved routes, HTTP methods come from a fixed list, backup file names must match the pattern the server generates, and request and configuration bodies are parsed into typed structures. |

### Common weaknesses countered

| Weakness (OWASP Top 10 2021, CWE) | Countered by |
|---|---|
| Broken access control (A01, CWE-862, CWE-863) | The authorization matrix, enforced in each handler and tested with real tokens (`src/server/api/authz_tests.rs`). |
| Cryptographic failures (A02) | No cryptography of its own; asymmetric token signatures only, never HMAC or `none`; outbound TLS by rustls, certificates verified. |
| Injection and cross-site scripting (A03, CWE-79, CWE-94) | No SQL and no shell; templates insert request values without evaluating them; scripts run in the sandbox without `eval`; the interface escapes every value (no raw HTML) under a content security policy that forbids inline scripts. |
| Insecure design (A04) | This model, the [reviewer guide](../../REVIEWING.md) and the fuzzed parsers. |
| Security misconfiguration (A05) | Safe defaults, the hardening checklist and manifests that apply it. |
| Vulnerable and outdated components (A06, CWE-1104) | Lock files, advisories checked on every change of a manifest and weekly, Dependabot updates. |
| Identification and authentication failures (A07, CWE-287) | Delegated to Keycloak; tokens checked locally for signature, issuer, expiry and client. |
| Software and data integrity failures (A08, CWE-502) | Typed deserialization only; actions and base images pinned; releases signed, with their provenance; configuration written atomically, with backups. |
| Security logging and monitoring failures (A09) | Request log and server logs without credentials; an audit log of configuration changes is planned. |
| Server-side request forgery (A10, CWE-918) | Requests go only to the target an editor configured, never to a host taken from the request; redirects are not followed and dot segments are refused. |
| Path traversal (CWE-22) | Backup names checked before any disk access; embedded interface files served from a fixed table, a `STATIC_DIR` through tower-http's `ServeDir`, which refuses parent segments. |
| Uncontrolled resource consumption (CWE-400) | Limits on bodies, logs, observations and queues; size-limited regular expressions; bounded script operations; timeouts on proxying and Keycloak. |
| Cross-site request forgery (CWE-352) | Cross-site writes refused by the browser guard. |
| Memory safety (CWE-787, CWE-125) | Mimicway's own code is Rust without `unsafe`, and its parsers of untrusted input are fuzzed with AddressSanitizer. |

To report a vulnerability, see [SECURITY.md](../../SECURITY.md).
