# Pinned inputs of a release build (docs/release.md), sourced by the scripts
# here. Bump a pin in a pull request of its own: the release dry run it starts
# must pass.

# The builder: AlmaLinux 8 with glibc 2.28 and GCC, so the binary runs on
# RHEL/Rocky/Alma 8 and anything newer.
BUILDER=quay.io/pypa/manylinux_2_28_x86_64:2026.10.22-0@sha256:076b19ef6e698f28649fa817c49d14443201e01da85240e8e52f93e8daa49bff
# A bare EL8 userland for the smoke test: no compiler, no Python.
RUNNER=docker.io/library/almalinux:8-minimal@sha256:5edccbe0aa69031af6604d963a0d9343241e321c55030be87ede663446a95455

# The Rust toolchain: Cargo.toml's rust-version, installed by a pinned
# rustup-init (which checks each component against the release manifest).
RUST=1.92.0
RUSTUP_URL=https://static.rust-lang.org/rustup/archive/1.29.2/x86_64-unknown-linux-gnu/rustup-init
RUSTUP_SHA256=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4

# The gates' tools, as their projects publish them; the checksums are theirs.
DENY_URL=https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz
DENY_SHA256=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
CYCLONEDX_URL=https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-0.5.9/cargo-cyclonedx-x86_64-unknown-linux-musl.tar.xz
CYCLONEDX_SHA256=9bd3e599314f50810c9d98b8b68a617ff9d3cc20873968d90b29d121f6b226ff
OSV_URL=https://github.com/google/osv-scanner/releases/download/v2.7.0/osv-scanner_linux_amd64
OSV_SHA256=160c6253e2922e9e0a6a9771c5c4c40a4b3c6090b34a3cdd5e80c96cc2e4218b
GITLEAKS_URL=https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz
GITLEAKS_SHA256=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
