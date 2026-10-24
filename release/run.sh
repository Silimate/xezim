#!/usr/bin/env bash
# Runs a release step in its pinned container (docs/release.md):
#
#   release/run.sh build   build, gate and pack a release into dist/
#   release/run.sh smoke   run dist/'s release on a bare EL8 userland
#   release/run.sh lock    refresh release/Cargo.lock for Cargo.toml
#
# The version is $VERSION, else the release tag's (vVERSION) when the pushed
# tag, or a checkout of one, is being built, else Cargo.toml's version with
# -dev.<commit> (a dry run). It must be Cargo.toml's version, or that version
# and a -label. RELEASE_DOCKER_ARGS adds `docker run` options, such as
# --cpus 6 --memory 24g on a shared machine.
set -euo pipefail
cd "$(dirname "$0")/.."
. release/pins.sh

crate=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if [ -n "${VERSION:-}" ]; then
  version=$VERSION
elif [ "${GITHUB_REF_TYPE:-}" = tag ]; then
  version=${GITHUB_REF_NAME#v}
elif tag=$(git describe --exact-match --tags --match 'v*' HEAD 2> /dev/null); then
  version=${tag#v}
else
  version=$crate-dev.$(git rev-parse --short=12 HEAD)
fi
case $version in
  "$crate" | "$crate"-*) ;;
  *) echo "version $version is not Cargo.toml's $crate, or $crate-<label>" >&2; exit 1 ;;
esac

run() { # IMAGE ARGS...: as this user, in a home of its own
  # shellcheck disable=SC2086
  docker run --rm ${RELEASE_DOCKER_ARGS:-} -u "$(id -u):$(id -g)" -e HOME=/tmp/home "$@"
}
case ${1:?usage: release/run.sh build|smoke|lock} in
  build | lock)
    arg=$version
    [ "$1" = lock ] && arg=lock
    run -e CARGO_BUILD_JOBS -e GITHUB_REPOSITORY -e GITHUB_REF -e GITHUB_SERVER_URL \
      -e GITHUB_RUN_ID -e GITHUB_RUN_ATTEMPT -v "$PWD:/src" -w /src "$BUILDER" release/build.sh "$arg"
    ;;
  smoke)
    rm -rf dist/smoke && mkdir dist/smoke
    tar -xzf "dist/xezim-$version-linux-x86_64.tar.gz" -C dist/smoke
    run -v "$PWD/dist/smoke:/opt:ro" -v "$PWD/release/smoke.sh:/smoke.sh:ro" "$RUNNER" \
      /smoke.sh "/opt/xezim-$version-linux-x86_64"
    rm -rf dist/smoke
    ;;
  *) echo "unknown step $1" >&2; exit 1 ;;
esac
