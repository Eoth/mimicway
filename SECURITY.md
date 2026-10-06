# Security policy

## Supported versions

Mimicway follows [semantic versioning](https://semver.org) (see [CHANGELOG.md](CHANGELOG.md)). While the version is `0.x`, only the **latest release** receives security fixes; maintenance branches will exist from `1.0.0` on.

| Version | Supported |
|---|---|
| latest release | ✅ |
| any older release | ❌ |

## Reporting a vulnerability

**Please do not open a public issue** for a potential vulnerability (information leak, authentication bypass, injection, path traversal, denial of service, sandbox escape...).

Report it privately through [GitHub Security Advisories](https://github.com/Eoth/mimicway/security/advisories/new). If that channel is not available to you, write to **etokan.devs@gmail.com**.

Please include, as far as possible:
- a description of the issue and its impact;
- steps to reproduce (Mimicway version, relevant configuration such as `AUTH_ENABLED`, `BIND_ADDRESS`, the requests involved);
- a fix or mitigation, if you have one.

### Response times

Mimicway is maintained on a best-effort basis: these are targets, not a contractual SLA.

| Step | Target |
|---|---|
| Acknowledgement | 5 business days |
| First assessment (confirmed, not reproducible, more information needed) | 10 business days |
| Fix or mitigation plan | Depends on severity, with priority to authentication, file access and script sandbox issues |

We ask for coordinated disclosure: please give us time to publish a fix before disclosing technical details.

### How a report is handled

1. **Acknowledgement**, through the channel the report came by. The maintainer ([GOVERNANCE.md](GOVERNANCE.md)) handles it.
2. **Assessment**: the issue is reproduced on the latest release and `develop`, and its severity rated with CVSS. Work continues in a private GitHub security advisory, where the reporter is invited.
3. **Fix**: written and reviewed in the advisory's private fork, with a test that fails without it, then released as a new version.
4. **Publication**: the advisory is published when the fixed release is out, with a CVE identifier requested through GitHub when the issue has an impact on users, and `CHANGELOG.md` lists the fix under **Security** for that version.
5. **Credit**: the advisory and the changelog name the reporter, unless they ask to stay anonymous.

The reporter hears from us at each step. A report that turns out not to be a vulnerability is answered with the reason, and moved to a public issue if it is a bug.

## Verifying a release

Every release is built by [the release workflow](.github/workflows/release.yml) from the tagged commit, on GitHub's runners. Next to its archives and SBOMs, the release publishes a signature per file (`<file>.sigstore.json`), the build provenance of all of them (`mimicway-<version>.intoto.jsonl`, also kept in GitHub's attestation store) and their checksums (`SHA256SUMS`). Both are signed with the workflow's identity (Sigstore, no long-lived key); the image carries its own signature, provenance and SBOM.

```bash
# An archive: signature (cosign), provenance (GitHub CLI, from GitHub or offline from the release's bundle), checksum
cosign verify-blob mimicway-0.2.0-x86_64-unknown-linux-musl.tar.gz \
  --bundle mimicway-0.2.0-x86_64-unknown-linux-musl.tar.gz.sigstore.json \
  --certificate-identity-regexp '^https://github.com/Eoth/mimicway/.github/workflows/release.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
gh attestation verify mimicway-0.2.0-x86_64-unknown-linux-musl.tar.gz --repo Eoth/mimicway
gh attestation verify mimicway-0.2.0-x86_64-unknown-linux-musl.tar.gz --repo Eoth/mimicway --bundle mimicway-0.2.0.intoto.jsonl
sha256sum --ignore-missing -c SHA256SUMS

# The image: provenance and signature
gh attestation verify oci://ghcr.io/eoth/mimicway:0.2.0 --repo Eoth/mimicway
cosign verify ghcr.io/eoth/mimicway:0.2.0 \
  --certificate-identity-regexp '^https://github.com/Eoth/mimicway/.github/workflows/release.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

### Rebuilding a release

The Linux binaries and their archives are reproducible: built again from the same tag, they give the same bytes, which shows that a release holds the published source and nothing else. The [Dockerfile](Dockerfile) builds them, with Rust and Node.js pinned by digest and the same paths on every machine, and [scripts/release-archive.sh](scripts/release-archive.sh) packs them with fixed times, order, owners and permissions. The release workflow builds each one a second time, on another machine, and publishes nothing unless both agree; the CI compares two builds whenever the way they are made changes, and every week.

To check a release yourself, on Linux with Docker:

```bash
git clone https://github.com/Eoth/mimicway && cd mimicway && git checkout v0.2.0
docker build --platform linux/amd64 --target binary --output type=local,dest=out .
bash scripts/release-archive.sh out/mimicway x86_64-unknown-linux-musl 0.2.0 dist
sha256sum dist/*   # the same line as mimicway-0.2.0-x86_64-unknown-linux-musl.tar.gz in SHA256SUMS
```

For `aarch64-unknown-linux-musl`, run the same commands on an arm64 machine with `--platform linux/arm64`. The macOS and Windows binaries are built with the same Rust version, but no second build checks them.

## Scope

In scope:
- the Rust binary (`src/`): matching engine, proxy, templates, Rhai script sandbox, authentication, persistence, REST API, raw TCP mock;
- the web UI (`frontend/src/`);
- the provided `Dockerfile` and Kubernetes manifests (`k8s/`).

Out of scope:
- the services that users choose to mock or proxy (`real_target_url`): Mimicway does not control their security;
- Keycloak or Kafka deployments provided by the user;
- vulnerabilities of third-party dependencies with no demonstrated impact on Mimicway: please report them upstream. Dependencies are checked continuously in CI (`cargo deny`, `npm audit`, image scan).

## How Mimicway is secured

[docs/en/security.md](docs/en/security.md) describes the threat model, every outbound network flow, the defaults and how to harden a deployment. [REVIEWING.md](REVIEWING.md) is a guide for a security or code review of the project.
