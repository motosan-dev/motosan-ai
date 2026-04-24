from __future__ import annotations

import json

import pytest

from tests._snapshots import assert_snapshot, snapshot_path


def test_snapshot_path_resolves_under_tests_snapshots():
    path = snapshot_path("my_test")
    assert path.name == "my_test.json"
    assert path.parent.name == "snapshots"


def test_assert_snapshot_writes_on_first_run(tmp_path, monkeypatch):
    monkeypatch.setenv("UPDATE_SNAPSHOTS", "1")
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    assert_snapshot("simple_text", {"role": "user", "content": "hi"})
    saved = json.loads((tmp_path / "simple_text.json").read_text())
    assert saved == {"role": "user", "content": "hi"}


def test_assert_snapshot_passes_on_match(tmp_path, monkeypatch):
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "match.json").write_text(json.dumps({"k": "v"}, indent=2, sort_keys=True))
    assert_snapshot("match", {"k": "v"})


def test_assert_snapshot_fails_on_diff(tmp_path, monkeypatch):
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "diff.json").write_text(json.dumps({"k": "old"}, indent=2, sort_keys=True))
    with pytest.raises(AssertionError, match="snapshot mismatch"):
        assert_snapshot("diff", {"k": "new"})


def test_update_env_var_overwrites_existing(tmp_path, monkeypatch):
    monkeypatch.setenv("UPDATE_SNAPSHOTS", "1")
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "over.json").write_text("{}")
    assert_snapshot("over", {"new": True})
    saved = json.loads((tmp_path / "over.json").read_text())
    assert saved == {"new": True}
