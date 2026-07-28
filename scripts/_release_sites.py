"""Registry of every version-coupled location in the repo.

`check-versions.py` verifies these; `bump-version.py` writes them. They share
this module so the two can never disagree about where a version lives — a
drifting list would defeat both tools at once.

Three manifests are the sources of truth; everything else is derived.
"""

from __future__ import annotations

import json
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

SDKS = ("rust", "python", "ts")

MANIFESTS = {
    "rust": "sdks/rust/Cargo.toml",
    "python": "sdks/python/pyproject.toml",
    "ts": "sdks/typescript/package.json",
}

CHANGELOGS = {
    "rust": "sdks/rust/CHANGELOG.md",
    "python": "sdks/python/CHANGELOG.md",
    "ts": "sdks/typescript/CHANGELOG.md",
}

UV_LOCK = "uv.lock"
NPM_LOCK = "sdks/typescript/package-lock.json"

# Every independently-tagged package, including the OAuth helper crates, which
# the SDK-shaped tables above deliberately exclude (they carry no doc banners,
# no lockfile entry, and are bumped by hand). `release-tags.py` derives tags
# from these manifests so a tag can never disagree with what it points at.
PACKAGES: dict[str, dict[str, str]] = {
    "rust": {
        "manifest": MANIFESTS["rust"],
        "kind": "cargo",
        "tag_prefix": "rust-v",
        "workflow": "publish-rust.yml",
    },
    "python": {
        "manifest": MANIFESTS["python"],
        "kind": "pyproject",
        "tag_prefix": "python-v",
        "workflow": "publish-python.yml",
    },
    "ts": {
        "manifest": MANIFESTS["ts"],
        "kind": "npm",
        "tag_prefix": "ts-v",
        "workflow": "publish-typescript.yml",
    },
    "motosan-ai-oauth": {
        "manifest": "sdks/rust/crates/motosan-ai-oauth/Cargo.toml",
        "kind": "cargo",
        "tag_prefix": "motosan-ai-oauth-v",
        "workflow": "publish-motosan-ai-oauth.yml",
    },
    "codex-oauth": {
        "manifest": "sdks/rust/crates/codex-oauth/Cargo.toml",
        "kind": "cargo",
        "tag_prefix": "codex-oauth-v",
        "workflow": "publish-codex-oauth.yml",
    },
    "anthropic-oauth": {
        "manifest": "sdks/rust/crates/anthropic-oauth/Cargo.toml",
        "kind": "cargo",
        "tag_prefix": "anthropic-oauth-v",
        "workflow": "publish-anthropic-oauth.yml",
    },
}


def package_version(package: str) -> str:
    """Read a package's version straight from its own manifest."""
    spec = PACKAGES[package]
    if spec["kind"] == "cargo":
        return load_toml(spec["manifest"])["package"]["version"]
    if spec["kind"] == "pyproject":
        return load_toml(spec["manifest"])["project"]["version"]
    return load_json(spec["manifest"])["version"]


# (path, template, anchor). The anchor locates the line even when its version
# is stale, so it must contain no version and must match exactly one line —
# `check_anchor_uniqueness` enforces that.
BANNERS: tuple[tuple[str, str, str], ...] = (
    (
        "AGENTS.md",
        "Rust v{rust} · Python v{python} (PyPI) · TypeScript v{ts} (npm)",
        "(PyPI) · TypeScript v",
    ),
    (
        "llms.txt",
        "- Python {python} · TypeScript {ts} · Rust {rust}",
        "· Rust ",
    ),
    (
        "skills/motosan-ai/SKILL.md",
        "Multi-provider LLM SDK — Python {python} / Rust {rust} / TypeScript {ts}",
        "Multi-provider LLM SDK — Python ",
    ),
    (
        "README.md",
        "| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v{rust} |",
        "| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) |",
    ),
    (
        "README.md",
        "| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v{python} |",
        "| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) |",
    ),
    (
        "README.md",
        "| TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) | v{ts} |",
        "| TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) |",
    ),
)

SDK_LABELS = {"rust": "Rust", "python": "Python", "ts": "TypeScript"}


def read_text(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def write_text(rel: str, text: str) -> None:
    (ROOT / rel).write_text(text, encoding="utf-8")


def load_toml(rel: str) -> dict:
    with (ROOT / rel).open("rb") as handle:
        return tomllib.load(handle)


def load_json(rel: str) -> dict:
    return json.loads(read_text(rel))


def read_versions() -> dict[str, str]:
    """The three sources of truth."""
    return {
        "rust": load_toml(MANIFESTS["rust"])["package"]["version"],
        "python": load_toml(MANIFESTS["python"])["project"]["version"],
        "ts": load_json(MANIFESTS["ts"])["version"],
    }


def uv_lock_version() -> str | None:
    """The workspace member's own version as recorded in uv.lock."""
    entries = [
        pkg
        for pkg in load_toml(UV_LOCK).get("package", [])
        if pkg.get("name") == "motosan-ai" and "version" in pkg
    ]
    return entries[0]["version"] if len(entries) == 1 else None


def npm_lock_versions() -> dict[str, str | None]:
    lock = load_json(NPM_LOCK)
    return {
        "top-level version": lock.get("version"),
        'packages[""].version': lock.get("packages", {}).get("", {}).get("version"),
    }


def anchor_matches(rel: str, anchor: str) -> list[str]:
    return [line for line in read_text(rel).splitlines() if anchor in line]
