#!/usr/bin/env bash
# Builds a release inside the pinned builder (release/run.sh starts it there):
# installs the pinned toolchain and tools, gates the source, builds, tests and
# packs xezim, and writes the release files to dist/ (docs/release.md).
#
#   release/build.sh VERSION   the release
#   release/build.sh lock      refresh release/Cargo.lock for Cargo.toml,
#                              keeping every version it already pins
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
. release/pins.sh

fetch() { # URL SHA256 FILE
  curl -fsSL --retry 3 "$1" -o "$3"
  echo "$2  $3" | sha256sum -c --quiet -
}
tools=$HOME/tools
mkdir -p "$tools"
export PATH="$tools:$HOME/.cargo/bin:$PATH"
fetch "$RUSTUP_URL" "$RUSTUP_SHA256" "$tools/rustup-init"
chmod +x "$tools/rustup-init"
"$tools/rustup-init" -y -q --no-modify-path --profile minimal --default-toolchain "$RUST"
git config --global --add safe.directory "$PWD"
cp release/Cargo.lock Cargo.lock

if [ "${1:?usage: release/build.sh VERSION|lock}" = lock ]; then
  cargo metadata --format-version 1 > /dev/null
  cp Cargo.lock release/Cargo.lock
  exit
fi
version=$1

fetch "$DENY_URL" "$DENY_SHA256" /tmp/deny.tar.gz
tar -xzf /tmp/deny.tar.gz -C "$tools" --strip-components=1 --wildcards '*/cargo-deny'
fetch "$CYCLONEDX_URL" "$CYCLONEDX_SHA256" /tmp/cyclonedx.tar.xz
tar -xJf /tmp/cyclonedx.tar.xz -C "$tools" --strip-components=1 --wildcards '*/cargo-cyclonedx'
fetch "$OSV_URL" "$OSV_SHA256" "$tools/osv-scanner"
chmod +x "$tools/osv-scanner"
fetch "$GITLEAKS_URL" "$GITLEAKS_SHA256" /tmp/gitleaks.tar.gz
tar -xzf /tmp/gitleaks.tar.gz -C "$tools" gitleaks

# The source: a clean checkout, and a lockfile that matches Cargo.toml
[ -z "$(git status --porcelain --untracked-files=no)" ] || {
  git status --short >&2
  echo 'the checkout has changes' >&2
  exit 1
}
cargo metadata --locked --format-version 1 > /dev/null || {
  echo 'release/Cargo.lock is out of date: run release/run.sh lock' >&2
  exit 1
}
commit=$(git rev-parse HEAD)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}

# The gates: licenses, sources and advisories of what ships; known
# vulnerabilities in the lockfile; secrets in the source and its history
cargo deny --locked --config release/deny.toml check
osv-scanner scan source --config release/osv-scanner.toml --lockfile Cargo.lock
gitleaks git . --redact --no-banner
rm -rf /tmp/tree && mkdir /tmp/tree && git archive HEAD | tar -x -C /tmp/tree
gitleaks dir /tmp/tree --redact --no-banner

# Build, then test as CI does, on the locked dependencies. One test builds C
# with UBSan, whose runtime the builder lacks; CI runs it.
cargo build --release --locked --bin xezim
cargo test --locked --no-fail-fast -- --skip dpi_header_width_helpers_have_defined_c_shifts

# The binary links glibc and the GCC runtime only, at most glibc 2.28's
# symbols: it runs on any x86_64 Linux from RHEL 8 on. glibc is not shipped.
bin=target/release/xezim
readelf -d "$bin" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort > /tmp/needed
grep -vxE 'ld-linux-x86-64\.so\.2|lib(c|m|dl|pthread|rt|util|gcc_s)\.so\.[0-9]+' /tmp/needed && {
  echo 'the binary needs a library beyond glibc and libgcc_s' >&2
  exit 1
}
glibc=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
[ "$(printf '%s\n' "$glibc" 2.28 | sort -V | tail -1)" = 2.28 ] || {
  echo "the binary needs glibc $glibc" >&2
  exit 1
}

# Pack: the binary with its debug info split off, the cocotb runner, the
# license, docs and notices
name=xezim-$version-linux-x86_64
rm -rf dist /tmp/stage && mkdir -p dist /tmp/stage/debug
root=/tmp/stage/$name
mkdir -p "$root/bin" "$root/share/xezim/cocotb" "$root/share/doc/xezim/docs"
objcopy --only-keep-debug "$bin" /tmp/stage/debug/xezim.debug
objcopy --strip-debug --add-gnu-debuglink=/tmp/stage/debug/xezim.debug "$bin" "$root/bin/xezim"
cp contrib/cocotb/xezim_runner.py "$root/share/xezim/cocotb/"
cp LICENSE README.md NOTES.md CONTRIBUTING.md "$root/share/doc/xezim/"
cp docs/*.md "$root/share/doc/xezim/docs/"
SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH cargo cyclonedx --format json --spec-version 1.5 \
  --no-build-deps --target x86_64-unknown-linux-gnu --describe binaries -q
cargo metadata --locked --format-version 1 > /tmp/metadata.json
/opt/python/cp312-cp312/bin/python3 release/record.py sbom "$version" "$root/bin/xezim" \
  xezim_bin.cdx.json /tmp/metadata.json dist/sbom.cdx.json dist/THIRD_PARTY_NOTICES
rm -f ./*_bin.cdx.json
cp dist/THIRD_PARTY_NOTICES "$root/share/doc/xezim/"
pack() { # DIR ENTRY TARBALL
  tar -C "$1" --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --mode=u+rwX,go+rX,go-w --format=gnu -cf - "$2" | gzip -9n > "$3"
}
pack /tmp/stage "$name" "dist/$name.tar.gz"
pack /tmp/stage/debug xezim.debug "dist/$name-debug.tar.gz"
(cd dist && sha256sum "$name.tar.gz" "$name-debug.tar.gz" sbom.cdx.json THIRD_PARTY_NOTICES > SHA256SUMS)

# The record of how it was made
{
  echo "rustc: $(rustc -vV | tr '\n' ';')"
  echo "cargo: $(cargo -V)"
  echo "cc: $(cc --version | head -1)"
  echo "cargo-deny: $(cargo-deny --version)"
  echo "cargo-cyclonedx: $(cargo-cyclonedx cyclonedx --version)"
  echo "osv-scanner: $(osv-scanner --version | head -1)"
  echo "gitleaks: $(gitleaks version)"
  echo "glibc: $glibc"
  echo "needed: $(tr '\n' ' ' < /tmp/needed)"
} > /tmp/tools.txt
/opt/python/cp312-cp312/bin/python3 release/record.py provenance "$version" "$commit" \
  /tmp/tools.txt dist/SHA256SUMS dist/provenance.json
cat dist/SHA256SUMS
