"""The records of a release build (docs/release.md), written by release/build.sh.

  record.py sbom VERSION BINARY CDX METADATA SBOM NOTICES
      Completes cargo-cyclonedx's SBOM of the xezim binary (CDX) into SBOM: the
      release version, the binary's hashes and a serial number derived from
      them, so a rebuild writes the same file. Writes NOTICES, the license
      texts of every component, found beside its manifest in cargo's
      METADATA; fails on a component without one.

  record.py provenance VERSION COMMIT TOOLS SHA256SUMS OUT
      Writes OUT, how the release was made: the source, the pinned builder
      and tools (TOOLS: `name: version` lines), the inputs' hashes, the gates
      and the artifacts' hashes.
"""

import hashlib
import json
import os
import sys
import uuid

LICENSE = ("license", "licence", "copying", "notice", "copyright", "unlicense")
SKIP = {".git", "target", "test", "tests", "testdata", "examples", "benches", "doc", "docs"}


def digest(path, algo="sha256"):
    h = hashlib.new(algo)
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def texts(directory, depth=4):
    """License files in a crate's directory, its bundled sources included."""
    found = []
    for root, dirs, files in os.walk(directory):
        level = os.path.relpath(root, directory).count(os.sep) + (root != directory)
        dirs[:] = sorted(d for d in dirs if d.lower() not in SKIP and level < depth)
        found += [os.path.join(root, f) for f in sorted(files) if f.lower().startswith(LICENSE)]
    return found


def owned(manifest, stop):
    """The license files of the crate at `manifest`: in its directory, or in
    the nearest directory above it that has any (a workspace's root), up to
    `stop`."""
    directory = os.path.dirname(manifest)
    while True:
        found = texts(directory)
        parent = os.path.dirname(directory)
        if found or not directory.startswith(stop) or parent == directory:
            return found
        directory = parent


def sbom(version, binary, cdx, metadata, out, notices):
    bom = json.load(open(cdx))
    meta = json.load(open(metadata))
    sha256 = digest(binary)
    component = bom["metadata"]["component"]
    component["version"] = version
    component["hashes"] = [
        {"alg": "SHA-256", "content": sha256},
        {"alg": "SHA-512", "content": digest(binary, "sha512")},
    ]
    bom["serialNumber"] = "urn:uuid:" + str(
        uuid.uuid5(uuid.NAMESPACE_URL, f"pkg:generic/xezim@{version}?checksum=sha256:{sha256}")
    )
    bom["metadata"]["properties"] = [
        {"name": "runtime", "value": "glibc 2.28 or later and libgcc_s, from the system; not shipped"},
    ]
    with open(out, "w") as f:
        json.dump(bom, f, indent=2, sort_keys=True)
        f.write("\n")

    home = os.environ.get("CARGO_HOME", os.path.expanduser("~/.cargo"))
    packages = {(p["name"], p["version"]): p for p in meta["packages"]}
    blocks, missing = [], []
    for c in sorted(bom.get("components", []), key=lambda c: (c["name"], c["version"])):
        p = packages.get((c["name"], c["version"]))
        files = owned(p["manifest_path"], home) if p else []
        if not files:
            missing.append(f"{c['name']} {c['version']}")
            continue
        expr = p.get("license") or "see below"
        source = p.get("repository") or p.get("homepage") or ""
        head = f"{c['name']} {c['version']} ({expr})" + (f"\n{source}" if source else "")
        body = "\n\n".join(
            f"--- {os.path.relpath(path, os.path.dirname(p['manifest_path']))}\n\n"
            + open(path, errors="replace").read().strip()
            for path in files
        )
        blocks.append(f"{'=' * 78}\n{head}\n{'=' * 78}\n\n{body}\n")
    if missing:
        sys.exit("no license text found for: " + ", ".join(missing))
    with open(notices, "w") as f:
        f.write(
            f"xezim {version} contains the following third-party software, under the\n"
            "licenses below. xezim itself is under the Apache License 2.0 (LICENSE).\n"
            "It runs on the system's glibc and libgcc_s, which it does not include.\n\n"
        )
        f.write("\n".join(blocks))


def provenance(version, commit, tools, sums, out):
    env = os.environ.get
    pins = dict(
        line.strip().split("=", 1)
        for line in open("release/pins.sh")
        if "=" in line and not line.lstrip().startswith("#")
    )
    run = None
    if env("GITHUB_RUN_ID"):
        run = f"{env('GITHUB_SERVER_URL')}/{env('GITHUB_REPOSITORY')}/actions/runs/{env('GITHUB_RUN_ID')}/attempts/{env('GITHUB_RUN_ATTEMPT')}"
    record = {
        "version": version,
        "source": {
            "repository": env("GITHUB_REPOSITORY", "Silimate/xezim"),
            "ref": env("GITHUB_REF"),
            "commit": commit,
            "source_date_epoch": int(env("SOURCE_DATE_EPOCH")),
        },
        "workflow_run": run,
        "builder": pins["BUILDER"],
        "smoke_test_image": pins["RUNNER"],
        "tools": dict(line.rstrip("\n").split(": ", 1) for line in open(tools) if ": " in line),
        "inputs": {p: digest(p) for p in ("Cargo.toml", "release/Cargo.lock", "release/pins.sh", "release/deny.toml")},
        "build": "cargo build --release --locked --bin xezim, in the builder, at /src",
        "gates": [
            "cargo deny check: licenses, sources, bans and advisories (release/deny.toml)",
            "osv-scanner on release/Cargo.lock (release/osv-scanner.toml)",
            "gitleaks on the history and on the tree",
            "cargo test --locked",
            "the binary needs glibc 2.28 or older, and no library beyond glibc and libgcc_s",
            "the packed release runs on the smoke-test image",
        ],
        "artifacts": dict(reversed(line.split()) for line in open(sums)),
    }
    with open(out, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    {"sbom": sbom, "provenance": provenance}[sys.argv[1]](*sys.argv[2:])
