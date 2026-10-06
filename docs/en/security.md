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
- CI fails on any known vulnerability or yanked crate, on a license outside a permissive allow-list and on a dependency from outside crates.io ([deny.toml](../../deny.toml)), on high-severity npm advisories, on critical or high vulnerabilities of the container image (Trivy) and on committed secrets (gitleaks).
- The server's parsers and matchers of untrusted input (request conditions, sub-paths, XML paths, templates, raw TCP hexadecimal, configuration imports) are fuzzed by ClusterFuzzLite, five minutes on each pull request that changes them and an hour every week; the UI's own parsers have property tests.
- CodeQL analyses the server (Rust), the interface (JavaScript) and the CI workflows whenever they change, the workflows on every pull request, and all three weekly; its alerts are listed in the repository's Security tab.
- The CI's third-party actions are pinned to commit SHAs, and Dependabot proposes their updates along with those of the crates, npm packages and base images.
- The code contains no `unsafe` Rust, tests included (`#![forbid(unsafe_code)]`).
- The image holds the static binary and its data directory, nothing else (`FROM scratch`): no operating system, shell or system package, so nothing in it but Mimicway's own code for a vulnerability to sit in. It runs as a non-root user (uid 1000); the Kubernetes manifests add a read-only root filesystem, no privilege escalation and no Linux capability.
- Releases are built from the tagged commit by `.github/workflows/release.yml`: every archive has a build provenance attestation and a CycloneDX SBOM (one for the Rust crates, one for the UI's shipped packages), and the image is signed keylessly with cosign. [SECURITY.md](../../SECURITY.md#verifying-a-release) gives the verification commands.
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

To report a vulnerability, see [SECURITY.md](../../SECURITY.md).
