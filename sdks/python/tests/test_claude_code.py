from __future__ import annotations

import json
import os
from unittest.mock import patch

import pytest

from motosan_ai.providers.claude_code import (
    ClaudeCodeClient,
    _messages_to_prompt,
    _model_to_forward,
    _parse_agent_json,
    _parse_ndjson_line,
)
from motosan_ai.types import Message

# ---------------------------------------------------------------------------
# model_to_forward
# ---------------------------------------------------------------------------


class TestModelToForward:
    def test_named_models(self):
        assert _model_to_forward("sonnet") == "sonnet"
        assert _model_to_forward("opus") == "opus"
        assert _model_to_forward("claude-sonnet-4-6") == "claude-sonnet-4-6"

    def test_trimmed(self):
        assert _model_to_forward("  sonnet  ") == "sonnet"

    def test_default_skipped(self):
        assert _model_to_forward("default") is None
        assert _model_to_forward("Default") is None
        assert _model_to_forward("DEFAULT") is None
        assert _model_to_forward("  default  ") is None

    def test_empty_and_whitespace_skipped(self):
        assert _model_to_forward("") is None
        assert _model_to_forward("   ") is None
        assert _model_to_forward("\t") is None


# ---------------------------------------------------------------------------
# messages_to_prompt
# ---------------------------------------------------------------------------


class TestMessagesToPrompt:
    def test_single_user_message(self):
        msgs = [Message.user("hello")]
        sys, prompt = _messages_to_prompt(msgs)
        assert sys is None
        assert prompt == "hello"

    def test_multi_turn(self):
        msgs = [
            Message.user("hi"),
            Message.assistant("hello"),
            Message.user("how are you?"),
        ]
        sys, prompt = _messages_to_prompt(msgs)
        assert sys is None
        assert "[user]\nhi" in prompt
        assert "[assistant]\nhello" in prompt
        assert "[user]\nhow are you?" in prompt

    def test_system_extraction(self):
        msgs = [Message.system("you are helpful"), Message.user("hello")]
        sys, prompt = _messages_to_prompt(msgs)
        assert sys == "you are helpful"
        assert prompt == "hello"

    def test_empty(self):
        sys, prompt = _messages_to_prompt([])
        assert sys is None
        assert prompt == ""


# ---------------------------------------------------------------------------
# parse_agent_json
# ---------------------------------------------------------------------------


class TestParseAgentJson:
    def test_with_usage(self):
        raw = json.dumps(
            {
                "result": "hello world",
                "usage": {"input_tokens": 10, "output_tokens": 5},
            }
        )
        text, usage, _sid = _parse_agent_json(raw)
        assert text == "hello world"
        assert usage.input_tokens == 10
        assert usage.output_tokens == 5

    def test_without_usage(self):
        raw = json.dumps({"result": "hello"})
        text, usage, _sid = _parse_agent_json(raw)
        assert text == "hello"
        assert usage.input_tokens == 0
        assert usage.output_tokens == 0

    def test_invalid_json(self):
        from motosan_ai.error import ProviderError

        with pytest.raises(ProviderError, match="failed to parse"):
            _parse_agent_json("not json")

    def test_returns_session_id(self):
        text, usage, sid = _parse_agent_json('{"result":"hi","session_id":"s1"}')
        assert text == "hi"
        assert sid == "s1"

    def test_session_id_none_when_absent(self):
        text, usage, sid = _parse_agent_json('{"result":"hi"}')
        assert sid is None


# ---------------------------------------------------------------------------
# parse_ndjson_line
# ---------------------------------------------------------------------------


