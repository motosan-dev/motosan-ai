"""Snapshot-testing helper for provider wire-format drift detection."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

SNAPSHOT_DIR = Path(__file__).parent / "snapshots"


def snapshot_path(name: str) -> Path:
    return SNAPSHOT_DIR / f"{name}.json"


def assert_snapshot(name: str, value: Any) -> None:
    path = snapshot_path(name)
    serialized = json.dumps(value, indent=2, sort_keys=True)
    update = os.environ.get("UPDATE_SNAPSHOTS") == "1"

    if update or not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(serialized + "\n")
        return

    stored = path.read_text().rstrip("\n")
    if stored != serialized:
        raise AssertionError(
            f"snapshot mismatch for {name}\n"
            f"  path:     {path}\n"
            f"  stored:   {stored[:200]}...\n"
            f"  received: {serialized[:200]}...\n"
            "  regenerate with: UPDATE_SNAPSHOTS=1 pytest tests/parity/"
        )
