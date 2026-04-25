from __future__ import annotations

from motosan_ai.providers.claude_code import _parse_ndjson_line


def test_result_without_usage_emits_single_done_event():
    events = _parse_ndjson_line('{"type": "result", "result": "ok"}')
    assert len(events) == 1
    assert events[0].done is True
    assert events[0].usage is None


def test_result_with_usage_emits_usage_then_done():
    line = '{"type": "result", "result": "ok", "usage": {"input_tokens": 50, "output_tokens": 20}}'
    events = _parse_ndjson_line(line)
    assert len(events) == 2

    usage_event, done_event = events
    assert usage_event.event_type == "usage"
    assert usage_event.usage is not None
    assert usage_event.usage.input_tokens == 50
    assert usage_event.usage.output_tokens == 20
    assert usage_event.done is False
    assert done_event.done is True


def test_text_event_unchanged():
    line = '{"type": "assistant", "message": {"content": [{"type": "text", "text": "hi"}]}}'
    events = _parse_ndjson_line(line)
    assert len(events) == 1
    assert events[0].content == "hi"
    assert events[0].done is False