class TestParseNdjsonLine:
    def test_assistant_text_event(self):
        line = json.dumps(
            {
                "type": "assistant",
                "message": {
                    "content": [{"type": "text", "text": "Hello"}],
                },
            }
        )
        events = _parse_ndjson_line(line)
        assert len(events) == 1
        event = events[0]
        assert event.content == "Hello"
        assert not event.done

    def test_assistant_multiple_text_blocks(self):
        line = json.dumps(
            {
                "type": "assistant",
                "message": {
                    "content": [
                        {"type": "text", "text": "Hello "},
                        {"type": "text", "text": "world"},
                    ],
                },
            }
        )
        events = _parse_ndjson_line(line)
        assert len(events) == 1
        event = events[0]
        assert event.content == "Hello world"
        assert not event.done

    def test_assistant_thinking_block_ignored(self):
        line = json.dumps(
            {
                "type": "assistant",
                "message": {
                    "content": [{"type": "thinking", "thinking": "hmm"}],
                },
            }
        )
        assert _parse_ndjson_line(line) == []

    def test_assistant_empty_content(self):
        line = json.dumps(
            {
                "type": "assistant",
                "message": {"content": []},
            }
        )
        assert _parse_ndjson_line(line) == []

    def test_assistant_empty_text_ignored(self):
        line = json.dumps(
            {
                "type": "assistant",
                "message": {
                    "content": [{"type": "text", "text": ""}],
                },
            }
        )
        assert _parse_ndjson_line(line) == []

    def test_result_event(self):
        events = _parse_ndjson_line('{"type":"result","subtype":"success","result":"done"}')
        assert len(events) == 1
        assert events[0].done

    def test_unknown_type_ignored(self):
        assert _parse_ndjson_line('{"type":"system","subtype":"init"}') == []

    def test_malformed_json(self):
        assert _parse_ndjson_line("not json") == []
        assert _parse_ndjson_line("{") == []
        assert _parse_ndjson_line("") == []

    def test_tool_use_block_yields_triplet(self):
        line = (
            '{"type":"assistant","message":{"content":['
            '{"type":"tool_use","id":"toolu_01","name":"Read","input":{"path":"/tmp/x"}}]}}'
        )
        events = _parse_ndjson_line(line)
        assert [e.event_type for e in events] == [
            "tool_call_start",
            "tool_call_args",
            "tool_call_end",
        ]
        assert events[0].tool_call_id == "toolu_01"
        assert events[0].tool_call_name == "Read"
        assert events[1].tool_call_args_delta == '{"path":"/tmp/x"}'
        assert events[2].tool_call_id == "toolu_01"

    def test_assistant_text_and_tool_use_preserves_text(self):
        line = (
            '{"type":"assistant","message":{"content":['
            '{"type":"text","text":"let me read it"},'
            '{"type":"tool_use","id":"toolu_01","name":"Read","input":{}}]}}'
        )
        events = _parse_ndjson_line(line)
        assert events[0].content == "let me read it"
        assert [e.event_type for e in events[1:]] == [
            "tool_call_start",
            "tool_call_args",
            "tool_call_end",
        ]


# ---------------------------------------------------------------------------
# ClaudeCodeClient construction
# ---------------------------------------------------------------------------


