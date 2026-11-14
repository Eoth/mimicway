# Roadmap

What is planned for Mimicway, release by release, and why. Finished work moves to [CHANGELOG.md](CHANGELOG.md); this file only holds what is still ahead.

## What Mimicway stays

Every item below is weighed against these promises; an idea that breaks one of them is reshaped or dropped.

- **One self-contained program.** No database, no agent, no cloud account, nothing to install next to it.
- **Quiet by default.** No telemetry, no update check. Every outbound connection is one the user configured, and the [security model](docs/en/security.md) lists them all.
- **Nothing hidden.** It writes under its data directory, and in a directory the operator names in a setting or on a command line, and nowhere else; it starts no other program, installs nothing, and never touches the settings of another tool. What it does on a machine can be listed and checked (T13).
- **Safe on a laptop.** Local-only until told otherwise; a web page cannot drive it.
- **Light for the machine.** It must stay usable on the smallest hardware a team owns: memory, processor time, image and saved file are measured, against the alternatives too; their budgets go down, and rise only for a stated reason (G1, G2).
- **Light for the person using it.** A mock should take a minute to write and zero minutes to maintain: import what exists, suggest what can be guessed, explain what went wrong.
- **Usable without a developer.** A tester or a business analyst reads every screen in plain words and finds out alone why a call got its answer; an internal code on a screen is a defect.
- **Accessible.** Every screen is built for WCAG 2.2 AA, by keyboard alone and with a screen reader; CI checks what a tool can check, a manual audit the rest, and the known gaps are items here (T18).
- **Easy to approve.** Every feature keeps the [reviewer guide](REVIEWING.md) true: small dependency set, no `unsafe` outside the allocator that counts what scripts hold (`alloc/`), documented flows and permissions.

## How to read an item

Each item has an identifier that never changes, a size (S: a day or less, M: a few days, L: a week or more, to be split before starting), a short **why**, a **what**, and a **done when** that can be checked. Items marked **Decision needed** wait for a choice from the maintainers before any code. An item that adds an input that grows with use adds its family to the scale test ([decision 2](decisions/0002-performance-is-judged-by-growth.md)).

Items are gathered in **themes**: the items of a theme touch the same screens or the same files, and are taken together, one after the other, so that the code is opened once. A **release** is a handful of themes that change one thing for the user; it ships when its themes are done, at no fixed date. The letters say what an item serves: R the launch, T trust and operations, U use, G the weight on the machine, C the request side (sending calls, not only answering them), E engineering.

## Releases at a glance

