#!/usr/bin/env python3
"""Verify every version-coupled location agrees with the SDK manifests.

Three manifests are the sources of truth:

    sdks/rust/Cargo.toml          [package] version
    sdks/python/pyproject.toml    [project] version
    sdks/typescript/package.json  version

Everything else that carries a version is derived and must agree: the two
tracked lockfiles, the four version banners in the docs, and the first
released heading of each SDK CHANGELOG. This script is the machine-checkable
replacement for the release checklist — the M1 release shipped with stale
install snippets precisely because that checklist lived in prose.

It also forbids re-introducing version-pinned `motosan-ai` Cargo install
snippets: docs teach `cargo add motosan-ai --features <feature>`, which
resolves at run time and cannot go stale.

The locations themselves live in `_release_sites.py`, shared with
`bump-version.py`.

Run: python3 scripts/check-versions.py   (stdlib only, needs Python >= 3.11)
"""

from __future__ import annotations

import re
import sys

import _release_sites as sites

# Historical planning records legitimately quote the pinned snippets of past
# releases; they document what was done, and must not be rewritten.
SNIPPET_SCAN_SUFFIXES = (".md", ".txt")
SNIPPET_SCAN_EXCLUDED_DIRS = {
    ".git",
    ".direnv",
    "node_modules",
    "target",
    ".venv",
    "docs/superpowers",
}

PINNED_CARGO_SNIPPET = re.compile(r"motosan-ai\s*=\s*\{\s*version\s*=")
PINNED_CARGO_ADD = re.compile(r"cargo add\s+motosan-ai@")

errors: list[str] = []


def fail(message: str) -> None:
    errors.append(message)


def check_lockfiles(v: dict[str, str]) -> None:
    """Both lockfiles record the workspace member's own version."""
    uv_version = sites.uv_lock_version()
    if uv_version is None:
        fail(f"{sites.UV_LOCK}: expected exactly one motosan-ai package entry")
    elif uv_version != v["python"]:
        fail(
            f"{sites.UV_LOCK}: motosan-ai is {uv_version}, "
            f"pyproject.toml is {v['python']} — run `uv sync --all-extras` in sdks/python/"
        )

    for label, found in sites.npm_lock_versions().items():
        if found != v["ts"]:
            fail(
                f"{sites.NPM_LOCK}: {label} is {found}, "
                f"package.json is {v['ts']} — run `npm install` in sdks/typescript/"
            )


def check_banners(v: dict[str, str]) -> None:
    """Each banner line is expected verbatim; the anchor locates near misses."""
    for rel, template, anchor in sites.BANNERS:
        expected = template.format(**v)
        near = sites.anchor_matches(rel, anchor)

        # An anchor that stops being unique silently weakens both this check
        # and bump-version.py's rewrite, so treat it as a failure of its own.
        if len(near) > 1:
            fail(
                f"{rel}: anchor {anchor!r} matches {len(near)} lines, expected 1 — "
                f"choose a more specific anchor in scripts/_release_sites.py"
            )
            continue

        if expected in sites.read_text(rel).splitlines():
            continue

        if near:
            fail(
                f"{rel}: version banner is stale\n"
                f"    expected: {expected}\n"
                f"    found:    {near[0]}"
            )
        else:
            fail(
                f"{rel}: version banner not found (was it reworded?)\n"
                f"    expected: {expected}\n"
                f"    no line contains the anchor {anchor!r} — "
                f"update scripts/_release_sites.py if the banner moved on purpose"
            )


def check_changelogs(v: dict[str, str]) -> None:
    """The first released heading after [Unreleased] is the shipped version."""
    heading = re.compile(r"^## \[(?P<version>[^\]]+)\]")
    for sdk, rel in sites.CHANGELOGS.items():
        versions = [
            match.group("version")
            for line in sites.read_text(rel).splitlines()
            if (match := heading.match(line)) and match.group("version") != "Unreleased"
        ]
        if not versions:
            fail(f"{rel}: no released version heading found")
        elif versions[0] != v[sdk]:
            fail(
                f"{rel}: first released heading is [{versions[0]}], "
                f"manifest is {v[sdk]} — the release entry was not renamed from [Unreleased]"
            )


def check_no_pinned_snippets() -> None:
    """Docs must teach `cargo add`, never a snippet that pins the version."""
    for path in sorted(sites.ROOT.rglob("*")):
        if path.suffix not in SNIPPET_SCAN_SUFFIXES or not path.is_file():
            continue
        parts = path.relative_to(sites.ROOT).parts
        rel = path.relative_to(sites.ROOT).as_posix()
        if any(part in SNIPPET_SCAN_EXCLUDED_DIRS for part in parts) or any(
            rel == excluded or rel.startswith(f"{excluded}/")
            for excluded in SNIPPET_SCAN_EXCLUDED_DIRS
        ):
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if PINNED_CARGO_SNIPPET.search(line) or PINNED_CARGO_ADD.search(line):
                fail(
                    f"{rel}:{number}: version-pinned motosan-ai install snippet\n"
                    f"    {line.strip()}\n"
                    f"    use `cargo add motosan-ai --features <feature>` — pinned "
                    f"snippets go stale on every release"
                )


def main() -> int:
    versions = sites.read_versions()
    check_lockfiles(versions)
    check_banners(versions)
    check_changelogs(versions)
    check_no_pinned_snippets()

    if errors:
        print("Version metadata is inconsistent:\n", file=sys.stderr)
        for error in errors:
            print(f"  ✗ {error}", file=sys.stderr)
        print(
            f"\n{len(errors)} problem(s). Sources of truth: sdks/rust/Cargo.toml, "
            "sdks/python/pyproject.toml, sdks/typescript/package.json.",
            file=sys.stderr,
        )
        return 1

    print(
        f"✅ version metadata consistent — rust {versions['rust']}, "
        f"python {versions['python']}, typescript {versions['ts']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
