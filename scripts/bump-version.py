#!/usr/bin/env python3
"""Perform the mechanical half of a release: bump versions everywhere.

    python3 scripts/bump-version.py --rust 0.28.0 --python 0.20.0
    python3 scripts/bump-version.py --ts 0.16.0 --dry-run

Several SDKs can be bumped in one run — they share the doc banners, so doing
them separately would rewrite the same lines twice. For each requested SDK it
updates the manifest, opens a dated CHANGELOG section by renaming the content
currently under `[Unreleased]`, and refreshes the derived lockfile entry
(`uv.lock` for Python, `package-lock.json` for TypeScript — a version-only
bump touches exactly the member's own version fields, which is all `uv lock`
or `npm install` would change). It then rewrites the shared version banners
and runs `check-versions.py` to prove the result is consistent.

Re-running with the same versions is a no-op: an already-bumped manifest is
left alone and an existing CHANGELOG heading is never inserted twice.

What it deliberately does NOT do: write the root CHANGELOG entry. That is
release prose, not a mechanical edit, and the summary prints a reminder.

Dates default to today in UTC — release commits are made from several
timezones and the CHANGELOG should not depend on who ran the script.

Needs Python >= 3.11; stdlib only. The locations it writes live in
`_release_sites.py`, shared with `check-versions.py`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import subprocess
import sys

import _release_sites as sites

SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?$")


class BumpError(Exception):
    """A problem the caller must fix before the release can proceed."""


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="bump-version.py",
        description="Bump one or more SDK versions and every location derived from them.",
    )
    for sdk in sites.SDKS:
        parser.add_argument(
            f"--{sdk}",
            metavar="X.Y.Z",
            help=f"new {sites.SDK_LABELS[sdk]} version",
        )
    parser.add_argument(
        "--date",
        metavar="YYYY-MM-DD",
        help="CHANGELOG date (default: today in UTC)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print what would change and write nothing",
    )
    args = parser.parse_args(argv)

    if not any(getattr(args, sdk) for sdk in sites.SDKS):
        parser.error("name at least one SDK to bump, e.g. --rust 0.28.0")

    for sdk in sites.SDKS:
        value = getattr(args, sdk)
        if value and not SEMVER.match(value):
            parser.error(f"--{sdk} {value!r} is not a valid X.Y.Z version")

    if args.date and not re.match(r"^\d{4}-\d{2}-\d{2}$", args.date):
        parser.error(f"--date {args.date!r} is not YYYY-MM-DD")

    return args


def replace_unique_line(text: str, predicate, new_line: str, description: str) -> str:
    """Rewrite the single line matching `predicate`."""
    lines = text.splitlines(keepends=True)
    hits = [index for index, line in enumerate(lines) if predicate(line.rstrip("\n"))]
    if len(hits) != 1:
        raise BumpError(
            f"{description}: expected exactly 1 matching line, found {len(hits)}"
        )
    ending = "\n" if lines[hits[0]].endswith("\n") else ""
    lines[hits[0]] = new_line + ending
    return "".join(lines)


def bump_manifest(
    sdk: str, old: str, new: str, pending: dict[str, str], log: list[str]
) -> None:
    rel = sites.MANIFESTS[sdk]
    if old == new:
        log.append(f"  = {rel} already at {new}")
        return

    text = pending.get(rel, sites.read_text(rel))
    if sdk == "ts":
        pending[rel] = replace_unique_line(
            text,
            lambda line: line.strip() == f'"version": "{old}",',
            f'  "version": "{new}",',
            rel,
        )
    else:
        pending[rel] = replace_unique_line(
            text,
            lambda line: line == f'version = "{old}"',
            f'version = "{new}"',
            rel,
        )
    log.append(f"  ✎ {rel}: {old} → {new}")


def open_changelog_section(
    sdk: str, new: str, date: str, pending: dict[str, str], log: list[str]
) -> None:
    """Rename the current [Unreleased] content into a dated release heading."""
    rel = sites.CHANGELOGS[sdk]
    text = pending.get(rel, sites.read_text(rel))
    lines = text.splitlines()

    if any(line.startswith(f"## [{new}]") for line in lines):
        log.append(f"  = {rel} already has a [{new}] section")
        return

    try:
        index = next(
            i for i, line in enumerate(lines) if line.strip() == "## [Unreleased]"
        )
    except StopIteration:
        raise BumpError(
            f"{rel}: no '## [Unreleased]' heading to release from"
        ) from None

    rest = lines[index + 1 :]
    next_heading = next(
        (offset for offset, line in enumerate(rest) if line.startswith("## ")),
        len(rest),
    )
    if not any(line.strip() for line in rest[:next_heading]):
        raise BumpError(
            f"{rel}: nothing under [Unreleased] — write the release notes before bumping {sdk}"
        )

    heading = f"## [{new}] - {date}"
    updated = lines[: index + 1] + ["", heading] + rest
    pending[rel] = "\n".join(updated) + ("\n" if text.endswith("\n") else "")
    log.append(f"  ✎ {rel}: opened {heading}")


def bump_uv_lock(old: str, new: str, pending: dict[str, str], log: list[str]) -> None:
    """Rewrite the workspace member's version inside its [[package]] block."""
    if old == new:
        return
    rel = sites.UV_LOCK
    text = pending.get(rel, sites.read_text(rel))
    lines = text.splitlines(keepends=True)

    member = [
        index
        for index, line in enumerate(lines)
        if line.rstrip("\n") == 'name = "motosan-ai"'
        and index + 1 < len(lines)
        and lines[index + 1].rstrip("\n") == f'version = "{old}"'
    ]
    if len(member) != 1:
        raise BumpError(
            f"{rel}: expected exactly 1 motosan-ai entry at version {old}, found {len(member)}"
        )
    lines[member[0] + 1] = f'version = "{new}"\n'
    pending[rel] = "".join(lines)
    log.append(f"  ✎ {rel}: motosan-ai {old} → {new}")


