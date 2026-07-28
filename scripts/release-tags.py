#!/usr/bin/env python3
"""Resolve which release tags to create, straight from the manifests.

Used by `.github/workflows/release-tag.yml`. Tag names are derived from each
package's own manifest rather than typed by a human, so a tag cannot disagree
with the version it points at.

    python3 scripts/release-tags.py --package rust --package python
    python3 scripts/release-tags.py --from-labels "release:rust,needs-review"

Prints a JSON array of {package, version, tag, workflow} to stdout, and (when
run inside Actions) writes it to `$GITHUB_OUTPUT` as `plan`.

Needs Python >= 3.11; stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import sys

import _release_sites as sites

LABEL_PREFIX = "release:"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="release-tags.py")
    parser.add_argument(
        "--package",
        action="append",
        default=[],
        choices=sorted(sites.PACKAGES),
        help="repeatable; package to tag",
    )
    parser.add_argument(
        "--from-labels",
        default="",
        help=f"comma-separated labels; `{LABEL_PREFIX}<package>` selects a package",
    )
    return parser.parse_args(argv)


def packages_from_labels(labels: str) -> list[str]:
    selected = []
    for label in labels.split(","):
        label = label.strip()
        if not label.startswith(LABEL_PREFIX):
            continue
        package = label[len(LABEL_PREFIX) :]
        if package not in sites.PACKAGES:
            raise SystemExit(
                f"release-tags: label {label!r} names no known package; "
                f"expected one of {', '.join(sorted(sites.PACKAGES))}"
            )
        selected.append(package)
    return selected


def main(argv: list[str]) -> int:
    args = parse_args(argv)

    packages = list(
        dict.fromkeys(args.package + packages_from_labels(args.from_labels))
    )
    if not packages:
        print(
            "release-tags: nothing selected — pass --package or a "
            f"`{LABEL_PREFIX}<package>` label",
            file=sys.stderr,
        )
        return 1

    plan = []
    for package in packages:
        version = sites.package_version(package)
        spec = sites.PACKAGES[package]
        plan.append(
            {
                "package": package,
                "version": version,
                "tag": f"{spec['tag_prefix']}{version}",
                "workflow": spec["workflow"],
            }
        )

    serialized = json.dumps(plan)
    print(serialized)

    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"plan={serialized}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