class TestClaudeCodeClientConstruction:
    def test_default_binary(self):
        with patch.dict("os.environ", {}, clear=True):
            client = ClaudeCodeClient()
            assert client._binary_path == "claude"

    def test_env_override(self):
        with patch.dict("os.environ", {"CLAUDE_CODE_PATH": "/usr/local/bin/claude"}):
            client = ClaudeCodeClient()
            assert client._binary_path == "/usr/local/bin/claude"

    def test_with_path(self):
        client = ClaudeCodeClient.with_path("/opt/claude")
        assert client._binary_path == "/opt/claude"

    def test_model_setter(self):
        client = ClaudeCodeClient().model("sonnet")
        assert client._model == "sonnet"

    def test_agent_mode_setter(self):
        client = ClaudeCodeClient().agent_mode(True)
        assert client._agent_mode is True

    def test_chaining(self):
        client = ClaudeCodeClient().model("opus").agent_mode(True)
        assert client._model == "opus"
        assert client._agent_mode is True

    def test_config_dataclass_holds_existing_state(self):
        client = ClaudeCodeClient(binary_path="/tmp/fake").model("sonnet").agent_mode(True)
        assert client._config.binary_path == "/tmp/fake"
        assert client._config.model == "sonnet"
        assert client._config.agent_mode is True

    def test_config_defaults_sane(self):
        client = ClaudeCodeClient()
        assert client._config.binary_path in (
            "claude",
            os.environ.get("CLAUDE_CODE_PATH", "claude"),
        )
        assert client._config.model is None
        assert client._config.agent_mode is False

    def test_private_attribute_shims_remain_mutable(self):
        client = ClaudeCodeClient()
        client._binary_path = "/tmp/claude"
        client._model = "opus"
        client._agent_mode = True

        assert client._config.binary_path == "/tmp/claude"
        assert client._config.model == "opus"
        assert client._config.agent_mode is True
        assert client._binary_path == "/tmp/claude"
        assert client._model == "opus"
        assert client._agent_mode is True

    def test_cwd_setter(self):
        client = ClaudeCodeClient().cwd("/work")
        assert client._config.cwd == "/work"

    def test_cwd_default_none(self):
        assert ClaudeCodeClient()._config.cwd is None

    def test_env_appends(self):
        client = ClaudeCodeClient().env("A", "1").env("B", "2")
        assert list(client._config.envs) == [("A", "1"), ("B", "2")]

    def test_envs_replaces(self):
        client = ClaudeCodeClient().env("X", "0").envs({"A": "1"})
        assert list(client._config.envs) == [("A", "1")]

    def test_repr_redacts_env_values(self):
        client = ClaudeCodeClient().env("ANTHROPIC_API_KEY", "sk-secret")
        r = repr(client._config)
        assert "sk-secret" not in r
        assert "<1 redacted>" in r


# ---------------------------------------------------------------------------
# _build_args
# ---------------------------------------------------------------------------


class TestBuildArgs:
    def test_basic(self):
        client = ClaudeCodeClient()
        args = client._build_args(model=None, system_prompt=None)
        assert args[:2] == ["claude", "--print"]
        assert args[-1] == "-"
        assert "--model" not in args

    def test_with_model(self):
        client = ClaudeCodeClient()
        args = client._build_args(model="sonnet", system_prompt=None)
        idx = args.index("--model")
        assert args[idx + 1] == "sonnet"

    def test_default_model_skipped(self):
        client = ClaudeCodeClient()
        args = client._build_args(model="default", system_prompt=None)
        assert "--model" not in args

    def test_client_model_used(self):
        client = ClaudeCodeClient().model("opus")
        args = client._build_args(model=None, system_prompt=None)
        idx = args.index("--model")
        assert args[idx + 1] == "opus"

    def test_request_model_overrides_client(self):
        client = ClaudeCodeClient().model("opus")
        args = client._build_args(model="sonnet", system_prompt=None)
        idx = args.index("--model")
        assert args[idx + 1] == "sonnet"

    def test_request_empty_model_overrides_client_and_skips(self):
        client = ClaudeCodeClient().model("opus")
        args = client._build_args(model="", system_prompt=None)
        assert "--model" not in args

    def test_agent_mode(self):
        client = ClaudeCodeClient().agent_mode(True)
        args = client._build_args(model=None, system_prompt=None)
        assert "--dangerously-skip-permissions" in args
        idx = args.index("--output-format")
        assert args[idx + 1] == "json"

    def test_stream_format(self):
        client = ClaudeCodeClient()
        args = client._build_args(model=None, system_prompt=None, output_format="stream-json")
        idx = args.index("--output-format")
        assert args[idx + 1] == "stream-json"
        assert "--verbose" in args

    def test_system_prompt(self):
        client = ClaudeCodeClient()
        args = client._build_args(model=None, system_prompt="be helpful")
        idx = args.index("--append-system-prompt")
        assert args[idx + 1] == "be helpful"

    def test_empty_system_prompt_skipped(self):
        client = ClaudeCodeClient()
        args = client._build_args(model=None, system_prompt="  ")
        assert "--append-system-prompt" not in args

    def test_agent_mode_with_stream_format_no_double_output_format(self):
        client = ClaudeCodeClient().agent_mode(True)
        args = client._build_args(model=None, system_prompt=None, output_format="stream-json")
        assert args.count("--output-format") == 1
        idx = args.index("--output-format")
        assert args[idx + 1] == "stream-json"
        assert "--dangerously-skip-permissions" in args