def bump_npm_lock(old: str, new: str, pending: dict[str, str], log: list[str]) -> None:
    if old == new:
        return
    rel = sites.NPM_LOCK
    text = pending.get(rel, sites.read_text(rel))
    lines = text.splitlines(keepends=True)

    hits = [
        index
        for index, line in enumerate(lines)
        if line.strip() in (f'"version": "{old}",', f'"version": "{old}"')
    ]
    if len(hits) != 2:
        raise BumpError(f"{rel}: expected 2 version fields at {old}, found {len(hits)}")
    for index in hits:
        lines[index] = lines[index].replace(f'"{old}"', f'"{new}"', 1)
    pending[rel] = "".join(lines)
    log.append(f'  ✎ {rel}: {old} → {new} (root + packages[""])')


def rewrite_banners(
    versions: dict[str, str], pending: dict[str, str], log: list[str]
) -> None:
    for rel, template, anchor in sites.BANNERS:
        expected = template.format(**versions)
        text = pending.get(rel, sites.read_text(rel))
        if expected in text.splitlines():
            continue
        pending[rel] = replace_unique_line(
            text,
            lambda line, anchor=anchor: anchor in line,
            expected,
            f"{rel} (anchor {anchor!r})",
        )
        log.append(f"  ✎ {rel}: {expected}")


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    date = args.date or dt.datetime.now(dt.UTC).strftime("%Y-%m-%d")

    current = sites.read_versions()
    target = dict(current)
    for sdk in sites.SDKS:
        if getattr(args, sdk):
            target[sdk] = getattr(args, sdk)

    pending: dict[str, str] = {}
    log: list[str] = []

    try:
        for sdk in sites.SDKS:
            if current[sdk] == target[sdk] and not getattr(args, sdk):
                continue
            log.append(f"{sites.SDK_LABELS[sdk]} {current[sdk]} → {target[sdk]}")
            bump_manifest(sdk, current[sdk], target[sdk], pending, log)
            open_changelog_section(sdk, target[sdk], date, pending, log)
            if sdk == "python":
                bump_uv_lock(current[sdk], target[sdk], pending, log)
            elif sdk == "ts":
                bump_npm_lock(current[sdk], target[sdk], pending, log)
        log.append("Version banners")
        rewrite_banners(target, pending, log)
    except BumpError as error:
        print(f"bump-version: {error}", file=sys.stderr)
        return 1

    for line in log:
        print(line)

    if not pending:
        print("\nNothing to do — every location already matches.")
        return 0

    if args.dry_run:
        print(f"\n--dry-run: {len(pending)} file(s) would be written, none were.")
        return 0

    for rel, text in pending.items():
        sites.write_text(rel, text)
    print(f"\nWrote {len(pending)} file(s).")

    result = subprocess.run(
        [sys.executable, str(sites.ROOT / "scripts" / "check-versions.py")],
        check=False,
    )
    if result.returncode != 0:
        return result.returncode

    changed = ", ".join(
        f"{sites.SDK_LABELS[sdk]} {target[sdk]}"
        for sdk in sites.SDKS
        if getattr(args, sdk)
    )
    print(
        f"\nNext: write the root CHANGELOG.md entry for {changed}, add an AGENTS.md\n"
        f"release paragraph, then open the release PR. Tags are pushed after it merges."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
