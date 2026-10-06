# Roadmap

What is planned for Mimicway, in the order it will be done, and why. Finished work moves to [CHANGELOG.md](CHANGELOG.md); this file only holds what is still ahead.

## What Mimicway stays

Every item below is weighed against these promises; an idea that breaks one of them is reshaped or dropped.

- **One self-contained program.** No database, no agent, no cloud account, nothing to install next to it.
- **Quiet by default.** No telemetry, no update check. Every outbound connection is one the user configured, and the [security model](docs/en/security.md) lists them all.
- **Safe on a laptop.** Local-only until told otherwise; a web page cannot drive it.
- **Light for the person using it.** A mock should take a minute to write and zero minutes to maintain: import what exists, suggest what can be guessed, explain what went wrong.
- **Easy to approve.** Every feature keeps the [reviewer guide](REVIEWING.md) true: small dependency set, no `unsafe`, documented flows and permissions.

## How to read an item

Each item has an identifier that never changes, a size (S: a day or less, M: a few days, L: a week or more, to be split before starting), a short **why**, a **what**, and a **done when** that can be checked. Items marked **Decision needed** wait for a choice from the maintainers before any code.

## Order of work

1. [Ready for an international launch](#1-ready-for-an-international-launch): R2, R10 (with R15, R16, R17, R18, R19), R11, R12, R13, R14, R9.
2. [Trust and operations](#2-trust-and-operations): T1 to T11.
3. [Make it indispensable](#3-make-it-indispensable): U1 to U16, in the listed order.
4. [Engineering backlog](#4-engineering-backlog): taken alongside, when they unblock an item above.

## 1. Ready for an international launch

### R2. Publish signed releases

Size S (what is left)

**Why.** Today every user builds from source, which is exactly the long review companies refused. A signed artifact with its bill of materials can be approved once and reused.

**Progress.** `.github/workflows/release.yml` (checked with actionlint) builds, on a `vX.Y.Z` tag, static Linux binaries (x86_64, arm64), macOS (Intel, Apple Silicon) and Windows binaries with the UI inside, a multi-architecture image on GHCR assembled from those binaries, CycloneDX SBOMs for the crates and the shipped UI packages, checksums, build provenance attestations for everything, and a keyless cosign signature of the image. The release procedure is in CONTRIBUTING.md and the verification commands in SECURITY.md.

**What.** Attach the signatures and the provenance of the archives to the release as assets, since the OpenSSF Scorecard (R10) looks for them there, not in GitHub's attestation store: a release with none scores 0 on its Signed-Releases check (high risk), signature files (`.sigstore.json`) 8, SLSA provenance (`.intoto.jsonl`) 10. Then cut the first release (0.2.0), make the GHCR package public, and fix whatever the first run reveals.

**Done when.** `cosign verify` and `gh attestation verify` succeed on the published image and binaries, the README quick start runs as written, and the Scorecard report gives Signed-Releases 10 and detects the publishing workflow (Packaging).

### R11. Unit tests in English

Size M (what is left: the comments and titles)

**Why.** The comments and test titles of the Vitest suite are mostly French: they are the last French comments of the repository, and contributors who do not read French cannot follow these tests.

**Progress.** The suite runs in English: `src/tests/setup.js` applies English and the assertions name the English texts. `src/tests/french.test.js` shows the screens of the pseudo-locale test (one list, `src/tests/helpers/screens.js`) in French, switched from English the way the language select does it, with the texts that French writes its own way; `setup.js` refuses the French catalogue to every other test file but `l10n.test.js`, which `french-catalogue-guard.test.js` checks. With every French translation altered, 7 tests of `french.test.js` fail and nothing else, against 201 tests in 30 files before: rewording the French catalogue (R9) no longer breaks the suite.

**What.** Rewrite the comments and test titles of `frontend/src/tests/` in English, keeping only why in comments and checking each claim against the code: 141 French comment lines of 263 (137 of 259 in the JavaScript files, 4 of 4 in the Svelte harnesses of `helpers/`) and 392 French titles of 596, counted by `scripts/check-french-comments.mjs`, which already covers the rest of the repository; then add the folder to the paths it covers.

**Done when.** `scripts/check-french-comments.mjs` covers `frontend/src/tests/` with nothing to report.

### R9. Polish the French catalogues

Size S

**Why.** Many French messages were written without accents ("reserve", "deja", "regle"), which reads as careless to French users.

**What.** Restore accents and typography (non-breaking space before `:`, `«»` quotes) in `src/locales/fr.json` and `frontend/src/locales/fr.json`; the existing catalogue tests keep placeholders intact. Fix the mistranslations the French screenshots show, such as "Réécriture annuaire" for "Directory URL rewriting" and an example name that differs from the English one ("ex: get-siret" for "e.g. get-customer"). Give both typed confirmation keywords the same rule: the restore confirmation asks for a translated keyword (`RESTAURER`), the reset confirmation for `RESET` in every language (`App.svelte`); a translated keyword must stay easy to type on any keyboard, and the guide names it in each language. Update the French texts that `frontend/src/tests/french.test.js` expects, the only unit tests that read the French catalogue. Then regenerate the screenshots (`npm run docs:screenshots`): the French images show the catalogue as it is today.

**Done when.** A spell check of both French catalogues passes, and the French screenshots are regenerated from them.

### R12. Readable condition labels in the rule form

Size S

**Why.** The list of a rule's conditions shows each one as `QueryParam(id) Eq(42)`: internal type names, identical in every language, which QA and business users cannot be expected to decode and which the guide's screenshots show in English and French alike. The pseudo-locale test does not catch it, because the label is assembled from data. The rule tester and the traffic suggestions already word conditions in the user's language.

**What.** Build the label in `RuleConditionsEditor.svelte` from the source and operator labels the condition form already translates (`Query parameter`, `HTTP header`, `Equals`, `Exists (any value)`…), through `t()` with placeholders, one message per operator shape, as `ObservationSuggestions.svelte` does; translate the new messages in `frontend/src/locales/fr.json`; regenerate the screenshots that show a condition list.

**Done when.** No condition in the interface shows a source or operator type name, in either language, and a Vitest test covers the label of each source and operator.

### R13. Screens open at their top

Size S

**Why.** The interface is one page without a router: switching screens keeps the scroll position. A service opened from far down the list, or reached by saving a long form, opens part-way down with its title and the navigation bar cut off, and the guide's screenshots of the service page show it.

**What.** Scroll to the top when the main screen changes (list, service page, logs, groups, backups, Kafka, TCP), and bring the list back to where it was when the user returns to it, as the expanded groups already are.

**Done when.** An end-to-end test opens a service from below the fold and finds its page at the top, then goes back and finds the list where it left it.

### R14. A header that fits in every language

Size S

**Why.** At 1280 px the navigation bar wraps onto two lines as soon as it holds more than the basic actions: in English for a signed-in super-admin (user badge, "Log out" and "Reset", see `docs/en/screenshots/authentication-user-badge.png`), in French already for a signed-in user or with the Kafka button (`docs/fr/screenshots/group-members.png`, `kafka-message-log.png`). Every language longer than English makes it worse.

**What.** Make the header hold on one line at common desktop widths whatever the language: shorter labels, icons with an accessible name for the secondary actions, or a menu for the rare ones (import, export, backups), chosen from a mock-up in both languages.

**Done when.** An end-to-end test measures the header at 1280 px in English and French, signed in as a super-admin, and finds a single line in both.

### R10. Public supply-chain score

Size L, split: R2 (signed releases), R15, R16, R17, R18, and the item below

**Why.** Reviewers increasingly start from the OpenSSF Scorecard: it checks pinned dependencies, token permissions, branch protection, signed releases, CI tests and more, and shows the result as a badge. A low score turns people away before they read anything else.

**Progress.** The repository is public, `.github/workflows/scorecard.yml` publishes the score weekly and on pushes to `develop`, and the README shows the badge. The first report (commit 91964b8) gave 6.4: 10 on Dangerous-Workflow, Token-Permissions, Binary-Artifacts, Security-Policy, License, Maintained and Dependency-Update-Tool; 9 on Pinned-Dependencies; 7 on Vulnerabilities, now fixed (jsonwebtoken 11, quinn-proto 0.11.19, a justified exception for smartstring in `osv-scanner.toml`, and CI runs osv-scanner, the scanner Scorecard uses); 0 on Code-Review, Branch-Protection, SAST, Fuzzing, CII-Best-Practices and Contributors; CI-Tests, Packaging and Signed-Releases inconclusive (no pull request, no release yet). The report on fbf07d2 (2026-10-03) gave 6.6, Vulnerabilities at 10. CodeQL (R15) and the fuzz targets (R16) are in the CI: once pushed, SAST and Fuzzing should reach 10, 7.7 with the same weights (the model gives back 6.6 for the published report). The report on 6cd8a57 (2026-10-05) gave 8.5: SAST, Fuzzing and Packaging at 10 (the CI builds the container image), Pinned-Dependencies at 9 for the rustup installer piped to a shell, and Branch-Protection left out of the average after an internal error: a classic branch protection rule, added to `develop` on 2026-10-03, which the workflow's token cannot read (R17 replaces it with a ruleset, which it can). Counted at 0 as before, the same report gives 7.8, the model's 7.7 with Packaging measured. Done since: the Linux bootstrap script checks Rust and Node.js and says where to install them, without downloading anything (Scorecard's parser knows no checksum, so a download checked against a pinned hash would still count as unpinned), and the Windows one installs rustup with winget, which checks the installer's hash. The report on 9c6fc01 (2026-10-06) gave 8.6: Pinned-Dependencies at 10, and Branch-Protection still left out after the same internal error, the classic rule being still in place (the branch API shows `develop` protected, the rules API no active ruleset). Expected once the ruleset replaces it: 8.1 with Branch-Protection read at 3 (no force push, no deletion), 8.3 with the badge at passing (CII-Best-Practices 5, low risk).

**What.** The maintainers aim at 9.5. Computed with Scorecard's weights (critical 10, high 7.5, medium 5, low 2.5), once every item below is done: about 8.4 without R17 (Branch-Protection at 3, Code-Review at 0), 9.5 with it (Branch-Protection stays at 8: its next tier asks for two reviewers), 9.55 with a silver badge; 10 is out of reach for a one-maintainer project, since Contributors asks for regular contributors from three organizations. Here: replace the installers piped to a shell in `scripts/bootstrap-linux.sh` (rustup, and NodeSource's setup script for Node.js) (Pinned-Dependencies), by downloads checked against a pinned hash or by instructions, whichever Scorecard accepts.

**Done when.** The published score is 9.5 or more.

### R15. Static analysis on every change

Size S

**Why.** Scorecard's SAST check is at 0: no static analysis runs. CodeQL finds classes of bugs tests rarely reach (injection, unsafe path handling, workflow mistakes) and costs nothing on a public repository.

**What.** A CodeQL workflow for JavaScript and TypeScript, Rust and the GitHub workflows (no build needed), on pushes to `develop` and `main`, on pull requests and weekly, pinned by commit like the other actions, with `security-events: write` on its job only. Fix or dismiss with a reason each finding of the first run.

**Progress.** The CI workflow runs CodeQL (default queries, build mode none) on the Rust server, the UI's JavaScript and the workflows when their files change, all three weekly, and the workflows on every pull request, documentation included: Scorecard counts the merged pull requests that carry a successful CodeQL check. `CI passed` waits for it. CodeQL 2.27.1 run locally (the bundle of codeql-action v4.38.2, same default suites, build mode none) on 8764b2c found 9 alerts, none on the workflows. Fixed: js/incomplete-sanitization in `scripts/check-french-comments.test.mjs` (a sample file name kept a star). False positives, to dismiss with their reason once the CI has uploaded them: js/bad-tag-filter in `scripts/check-french-comments.mjs` (Svelte reads `<SCRIPT>` and `<Style>` as components, not code; a test pins it) and 7 rust/path-injection in `src/store/mod.rs`, in the backup functions (the paths come from `DATA_PATH` and from file names the server generates or lists in its own backup directory; the request's configuration is only serialized in `prepare_and_backup`, where CodeQL's flow joins the two). The CI analysed the three languages on the push of 6cd8a57 and again in the weekly run, both green, and Scorecard's SAST check is at 10 since its report of 2026-10-05. The alerts themselves are readable only with a token (the code scanning API answers 401 without one); since the local run, the analysed code changed by two test messages and two `expect` messages in Rust, one comment in the comment guard, one test of it and its fixed test sample, and the workflows by their Node.js version and the image job's cache, with no new input from a request or an event, so the page should list the same 8. Left: the maintainer checks the list in Security > Code scanning, dismisses those 8 with their reason, and hands any other alert over.

**Done when.** Scorecard's SAST check is at 10 and the code scanning page lists no open alert.

### R16. Fuzzing the parsers of untrusted input

Size M

**Why.** Every request a mock receives is untrusted, and Mimicway parses it: paths and query strings, JSON bodies read by JSON Pointer, XML bodies read by XPath, templates, hexadecimal TCP payloads, imported configurations. Scorecard's Fuzzing check is at 0, and it does not see what exists (Rust property tests with proptest): it recognizes OSS-Fuzz, ClusterFuzzLite, cargo-fuzz targets (libFuzzer) and property tests with fast-check in JavaScript.

**What.** Fuzz targets (cargo-fuzz) for the server's parsers and matchers, run by ClusterFuzzLite on pull requests and weekly (the maintainers' choice, five minutes per pull request and an hour a week); fast-check property tests for the UI's own parsers (templates to fields and back in `tpl-utils.js`, `hex-utils.js`, `path-params.js`). Every crash found becomes a fix with its regression test.

**Progress.** Six targets in `fuzz/` (what each checks: `src/fuzzing.rs`, compiled only by cargo-fuzz): request conditions, where the rule tester must agree with production; sub-paths; XML paths; templates; the hexadecimal of raw TCP mocks; configuration imports, which must read back unchanged from the stored YAML. ClusterFuzzLite builds them against the server's lock file (`.clusterfuzzlite/`) and runs them in the CI workflow, behind `CI passed`. Each ran five minutes on Linux without a crash (0.6 to 14 million inputs). fast-check property tests cover `tpl-utils.js`, `hex-utils.js` and `path-params.js`; they found that the response builders lost special characters (fixed). Scorecard's Fuzzing check is at 10 since its report of 2026-10-05 (ClusterFuzzLite, the six cargo-fuzz targets and the three property test files). The first weekly campaign ran the same day: an hour shared by the six targets, 65.7 job-minutes with the build, no crash; it keeps one corpus per target as a run artifact (4 kB for the hexadecimal to 871 kB for configuration imports, for 90 days). Left: an hour per target, which the weekly campaign accumulates (about ten minutes per target and week, so five more weeks), and the five-minute run of a pull request, which has not happened yet: no pull request has changed the server since.

**Done when.** Scorecard's Fuzzing check is at 10 and each target has run for an hour without a crash, or what it found is fixed.

### R17. Reviewed changes on protected branches

Size M. Decision taken by the maintainers (2026-10-03): changes are proposed by a machine account and approved by the maintainer.

**Why.** Code-Review and Branch-Protection are high-risk checks, both at 0. Code-Review counts the last 30 changes approved by a person other than their author; reviews by bots, AI included, do not count. Here the code is written by an automated agent and the maintainer is the one reader who can approve it, which is what the check asks for, provided the review is real.

**What.** The maintainer creates a machine account (GitHub allows one, run by a person, for automation), gives it write access, and adds a ruleset on `develop` and `main`: no force push, no deletion, pull request with one approval, the CI jobs as required checks, no bypass. Each change becomes a pull request from that account, small enough to read in ten minutes, with a review packet in its description (what changes and why, what could go wrong, what to look at first, how it was checked) and an independent automated review posted as comments (fresh-context agents, CodeQL, mutation testing). CONTRIBUTING describes the flow; the rule on commit hours applies to merges.

**Progress.** On 2026-10-03 the maintainer added a classic branch protection rule to `develop` and a ruleset, `default`, left disabled. Scorecard cannot read classic rules with the workflow's token, and its Branch-Protection check now ends in an internal error; it reads rulesets. Decided by the maintainers (2026-10-06): the classic rule goes, and the ruleset carries everything, starting with no force push and no deletion on `develop` (Branch-Protection 3), which leaves direct pushes possible until the pull request flow is ready. On 2026-10-06, after the push of 9c6fc01, the classic rule was still there and no ruleset active.

**Done when.** The last 30 changes of `develop` are approved pull requests (Code-Review 10), Branch-Protection is at 8 and CI-Tests at 10.

### R18. OpenSSF Best Practices badge

Size S

**Why.** Scorecard's CII-Best-Practices check is at 0. The badge (passing 5, silver 7, gold 10, gold needing several maintainers) records practices Scorecard cannot detect, with the evidence for each.

**What.** Fill the questionnaire at bestpractices.dev with the evidence the repository already holds (security policy, CI, tests, signed releases, vulnerability handling), fix what passing still lacks, aim at silver, and show the badge in both READMEs.

**Progress.** The repository was reviewed against the 67 passing and the 55 silver criteria, as the badge's own source lists them (2026-10-06). Added for them: ARCHITECTURE.md; GOVERNANCE.md (roles, decisions); how a vulnerability report is handled, and its reporter credited, in SECURITY.md; an assurance case at the end of the security model (requirements, threat model, trust boundaries, design principles, common weaknesses); a written rule that a new feature comes with its tests, and the Developer Certificate of Origin by reference, in CONTRIBUTING; a coding style per language, enforced by the CI (Prettier for the interface and the scripts, in a `format` job, and the Svelte compiler's warnings now fatal), with each silenced warning explained where it is; the documentation defects found on the way. Measured: the unit tests cover 82.6 % of the server's production lines (cargo llvm-cov) and 85.0 % of the interface's statements (Vitest, v8), and 47 of the 61 bug fixes of the last six months add a test case. Every passing criterion is met or justified, once `develop` is pushed (the evidence is linked there) and GitHub's private vulnerability reporting, which SECURITY.md points to, is enabled: it was off on 2026-10-06. Left for passing: the maintainer registers the project and fills the questionnaire. Left for silver: the maintainer names a successor in the account settings of GitHub, after which GOVERNANCE.md gets its continuity section; the first signed release (R2); reproducible builds (R19); the badge in both READMEs.

**Done when.** The badge is at passing or above and Scorecard reads it.

### R19. Reproducible release builds

Size M

**Why.** From its silver level, the OpenSSF Best Practices badge (R18) asks that building the same source twice gives the same bits, which lets anyone check a release against its source. Nothing guarantees it today: the release workflow builds with whatever stable Rust is current that day (`toolchain: stable`), the archives keep the time and owner of each file, and no build has been compared with another.

**What.** Pin the Rust version of the release builds, in a place that Dependabot or the release procedure keeps up to date; build the archives with fixed times, order and owners (`SOURCE_DATE_EPOCH`, `tar --sort=name --mtime --owner=0 --group=0`, `gzip -n`); add a check that builds the Linux binary twice, from clean checkouts in different directories, and compares the hashes (with `--remap-path-prefix` if a path leaks in). SECURITY.md says how to rebuild a release and compare it.

**Done when.** Two builds of the same tag give the same SHA-256 for the Linux binaries and archives, the CI checks it, and SECURITY.md explains how to reproduce it.

## 2. Trust and operations

### T1. Any OpenID Connect provider, browser login with PKCE

Size L, to split (discovery and validation first, then the login flow)

**Why.** Authentication works with Keycloak only, through the password grant: the UI sees users' passwords, and OAuth 2.1 drops that grant. Entra ID, Okta, Auth0, Google or GitLab users cannot log in at all.

**What.** Configure an issuer URL and a client; read its discovery document and keys; log in with the authorization code flow and PKCE (the UI never sees a password); map groups or roles from a configurable claim to super-admins. Keycloak becomes one provider among others, and its current variables keep working.

**Done when.** End-to-end tests log in against a fake provider through the redirect flow; the password form is gone when the new mode is configured.

### T2. API tokens for automation

Size M

**Why.** CI pipelines and test suites need to change mocks without a person's credentials.

**What.** Named tokens with a scope (read, write on given groups, admin) and an expiry, created by admins, stored hashed, shown once, revocable from the UI.

**Done when.** A token can drive the management API within its scope and nowhere else, which the authorization tests check for each endpoint.

### T3. Audit log of configuration changes

Size M

**Why.** "Who changed this mock, and when?" is the first question on a shared instance, and reviewers ask for traceability.

**What.** An append-only log of every change (who, when, what, a diff of the service or group), stored next to the configuration with rotation, visible to admins, exportable as JSON lines.

**Done when.** Every mutating endpoint writes one entry, which a test asserts for each of them.

### T4. Restrict where the proxy may connect

Size S

**Why.** An editor can point a proxy at any host the server reaches: internal services, cloud metadata endpoints. Today only the network can stop it.

**What.** `PROXY_ALLOWED_TARGETS`: a list of hosts, domains or CIDR ranges that proxy targets must match, checked when a service is saved and again when a request is forwarded (after DNS resolution); link-local addresses refused unless listed.

**Done when.** A target outside the list is refused at save time and at forwarding time, with tests for DNS names that resolve to a forbidden address.

### T5. Host allow-list on every interface

Size S

**Why.** The DNS-rebinding guard only applies when Mimicway listens on loopback. Listening on `0.0.0.0` without authentication leaves that door open.

**What.** `ALLOWED_HOSTS`: the host names the management API answers to, required (or derived from `API_BASE_URL`) when listening beyond loopback without authentication.

**Done when.** A request with an unlisted `Host` is refused on any interface, and startup warns when the configuration leaves the API reachable by any name.

### T6. Rate limits

Size S

**Why.** The login endpoint and the management API have no throttling.

**What.** Per-client limits on `/api/auth/login` and on mutating API calls, with `429` and `Retry-After`; mocked services stay unlimited unless a rule asks for it (see U11).

**Done when.** Repeated failed logins are slowed down, as a test shows, without any cost on mocked traffic.

### T7. Metrics and traces

Size M

**Why.** Operators want to know whether the mock answers, how fast, and which rules are hit, in the tools they already use.

**What.** An opt-in Prometheus endpoint (requests by service, rule, mode and status; latency; proxy errors; configuration writes) and OpenTelemetry traces exported to a configured endpoint. Both off by default, both listed as outbound flows when on.

**Done when.** A Grafana dashboard example in `docs/` works against a running instance.

### T8. Packaging for clusters and compose

Size M

**Why.** Kubernetes users expect a Helm chart, and compose users a ready file.

**What.** A Helm chart (published as an OCI artifact with the image) built from the same settings as `k8s/base`, and a `compose.yaml` at the root; security contexts stay as strict as the current manifests.

**Done when.** `helm install` and `docker compose up` both give a working instance, tested in CI with kind.

### T9. Configuration as code

Size M

**Why.** Teams want mocks reviewed and versioned with the application, and the same mocks in every environment.

**What.** A read-only mode that loads services from files in a directory (one file per service or group, YAML or JSON), reloads on change, and refuses changes through the API; a `mimicway validate <dir>` command that checks files in CI; a JSON Schema of the configuration for editor completion.

**Done when.** A mock committed in a repository reaches a running instance without the UI, and an invalid file fails the CI command with the same message the UI would show.

### T10. Leave the deprecated YAML crate

Size S

**Why.** `serde_yaml` is no longer maintained, and `smartstring` (pulled in by the script engine) is flagged unmaintained, which `deny.toml` currently tolerates.

**What.** Move to a maintained YAML implementation, keeping the file format byte-compatible; follow Rhai's releases and drop the `smartstring` exception as soon as a release no longer depends on it.

**Done when.** `cargo deny check` passes with an empty `ignore` list.

### T11. Serve under a path prefix

Size M

**Why.** The UI loads `/assets`, `/runtime-config.json` and `/api` from the root of its host, so Mimicway cannot live under a path such as `https://tools.example.com/mimicway/`, which many platforms impose (one host, one prefix per tool).

**What.** A `BASE_PATH` setting: the server mounts the UI, the API and the mocked services under it, the UI is built with relative asset paths and reads the prefix from `runtime-config.json`, and service URLs shown in the UI include it.

**Done when.** An end-to-end run with `BASE_PATH=/mimicway` behind a prefix-stripping proxy passes, and a Kubernetes overlay shows it.

## 3. Make it indispensable

### U1. Import an API description

Size L, to split by format

**Why.** Writing the first mocks by hand is the main cost. Most teams already have an OpenAPI file, a WSDL, a Postman collection or a HAR capture.

**What.** Import an OpenAPI 3 or Swagger 2 document into a service: one rule per operation and status, response bodies from the document's examples, or generated from its schemas with fake data when there are none. Then WSDL (one rule per operation, matched on `SOAPAction` and the body element, with sample envelopes), then Postman collections, HAR files and pasted `curl` commands.

**Done when.** The Petstore OpenAPI document and a public WSDL import into working mocks, as end-to-end tests check, and nothing is sent anywhere during an import.

### U2. Record and replay

Size M

**Why.** Traffic observation suggests rules but leaves each one to accept by hand. Teams often want a faithful snapshot of a real backend in one go.

**What.** A "record" switch on a proxied service that turns every distinct exchange into a rule (grouped by the same correlation as suggestions), credentials removed, then switches the service to mock mode on demand.

**Done when.** Recording a session against a real backend, then cutting the network, replays the same responses for the same requests.

### U3. Verify what the application called

Size M

**Why.** Automated tests need to assert that the application under test called a dependency, how many times and with what: today they can only read the request log by hand.

**What.** `POST /api/verify` with a request pattern (method, path, conditions like a rule's) and an expected count; `DELETE /api/logs` to start a test from a clean journal; a journal size setting for test runs.

**Done when.** A Java and a JavaScript example test use it to assert calls, in the docs.

### U4. Scenarios selected per request

Size M

**Why.** Testing an error case means editing the mock and editing it back, which breaks parallel tests sharing an instance.

**What.** Named variants of a rule's response, picked by a request header (`X-Mimicway-Scenario: timeout`), a cookie or a query parameter, with a default; the UI shows the variants side by side.

**Done when.** Two test runs against the same instance get different scenarios at the same time.

### U5. Stateful mocks and instant CRUD

Size L, to split

**Why.** Many APIs are "create, then read what was created". Static rules cannot follow, and scripts have no memory.

**What.** First, a per-service state machine: a rule can require a state and move to another one (like an order going from `created` to `paid`). Then a "resource" service type: from a path such as `/orders/{id}`, Mimicway answers POST, GET (one and list), PUT, PATCH and DELETE on an in-memory collection, seeded from examples, reset by API.

**Done when.** A create-then-read test passes against a resource service with no rule written.

### U6. Frozen and shifted time

Size S

**Why.** Responses with dates (`now_iso`, `date_future`…) make snapshots and assertions flaky.

**What.** A clock per service or per request (`X-Mimicway-Time`): fixed at an instant, or shifted by a duration; templates and scripts read it instead of the system clock.

**Done when.** The same request with the same frozen time returns byte-identical responses.

### U7. Callbacks and webhooks

Size M

**Why.** Asynchronous APIs answer "accepted", then call back later (payments, document processing). Mocking them today needs another tool.

**What.** A rule can schedule an HTTP call after its response: target, delay, method, templated body and headers. The callback target is an outbound flow: it obeys the proxy allow-list (T4) and appears in the security model.

**Done when.** An end-to-end test receives the callback with values taken from the original request.

### U8. Request coverage and one-click rules from misses

Size S

**Why.** Users do not know which rules are dead, nor what the application asked that no rule answered.

**What.** Count hits per rule (shown in the rule list, reset with the log), and an "unmatched requests" view where "Create a rule" fills the rule form from the request.

**Done when.** A 404 from Mimicway can become a working rule in two clicks.

### U9. Built-in identity provider mock

Size M

**Why.** Almost every application under test needs tokens. Running a real identity server for tests is heavy, and hand-made JWKS mocks are error-prone.

**What.** A service type that behaves as an OpenID Connect provider: discovery document, key set, token endpoint (client credentials, password, authorization code with PKCE), with configurable claims and lifetimes, signing keys generated per instance.

**Done when.** A sample application configured with Mimicway as its issuer logs in and validates tokens.

### U10. More protocols

Size L, to split by protocol

**Why.** Modern applications also talk GraphQL, WebSocket and server-sent events.

**What.** GraphQL first (match on operation name and variables, answer per operation), then server-sent events (a scripted sequence of events with delays), then WebSocket (message-to-message rules, like the raw TCP mock), and gRPC last if asked for.

**Done when.** Each protocol has a guide page and end-to-end tests.

### U11. Faults beyond status codes

Size S

**Why.** Real failures are rarely a clean `500`: connections drop, bodies trickle, responses are cut.

**What.** Chaos options for a dropped connection, a reset after headers, a body sent slowly (bytes per second), a truncated body, and a per-rule rate limit answering `429`.

**Done when.** Each fault is reproduced by a test client that sees the expected transport error.

### U12. Responses from files

Size S

**Why.** Some mocks must return PDFs, images or archives, which templates cannot hold.

**What.** A body fragment that serves a file uploaded through the UI and stored in `DATA_PATH/files`, with its content type; included in exports.

**Done when.** A rule returns an uploaded PDF byte for byte.

### U13. Partial export and mock packs

Size S

**Why.** Import and export only cover the whole configuration, so sharing one service between teams means editing JSON by hand.

**What.** Export a service or a group; import it into another instance with a preview of conflicts. A small gallery of generic, ready-to-import packs in `examples/` (an OAuth2 provider, a payment flow, a CI toolchain).

**Done when.** A group exported from one instance imports into another with its rules intact.

### U14. Contract drift detection

Size M

**Why.** Mocks lie silently when the real API changes.

**What.** For a service with a target, compare on demand the mocked responses with the real backend's for the same captured requests, and report differences in status, fields and types.

**Done when.** Changing a field in the real backend shows up as a drift on its rule.

### U15. Fake data in every locale

Size M

**Why.** Fake data is French today (names, addresses, phone numbers, IBAN, SIREN), which looks odd to most users.

**What.** A locale per service (default from the UI language): names, streets, cities, postcodes, phone numbers and IBANs in that locale's formats; the French-specific kinds stay available.

**Done when.** A service in `en-US` and one in `de-DE` produce plausible local data, and each locale has tests for its formats.

### U16. More interface languages

Size S per language

**Why.** English and French cover a small part of the people who could use Mimicway.

**What.** Spanish, German, Portuguese, Italian and Japanese catalogues for the UI and the server messages, contributed and reviewed by native speakers; the existing tests already check each catalogue.

**Done when.** Each language passes the catalogue tests and a native speaker's review.

## 4. Engineering backlog

### E2. Response builder leftovers

Size M

**Why.** The by-example level cannot rename, add or remove fields, and there is no live preview next to the builder.

**What.** Make the by-example level fully editable, then add a side-by-side preview that highlights the field being edited.

**Done when.** A pasted sample can be reshaped without switching to the detailed level.

### E8. Room for long values in the detailed XML builder

Size S

**Why.** In the detailed XML builder, a node holds its tag, type, source, value and transformation on one line, and the value field gets what is left: an XPath such as `Envelope/Body/recherche/Siret` shows cut after `Envelope/Body/recherche/S` in English and after `Envelope/Body/re` in French (`response-xml-xpath-source.png`), so the user cannot read back what they typed.

**What.** Give the value field a minimum width that fits a usual XPath, letting the line wrap before the value instead of shrinking it, and check the JSON builder's rows the same way.

**Done when.** The XPath of `response-xml-xpath-source.png` shows whole in both languages, the image's subject includes the value field, and the screenshot guard fails on an input whose value overflows it.

### E9. The WSDL mode in the service form

Size S. **Decision needed** (what a REST service does with `?wsdl`).

**Why.** The guide says a SOAP service either relays WSDL requests to the real backend (`Proxy`/`Auto`) or answers them with its rules (`Mock`), but the service form offers no such choice: `Mock` can only be set through the API or the configuration file (saving the form now keeps it instead of resetting it to `auto`). And a REST service, which the guide describes as having "no SOAP-specific handling", still sends `?wsdl` requests to the backend past its rules (`auto`, the default), which on a purely mocked service ends in a 502. `Auto` and `Proxy` behave the same.

**What.** Offer the WSDL mode in the form for a SOAP service; decide what a REST service does with `?wsdl` (its rules, like any request, or the current relay) and make the guide say it in both languages; give `Auto` a meaning of its own or drop one of the two values.

**Done when.** A SOAP service's WSDL mode can be set and read back in the form, the guide describes what REST and SOAP services do with `?wsdl`, and end-to-end tests cover each mode.

### E3. Kafka parity

Size M

**Why.** Kafka rules cannot run scripts, and a single global topic is listened to.

**What.** Run the three script slots for messages, and allow a topic per service.

**Done when.** A Kafka rule can compute its reply with a script, tested with the simulator.

### E4. Raw TCP mock, next step

Size M

**Why.** The TCP mock answers fixed bytes only.

**What.** Templated responses that reuse bytes of the request (an identifier echoed back, a length recomputed), and a capture log like the HTTP one.

**Done when.** A request-response protocol with a correlation identifier can be mocked.

### E5. Observation keys by route pattern

Size S

**Why.** Observation groups exchanges by literal path, so `/orders/1` and `/orders/2` never pool their samples.

**What.** Group by the service's route pattern and suggest path-parameter conditions; add a fuzz target for the suggestion code next to its property tests.

**Done when.** Calls to `/orders/{id}` with different ids produce one suggestion with conditions on `id`.

### E6. Shared test application builder

Size S

**Why.** Several test modules build their own router and server with slight variations.

**What.** Use `server::test_support` everywhere and delete the copies.

**Done when.** One function builds every test application.

### E7. Request context for the proxy path

Size S

**Why.** `do_proxy` and `do_proxy_observed` (`src/server/intercept.rs`) take more arguments than clippy accepts, silenced with `#[allow(clippy::too_many_arguments)]`; so do a test helper of the template engine and the constructors of observation and Kafka log entries.

**What.** Group the request-scoped values in one struct, and give the entry constructors a parameter struct.

**Done when.** No `too_many_arguments` allowance is left in `src/`.

### E10. Serve the interface compressed

Size S

**Why.** The embedded interface is served as it was built, uncompressed, whatever the browser accepts: 290.6 kB of JavaScript, 70.6 kB of CSS and a 48.1 kB French catalogue, against 87.3 kB, 9.7 kB and 17.0 kB with gzip (measured with `gzip -9`). The assets are cached for a year, so the cost comes back with each release and each new browser, notably over a slow cluster ingress.

**What.** Compress the built files once, at build time, with the zlib that Node already ships (no new Rust dependency), embed the `.gz` variants next to the originals, and serve one with `Content-Encoding: gzip` and `Vary: Accept-Encoding` when the request accepts it; `index.html` and `STATIC_DIR` keep working as today.

**Done when.** An end-to-end test reads the main script with and without `Accept-Encoding: gzip` and gets the compressed and the plain bytes, and the reviewer guide still lists every file the binary serves.

### E11. Unit tests that depend on the speed of the machine

Size S

**Why.** Under CPU load (another build running), the pseudo-locale test of the application shell (`src/tests/l10n.test.js`, "the application shell, its list and its dialogs") failed once and passed when run again: it waits for the first render with the default one-second `waitFor`, after importing and mounting the whole application. `src/tests/french.test.js` renders the same screens (`src/tests/helpers/screens.js`): with two `cargo test --workspace` of other projects running (CPU at 65 %), "the application shell…" and "the service screens" of both files went past the five-second test timeout, and passed run alone. A test that fails without a defect teaches people to ignore red.

**What.** Find the unit tests that wait on rendering with the default timeout after heavy work, and make them wait on a condition with a timeout sized for a loaded CI runner, or do the heavy import once per file (for the two files above: load the components of `helpers/screens.js` before the timed tests).

**Done when.** The UI unit tests pass ten times in a row while a `cargo build` runs on the same machine.

### E12. Messages on screen follow a language switch

Size M

**Why.** Switching languages re-renders the whole interface at once, except the messages already on screen: they keep the language they were written in. The error of the by-example JSON and XML builders stays English after switching to French, so does the sign-in form's "The user name is required." (both checked in a unit test), and the same holds wherever a component stores a translated sentence rather than what it says: 17 form and parse errors (service, rule, group, TCP and sign-in forms, by-example builders) and about fifty notifications translated before they are shown. The leftover reads as a missed translation.

**What.** Store what a message says, not its translation: a function that translates it (`() => t("…")`, which keeps the message a literal for the extraction), called when rendering; notifications accept the same form. An error text sent by the server stays in the language of the request that got it.

**Done when.** `frontend/src/tests/french.test.js` opens each screen's state before switching languages (today it opens them after, because of this) and still finds no English text left.

### E13. A lighter CI

Size M

**Why.** A push to `develop` costs far more machine time than it checks. Every CI run starts its 9 jobs whatever changed (9.8 to 16.6 job-minutes per run, the container image alone 4 to 6), a change to a Markdown file included; and a push that touches a lock file makes Dependabot rebase each pull request it conflicts with, whose full CI runs again: after the push of fbf07d2 (jsonwebtoken 11 in Cargo.lock), 5 pull request runs, 60.4 job-minutes. The runs also warn that their actions target Node.js 20, now deprecated, and that `ubuntu-latest` moves to Ubuntu 26 on 2026-10-19.

**What.** Measure the job-minutes of a push first (GitHub API), then: run each job only when the files it checks change, behind one aggregate job that branch protection can require (R17); stop Dependabot from rebasing its pull requests on every push, and group or space its updates; cache what can be cached (Rust, npm, image layers); move the actions to their Node.js 24 versions and pin the runner images. Keep every check that runs today for the changes it concerns.

**Progress.** Measured before (GitHub API, the five runs of 2026-10-03; the push runs had been deleted): 9.8 to 16.6 job-minutes per run, 16.6 (22 billed minutes) for the one with every job green. Done: `scripts/ci-plan.mjs` plans the jobs from the changed files, `CI passed` fails on a planned job that did not pass, and node tests check both (a change of code runs the Rust, UI, image and end-to-end jobs, documentation runs none of them, ci.yml conditions and awaits every job); Dependabot monthly, grouped, without rebases, a week of cooldown; the Rust caches written by develop only, Playwright's browser and the image layers cached, the image's dependencies in a layer of their own; every action on Node.js 24, the runners pinned to ubuntu-24.04. Measured after, on the push of 6cd8a57 (2026-10-03), which changed every kind of file and so ran every job but the fuzzing ones: 23.1 job-minutes, 32 billed (a job is billed by the started minute), all green. The nine jobs that ran before took 15.9, the image 6.9 of it, building the new Dockerfile on an empty layer cache; CodeQL added 6.2 (Rust 4.6, the UI 0.9, the workflows 0.6), the supply-chain job 0.8, the plan and `CI passed` 0.2 together. A pull request that changes the Dockerfile alone (Dependabot's #19) ran the plan, the repository checks, the secret scan, the image and CodeQL on the workflows: 5.1 job-minutes. The weekly run of 2026-10-05 took 75.1 job-minutes, 65.7 of them the fuzzing campaign and 8.6 CodeQL. After the push, the 17 open Dependabot pull requests were closed without a rebase: the one pull request run of the evening is the first run of a new one, #19. Dependabot's update jobs took 10.6 minutes, now once a month. No CI or Scorecard run carries an annotation; Dependabot's own update jobs carry GitHub's notice that `ubuntu-latest` moves to Ubuntu 26, on a runner GitHub picks for them. The push of 9c6fc01 (2026-10-06), which changed the server, the UI, the scripts and the documentation, ran every job but the fuzzing ones, the new `format` job included: 20.9 job-minutes, 32 billed, all green, no annotation. The image took 4.0 minutes against 6.9 on an empty cache: the compiled Rust dependencies came from the cache, the UI was built again for its new lock file. CodeQL on Rust took 6.6 of them. Left: a push of documentation alone.

**Done when.** A documentation-only push runs the documentation checks only (and Scorecard, kept on every push by the maintainers' choice), a push no longer reruns the Dependabot pull requests, the job-minutes of a typical push are measured before and after, and no run warns about deprecated actions.

### E15. The interface with a screen reader

Size M

**Why.** The tests hold the contrast, the keyboard access, the visible focus and the compiler's accessibility warnings, but nobody has used the interface with a screen reader. Labels that read well on screen can still be announced badly or not at all: icons, notifications, the folds of the rule form, the dialogs. WCAG asks that it work, and the OpenSSF Best Practices badge (R18) lists a screen reader pass among the accessibility practices.

**What.** Do the main tasks with NVDA on Windows and VoiceOver on macOS: create a service, write a rule with conditions and a templated response, test it against a captured request, read the request log. Fix what is announced wrong or not at all, and turn each fix into a test where the page can hold it (accessible names, roles, live regions).

**Done when.** The main tasks can be done with a screen reader alone, and each fix has its test.

### E16. Shell scripts checked like the workflows

Size S

**Why.** actionlint runs shellcheck on the shell steps of the workflows, but nothing checks `scripts/bootstrap-linux.sh`, which people run on their own machines. shellcheck 0.10.0 reports two findings in it, both at the info level: SC2086 on the package list given to apt-get, which has to be split into words, and SC1091 on the sourced cargo environment, which it cannot follow.

**What.** Run shellcheck on the shell scripts of the repository in the workflows job, planned by `scripts/ci-plan.mjs` for a change of one of them (with its test), and fix each finding or silence it at its line with the reason.

**Done when.** The CI fails on a shellcheck finding in a shell script of the repository, and the two findings of today are fixed or justified where they are.
