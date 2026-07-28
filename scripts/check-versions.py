#!/usr/bin/env python3
"""Verify every version-coupled location agrees with the SDK manifests.

Three manifests are the sources of truth:

    sdks/rust/Cargo.toml        [package] version
    sdks/python/pyproject.toml  [project] version
    sdks/typescript/package.json  version

Everything else that carries a version is derived and must agree: the two
tracked lockfiles, the four version banners in the docs, and the first
released heading of each SDK CHANGELOG. This script is the machine-checkable
replacement for the release checklist — the M1 release shipped with stale
install snippets precisely because that checklist lived in prose.

It also forbids re-introducing version-pinned `motosan-ai` Cargo install
snippets: docs teach `cargo add motosan-ai --features <feature>`, which
resolves at run time and cannot go stale.

Run: python3 scripts/check-versions.py   (stdlib only, needs Python >= 3.11)
"""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

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


def read_text(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def load_toml(rel: str) -> dict:
    with (ROOT / rel).open("rb") as handle:
        return tomllib.load(handle)


def load_json(rel: str) -> dict:
    return json.loads(read_text(rel))


def manifest_versions() -> dict[str, str]:
    rust = load_toml("sdks/rust/Cargo.toml")["package"]["version"]
    python = load_toml("sdks/python/pyproject.toml")["project"]["version"]
    ts = load_json("sdks/typescript/package.json")["version"]
    return {"rust": rust, "python": python, "ts": ts}


def check_lockfiles(v: dict[str, str]) -> None:
    """Both lockfiles record the workspace member's own version."""
    uv_packages = [
        pkg
        for pkg in load_toml("uv.lock").get("package", [])
        if pkg.get("name") == "motosan-ai" and "version" in pkg
    ]
    if len(uv_packages) != 1:
        fail(
            f"uv.lock: expected exactly one motosan-ai package entry, found {len(uv_packages)}"
        )
    elif uv_packages[0]["version"] != v["python"]:
        fail(
            f"uv.lock: motosan-ai is {uv_packages[0]['version']}, "
            f"pyproject.toml is {v['python']} — run `uv sync --all-extras` in sdks/python/"
        )

    lock = load_json("sdks/typescript/package-lock.json")
    for label, found in (
        ("top-level version", lock.get("version")),
        ('packages[""].version', lock.get("packages", {}).get("", {}).get("version")),
    ):
        if found != v["ts"]:
            fail(
                f"sdks/typescript/package-lock.json: {label} is {found}, "
                f"package.json is {v['ts']} — run `npm install` in sdks/typescript/"
            )


def check_banners(v: dict[str, str]) -> None:
    """Each banner line is expected verbatim; the anchor locates near misses."""
    expectations = [
        (
            "AGENTS.md",
            f"Rust v{v['rust']} · Python v{v['python']} (PyPI) · TypeScript v{v['ts']} (npm)",
            "(PyPI) · TypeScript v",
        ),
        (
            "llms.txt",
            f"- Python {v['python']} · TypeScript {v['ts']} · Rust {v['rust']}",
            "- Python ",
        ),
        (
            "skills/motosan-ai/SKILL.md",
            f"Multi-provider LLM SDK — Python {v['python']} / Rust {v['rust']} / TypeScript {v['ts']}",
            "Multi-provider LLM SDK — Python ",
        ),
        (
            "README.md",
            f"| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v{v['rust']} |",
            "| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) |",
        ),
        (
            "README.md",
            f"| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v{v['python']} |",
            "| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) |",
        ),
        (
            "README.md",
            f"| TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) | v{v['ts']} |",
            "| TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) |",
        ),
    ]

    for rel, expected, anchor in expectations:
        lines = read_text(rel).splitlines()
        if expected in lines:
            continue
        near = [line for line in lines if anchor in line]
        if near:
            fail(
                f"{rel}: version banner is stale\n    expected: {expected}\n    found:    {near[0]}"
            )
        else:
            fail(
                f"{rel}: version banner not found (was it reworded?)\n"
                f"    expected: {expected}\n"
                f"    no line contains the anchor {anchor!r} — "
                f"update scripts/check-versions.py if the banner moved on purpose"
            )


def check_changelogs(v: dict[str, str]) -> None:
    """The first released heading after [Unreleased] is the shipped version."""
    heading = re.compile(r"^## \[(?P<version>[^\]]+)\]")
    for rel, key in (
        ("sdks/rust/CHANGELOG.md", "rust"),
        ("sdks/python/CHANGELOG.md", "python"),
        ("sdks/typescript/CHANGELOG.md", "ts"),
    ):
        versions = [
            match.group("version")
            for line in read_text(rel).splitlines()
            if (match := heading.match(line)) and match.group("version") != "Unreleased"
        ]
        if not versions:
            fail(f"{rel}: no released version heading found")
        elif versions[0] != v[key]:
            fail(
                f"{rel}: first released heading is [{versions[0]}], "
                f"manifest is {v[key]} — the release entry was not renamed from [Unreleased]"
            )


def check_no_pinned_snippets() -> None:
    """Docs must teach `cargo add`, never a snippet that pins the version."""
    for path in sorted(ROOT.rglob("*")):
        if path.suffix not in SNIPPET_SCAN_SUFFIXES or not path.is_file():
            continue
        rel = path.relative_to(ROOT).as_posix()
        if any(
            rel == excluded or rel.startswith(f"{excluded}/")
            for excluded in SNIPPET_SCAN_EXCLUDED_DIRS
        ):
            continue
        if any(
            part in SNIPPET_SCAN_EXCLUDED_DIRS for part in path.relative_to(ROOT).parts
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
    versions = manifest_versions()
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