| Release | What changes for the user | Themes |
|---|---|---|
| [0.6](#release-06-approved-at-first-sight) | A security team can approve it from one page, its weight is proved, and anyone can read its screens | Bounded · Proved · Scored · Lightest, with the proof · Accessible · Words anyone can read |
| [0.7](#release-07-a-first-mock-without-writing-one) | A first mock comes from a file or from real traffic, never from a blank form | From what exists · Recorded, without personal data · Guided and explained · Shared |
| [0.8](#release-08-mocks-that-behave-like-the-real-service) | Mocks remember, change with the situation, and fail like real services | State and data · Situations and time · Real failures and payloads |
| [0.9](#release-09-tests-can-rely-on-it) | Automated tests drive it, assert on it, and keep its mocks in their repository | Assert and automate · The saved file · Shipped · See what happened |
| [1.0](#release-10-shared-across-a-company) | One instance serves a whole company; its formats and its promises are frozen | Sign-in and traces · Deployed anywhere · Confined · Stated · Frozen formats |
| [1.1](#release-11-more-protocols) | GraphQL, WebSocket and tokens are mocked like HTTP | Protocols · Identity |
| [2.0](#release-20-the-request-side) | The same program sends calls, checks answers and runs collections: an API client with nothing hidden | Send · Collections · Checks and runs · One loop |

[Kept open](#kept-open) holds the ideas that wait for a decision; the [engineering backlog](#engineering-backlog) is taken alongside.

## Order of work

1. The releases in the order above; inside a release, the themes in the order of its table; inside a theme, the items in the order listed. A theme is taken whole. An item of size L is split before it starts, and its parts follow each other.
2. A defect that lets untrusted input exhaust memory or time, or that leaks a secret, passes before any theme.
3. The [engineering backlog](#engineering-backlog) is taken by bundle, when a theme opens the same files or when a bundle unblocks it.
4. The judges come before the lists above: E52, a test whose verdict comes from outside the code under test (mutations).
5. An item that waits for a step of the maintainers, for a decision or for an upstream release holds neither its theme nor its release: what can be done is done, the wait is written in its **Progress**, and the work goes on.

At the maintainers' request, templates in response headers then server-sent events came first, before the lists above; both are in the [changelog](CHANGELOG.md).

## Release 0.6: approved at first sight

| Theme | Items, in order | Taken together because |
|---|---|---|
| Bounded | T4, T6, T10 | what the instance may reach, how often it may be asked, and the last unmaintained library |
| Proved | T13, T15 | the proof a security team asks for, and the page that holds it |
| Scored | R10, R16, R17, R18 | the public scores; most of what is left waits for the maintainer's own steps |
| Lightest, with the proof | E10 (in the backlog), G1, G2 | one measuring job, its budgets |
| Accessible | T20, then the keyboard bundle and the screen reader bundle | the same judge and the same screens |
| Words anyone can read | R12, R13, R14, R9, then the forms and messages bundle | the same screens and catalogues, one regeneration of the screenshots |

### T4. Restrict where the proxy may connect

Size S

**Why.** An editor can point a proxy at any host the server reaches: internal services, cloud metadata endpoints. Today only the network can stop it.

**What.** `PROXY_ALLOWED_TARGETS`: a list of hosts, domains or CIDR ranges that proxy targets must match, checked when a service is saved and again when a request is forwarded (after DNS resolution); link-local addresses refused unless listed.

**Done when.** A target outside the list is refused at save time and at forwarding time, with tests for DNS names that resolve to a forbidden address.

### T6. Rate limits

Size S

**Why.** The login endpoint and the management API have no throttling.

**What.** Per-client limits on `/api/auth/login` and on mutating API calls, with `429` and `Retry-After`; mocked services stay unlimited unless a rule asks for it (see U11).

**Done when.** Repeated failed logins are slowed down, as a test shows, without any cost on mocked traffic.

### T10. Leave the deprecated YAML crate

Size S

**Why.** `serde_yaml` is no longer maintained, nor is `unsafe-libyaml`, which it brings, and `smartstring` (pulled in by the script engine) is flagged unmaintained: `deny.toml` and `osv-scanner.toml` currently tolerate both notices.

**What.** Move to a maintained YAML implementation, keeping the file format byte-compatible; follow Rhai's releases and drop the `smartstring` exception as soon as a release no longer depends on it.

**Done when.** `cargo deny check` passes with an empty `ignore` list.

### T13. Nothing hidden, proved by a test

Size M

**Why.** Security teams now refuse a tool for what it could do unseen: write outside its folder, start other programs, call a vendor, change the settings of another tool. They ask for proof, not for a statement. Mimicway does none of this, but only says so.

**What.** The end-to-end job starts every instance it uses under a system-call trace (`strace` on Linux) and compares what it saw with a short list kept in the repository: files opened for writing (under `DATA_PATH` only), sockets (the listening address and the configured targets only), no program started. The job is red too when the trace lacks what must be there (the bind of the listening address, the write of the configuration file): an empty trace proves nothing. A `mimicway report` command, which reads its arguments without a new crate, prints what this instance does: version, commit, compiled features, listening address, data directory, every outbound flow its configuration allows, the limits in force; the security model points to it. The header of the job says what the trace does not see (other systems than Linux, code paths the suite does not reach).

**Done when.** A write outside `DATA_PATH`, or a connection the list does not hold, added on purpose to the code, turns the job red (seen once), and `mimicway report` lists the flows of the security model and no other.

### T15. The security reviewer's file

Size M

**Why.** Approving a tool means answering the same questionnaire each time: what data it holds, what it connects to, who signs in, what it logs, how updates and vulnerabilities are handled, how a download is verified. The answers exist, spread over the security model, the reviewer guide, the security policy and the release notes; the reviewer has to assemble them, and gives up or refuses.

**What.** One page for the reviewer, in English and French (`docs/en/security-review.md`): each usual question, its answer, and the proof (a file, a test, a command to run). What can drift is generated from the code and checked in CI: the outbound flows, the settings with their defaults, the authorization matrix. With each release: the dependency list embedded in the binary, so that a scanner reads it from the file alone; a statement of which published advisories do not apply and why (VEX); the number of third-party crates of the default build (150 today, build dependencies not counted), held in CI by a budget, raised as G2 says. Installing and verifying without network access is described and tested.

**Done when.** The commands of the page run in CI as written, like those of the API tour; a reviewer verifies a download and lists its flows and dependencies with that page and the binary alone; a crate added above the budget fails the CI.

### R10. Public supply-chain score

Size L, split: R16, R17, R18, and the item below; the signed releases came with 0.2.0

**Why.** Reviewers increasingly start from the OpenSSF Scorecard: it checks pinned dependencies, token permissions, branch protection, signed releases, CI tests and more, and shows the result as a badge. A low score turns people away before they read anything else.

**Progress.** The repository is public, `.github/workflows/scorecard.yml` publishes the score weekly and on pushes to `develop`, and the README shows the badge. The first report (commit 91964b8) gave 6.4: 10 on Dangerous-Workflow, Token-Permissions, Binary-Artifacts, Security-Policy, License, Maintained and Dependency-Update-Tool; 9 on Pinned-Dependencies; 7 on Vulnerabilities, now fixed (jsonwebtoken 11, quinn-proto 0.11.19, a justified exception for smartstring in `osv-scanner.toml`, and CI runs osv-scanner, the scanner Scorecard uses); 0 on Code-Review, Branch-Protection, SAST, Fuzzing, CII-Best-Practices and Contributors; CI-Tests, Packaging and Signed-Releases inconclusive (no pull request, no release yet). The report on fbf07d2 (2026-10-03) gave 6.6, Vulnerabilities at 10. CodeQL and the fuzz targets (R16) are in the CI: once pushed, SAST and Fuzzing should reach 10, 7.7 with the same weights (the model gives back 6.6 for the published report). The report on 6cd8a57 (2026-10-05) gave 8.5: SAST, Fuzzing and Packaging at 10 (the CI builds the container image), Pinned-Dependencies at 9 for the rustup installer piped to a shell, and Branch-Protection left out of the average after an internal error: a classic branch protection rule, added to `develop` on 2026-10-03, which the workflow's token cannot read (R17 replaces it with a ruleset, which it can). Counted at 0 as before, the same report gives 7.8, the model's 7.7 with Packaging measured. Done since: the Linux bootstrap script checks Rust and Node.js and says where to install them, without downloading anything (Scorecard's parser knows no checksum, so a download checked against a pinned hash would still count as unpinned), and the Windows one installs rustup with winget, which checks the installer's hash. The report on 9c6fc01 (2026-10-06) gave 8.6: Pinned-Dependencies at 10, and Branch-Protection still left out after the same internal error, the classic rule being still in place (the branch API shows `develop` protected, the rules API no active ruleset). Expected once the ruleset replaces it, before any release: 8.1 with Branch-Protection read at 3 (no force push, no deletion), 8.3 with the badge at passing (CII-Best-Practices 5, low risk). The report on 2f6cee2 (2026-10-06, 05:27 UTC) gave 8.6 again, every check unchanged, the classic rule still in place. So did the reports on 06c595c (2026-10-06, 18:46 UTC) and abc6167 (19:33 UTC): Branch-Protection, Signed-Releases and CI-Tests left out, Code-Review, CII-Best-Practices and Contributors at 0, every other check at 10; and the reports on a14874c and ad87cf3 (2026-10-07, before the release). The first report after the release of 0.2.0, on e8210df (2026-10-12, 19:29 UTC), gave 8.7: Signed-Releases at 10 (a signature and the provenance found among the assets of v0.2.0), Packaging at 10, Branch-Protection still left out after the same internal error, CI-Tests inconclusive, Code-Review, CII-Best-Practices and Contributors at 0, every other check at 10. Scorecard credits Packaging to the first workflow it lists with a publishing step and a successful run, here the CI's container job (it uses `docker/build-push-action`, to build and scan without pushing); Scorecard 5.5.0 run locally (`--local`, Packaging only) on the same tree without `ci.yml` finds `release.yml` instead (its image job), so the score does not rest on the CI job. Expected now: 8.3 with Branch-Protection at 3, 8.4 with the badge at passing as well.

**What.** The maintainers aim at 9.5. Computed with Scorecard's weights (critical 10, high 7.5, medium 5, low 2.5), once every item below is done: about 8.4 without R17 (Branch-Protection at 3, Code-Review at 0), 9.5 with it (Branch-Protection stays at 8: its next tier asks for two reviewers), 9.55 with a silver badge; 10 is out of reach for a one-maintainer project, since Contributors asks for regular contributors from three organizations. Here: replace the installers piped to a shell in `scripts/bootstrap-linux.sh` (rustup, and NodeSource's setup script for Node.js) (Pinned-Dependencies), by downloads checked against a pinned hash or by instructions, whichever Scorecard accepts.

**Done when.** The published score is 9.5 or more.

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

**Progress.** On 2026-10-03 the maintainer added a classic branch protection rule to `develop` and a ruleset, `default`, left disabled. Scorecard cannot read classic rules with the workflow's token, and its Branch-Protection check now ends in an internal error; it reads rulesets. Decided by the maintainers (2026-10-06): the classic rule goes, and the ruleset carries everything, starting with no force push and no deletion on `develop` (Branch-Protection 3), which leaves direct pushes possible until the pull request flow is ready. On 2026-10-06, after the pushes of 9c6fc01, 2f6cee2, 06c595c and abc6167, the classic rule was still there and no ruleset active, and still so on 2026-10-07 after the release, and on 2026-10-19 (the branch API shows `develop` protected, the rules API no active ruleset).

**Done when.** The last 30 changes of `develop` are approved pull requests (Code-Review 10), Branch-Protection is at 8 and CI-Tests at 10.

### R18. OpenSSF Best Practices badge

Size S

**Why.** Scorecard's CII-Best-Practices check is at 0. The badge (passing 5, silver 7, gold 10, gold needing several maintainers) records practices Scorecard cannot detect, with the evidence for each.

**What.** Fill the questionnaire at bestpractices.dev with the evidence the repository already holds (security policy, CI, tests, signed releases, vulnerability handling), fix what passing still lacks, and show the badge in both READMEs. Decided by the maintainers (2026-10-06): the target is passing. Silver asks for a second person able to take the project over (access continuity, bus factor), which a one-maintainer project cannot promise; it waits for one.

**Progress.** The repository was reviewed against the 67 passing and the 55 silver criteria, as the badge's own source lists them (2026-10-06). Added for them: ARCHITECTURE.md; GOVERNANCE.md (roles, decisions); how a vulnerability report is handled, and its reporter credited, in SECURITY.md; an assurance case at the end of the security model (requirements, threat model, trust boundaries, design principles, common weaknesses); a written rule that a new feature comes with its tests, and the Developer Certificate of Origin by reference, in CONTRIBUTING; a coding style per language, enforced by the CI (Prettier for the interface and the scripts, in a `format` job, and the Svelte compiler's warnings now fatal), with each silenced warning explained where it is; the documentation defects found on the way. Measured: the unit tests cover 82.6 % of the server's production lines (cargo llvm-cov) and 85.0 % of the interface's statements (Vitest, v8), and 47 of the 61 bug fixes of the last six months add a test case. Every passing criterion is met or justified (the evidence is linked on `develop`, pushed on 2026-10-06), provided GitHub's private vulnerability reporting, which SECURITY.md points to, is enabled: it was still off after the pushes of that day, the last at 19:33 UTC, and on 2026-10-07 after the release; it is on since (the API answers `"enabled": true` on 2026-10-19). Of the silver criteria, only access continuity and the bus factor are out of reach today; the signed releases of 0.2.0 cover the last other one. Left: the maintainer registers the project, fills the questionnaire with the answers kept ready, and gives its number for the badge in both READMEs.

**Done when.** The badge is at passing and Scorecard reads it.

### G1. The footprint, measured against the alternatives

Size M

**Why.** "Light" is a claim until everyone is measured the same way. A first run by hand (one mock, 5,000 calls, default settings) gave Mimicway more than twenty times less memory and processor time than two widely used alternatives, and an image fifteen times smaller. Nothing keeps these figures true, and nobody else can redo them.

**What.** A script (`scripts/footprint.mjs`) that starts Mimicway and each alternative it names, from their official images pinned by digest, gives each the same mocks and the same calls, and reads from the kernel the peak memory and the processor time, then the size of the image and of the file saved for a reference set of mocks. Each tool runs with the settings its own documentation gives; the method is written beside the table so that anyone can redo it or contest it, and so are its blind spots (desktop applications are not measured, one kind of machine). It runs in a job of its own, weekly and before a release, never on a pull request. The table of both READMEs is generated from its result.

**Done when.** The table of both READMEs comes from the job, and the job is red when Mimicway is not the lightest on memory, processor time or image in its own run; the saved file is shown without a verdict until G3. Each tool is compared with the others on the same machine in the same run; no time is compared with a stored value ([decision 2](decisions/0002-performance-is-judged-by-growth.md)).

### G2. Budgets that go down

Size S

**Why.** Each feature adds a little weight, and no check says when the sum has stopped being light: the binary is about 15 MB, the instance holds 11 MiB at rest, and nothing would notice 20 and 30.

**What.** A file of budgets read by CI, for what gives the same figure on every run: bytes of the release binary per target, bytes of the image, bytes of the interface sent at first load (E10), memory at rest with a stated margin, wake-ups per minute at rest (none: no timer, counted over a minute), and a start followed by the reference calls of G1 inside 32 MiB, the memory the Kubernetes manifests request. Time has no budget: its growth is judged by the scale test ([decision 2](decisions/0002-performance-is-judged-by-growth.md)). A budget is lowered when the measure drops, and raised only with a line in the changelog that says what the user gets for it, or the security update that needs it. Build settings for size (`opt-level`, `panic`, `codegen-units`) are tried once, each family of the scale test counted before and after as that decision asks, and kept only if no count worsens: size is not bought with time.

**Done when.** A change that adds 1 % to the binary without raising its budget is red in CI, and the binary answers the reference calls inside 32 MiB.

### T20. The accessibility judge reads WCAG 2.2

Size S

**Why.** The end-to-end suite judges every screen it reaches with axe-core, on the rules of WCAG 2.0 and 2.1 only (`frontend/e2e/fixtures.mjs`), while the interface is built for 2.2: target size, focus not hidden and the other criteria of 2.2 that a tool can check are not checked.

**What.** Add the 2.2 rule sets (`wcag22a`, `wcag22aa`) to the judge, fix what it then reports, or record it as a dated exception with its item.

**Done when.** The suite is green with the 2.2 rule sets on, and a target made too small on purpose turns it red (seen once).

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

### R9. Polish the French catalogues

Size S

**Why.** Many French messages were written without accents ("reserve", "deja", "regle"), which reads as careless to French users.

**What.** Restore accents and typography (non-breaking space before `:`, `«»` quotes) in `src/locales/fr.json` and `frontend/src/locales/fr.json`; the existing catalogue tests keep placeholders intact. Fix the mistranslations the French screenshots show, such as "Réécriture annuaire" for "Directory URL rewriting" and an example name that differs from the English one ("ex: get-siret" for "e.g. get-customer"). Give both typed confirmation keywords the same rule: the restore confirmation asks for a translated keyword (`RESTAURER`), the reset confirmation for `RESET` in every language (`App.svelte`); a translated keyword must stay easy to type on any keyboard, and the guide names it in each language. Update the French texts that `frontend/src/tests/french.test.js` expects, the only unit tests that read the French catalogue. Then regenerate the screenshots (`npm run docs:screenshots`): the French images show the catalogue as it is today.

**Done when.** A spell check of both French catalogues passes, and the French screenshots are regenerated from them.

## Release 0.7: a first mock without writing one

| Theme | Items, in order | Taken together because |
|---|---|---|
| From what exists | U1 (OpenAPI, then WSDL; its other readers come with C2), U8 | both fill the rule form from something that already exists |
| Recorded, without personal data | U2, U15, U24 | one recording path, cleaned as it is written, with the fake data that replaces what it removes |
| Guided and explained | U29, U18, U19, U28 | the first minute, then the same screens: home, rule list, request log |
| Shared | U13 | export and packs |

### U1. Import an API description

Size L, to split by format

**Why.** Writing the first mocks by hand is the main cost. Most teams already have an OpenAPI file, a WSDL, a Postman collection or a HAR capture.

**What.** Import an OpenAPI 3 or Swagger 2 document into a service: one rule per operation and status, response bodies from the document's examples, or generated from its schemas with fake data when there are none. Then WSDL (one rule per operation, matched on `SOAPAction` and the body element, with sample envelopes), then Postman collections, HAR files and pasted `curl` commands.

**Done when.** The Petstore OpenAPI document and a public WSDL import into working mocks, as end-to-end tests check, and nothing is sent anywhere during an import.

### U8. Request coverage and one-click rules from misses

Size S

**Why.** Users do not know which rules are dead, nor what the application asked that no rule answered.

**What.** Count hits per rule (shown in the rule list, reset with the log), and an "unmatched requests" view where "Create a rule" fills the rule form from the request.

**Done when.** A 404 from Mimicway can become a working rule in two clicks.

### U2. Record and replay

Size M

**Why.** Traffic observation suggests rules but leaves each one to accept by hand. Teams often want a faithful snapshot of a real backend in one go.

**What.** A "record" switch on a proxied service that turns every distinct exchange into a rule (grouped by the same correlation as suggestions), credentials removed, then switches the service to mock mode on demand.

**Done when.** Recording a session against a real backend, then cutting the network, replays the same responses for the same requests.

### U15. Fake data in every locale

Size M

**Why.** Fake data is French today (names, addresses, phone numbers, IBAN, SIREN), which looks odd to most users.

**What.** A locale per service (default from the UI language): names, streets, cities, postcodes, phone numbers and IBANs in that locale's formats; the French-specific kinds stay available.

**Done when.** A service in `en-US` and one in `de-DE` produce plausible local data, and each locale has tests for its formats.

### U24. Recorded without personal data

Size L, to split (the kinds recognised and their replacement, then the preview)

**Why.** Traffic recorded from a real backend carries real people: names, e-mail addresses, phone numbers, bank accounts. Recording removes credentials, not people, so a recorded mock spreads personal data to test machines, exports and repositories, which the team's data protection officer has to refuse.

**What.** While recording (U2) and in the suggestions from observed traffic, a value recognised as personal, by the name of its field and by its shape (e-mail, phone number, IBAN, card number with its checksum, the national identifiers of the locales of U15), is replaced by the matching fake-data placeholder in a seeded form, which templates lack today. The seed is a keyed hash of the original value, the key drawn for the recording and never saved: two responses about the same person stay consistent, and the original cannot be found again from the file. The original value is never written to disk. A condition on a personal value becomes a condition on its shape. A preview lists what was replaced, and lets the user add a field or keep one; the replay of U2 then holds for every value the preview does not list. The page says what the detection does not see (free text, images, fields named in a way it does not know).

**Done when.** Recording a backend that returns each recognised kind leaves none of the original values in the saved configuration (a test searches its bytes), two recorded responses about the same customer name the same fake customer, and a request recorded with an e-mail address in its query is still answered.

### U29. The interface, one click away

Size S

**Why.** Someone who starts Mimicway gets a log line that holds an address among other fields, and has to find it and type it in a browser; the interface is then one tab among many.

**What.** At start, one plain line that says where the interface is, written so that a terminal makes it a link. A web application manifest, so that the browser offers to keep the interface in a window and under an icon of its own. No desktop application and no launcher: the binary starts no program, and nothing is installed.

**Done when.** A test reads the address of the interface on its own line in what the binary prints at start, and another reads the manifest the interface serves, with its name, its icon and its start address.

### U18. A guided first mock in the interface

Size L, in three parts

**Why.** The demo at the top of the README shows the screens of a first mock, but someone who then opens the interface finds an empty home screen and a **Load an example** button, and nothing that walks them through the service form, a rule, a condition and the request log on their own instance: the screens are learnt by trial.

**What.** A tutorial panel, offered on the empty home screen and opened from the navigation bar, that walks through one scenario: create a service, call it, add a rule with a condition, see the answer change, read the request log. For every part:

- The scenario is a data file (`frontend/src/tutorial/first-mock.json`): its steps, the element each one points at by `data-testid`, the keys of its texts; the panel interprets it and holds no step of its own.
- No guided-tour dependency. The panel is a region with a heading that takes the focus when it opens, its step text announced politely (`aria-live`), every action reachable by keyboard; no animation, no overlay over the page.
- Offline entirely; English and French through the existing catalogues, whose extraction tests cover its texts.
- Closed at any step, it reopens where it stopped (the step kept in `localStorage`), and it can be turned off for good.
- The steps that change something act only on a demo service the tutorial creates for the occasion, never on one of the user's; closing it offers to delete that service.
- Once closed, it leaves no listener, observer or timer behind, which a test counts.
- Loaded on demand (`import()` when opened): the initial bundle does not grow.

Parts, in this order:

- **a. Panel and scenario** (M): the data file, the panel loaded on demand, the first two steps (create the demo service, call it from the panel).
- **b. Condition and log** (S): the rule with a condition, the changed answer, the log entry.
- **c. Resume, turn off, clean up** (S): reopening after a reload, the off switch, the deletion of the demo service, and the guide's screenshots in both languages.

**Done when.** From the empty home screen, a first-time user reaches a rule with a condition and its log entry without leaving the panel, by keyboard alone (an end-to-end scenario) and with each step announced (an accessibility check of the panel); the initial JavaScript of the bundle keeps its size in bytes; after closing, no listener or timer of the panel is left.

### U19. Rules and answers explained in words

Size M

**Why.** Someone who does not write code reads a rule as a list of fields and a log line as a status code, and asks a developer "why did I get this answer?". The rule tester already knows the reason, condition by condition, but only for a draft rule and a request chosen by hand.

**What.** Each rule of the list reads as a sentence built from the labels the forms already translate (R12): "When `GET /users/{id}` is called with `plan` equal to `vip`, answer 200 with…". Each line of the request log gets a "Why this answer" panel: the rule that answered and the conditions that decided; for a request no rule answered, the nearest rule and its first condition that failed. The verdict is computed on demand from the captured request, with the code of the rule tester; nothing is added to the path of a request. When the captured body was cut or a condition reads a redacted header, the panel says that the verdict cannot be recomputed for that condition.

**Done when.** An end-to-end test calls a service with a wrong parameter and finds, on its log line, the nearest rule and the failed condition, in English and in French; the suite searches each screen it opens for the names of the condition types, listed from the code, and finds none.

### U28. A problem report in one click

Size S

**Why.** When a mock does not answer as expected, the tester describes it in a ticket from memory, and the developer asks for the request, the rule and the version, one message at a time.

**What.** A "Copy a report" action on a log line and on a rule: a text that holds the request (secrets replaced as in the log), the verdict of each condition, the rule as saved and the version. It is copied to the clipboard and sent nowhere; pasted into the rule tester of another instance, it replays the case.

**Done when.** A test finds no secret header value in the report, and replays the report in the rule tester with the same verdict.

### U13. Partial export and mock packs

Size S

**Why.** Import and export only cover the whole configuration, so sharing one service between teams means editing JSON by hand.

**What.** Export a service or a group; import it into another instance with a preview of conflicts. A small gallery of generic, ready-to-import packs in `examples/` (an OAuth2 provider, a payment flow, a CI toolchain).

**Done when.** A group exported from one instance imports into another with its rules intact.

## Release 0.8: mocks that behave like the real service

| Theme | Items, in order | Taken together because |
|---|---|---|
| State and data | U5 a, U5 b, U21 | one in-memory state per service, shown in the request log |
| Situations and time | U4, U20, U6 | all three choose an answer from the context of the request |
| Real failures and payloads | U11, U12, U7, U25 | the response path: how a body is sent, from where, and what follows it |

### U5. Stateful mocks and instant CRUD

Size L, in two parts

**Why.** Many APIs are "create, then read what was created", or "start, then poll until it is done". Static rules cannot follow, and scripts have no memory. Today the end-to-end tests of a client that polls a build change a rule through the API between two calls to make the mock move on: a build seen "in progress" then finished, a "last build" that still names the previous build right after a replay and the new one at the next call, an import that fails then succeeds once corrected, a stream cut at its first read and whole at the next.

**What.** For every part: the state lives in memory, per service, like the call counter; a reset of the configuration, and any change of the service, put it back to its start; the request log shows it for each call; nothing new is written to the configuration file but the rules themselves.

Parts, in this order:

- **a. A state machine per service** (M): a service is in a named state, `start` until a rule moves it. A rule may require a state (it matches only in that state, beside its conditions) and move the service to another state after it answers, at once or after it has answered a given number of times in that state (a build "in progress" for two polls, then finished). The current state is a template variable (`{{state}}`), readable and settable through the API (`GET` and `PUT …/{name}/state`), and shown and reset from the service page. The rule form edits "only in state" and "then go to state" next to the conditions; the rule tester says whether the state of the service lets the draft rule answer.
- **b. Resource services** (M): a service type that, from a path such as `/orders/{id}`, answers POST, GET (one and list), PUT, PATCH and DELETE on an in-memory collection, seeded from examples, reset by API.

**Done when.** a: a test polls a build that answers "in progress" twice then "finished", reads a "last build" that changes after a replay, and reads a stream cut once then whole, with no call to the management API between the calls; a reset brings every case back to its start. b: a create-then-read test passes against a resource service with no rule written.

### U21. Data tables

Size L, to split (the table and the rule that reads it, then the grid)

**Why.** A tester holds the test data as a spreadsheet: twenty customers, their status, their balance. Turning it into mocks means one rule per row, or a script.

**What.** A table per service or per group: named columns and rows, edited in a grid that works by keyboard, imported from and exported to CSV (parsed in the browser, bounded in rows and bytes like the configuration; exported cells that start with `=`, `+`, `-` or `@` are written as text, so that a spreadsheet does not run them). A rule answers with the row whose key equals a value of the request (`{{row.name}}`), and another rule answers when no row matches. A resource service (U5 b) can start from a table.

**Done when.** A CSV of 1,000 customers, imported, answers `GET /customers/{id}` with the right row and without a script; the grid passes the accessibility check of the end-to-end suite; a cell that starts with `=` comes back as text from an export.

### U4. Scenarios selected per request

Size M

**Why.** Testing an error case means editing the mock and editing it back, which breaks parallel tests sharing an instance.

**What.** Named variants of a rule's response, picked by a request header (`X-Mimicway-Scenario: timeout`), a cookie or a query parameter, with a default; the UI shows the variants side by side.

**Done when.** Two test runs against the same instance get different scenarios at the same time.

### U20. Situations: one click changes several services

Size M

**Why.** A business case ("the bank is down", "unknown customer") spans several services. With U4 each rule has its variants, but someone has to know which rules of which services go together.

**What.** A named situation gathers variants of rules across the services of a group. It is switched from the home screen in one click, shown on every screen while it is on, and left in one click. A request can ask for one with a header of its own (`X-Mimicway-Situation`), so that parallel tests do not disturb each other; a variant asked by the header of U4 wins for its rule.

**Done when.** A tester switches a situation that changes two services without opening a rule, and two test runs use two situations on the same instance at the same time.

### U6. Frozen and shifted time

Size S

**Why.** Responses with dates (`now_iso`, `date_future`…) make snapshots and assertions flaky.

**What.** A clock per service or per request (`X-Mimicway-Time`): fixed at an instant, or shifted by a duration; templates and scripts read it instead of the system clock.

**Done when.** The same request with the same frozen time returns byte-identical responses.

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

### U7. Callbacks and webhooks

Size M

**Why.** Asynchronous APIs answer "accepted", then call back later (payments, document processing). Mocking them today needs another tool.

**What.** A rule can schedule an HTTP call after its response: target, delay, method, templated body and headers. The callback target is an outbound flow: it obeys the proxy allow-list (T4) and appears in the security model.

**Done when.** An end-to-end test receives the callback with values taken from the original request.

### U25. A mock pack for language-model APIs

Size S

**Why.** The tests of an application that calls a language model pay for each call, wait for it, get a different answer each time, and spend energy on a result nobody reads.

**What.** A ready-to-import pack in `examples/` (U13): a chat endpoint in the request format most providers accept, answers streamed as server-sent events, the same text for the same request and seed, and the usual failures (rate limit, stream cut half-way with U11, slow first token); a guide page shows how to point an application at it.

**Done when.** A sample client reads the same streamed answer twice, and sees each failure case as the transport error it expects.

## Release 0.9: tests can rely on it

| Theme | Items, in order | Taken together because |
|---|---|---|
| Assert and automate | U3, T2, U26 | the management API as tests call it, and the helpers that wrap it |
| The saved file | T9, G3 | the configuration file: its form on disk, its version, its size |
| Shipped | T8, U27 | what the release workflow publishes besides the archives |
| See what happened | U14, U22 | both read the request log after the fact |

### U3. Verify what the application called

Size M

**Why.** Automated tests need to assert that the application under test called a dependency, how many times and with what: today they can only read the request log by hand.

**What.** `POST /api/verify` with a request pattern (method, path, conditions like a rule's) and an expected count; `DELETE /api/logs` to start a test from a clean journal; a journal size setting for test runs.

**Done when.** A Java and a JavaScript example test use it to assert calls, in the docs.

### T2. API tokens for automation

Size M

**Why.** CI pipelines and test suites need to change mocks without a person's credentials.

**What.** Named tokens with a scope (read, write on given groups, admin) and an expiry, created by admins, stored hashed, shown once, revocable from the UI.

**Done when.** A token can drive the management API within its scope and nowhere else, which the authorization tests check for each endpoint.

### U26. Helpers for test frameworks

Size M

**Why.** A team adopts a mock server through its test framework: one line starts it, one line asserts on it. Without that line, each team writes its own HTTP calls to the management API.

**What.** For Java, a Testcontainers module and a JUnit 5 extension (start an instance, load a file, reset, verify with U3); for JavaScript, a helper for Playwright and Vitest. Each is a thin client of the management API with no dependency beyond its framework, kept in this repository and tested by its CI. Publishing them under the project's name on the public registries is the maintainer's step.

**Done when.** The Java and JavaScript example tests of U3 use the helpers, in CI.

### T9. Configuration as code

Size M

**Why.** Teams want mocks reviewed and versioned with the application, and the same mocks in every environment.

**What.** A read-only mode that loads services from files in a directory (one file per service or group, YAML or JSON), reloads on change, and refuses changes through the API; a `mimicway validate <dir>` command that checks files in CI; a JSON Schema of the configuration for editor completion.

**Done when.** A mock committed in a repository reaches a running instance without the UI, and an invalid file fails the CI command with the same message the UI would show.

### G3. The smallest saved file

Size M

**Why.** The configuration is saved, kept in up to 17 copies (the file, its temporary copy, 5 backups, 10 protected ones), exported, attached to tickets and committed. Every byte of it travels, and a file that repeats defaults and identical bodies is also harder to review.

**What.** A version in the configuration file, so that a default never changes inside a format version. Write no value that equals its default; store a body used by several rules once; keep the file plain text that reads and compares line by line. Older files load unchanged. Compressing backups and exports waits for a measure: it is not worth a crate. The reference set of mocks of G1 gives the measure, against the files the alternatives save for the same mocks, and G1 then judges that line too.

**Done when.** The reference set saved by Mimicway is smaller than the file of each alternative of G1 for the same mocks, and a configuration saved by each released version still loads.

### T8. Packaging for clusters and compose

Size M

**Why.** Kubernetes users expect a Helm chart, and compose users a ready file.

**What.** A Helm chart (published as an OCI artifact with the image) built from the same settings as `k8s/base`, and a `compose.yaml` at the root; security contexts stay as strict as the current manifests.

**Done when.** `helm install` and `docker compose up` both give a working instance, tested in CI with kind.

### U27. Install channels

Size S

**Why.** People install with the tool they already use; a release page they have to find, then verify by hand, loses most of them.

**What.** A Homebrew tap, a Scoop bucket, a winget manifest and a mirror of the image on Docker Hub, each written from the signed archives and their checksums, in a job apart from the signing job, each with a token limited to its repository. Opening the accounts is the maintainer's step.

**Done when.** `brew install` and `scoop install` give a binary whose hash is in `SHA256SUMS`.

### U14. Contract drift detection

Size M

**Why.** Mocks lie silently when the real API changes.

**What.** For a service with a target, compare on demand the mocked responses with the real backend's for the same captured requests, and report differences in status, fields and types.

**Done when.** Changing a field in the real backend shows up as a drift on its rule.

### U22. The flow of a user action, drawn

Size M

**Why.** One click in the application under test calls several services in a row. The request log shows them as lines among others; nobody sees the sequence, nor where it stopped.

**What.** From the request log, the calls of one action (the same trace identifier in `traceparent` when the application sends one, otherwise a time window chosen on screen) are drawn as a sequence: who was called, in which order, answered by a mock or relayed (the dashed and solid lines of the design system), with its status and duration. A table gives the same content to a screen reader. The drawing is made in the browser, on demand, with no dependency, and exports as SVG and as text.

**Done when.** An end-to-end test sends three calls with one trace identifier and finds them in order with their mode, in the drawing and in its table.

## Release 1.0: shared across a company

| Theme | Items, in order | Taken together because |
|---|---|---|
| Sign-in and traces | T1, T3, T16, U23 | who did what: the login, then the log every change writes |
| Deployed anywhere | T11, T12, T7, G4 | how the instance is exposed and what it reports about itself |
| Confined | T14 | the process itself, once its flows are final |
| Stated | T18 | the manual audit, once the screens of 1.0 exist |
| Frozen formats | T19, U16 | what an upgrade may no longer break; languages come as they are contributed |

### T1. Any OpenID Connect provider, browser login with PKCE

Size L, to split (discovery and validation first, then the login flow)

**Why.** Authentication works with Keycloak only, through the password grant: the UI sees users' passwords, and OAuth 2.1 drops that grant. Entra ID, Okta, Auth0, Google or GitLab users cannot log in at all.

**What.** Configure an issuer URL and a client; read its discovery document and keys; log in with the authorization code flow and PKCE (the UI never sees a password); map groups or roles from a configurable claim to super-admins. Keycloak becomes one provider among others, and its current variables keep working.

**Done when.** End-to-end tests log in against a fake provider through the redirect flow; the password form is gone when the new mode is configured.

### T3. Audit log of configuration changes

Size M

**Why.** "Who changed this mock, and when?" is the first question on a shared instance, and reviewers ask for traceability.

**What.** An append-only log of every change (who, when, what, a diff of the service or group), stored next to the configuration with rotation, visible to admins, exportable as JSON lines.

**Done when.** Every mutating endpoint writes one entry, which a test asserts for each of them.

### T16. An audit log whose edits show

Size S

**Why.** An audit log (T3) that an administrator of the machine can edit proves little to an auditor. And T3 records the changes of the configuration, not who signed in or was refused.

**What.** Sign-ins, accepted and refused, become entries of the log. Each entry carries the hash of the one before it; `mimicway report --audit` and the audit screen check the chain and name the first entry that breaks it. Rotation keeps the last hash of the file it closes. The hash of the last entry is printed by `mimicway report` and written to the server log at each rotation, so that it can be kept elsewhere; without that copy, a rewrite of the whole chain or a cut at its end is not seen, and the page says so.

**Done when.** A line changed or removed before the last entry is reported, with its position, by the command and by the screen, and a cut at the end is reported against a head hash kept aside.

### U23. History in words, undo for one service

Size M

**Why.** On a shared instance, "it worked yesterday" is answered today by restoring the whole configuration from a backup, which undoes everybody's work.

**What.** The entries of the audit log read as sentences ("Marie changed the answer of rule `vip` of service `users`"), filtered by service. "Restore this service as it was then" brings one service back and leaves the others as they are, after a preview of what changes. For that, the log keeps the whole service after each change, not only its difference (an addition to T3); history goes back as far as the rotation.

**Done when.** A test restores one service from its history and finds a later change of another service intact.

### T11. Serve under a path prefix

Size M

**Why.** The UI loads `/assets`, `/runtime-config.json` and `/api` from the root of its host, so Mimicway cannot live under a path such as `https://tools.example.com/mimicway/`, which many platforms impose (one host, one prefix per tool).

**What.** A `BASE_PATH` setting: the server mounts the UI, the API and the mocked services under it, the UI is built with relative asset paths and reads the prefix from `runtime-config.json`, and service URLs shown in the UI include it.

**Done when.** An end-to-end run with `BASE_PATH=/mimicway` behind a prefix-stripping proxy passes, and a Kubernetes overlay shows it.

### T12. Services on their own port

Size L, to split (the second listener first, then the interface's URLs, then the overlays)

**Why.** Service responses render in an opaque origin, so that a page written by an editor, a backend or a reflected request value cannot act as the interface ([decision 1](decisions/0001-service-responses-in-an-opaque-origin.md)). A proxied web application that needs its own origin (cookies, storage, same-origin calls) therefore no longer works when browsed through Mimicway.

**What.** A `SERVICES_PORT` setting: the mocked and proxied services answer on that port, the interface and the management API on `PORT`; their responses then keep their own origin, without the sandbox. The interface shows service URLs with the right port, and the overlays route a second host to it.

**Done when.** With `SERVICES_PORT` set, an end-to-end test browses a proxied page that uses its own storage, and finds the interface unreachable from it; without it, nothing changes.

### T7. Metrics and traces

Size M

**Why.** Operators want to know whether the mock answers, how fast, and which rules are hit, in the tools they already use.

**What.** An opt-in Prometheus endpoint (requests by service, rule, mode and status; latency; proxy errors; configuration writes) and OpenTelemetry traces exported to a configured endpoint. Both off by default, both listed as outbound flows when on.

**Done when.** A Grafana dashboard example in `docs/` works against a running instance.

### G4. What this instance costs, on screen

Size S

**Why.** A team that cares about its footprint has no figure for its mock server, and no way to show that answering locally spares calls to real backends.

**What.** A page for administrators: memory now and at its peak, processor time since the start, bytes on disk, requests answered by a mock and requests relayed. Figures are read when the page is opened, by no timer. No carbon estimate: it could not be honest without knowing the machine and its electricity.

**Done when.** A test compares the figures of the page with the kernel's for the same process.

### T14. The process confines itself

Size L, to split (the rules on files first, then what newer kernels add)

**Why.** Every request is untrusted input read by parsers. In a container the manifests restrict the process; on a laptop it runs with all the rights of its user, and a flaw in a parser would reach that user's files.

**What.** On Linux, before the runtime starts its threads, `main` reads the settings and restricts itself: Landlock rules through the `landlock` crate (no `unsafe` of our own; the dependency is justified in the reviewer guide): read and write under `DATA_PATH`, read under `STATIC_DIR` when set and on `/etc/resolv.conf` and `/etc/hosts`, no execution of any file, `no_new_privs`. Ports are not restricted: targets and raw TCP listeners are created at run time, and a Landlock domain never widens. On a kernel without Landlock it starts and says so, in its log and in `mimicway report`: never a silent fallback. Windows and macOS are documented as not confined, with a container or a dedicated account as the advice.

**Done when.** An integration test starts the binary, reads `NoNewPrivs: 1` for every thread under `/proc/<pid>/task`, and sees a read and a write outside the allowed paths refused on each worker thread, through a route compiled for that test only.

### T18. An accessibility statement

Size M. The audit is the maintainer's step, or an auditor's.

**Why.** Automated checks see only part of the WCAG criteria (about a third, by the usual estimates). Public bodies and large companies ask for a statement before they deploy a tool to their staff, and people who need assistive technology want to know what works before they try.

**What.** A manual audit of every screen against WCAG 2.2 AA, then against the clauses EN 301 549 and RGAA add to it, from their own lists, with two screen readers. Its grid (screen by criterion) is kept in the repository. A statement in English and French: what conforms, what does not with its item in this file, how to report a barrier, the version and date of the audit. Redone at each release that adds a screen.

**Done when.** The audit grid is in the repository, the statement is published from it, and each non-conformity it lists is an item here.

### T19. Formats frozen at 1.0

Size M

**Why.** Before a team builds on Mimicway, it wants to know that an upgrade will not break its files, its scripts or its pipelines.

**What.** A configuration saved by every released version kept as a fixture and loaded in CI, with the version G3 writes in the file; the management API, the settings and the template language under a written rule (additions only inside a major version, a removal announced one release ahead in [MIGRATING.md](MIGRATING.md)).

**Done when.** The configuration of each released version loads in CI, the answers of the API tour are compared with those of the last release, and the rule is written.

### U16. More interface languages

Size S per language

**Why.** English and French cover a small part of the people who could use Mimicway.

**What.** Spanish, German, Portuguese, Italian and Japanese catalogues for the UI and the server messages, contributed and reviewed by native speakers; the existing tests already check each catalogue.

**Done when.** Each language passes the catalogue tests and a native speaker's review.

## Release 1.1: more protocols

| Theme | Items, in order | Taken together because |
|---|---|---|
| Protocols | U10 (GraphQL, then WebSocket, gRPC if asked for), the Kafka and TCP bundle, T17 once decided | each adds a way to receive and answer a message |
| Identity | U9 | a service type of its own |

### U10. More protocols

Size L, to split by protocol

**Why.** Modern applications also talk GraphQL, WebSocket and server-sent events.

**What.** Server-sent events are done (a rule answers with a list of events). Then GraphQL (match on operation name and variables, answer per operation), then WebSocket (message-to-message rules, like the raw TCP mock), and gRPC last if asked for.

**Done when.** Each protocol has a guide page and end-to-end tests.

### T17. HTTPS without a reverse proxy

Size M. **Decision needed** (whether the binary terminates TLS at all, and never with a certificate it would generate and whose key it would have to guard).

**Why.** Some applications refuse a dependency served over plain HTTP, and the main alternatives listen on HTTPS themselves. Today Mimicway asks for a reverse proxy, which a tester on a laptop does not have.

**What.** `TLS_CERT_FILE` and `TLS_KEY_FILE`: the services, and the interface, answer over HTTPS with a certificate the user provides, read by the TLS library the proxy already uses; the guide shows how to make one for a laptop. No certificate authority, no generated key, no interception of traffic.

**Done when.** An end-to-end run passes over HTTPS with a certificate made by the test; a key file that others can write is refused, and one that others can read gives a warning.

### U9. Built-in identity provider mock

Size M

**Why.** Almost every application under test needs tokens. Running a real identity server for tests is heavy, and hand-made JWKS mocks are error-prone.

**What.** A service type that behaves as an OpenID Connect provider: discovery document, key set, token endpoint (client credentials, password, authorization code with PKCE), with configurable claims and lifetimes, signing keys generated per instance.

**Done when.** A sample application configured with Mimicway as its issuer logs in and validates tokens.

## Release 2.0: the request side

Mimicway answers calls; this release also sends them. The aim is to replace a separate API client for the people who already keep Mimicway open, with the same promises: one program, no account, nothing synchronized, nothing hidden. A request is sent by the server, so it is an outbound flow like the proxy's, written in the security model and listed by `mimicway report` (T13).

| Theme | Items, in order | Taken together because |
|---|---|---|
| Send | C1 | the composer and its outbound flow, reviewed alone |
| Collections | C2 (the file format first, then one import at a time) | one format on disk, read by everything after |
| Checks and runs | C3, C4 | a check is defined once, run on screen and in CI |
| One loop | C5 | joins what is sent with what is mocked |

### C1. Send a request from the interface

Size L, to split (the composer, the sending by the server, the display of the answer)

**Why.** To try a mock, or the real API behind it, the user leaves for another tool, then comes back to read the log. The guided first mock (U18) already calls the demo service from its panel; nothing does it for the user's own services.

**What.** A composer: method, URL, headers, body, with the helpers of the response templates. The server sends the request (no cross-origin limit, one place to bound and to log). Where it may go: on an instance that listens on this machine only, anywhere, as any tool of its user could; on an instance that listens beyond it, only to the targets of `PROXY_ALLOWED_TARGETS` (T4), an unset list allowing the loopback and the targets of the user's own services, and, with authentication on, only for a member of a group. Redirects are not followed (the answer is shown), and the address connected to is the one that was checked. The response is shown like a service response, in an opaque origin ([decision 1](decisions/0001-service-responses-in-an-opaque-origin.md)), bounded in size and streamed beyond. A line of the request log can be sent again, edited first. The security model, which says today that Mimicway never connects to a host taken from a request, is rewritten for this flow.

**Done when.** End-to-end tests send a request to a mocked service and to an allowed external target, see a target outside the list refused on an instance that listens beyond this machine, see a redirect to a refused target shown and not followed, and find that the displayed response cannot reach the interface.

### C2. Collections as plain files, and imports from other clients

Size L, to split (the format, then one import at a time)

**Why.** Requests worth keeping belong with the code they test, reviewed and versioned like it. Clients that keep them in an account, or in one large generated file, make that hard, and have lost the trust of teams whose secrets ended up synchronized.

**What.** A collection is a directory of small text files, one per request, that read and compare line by line, under `DATA_PATH/collections` or in a directory the operator names in a setting (a repository). Environments hold variables. A secret is never written to a request file, a log or a report: in the interface it belongs to the user who typed it, is kept in memory, and is redacted by value in the request log; a running instance never reads its own environment for a request. Imports, one at a time: Postman collections and environments, Bruno collections, `.http` files, `curl` commands and HAR captures, with the readers of U1.

**Done when.** A public Postman collection and a Bruno collection import and send their requests; a test sends a secret in a header and in a body to a mocked service, then searches every saved file and the request log for its value and finds none.

### C3. Checks and chained values without code

Size M

**Why.** Checking an answer is where most clients ask for JavaScript, which shuts out the people who do not code and brings a general-purpose runtime into the tool.

**What.** A table of checks per request, in words: "the status is 200", "`body.id` exists", "the header `X` contains…", "answered in less than 300 ms". A value of a response is kept under a name and used by the next request (sign in, then call). What a table cannot express goes to a Rhai script, in the same sandbox and with the same limits as the scripts of rules.

**Done when.** A "sign in, then call with the token" chain passes its checks without a script, by keyboard alone.

### C4. Run a collection from the command line

Size M

**Why.** Checks that only run on screen are forgotten; a pipeline needs an exit code and a report.

**What.** `mimicway run <directory> --env <name> --report <file>`: the same binary, no server started, no account, an exit code, a JUnit XML report and a text one, written where the command line says. Secrets come from the environment of the process, and only from variables named `MIMICWAY_SECRET_*`.

**Done when.** An example pipeline in the docs runs a collection and fails when a check breaks, and a variable of the environment outside that prefix cannot be read by a request.

### C5. One loop: from a request to its mocks and back

Size M

**Why.** A client shows the answer; a mock server shows what was called. Only a tool that is both can show, for one request, the calls it caused behind it.

**What.** A request sent from the interface shows the calls its target made to the services Mimicway mocks or relays (the flow of U22). "Make a mock of this response" turns the answer into a rule, and "Make a request of this rule" turns a rule into a request of the collection, one click each.

**Done when.** An end-to-end test goes from a sent request to a rule that answers the same, and back, in two clicks each way.

## Kept open

Ideas that fit the promises but wait for a decision of the maintainers; none is scheduled.

### U30. Try it in the browser, nothing installed

Size L. **Decision needed** (a hosted page is a publication, and a second build target to keep).

**Why.** Trying a tool that has to be downloaded first is already an approval request in many companies.

**What.** The rule engine (matching, templates) compiled to WebAssembly in a static page: write a rule, send it a request, read the answer and its explanation, with no server and no network call.

**Done when.** The page works offline once loaded, and its rule engine passes the same test vectors as the server's.

### U31. A doorway for assistants

Size M. **Decision needed** (whether to offer it at all; never installed by Mimicway into another program).

**Why.** Teams that work with a coding assistant want it to write and read mocks; assistants speak the Model Context Protocol, not a management API.

**What.** An endpoint, off by default, that offers the management API over that protocol, with the tokens and scopes of T2. The user declares it in their own tool; Mimicway writes nothing outside its data directory to make itself known.

**Done when.** With the endpoint on, a client of the protocol lists and creates a mock within the scope of its token; with it off, its path answers 404.

## Engineering backlog

Defects and debts found on the way. They are taken by bundle: the items of a bundle open the same files, and a bundle goes with the theme named beside it, or before it when it unblocks it.

| Bundle | Items | Goes with |
|---|---|---|
| The judges | E52, then E50; E49 waits for the maintainer | first, before the themes |
| Weight | E10 | "Lightest, with the proof" (0.6) |
| Keyboard | E42, E47 | "Accessible" (0.6) |
| Screen reader | E15, E43 | "Accessible" (0.6) |
| Forms and messages | E8, E12, E19, E21, E31, E32 | "Words anyone can read" (0.6) |
| Proxy path and suggestions | E7, E34, E35, E36, E46, E59, and E48 once it is unblocked | "Recorded, without personal data" (0.7) |
| Builders and rule engine | E2, E28, E45, and E9 and E54 once decided | "Guided and explained" (0.7) |
| Tests | E6, E11, E16 | the first theme that touches them |
| Kafka and TCP | E3, E4 | "Protocols" (1.1) |

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

**Why.** The guide says a SOAP service either relays WSDL requests to the real backend (`Proxy`/`Auto`) or answers them with its rules (`Mock`), but the service form offers no such choice: `Mock` can only be set through the API or the configuration file (saving the form now keeps it instead of resetting it to `auto`). And a REST service, which the guide describes as having "no SOAP-specific handling", still sends `?wsdl` requests to the backend past its rules (`auto`, the default), which on a purely mocked service ends in a 502. `Auto` and `Proxy` behave the same. The form's REST/SOAP selector changes none of this: its only trace is the `rewrite_directory_urls` key of the service, which no code reads and which only stores the selector's choice.

**What.** Offer the WSDL mode in the form for a SOAP service; decide what a REST service does with `?wsdl` (its rules, like any request, or the current relay) and make the guide say it in both languages; give `Auto` a meaning of its own or drop one of the two values. Store the service type in what it decides (the WSDL mode) or drop the selector, so that the `rewrite_directory_urls` key can stop being written.

**Done when.** A SOAP service's WSDL mode can be set and read back in the form, the guide describes what REST and SOAP services do with `?wsdl`, and end-to-end tests cover each mode.

### E3. Kafka parity

Size M

**Why.** Kafka rules cannot run scripts, and a single global topic is listened to.

**What.** Run the rule's script for messages, and allow a topic per service.

**Done when.** A Kafka rule can compute its reply with a script, tested with the simulator.

### E4. Raw TCP mock, next step

Size M

**Why.** The TCP mock answers fixed bytes only.

**What.** Templated responses that reuse bytes of the request (an identifier echoed back, a length recomputed), and a capture log like the HTTP one.

**Done when.** A request-response protocol with a correlation identifier can be mocked.

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

### E19. A listen path check in the service form that refuses nothing

Size S

**Why.** `validatePath` in `ServiceForm.svelte` always returns `null`: the form looks as if it checks the listen path, and its four unit tests on the listen path can only show that a form is saved, never that a path is refused. The server already refuses the one invalid case, a pattern that would hit an internal route of Mimicway (`src/server/validation.rs`).

**What.** Remove the stub and its call, or make it refuse what the server refuses, so that the form says it before a round trip; the four tests then say which one it is.

**Done when.** No check in the form returns a constant, and a listen path the server refuses is shown as an error on the form, with a unit test that fails without it.

### E21. Feature buttons that follow the signed-in user

Size S

**Why.** The interface finds out once, when the page loads, whether the binary has Kafka and raw TCP mocks (`init` in `frontend/src/App.svelte`, from `GET /api/messaging/status` and `GET /api/tcp/status`). With authentication on, a first visit has no token yet: both routes answer 401, and the "Kafka messages" and "TCP mock" buttons stay hidden after signing in, until the page is reloaded; signing out and in as another user without reloading keeps the buttons of the previous one (`handleLogin` and `handleLogout` only load or clear the data; read in the code, not yet seen in a browser). And the Kafka page only reads the message log and simulates messages, both reserved to super-admins, yet its button shows to every signed-in user, who then gets an error.

**What.** Check again what the signed-in user may open after signing in and after signing out, and show each button only to the users the server lets use its page.

**Done when.** A unit test of the application signs in on an instance with authentication and finds, without reloading, the buttons the server allows to that user, and none after signing out.

### E28. Limits of a script value that let a list of objects through

Size S

**Why.** Rhai counts the 500 map entries and the 1,000 array items of a value across everything it holds, nested maps and arrays included: a request whose JSON body lists 251 objects of two fields is refused by `parse_json` ("a map of more than 500 entries"), although the guide promises "1,000-item arrays, 500-entry maps", which a reader takes per array and per map. These limits were the only bound on the memory of a run; since a run stops at 16 MiB of memory counted by the allocator, they no longer need to be that tight.

**What.** Raise the array, map and text limits of a value to what the memory budget of a run allows (for instance 10,000 items and 5,000 entries), measure the slowest and the largest legitimate script again, and describe the limits as totals per value in the guide (both languages) and the README.

**Done when.** A JSON body of 1,000 objects of three fields parses and is written back within the time and memory budget of a run, the regression test of the budget still passes, and the documented limits say that they count a whole value.

### E31. Guide screenshots that show a removed line

Size S

**Why.** `docs/*/screenshots/rule-create-form.png` still shows "Directory URL rewriting" on the service page, a line removed in 0.3.0: the screenshots are kept only when their content changes, and none was taken again after that removal.

**What.** Regenerate the screenshots of the service page in both languages, keep those whose content changes, and add to the review of a removed interface text a search of the screenshots that showed it.

**Done when.** No screenshot shows a text that the interface no longer has, checked against the list of removed catalogue entries.

### E32. The reason of a refused request body, shown by the interface

Size S

**Why.** A request body that does not match the expected type is refused by axum with a 422 and a plain-text reason; `request()` in `frontend/src/lib/api.js` reads only a JSON `error`, so the interface shows "422 Unprocessable Entity" and nothing of the reason (an import with a missing field, for instance). The import of a removed script block is checked by the interface first for this reason.

**What.** Errors of the management API as JSON `{ "error": … }` for a refused body too, worded in the language of the request, and the interface showing them.

**Done when.** An import with a missing field shows which field, in both languages, shown by an end-to-end test.

### E34. The request log names the rule that forwarded a request

Size S

**Why.** A request that a rule forwards to the backend (`"action":"proxy"`) is logged with `"mode":"proxy"` and `"rule_matched":null`, like a request relayed by a service in proxy mode, where no rule runs: the log cannot tell which rule forwarded it, while the README and the request log guide say that an entry names its matching rule. Seen on the entry that step 4 of the README tour leaves.

**What.** `RequestLog::log_proxy` (`src/server/request_log.rs`) takes the rule that forwarded the request, none for a service in proxy mode; the log screen shows it as it does for a mocked request.

**Done when.** A test forwards a request through a proxy rule and reads the rule's name in the log entry, and one relayed by a service in proxy mode reads none.

### E35. Suggested rules that use header templates

Size S

**Why.** Header values are templates now, but the suggestions from observed traffic still write them as literal text: a `Location` sent to a service of Mimicway holds the host seen while observing (`OwnAddress` in `src/server/suggestion/links.rs`), so it breaks when the application calls another address, while the same address in a body is built from the request (`{{header.host}}`). And a response that repeats the value telling the calls apart in a header is not generalized (`echo_response` in `src/server/suggestion.rs`), where the same value in the body is.

**What.** The redirected part of an address header is written like the one of a body, from the request the rule answers; an echo response repeats the value in its headers too. Everything else a backend sent stays literal (`literal_template`).

**Done when.** A suggested rule answers a `Location` with the host of the request that reaches it, not the observed one, and an echo rule generalizes a value found in a header; the property tests that replay suggestions on the calls they come from still pass.

### E36. Shutdown that waits for no held response

Size S

**Why.** On a stop signal, the server stops accepting connections and waits for the responses in progress (`serve` in `src/server/limits.rs`), then flushes the pending writes of the configuration (`src/main.rs`). A response held on purpose keeps it waiting: a chaos delay, which has no upper bound, or a stream of server-sent events, up to ten minutes. The Kubernetes manifests give the pod 10 seconds (`terminationGracePeriodSeconds`), after which it is killed before the final flush.

**What.** On a stop signal, held responses end at once: a chaos delay answers as if it had elapsed, and an event stream is cut as if its rule said so, so that the drain takes the time of the real work only. A chaos delay gets an upper bound checked when the rule is saved, like the delays of a stream.

**Done when.** A test stops a server holding an open stream and a long chaos delay, and sees it drain and flush within a second; a chaos delay past its bound is refused when saved.

### E42. Arrow keys in the response format selector

Size S

**Why.** The response format selector is a `role="radiogroup"` (`frontend/src/lib/components/RuleResponseSection.svelte`) made of buttons: each one is a tab stop, and the arrow keys do nothing, while a radio group is expected to move its selection with them (WAI-ARIA radio group pattern, RGAA criterion 7.1).

**What.** One tab stop for the group, the arrow keys moving and selecting, like native radios, with the mode change still asking before it discards the edited response.

**Done when.** A component test and an end-to-end test choose every format with the arrow keys alone, and the change of mode still asks for confirmation when the response was edited.

### E43. The raw TCP mock screen seen in a browser

Size S

**Why.** The screen of raw TCP mocks (`frontend/src/lib/components/TcpServiceManager.svelte`) has component tests, but no end-to-end test opens it on a binary built with `tcp-mock`, and no screenshot of the guide shows it: whether it works in a browser was never seen.

**What.** An end-to-end scenario on a `tcp-mock` binary that creates a TCP mock in the screen, sends it a frame from the test and reads the answer, plus the screenshot of the guide in both languages.

**Done when.** The scenario runs in the CI on a `tcp-mock` binary and passes, and the guide shows the screen.

### E45. A rule hidden by several rules together

Size M

**Why.** The conflict warning compares rules two by two (`src/engine/shadowing.rs`): a rule whose requests are all taken, but by two earlier rules between them (`q=1` then `q=2` before a rule on `q=1` or `q=2`), never triggers and is not reported.

**What.** Inclusion in the union of the earlier rules, decided on the same model of independent fields, with a bound on the work for a service with many rules; the warning names every rule that takes part.

**Done when.** The property test of the warning (`src/engine/matcher/conflict_judge.rs`) generates three rules and requires the report exactly when `first_match` never reaches the last one, and a service of 200 rules is checked within 50 ms.

### E46. `OPTIONS` requests that reach the services

Size S

**Why.** The CORS layer of the router (`BrowserGuard::cors_layer`, tower-http) answers every `OPTIONS` request itself, with an empty 200 and its own headers, before Mimicway looks for a service. A rule on `OPTIONS`, which the rule form offers, never answers, and a service in proxy mode never forwards `OPTIONS` to its backend: an application that asks the real service which methods it allows gets Mimicway's answer. Browsers' preflights to services are meant to stay open (`api_cors_is_limited_to_listed_origins_while_mocks_stay_open`), but a preflight is an `OPTIONS` with `Origin` and `Access-Control-Request-Method`; other `OPTIONS` requests are ordinary requests.

**What.** Only CORS preflights are answered by the layer; any other `OPTIONS` goes to the service, mocked or forwarded, and its response gets the same CORS headers as any other service response.

**Done when.** A rule on `OPTIONS` answers a request without `Access-Control-Request-Method`, a preflight to a service is still answered as today, and the proxy judge (`src/engine/proxy/judge.rs`) generates `OPTIONS` again and passes.

### E47. Keyboard access to the suggestion list of the script editor

Size S

**Why.** axe-core, run live on the rule form, reported the suggestion list of the script editor (`#rule-script-rhai-suggestions`) as a scrollable region that the keyboard cannot reach (`scrollable-region-focusable`, WCAG 2.1.1). The list follows the combobox pattern (the focus stays in the editor, the arrow keys move the active option), which the rule does not account for; whether a long list scrolls to the active option, and whether every option can be reached, was never checked. The accessibility check of the end-to-end suite judges copies of still screens, where the list, shown only while the editor has the focus, never appears.

**What.** A keyboard test that opens a list longer than its height and goes through every option with the arrow keys, each one scrolled into view; then either the list made reachable as axe-core asks, or the rule set aside for this element in `frontend/e2e/a11y-exceptions.json` with the reason, if the test shows the keyboard reaches everything.

**Done when.** The keyboard test passes in the CI, and axe-core run on the open list reports nothing that is not explained.

### E48. The proxy judge on a tower-http upgrade, `Vary` included

Size S

**Why.** tower-http 0.7 (0.6.11 today) cannot be taken as it is: the dependency update was left open because the proxy judge (`src/engine/proxy/judge.rs`) does not say whether the CORS layer still leaves a relayed response as the backend sent it, `Vary` in particular, which the layer rewrites. The tree also holds two copies of tower-http once reqwest is on its own version, which doubles what the build compiles and the review has to read. It touches E46, which moves where the layer answers.

**What.** The judge generates `Vary` (and `Origin`, `Access-Control-Request-Method` on the request side) among its headers and requires the relayed response to carry the backend's `Vary` values, with the layer's own added and none lost. Then the upgrade of tower-http, once reqwest and Mimicway agree on one copy (`cargo tree --locked -d` shows a single tower-http).

**Done when.** The judge is red on a layer that drops or replaces `Vary`, green on the current one, and tower-http 0.7 passes it with one copy in `cargo tree --locked -d`.

**Blocked** on the single copy: every published reqwest still requires tower-http `^0.6.8`, the newest (0.13.5, 2026-09-08) as well as the one in use (0.12.28), so tower-http 0.7 would add a second copy today whatever reqwest version is taken. The judge part can be done before; the upgrade waits for a reqwest release on tower-http 0.7 (`curl https://crates.io/api/v1/crates/reqwest/<version>/dependencies`).

### E49. A red run of the Rust job on the merge of the Rust minor and patch group

Size S

**Why.** The run of the CI on the merge of the `rust-minor-and-patch` group (7 crates, `Cargo.lock` only) failed in `cargo test --features tcp-mock` (exit code 101), while the commit before it passed; the step `cargo test` before it, on the same tests without the feature, passed. The log cannot be read without a token, and the public annotations only give the exit code. Two tests were found failing at random on the code before that merge, neither tied to the 7 crates, both fixed since: the conflict judge (`conflict_reported_exactly_when_first_match_never_reaches_the_rule`) failed 2 runs in 60 on fresh seeds, on a rule hidden behind a condition `id == ""` that a captured segment can never meet; the raw TCP API test `creations_at_the_same_time_are_all_kept_or_refused` failed when two of its eight ports, each read from a closed port-0 listener, were equal, which the kernel gave in 38 runs out of 20,000. The first is the likely cause, not a proven one. A rerun of the same run on the same commit passed: the failure was intermittent, not a break of one of the 7 crates.

**What.** Read the log of the first attempt of that run (a maintainer, with a token) and name the failing test; if it is neither of the two, open its ticket.

**Done when.** The test that failed is named, and the Rust job is green on the commit that follows.

### E50. Property tests that draw new cases on every CI run

Size S

**Why.** The property tests (`conflict_judge.rs`, `json_pointer/tests.rs`, `template.rs`, the two `proptests.rs` of `server/suggestion/`) draw new cases on every run. A defect they find then turns the CI red on whatever commit runs next, as a dependency update did with the conflict judge (E49), and the failing case is lost with the runner: the `proptest-regressions/` file stays on it, and the log needs a token. A fixed seed would keep the CI on known cases and leave exploration to a run by hand; a drawn seed keeps finding defects.

**What.** Decide between a fixed seed in the CI (`PROPTEST_RNG_SEED`, read by proptest 1.12 when the test's config keeps the defaults), with an exploration run of many seeds when a judge or its subject changes, and drawn seeds printed in the job so a red run can be replayed. Write the decision where the tests are, then apply it to every property test, found by reading the code.

**Done when.** Every property test runs on the chosen seeds in the CI, a test or check refuses one that does not, and a red run can be replayed from what the job shows.

### E52. The rest of the rule engine in the mutation gate

Size M (one file at a time, the largest first)

**Why.** The mutation gate (`scripts/mutants-gate.sh`, CI job "Mutation testing of the rule engine") covers `src/engine/matcher.rs` only. There, 145 mutants left 6 survivors: five changes of behaviour that no test saw, now caught, and one equivalent. The other files of `src/engine/` hold 913 mutants that were never run (`cargo mutants --list`): `template.rs` 285, `script.rs` 167, `script_budget.rs` 104, `renderer.rs` 93, `template_dates.rs` 86, `shadowing.rs` 55, `script_json.rs` 45, `proxy.rs` 43, `json_pointer.rs` 25, `regex_cache.rs` 10. `matcher.rs` took 64 minutes with 3 jobs on a loaded machine, 53 seconds per caught mutant on average.

**What.** For each file, `cargo mutants -f <file>`; each survivor is caught by a new test or explained as equivalent (in the test module of its file and in `.cargo/mutants.toml`), then the file joins `SCOPE` in `scripts/mutants-gate.sh`. A file whose code only the integration tests reach (`tests/script_budget.rs` runs scripts) gets the test arguments it needs. The weekly run of the whole scope is split into shards (`--shard k/n`, one job each) once it would pass the six hours of a GitHub job: at about 40 seconds a mutant, the whole engine is 12 hours in one job.

**Done when.** Every file of `src/engine/` is in the gate's scope, and the weekly run of the whole scope passes within the job limits.

**Progress.** `matcher.rs` (145 mutants) and `template.rs` (285: 33 survivors caught, 2 equivalent) are in the scope: about 4.8 hours for the weekly run at 40 seconds a mutant. The weekly run is split into four shards (`MUTANT_SHARDS` in `scripts/ci-plan.mjs`, one matrix job each, round-robin), and the gate fails a CI job (a shard, or the diff of a push) of more than 400 mutants before judging any. The tests of a mutant get 2 GiB of data at most: the first weekly run on GitHub (2026-11-09, `matcher.rs` alone) was stopped after 55 minutes with exit code 143, and the endless loop of `url_decode` (`i *= 1`) fills 6 GB in 19 seconds without the limit, 7 seconds to its abort with it. `script.rs` joined on 2026-11-10: its first run left 38 survivors, 23 of them in the depth counting of `parse_xml_items`, which was wrong (items lost after a sibling, found under a parent from another branch, self-closing ones skipped); it now follows its path from the root as XPath conditions do, judged against a walk of generated trees, and the other survivors (`parse_date`, `seeded_int`, `xml_element`) are caught by judges that generate their cases. `script_budget.rs` joined on 2026-11-12: its first run left 30 survivors, none a defect: the length a `replace` computes before building its result, the shortcuts of the `split` family, the bound of a value's text form and the messages of limits reached inside functions had no test at their edges. Rhai's own `split` family and Rust's own `str::replace` now judge generated cases, two mutants are explained (the clock read 15 operations out of 16, and Rhai's own `split` in place of ours, whose memory only `tests/script_budget.rs` sees), and a branch no run can reach is removed. `renderer.rs` joined on 2026-11-13: its first run left 27 survivors among 93 mutants, 17 of them in fake data, whose tests checked that a value was not empty; a judge now draws every kind 512 times against the form its name promises, and found fake emails that kept the accent of `François`, which a web form refuses (fixed). A condition of chaos mode that changed no result is removed, its delay is drawn by a function of its own that a test reads without waiting, and three mutants are explained. `template_dates.rs` joined on 2026-11-14: its first run left 9 survivors among 86 mutants, no test having a date with one wrong separator, an offset with minutes, or an hour or a minute just past its range; judges now read every two-digit month and day in years chosen for the leap year rules, every two-digit hour and minute in a time and in each form of offset, and generated date-times, against a calendar written in the test, and found offsets read as `+02:00` whatever followed their hours (`+02ab`, `+0:200`, fixed). Two range checks that the calendar already made are removed; the second run caught all 78 mutants. The scope holds 937 mutants, 234 or 235 a shard (the check digits of fake identifiers added 45 to `renderer.rs`). Next: `shadowing.rs` (55), `script_json.rs` (45), `proxy.rs` (43), `json_pointer.rs` (25), `regex_cache.rs` (10).

### E54. A parenthesis alone in a quoted pipe argument breaks the expression

Size S

**Why.** `find_closing_double_brace` (`src/engine/template.rs`) counts parentheses outside double quotes, and reads a double quote only outside parentheses. Inside a pipe argument, a quoted text that holds a parenthesis alone therefore changes the depth: `{{ "x" | replace("x", ")") }}` and `{{ "x" | append("a(b") }}` are not expressions, and render as they are written. Balanced parentheses (`append(" (beta)")`) work. Recognising double quotes at every depth would fix it, but would break a single-quoted argument that holds a double quote alone (`default('5" screen')`), which works today and which a test now keeps.

**What.** Decide which quotes protect what an argument holds (double only, or double and single, each closing only itself), write it next to the parser, then change `find_closing_double_brace` and `split_pipes` together (`split_pipes` counts parentheses without reading quotes at all: `replace("x", ")")` leaves its depth at -1, and no later `|` splits), with their copies in the interface, which follow them by hand: `closingBraces` of `frontend/src/lib/template-help.js`, and `findDoubleClose` and `findPipeSeparator` of `frontend/src/lib/tpl-utils.js` (`canPipeFixedValue` relies on them). An apostrophe in an unquoted argument (`default(l'API)`) works today and must keep working, whichever quotes protect; the generated expressions of `an_expression_ends_at_its_own_closing_braces_whatever_its_texts_hold` then include parentheses in quoted arguments.

**Done when.** Both templates above render `)` and `xa(b`, the single-quoted case keeps its rendering or its change is written in the changelog, and the judge generates the cases.

### E59. The shapes of observed responses, described again at every level of a path

Size S

**Why.** `same_shape_words` (`src/server/suggestion/routes.rs`) describes the response of every exchange under a group of words (`dominant_shape`, then `shape`, which parses a JSON body or reads an XML one up to 4,096 events) each time the inference meets two words or more at one place of the paths, and keeps no result: an exchange is described once for each level of its path where its group holds several words, up to `MAX_SEGMENTS` (32). Estimated from the code, not measured end to end: an XML body built to cost the most to describe takes 0.64 ms at 16 KiB (release build, the function alone, 2026-11-12), and the store keeps up to 1,600 exchanges (200 endpoints of 8 samples), so one suggestion request could spend about a second of CPU per level, and up to about thirty. The request runs on a worker thread of the async runtime (`get_service_suggestions_impl`, `src/server/api.rs`), not on a blocking task, and holds that thread meanwhile.

**What.** Measure first: the family of suggestions in the scale test, over paths of many levels of words answered by costly XML bodies. Then describe each exchange once per request, before the inference, and move the computation off the async workers if it still takes more than a few milliseconds.

**Done when.** Each exchange is described once per suggestion request, counted by the scale probe or a test, and a request over the largest store above stays within a bound written next to its test, seen above it on the current code.
