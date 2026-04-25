from __future__ import annotations

import os

from motosan_ai.providers.gemini_cli import ApprovalMode, GeminiCliClient


def _args(client: GeminiCliClient) -> list[str]:
    return client._build_args()


def test_default_binary_path_is_gemini_or_env():
    c = GeminiCliClient()
    assert c._config.binary_path in ("gemini", os.environ.get("GEMINI_CLI_PATH", "gemini"))


def test_explicit_binary_path():
    c = GeminiCliClient(binary_path="/opt/gemini")
    assert c._config.binary_path == "/opt/gemini"


def test_with_path_classmethod():
    c = GeminiCliClient.with_path("/opt/gemini-2")
    assert c._config.binary_path == "/opt/gemini-2"


def test_minimal_args_emit_headless_prefix_no_trailing_dash():
    c = GeminiCliClient(binary_path="gemini")
    assert _args(c) == ["gemini", "-p", "", "-o", "stream-json"]


def test_approval_mode_values_match_cli_flags():
    assert ApprovalMode.default.value == "default"
    assert ApprovalMode.auto_edit.value == "auto_edit"
    assert ApprovalMode.yolo.value == "yolo"
    assert ApprovalMode.plan.value == "plan"


def test_yolo_flag():
    assert "--yolo" in _args(GeminiCliClient(binary_path="gemini").yolo(True))


def test_yolo_absent_by_default():
    assert "--yolo" not in _args(GeminiCliClient(binary_path="gemini"))


def test_sandbox_flag():
    assert "--sandbox" in _args(GeminiCliClient(binary_path="gemini").sandbox(True))


def test_sandbox_absent_by_default():
    assert "--sandbox" not in _args(GeminiCliClient(binary_path="gemini"))


def test_model_flag_emitted():
    args = _args(GeminiCliClient(binary_path="gemini").model("gemini-2.5-pro"))
    assert args[args.index("-m") + 1] == "gemini-2.5-pro"


def test_model_default_sentinel_skipped():
    for sentinel in ("", "   ", "default", "Default", "DEFAULT"):
        c = GeminiCliClient(binary_path="gemini").model(sentinel)
        assert "-m" not in _args(c), f"unexpected -m for {sentinel!r}"


def test_request_empty_model_overrides_client_model_and_skips():
    c = GeminiCliClient(binary_path="gemini").model("gemini-2.5-pro")
    assert "-m" not in c._build_args(model="")


def test_approval_mode_flag():
    args = _args(GeminiCliClient(binary_path="gemini").approval_mode(ApprovalMode.auto_edit))
    assert args[args.index("--approval-mode") + 1] == "auto_edit"


def test_resume_flag_emitted():
    args = _args(GeminiCliClient(binary_path="gemini").resume("latest"))
    assert args[args.index("--resume") + 1] == "latest"


def test_resume_trims_value():
    args = _args(GeminiCliClient(binary_path="gemini").resume("  3  "))
    assert args[args.index("--resume") + 1] == "3"


def test_resume_blank_skipped():
    for blank in ("", "   ", "\t\n"):
        c = GeminiCliClient(binary_path="gemini").resume(blank)
        assert "--resume" not in _args(c)


def test_include_dir_appends():
    c = GeminiCliClient(binary_path="gemini").include_dir("/proj/a").include_dir("/proj/b")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--include-directories"]
    assert occ == ["/proj/a", "/proj/b"]


def test_extension_appends():
    c = GeminiCliClient(binary_path="gemini").extension("foo").extension("bar")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-e"]
    assert occ == ["foo", "bar"]


def test_extension_whitespace_skipped():
    assert "-e" not in _args(GeminiCliClient(binary_path="gemini").extension("   "))


def test_allowed_mcp_server_appends():
    c = (
        GeminiCliClient(binary_path="gemini")
        .allowed_mcp_server("srv-a")
        .allowed_mcp_server("srv-b")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--allowed-mcp-server-names"]
    assert occ == ["srv-a", "srv-b"]


def test_allowed_mcp_server_whitespace_skipped():
    c = GeminiCliClient(binary_path="gemini").allowed_mcp_server("\t  ")
    assert "--allowed-mcp-server-names" not in _args(c)


def test_include_dirs_replaces():
    c = GeminiCliClient(binary_path="gemini").include_dir("/old").include_dirs(["/new1", "/new2"])
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--include-directories"]
    assert occ == ["/new1", "/new2"]


def test_extensions_replaces():
    c = GeminiCliClient(binary_path="gemini").extension("old").extensions(["a", "b"])
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-e"]
    assert occ == ["a", "b"]


def test_allowed_mcp_servers_replaces():
    c = (
        GeminiCliClient(binary_path="gemini")
        .allowed_mcp_server("old")
        .allowed_mcp_servers(["x", "y"])
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--allowed-mcp-server-names"]
    assert occ == ["x", "y"]


def test_full_config_argv_order_matches_rust_common_args():
    c = (
        GeminiCliClient(binary_path="gemini")
        .model("gemini-2.5-pro")
        .yolo(True)
        .sandbox(True)
        .approval_mode(ApprovalMode.auto_edit)
        .include_dir("/proj/a")
        .include_dir("/proj/b")
        .extension("ext1")
        .allowed_mcp_server("srv1")
        .resume("latest")
    )
    assert _args(c) == [
        "gemini",
        "-p",
        "",
        "-o",
        "stream-json",
        "-m",
        "gemini-2.5-pro",
        "--yolo",
        "--sandbox",
        "--approval-mode",
        "auto_edit",
        "--include-directories",
        "/proj/a",
        "--include-directories",
        "/proj/b",
        "-e",
        "ext1",
        "--allowed-mcp-server-names",
        "srv1",
        "--resume",
        "latest",
    ]
