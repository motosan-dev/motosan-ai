#!/usr/bin/env python3
"""Compare what a registry serves against the artifact this run built.

`publish-rust.yml` already refuses to call a release done until crates.io
serves the exact SHA-256 it packaged. This gives PyPI and npm the same
guarantee, because "the version appeared" is a much weaker claim than "the
bytes people will download are the bytes we built".

Two modes, mirroring the Rust workflow's shape:

  --mode precheck   Is this version already published?
                    Matching digests  -> published=true  (publish is skipped)
                    Absent            -> published=false (publish proceeds)
                    Different digests -> failure; something else owns that
                                         version and republishing would lie
  --mode verify     Poll until the version is served, then compare digests.

Both modes fail on a digest mismatch, which makes the publish step safe to
re-run: a retried job either finds its own artifact and skips, or finds
nothing and uploads.

    python3 scripts/verify-published.py --registry pypi \\
        --package motosan-ai --version 0.19.0 \\
        --digest motosan_ai-0.19.0-py3-none-any.whl=<sha256> --mode precheck

    python3 scripts/verify-published.py --registry npm \\
        --package @motosan-ai/sdk --version 0.15.0 \\
        --digest integrity=sha512-... --mode verify

Digests are supplied by the caller rather than computed here, so the script
needs no build tooling and can be exercised against the live registries.
PyPI digests are per-file SHA-256 (`sha256sum dist/*`); npm's is the
subresource integrity string that `npm pack --json` reports for the tarball
it would upload.

Needs Python >= 3.11; stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

USER_AGENT = "motosan-ai publish workflow"
POLL_ATTEMPTS = 30
POLL_INTERVAL_SECONDS = 10


class NotPublished(Exception):
    """The registry does not serve this version (yet)."""


class DigestMismatch(Exception):
    """The registry serves this version with different bytes."""


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            raise NotPublished(url) from None
        raise


def pypi_digests(package: str, version: str) -> dict[str, str]:
    """Per-file SHA-256, keyed by filename."""
    payload = fetch_json(f"https://pypi.org/pypi/{package}/{version}/json")
    return {entry["filename"]: entry["digests"]["sha256"] for entry in payload["urls"]}


def npm_digests(package: str, version: str) -> dict[str, str]:
    """The tarball's subresource integrity, under the key `integrity`."""
    payload = fetch_json(
        f"https://registry.npmjs.org/{urllib.parse.quote(package, safe='')}"
    )
    release = payload.get("versions", {}).get(version)
    if release is None:
        raise NotPublished(f"{package}@{version}")
    return {"integrity": release["dist"]["integrity"]}


REGISTRIES = {"pypi": pypi_digests, "npm": npm_digests}


def compare(expected: dict[str, str], served: dict[str, str]) -> None:
    problems = []
    for name, digest in expected.items():
        actual = served.get(name)
        if actual is None:
            problems.append(f"    {name}: absent from the registry")
        elif actual != digest:
            problems.append(
                f"    {name}:\n      built    {digest}\n      registry {actual}"
            )
    if problems:
        raise DigestMismatch("\n".join(problems))


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="verify-published.py")
    parser.add_argument("--registry", required=True, choices=sorted(REGISTRIES))
    parser.add_argument("--package", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument(
        "--digest",
        required=True,
        action="append",
        metavar="NAME=DIGEST",
        help="repeatable; NAME is a filename (pypi) or the literal `integrity` (npm)",
    )
    parser.add_argument("--mode", required=True, choices=("precheck", "verify"))
    args = parser.parse_args(argv)

    expected = {}
    for item in args.digest:
        name, separator, digest = item.partition("=")
        if not separator or not name or not digest:
            parser.error(f"--digest {item!r} is not NAME=DIGEST")
        expected[name] = digest
    args.expected = expected
    return args


def emit_output(published: bool) -> None:
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"published={'true' if published else 'false'}\n")


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    lookup = REGISTRIES[args.registry]
    label = f"{args.package} {args.version}"

    if args.mode == "precheck":
        try:
            served = lookup(args.package, args.version)
        except NotPublished:
            print(f"{label} is not on {args.registry} yet — publishing.")
            emit_output(False)
            return 0
        try:
            compare(args.expected, served)
        except DigestMismatch as mismatch:
            print(
                f"::error::{label} is already on {args.registry} with different "
                f"artifacts:\n{mismatch}",
                file=sys.stderr,
            )
            return 1
        print(
            f"{label} is already on {args.registry} with matching digests — skipping publish."
        )
        emit_output(True)
        return 0

    for attempt in range(1, POLL_ATTEMPTS + 1):
        try:
            served = lookup(args.package, args.version)
        except NotPublished:
            print(f"Waiting for {label} on {args.registry} ({attempt}/{POLL_ATTEMPTS})")
            time.sleep(POLL_INTERVAL_SECONDS)
            continue
        try:
            compare(args.expected, served)
        except DigestMismatch as mismatch:
            print(
                f"::error::{label} on {args.registry} does not match what this run "
                f"built:\n{mismatch}",
                file=sys.stderr,
            )
            return 1
        for name, digest in sorted(args.expected.items()):
            print(f"  ✓ {name} {digest}")
        print(f"Verified {label} on {args.registry}.")
        return 0

    print(
        f"::error::{label} did not become visible on {args.registry} after "
        f"{POLL_ATTEMPTS * POLL_INTERVAL_SECONDS}s",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
