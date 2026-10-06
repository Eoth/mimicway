# Contributing to Mimicway

Thank you for helping. Bug reports, documentation fixes, translations and code are all welcome. This page says how to get a change merged quickly.

Security issues are the exception: never in a public issue, see [SECURITY.md](SECURITY.md).

## Before you start

- **Bugs**: open an issue with the bug template; a failing request (method, path, headers, body) and the service configuration that produced it save most of the back and forth.
- **Features**: open an issue first, or comment on the matching [roadmap](ROADMAP.md) item, so that the design is agreed before the code. Mimicway keeps a few promises (one program, no telemetry, every outbound flow opt-in, easy to review); a change that bends one of them needs that discussion.
- **Small fixes** (typos, wrong docs, obvious bugs): a pull request alone is fine.

## Set up

Rust 1.85 or later, Node.js 22.12 or later. The bootstrap scripts build everything: `scripts/bootstrap-windows.ps1` first installs Rust and Node.js with winget when they are missing; `scripts/bootstrap-linux.sh` checks their versions and, when one is missing or too old, says where to install it, without downloading anything itself.

```bash
cd frontend && npm ci && npm run build && cd ..
cargo build
DATA_PATH=./data ./target/debug/mimicway     # http://localhost:7342
cd frontend && npm run dev                     # UI with hot reload on http://localhost:5173
```

## Checks

CI runs the ones a pull request needs: each job runs when a file it checks changes (`scripts/ci-plan.mjs` maps files to jobs), and the `CI passed` check fails when a job that had to run did not pass. Run the ones your change touches before pushing.

```bash
cargo fmt --check
cargo clippy --all-targets --features tcp-mock -- -D warnings
cargo test --features tcp-mock
cargo deny check                       # when Cargo.toml or Cargo.lock change
cd frontend && npm test                # unit tests, including translation checks
cd frontend && npm run test:e2e        # with Mimicway running, see frontend/e2e/README.md
```

The fuzz targets (`fuzz/`; `src/fuzzing.rs` says what each one checks) build with a nightly toolchain and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), against the server's lock file: `cp Cargo.lock fuzz/ && cargo +nightly fuzz run config_import -- -max_total_time=300`. CI runs them five minutes on a pull request that changes the server, and an hour every week; a crash becomes a fix with its regression test.

The Kafka feature builds librdkafka from source: it needs cmake, a C toolchain and, on Linux, the libcurl headers (`libcurl4-openssl-dev` on Debian and Ubuntu). Then `cargo test --features messaging-kafka`.

## What a good change looks like

- **One logical change per commit**, with a [Conventional Commits](https://www.conventionalcommits.org) message in English, in the imperative (`fix(proxy): keep the query string on rule-level proxying`). The body says why: the cause, what was measured, what was traded off. Code comments are in English and explain why the code is the way it is today; history belongs in the commit message. CI fails on a French comment or test title (including the names of end-to-end scenarios) in the paths listed by `scripts/check-french-comments.mjs`.
- **A bug fix comes with the test that failed before it.**
- **Tests sit next to the code**: Rust tests below `#[cfg(test)]` or in a sibling `tests.rs`, UI tests in `frontend/src/tests/`, end-to-end scenarios in `frontend/e2e/`.
- **No `unsafe`**, tests included: the crate forbids it. A setting read from an environment variable goes through a lookup (`src/settings.rs`), so that a test passes its own values instead of changing the environment that the tests running beside it share.
- **A new dependency is justified** in the commit message (what it replaces, its size, its license); `cargo deny` must pass.
- **Every visible text is translatable.** Write the English sentence once, where it is used: `t("...")` in the UI, `tr("...", &[...])` in the server, with `{0}`, `{1}` placeholders for values. Add the French translation to `frontend/src/locales/fr.json` or `src/locales/fr.json`; the tests fail on a missing or unused entry. Data (names, URLs, values typed by users) is never translated: mark it `translate="no"` in the UI.
- **The docs follow the code.** Update the README, the guide and `CHANGELOG.md` (section `Unreleased`) in the same pull request. A new environment variable goes into the README's configuration table; a new outbound connection into the security model (`docs/en/security.md`).
- **The docs exist in English and French, page for page.** `docs/en/` and `docs/fr/` hold the same file names, `README.md` and `README.fr.md` mirror each other, and each page starts with a link to its other language. Change both languages in the same pull request: CI fails when a page, a screenshot, a heading or an image exists in one language only (`node scripts/check-doc-translations.mjs`). If you do not write French, say so in the pull request and a maintainer completes the translation before merging. Screenshots are never edited by hand: `npm run docs:screenshots` (see `frontend/e2e/README.md`) takes each one in both languages.
- **The interface follows its design system**, Phasme ([frontend/design-system.md](frontend/design-system.md)): colors, sizes and spacing come from `frontend/src/tokens.css`, a class two components need lives once in `frontend/src/app.css`, and `npm test` fails on a literal color, a variable defined nowhere or a contrast under WCAG AA. `npm run design:preview` shows every token and shared class in both themes.
- **Accessibility holds**: visible labels, keyboard access, a visible focus, 4.5:1 contrast for text and 3:1 for the boundaries of controls, in both themes.

## Adding a language

1. Copy `frontend/src/locales/fr.json` and `src/locales/fr.json` to `<code>.json` and translate the values, keeping every `{0}` placeholder.
2. Register the language: `loaders` and `LOCALES` in `frontend/src/lib/i18n.svelte.js`, the `Language` enum and its catalogue in `src/i18n.rs`.
3. `npm test` and `cargo test` check that the catalogues are complete.

A native speaker's review is welcome on every translation pull request.

## Releasing

1. Set the new version in `Cargo.toml` and `frontend/package.json` (run `cargo build` and `npm install` to update the lockfiles).
2. In `CHANGELOG.md`, rename the `Unreleased` section to `[x.y.z] - YYYY-MM-DD` and add a new empty `Unreleased` above it.
3. Commit, then push a tag `vx.y.z` on that commit.

The release workflow checks that the tag, both versions and the changelog agree, builds the binaries for every platform with the UI inside, publishes the image to GHCR, attaches SBOMs and checksums, attests everything, signs the image, and creates the GitHub release with the changelog section as notes. The first time, make the `mimicway` package public in the GitHub package settings.

## Licensing

Mimicway is under the [MIT license](LICENSE). By contributing, you agree that your contribution is licensed under the same terms.

## Conduct

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).
