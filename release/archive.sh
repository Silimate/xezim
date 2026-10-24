#!/usr/bin/env bash
# Uploads each FILE to gs://RELEASE_BUCKET/VERSION/, create-only, as GCP_TOKEN,
# an access token of the release service account (docs/release.md). The bucket
# keeps every release, its debug info included.
set -euo pipefail

[ $# -gt 1 ] || { echo 'usage: release/archive.sh VERSION FILE...' >&2; exit 1; }
version=$1
shift
auth="Authorization: Bearer ${GCP_TOKEN:?}"
for f; do
  # ifGenerationMatch=0: never replace an object already there
  name=$(jq -rn --arg n "$version/$(basename "$f")" '$n | @uri')
  curl -fsS -X POST -H @<(echo "$auth") -H 'Content-Type: application/octet-stream' \
    --data-binary @"$f" -o /dev/null \
    "https://storage.googleapis.com/upload/storage/v1/b/${RELEASE_BUCKET:?}/o?uploadType=media&ifGenerationMatch=0&name=$name"
  echo "gs://$RELEASE_BUCKET/$version/$(basename "$f")"
done
