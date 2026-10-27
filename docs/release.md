# Releases

This page covers the binary releases of this repository: what a release
contains, how it is built and checked, how to verify one, and how to cut one.
The files are in [`release/`](../release) and
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

* [What a release contains](#what-a-release-contains)
* [How a release is built](#how-a-release-is-built)
* [Verifying a release](#verifying-a-release)
* [Cutting a release](#cutting-a-release)
* [Setting up](#setting-up)
* [Recovering](#recovering)
* [Decisions](#decisions)

## What a release contains

Pushing a tag `vVERSION` runs the Release workflow. `VERSION` is the version in
`Cargo.toml`, or that version and a label: `v0.11.2`, `v0.11.2-s1`, or
`v0.11.2-rc.1` for a pre-release. (Tags without the `v` are upstream's.) The
workflow publishes a GitHub release with:

| File | Contents |
|---|---|
| `xezim-VERSION-linux-x86_64.tar.gz` | The release: `bin/xezim`, the cocotb runner in `share/xezim/cocotb/`, and the license, notices and documentation in `share/doc/xezim/` |
| `sbom.cdx.json` | A CycloneDX 1.5 SBOM of the binary: every crate it links, with its version, license and source |
| `THIRD_PARTY_NOTICES` | The license texts of those crates, the C libraries they bundle included |
| `provenance.json` | How the release was made: the commit, the workflow run, the builder image, the toolchain and tool versions, the hashes of the inputs, the gates passed and the hashes of the outputs |
| `SHA256SUMS` | The hashes of the tarballs, the SBOM and the notices |
| `SHA256SUMS.sig`, `provenance.json.sig` | Their signatures: ECDSA P-256 with SHA-256, by a key that never leaves Cloud KMS |

GitHub also holds a build-provenance attestation of the tarballs, signed
through Sigstore.

The bucket `gs://RELEASE_BUCKET/VERSION/` keeps the same files, write-once,
plus `xezim-VERSION-linux-x86_64-debug.tar.gz`: the binary's debug info, for
symbolizing a crash (`addr2line -e xezim.debug ADDRESS`).

The binary runs on x86_64 Linux with glibc 2.28 or later: RHEL, Rocky and
AlmaLinux 8 and newer, Ubuntu 18.10 and newer, Debian 10 and newer. It links
every Rust and C dependency statically and takes only glibc and libgcc_s from
the system.

## How a release is built

`release/run.sh build` runs [`release/build.sh`](../release/build.sh) in the
builder container, and `release/run.sh smoke` tests the result in a second one.
Every input is pinned in [`release/pins.sh`](../release/pins.sh):

* **The builder** is the `manylinux_2_28` image (AlmaLinux 8, glibc 2.28,
  GCC 14), by digest. The binary links glibc 2.28's symbols, so it runs on any
  later glibc.
* **The toolchain** is Rust 1.92.0, `Cargo.toml`'s `rust-version`, installed by
  a rustup-init of known SHA-256. **The dependencies** are those of
  [`release/Cargo.lock`](../release/Cargo.lock), and every cargo command runs
  with `--locked`. (The repository's own `Cargo.lock` stays untracked, as
  upstream keeps it.)
* **The tools** (cargo-deny, cargo-cyclonedx, OSV-Scanner, gitleaks) are their
  projects' release binaries, by URL and SHA-256.

The build stops at the first gate that fails:

1. The checkout has no changes, and `release/Cargo.lock` matches `Cargo.toml`.
2. **cargo-deny** ([`release/deny.toml`](../release/deny.toml)) on what the
   binary links:
   * licenses: permissive only (MIT, Apache-2.0, BSD, ISC, Zlib, BSL-1.0,
     Unicode-3.0); no GPL, LGPL or other copyleft;
   * sources: crates.io, and the pinned xezim-core repository;
   * advisories: every RustSec advisory, unmaintained and unsound crates and
     yanked versions included.
3. **OSV-Scanner** on `release/Cargo.lock`
   ([`release/osv-scanner.toml`](../release/osv-scanner.toml)).
4. **gitleaks** on the repository's history and on the tagged tree.
5. **The test suite**, as CI runs it (`cargo test --locked`), on the release
   lockfile.
6. **The binary** needs glibc 2.28 or older, and no library besides glibc and
   libgcc_s.
7. **The smoke test** ([`release/smoke.sh`](../release/smoke.sh)) runs the
   packed release on a bare AlmaLinux 8 image, which has no compiler or
   Python: the binary starts and simulates a design.

An exception to a gate goes in `release/deny.toml` or `release/osv-scanner.toml`
with its reason and when to drop it, in a pull request of its own.

The build works on a fresh clone of the checkout's commit, so an untracked or
ignored file can't reach the release. Then it packs the release reproducibly: the binary's debug info goes to
`xezim.debug`, linked from the binary by `.gnu_debuglink`; the tarballs are
sorted, dated at the commit's time, owned by root and compressed without a
timestamp. cargo-cyclonedx writes the SBOM, with no build or test
dependencies; [`release/record.py`](../release/record.py) adds the binary's
hashes to it, gathers the notices from the crates' sources, and writes the
provenance.

The publish job, on a tag push only, then:

1. attests the tarballs' build provenance (GitHub, Sigstore);
2. signs `SHA256SUMS` and `provenance.json` with the Cloud KMS key, and
   checks each signature against
   [`release/signing.pub.pem`](../release/signing.pub.pem)
   ([`release/sign.sh`](../release/sign.sh));
3. drafts the GitHub release with the files;
4. uploads every file, the debug info included, to the bucket, create-only
   ([`release/archive.sh`](../release/archive.sh));
5. publishes the draft. A published release is immutable.

A manual run of the workflow, or a pull request that changes `release/` or the
workflow, is a dry run: it builds, gates and packs, uploads the files as a
workflow artifact, and signs, archives and publishes nothing.

## Verifying a release

With the release's files and `release/signing.pub.pem` from this repository
at the release's tag:

```bash
openssl dgst -sha256 -verify signing.pub.pem -signature SHA256SUMS.sig SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify xezim-VERSION-linux-x86_64.tar.gz --repo Silimate/xezim
```

OpenSSL 1.1.1, as RHEL 8 ships it, is enough.

To check that a release is what its tag builds, run the workflow manually on
the tag, or `release/run.sh build` on a checkout of it (which takes the
version from the tag), and compare the `SHA256SUMS` it writes with the
release's. They match.

## Cutting a release

1. In a pull request, set the version in `Cargo.toml` if it changes, and run
   `release/run.sh lock` if the dependencies changed (a new crate, or a new
   xezim-core revision): it adds what `Cargo.toml` needs to
   `release/Cargo.lock` and keeps every version already pinned. To move the
   pinned versions forward, run `cargo update` with the lockfile in place and
   copy it back. The pull request's dry run must pass.
2. After it merges, tag the merge commit on `main` and push the tag:

   ```bash
   git tag -a v0.11.2-s1 -m "xezim 0.11.2-s1" <commit>
   git push origin v0.11.2-s1
   ```

3. The workflow builds, tests and publishes the release; the build takes
   about 15 minutes on six cores.

## Setting up

The publish job needs these, made once by a GCP admin of the project:

* A Cloud KMS key for signing, `ec-sign-p256-sha256`, non-exportable. Its
  public key goes in `release/signing.pub.pem`
  (`gcloud kms keys versions get-public-key 1 --key KEY --keyring RING
  --location LOCATION --output-file release/signing.pub.pem`), in a pull
  request.
* A bucket for the archive, and a service account that may only sign with that
  key (`roles/cloudkms.signer`) and create objects in that bucket
  (`roles/storage.objectCreator`).
* A workload identity provider for GitHub that accepts only this workflow on a
  `v*` tag of this repository: the attribute condition
  `assertion.repository == "Silimate/xezim" && assertion.ref.startsWith("refs/tags/v") && assertion.workflow_ref.startsWith("Silimate/xezim/.github/workflows/release.yml@")`,
  with `roles/iam.workloadIdentityUser` on the service account for it.
* The repository variables `RELEASE_WIF_PROVIDER` (the provider's full name),
  `RELEASE_SERVICE_ACCOUNT` (its email), `RELEASE_SIGNING_KEY` (the key
  version's full name, `projects/…/cryptoKeyVersions/1`) and `RELEASE_BUCKET`.
* A tag ruleset that lets only maintainers create `v*` tags, and immutable
  releases turned on in the repository's settings.

`release/signing.pub.pem` is the public key of version 1 of the signing key. Its
SHA-256 fingerprint, of the DER-encoded key (`openssl pkey -pubin -in
release/signing.pub.pem -outform DER | sha256sum`), is
`6e41f746dbae35449c1ee2f5705902c7791641fed68fc7bdc6d7c6665f1b48bd`. If the
key is rotated, the new version's public key replaces it in a pull request,
and `RELEASE_SIGNING_KEY` moves to the new version; a release still verifies
against the file at its own tag.

## Recovering

* The workflow publishes nothing before its last step. If it fails before the
  archive step, delete the draft release it left, if any, and rerun the failed
  jobs; they reuse the build's files. If it fails after, upload what the bucket
  lacks by hand: the signatures and the other release files from the draft
  release, the debug info from the run's artifact. Then publish the draft.
* Never move or reuse a tag. Supersede a bad release with a new label
  (`v0.11.2-s2`); the bad one stays downloadable.

## Decisions

* **glibc, not a fully static binary.** xezim loads VPI modules (cocotb's among
  them) and DPI libraries with `dlopen`, and exports its VPI functions to them,
  which a static musl binary can't do. So the binary links every dependency
  statically but glibc and libgcc_s, which every Linux system has, and is
  built against the oldest glibc still supported (RHEL 8's 2.28).
* **A lockfile of its own.** Upstream doesn't track `Cargo.lock`, and the fork
  follows upstream closely. `release/Cargo.lock` pins what a release builds
  without changing how the fork is developed; refreshing it is a step of
  cutting a release.
* **Pinned images and tools, no caches.** A release builds from scratch in a
  container pinned by digest, with tools pinned by hash, so a rebuild of a tag
  gives the same bytes and nothing outside the pins can change it.
* **Signed hashes, not signed files.** One signature over `SHA256SUMS` covers
  every file; ECDSA P-256 over a SHA-256 digest is what Cloud KMS signs and what
  RHEL 8's OpenSSL verifies. The key never leaves KMS, and only the publish
  job, on a tag, can use it.
* **The test suite and the gates run again at release time,** on the release
  lockfile, though CI runs the suite on every push: CI resolves dependencies
  afresh, so the lockfile a release ships is checked only here.
