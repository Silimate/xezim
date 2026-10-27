#!/usr/bin/env bash
# Signs each FILE's SHA-256 into FILE.sig, a DER ECDSA P-256 signature, with the
# Cloud KMS key version RELEASE_SIGNING_KEY, as GCP_TOKEN, an access token of
# the release service account, and checks it against release/signing.pub.pem
# (docs/release.md). Needs base64, curl, jq and OpenSSL.
set -euo pipefail

[ $# -gt 0 ] || { echo 'usage: release/sign.sh FILE...' >&2; exit 1; }
pem=release/signing.pub.pem
[ -f "$pem" ] || { echo "$pem is missing: see docs/release.md, Setting up" >&2; exit 1; }
auth="Authorization: Bearer ${GCP_TOKEN:?}"
key=${RELEASE_SIGNING_KEY:?}
for f; do
  # the token goes in a header file, off the command line
  openssl dgst -sha256 -binary "$f" | base64 -w0 | jq -R '{digest: {sha256: .}}' |
    curl -fsS -H @<(echo "$auth") -H 'Content-Type: application/json' -d @- \
      "https://cloudkms.googleapis.com/v1/$key:asymmetricSign" |
    jq -r .signature | base64 -d > "$f.sig"
  openssl dgst -sha256 -verify "$pem" -signature "$f.sig" "$f"
done
