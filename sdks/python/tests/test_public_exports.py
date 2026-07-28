"""Pin the package's public export surface.

motosan_ai/__init__.py re-exports through explicit imports and a
hand-maintained __all__, so a symbol that is never listed is invisible to
callers no matter how correct it is. TypeScript pins its exports in
tests/index.test.ts; this is the Python equivalent.
"""

from __future__ import annotations

import motosan_ai

NATIVE_MODEL_EXPORTS = [
    "FreeformTool",
    "FreeformToolFormat",
    "FunctionCallOutputContent",
    "FunctionCallOutputContentItem",
    "FunctionCallOutputEncryptedContent",
    "FunctionCallOutputInputImage",
    "FunctionCallOutputInputText",
    "FunctionCallOutputPayload",
    "FunctionCallOutputText",
    "ImageDetail",
    "ModelChatRequest",
    "ModelChatRequestBuilder",
    "ModelChatResponse",
    "ModelContextItem",
    "ModelContextMessage",
    "ModelContextToolCall",
    "ModelContextToolOutput",
    "ModelStreamDelta",
    "ModelStreamDone",
    "ModelStreamFreeformInput",
    "ModelStreamFunctionArguments",
    "ModelStreamText",
    "ModelStreamThinkingDelta",
    "ModelStreamThinkingDone",
    "ModelStreamToolCallDone",
    "ModelStreamUsage",
    "ModelToolCall",
    "ModelToolCallFreeform",
    "ModelToolCallFunction",
    "ModelToolOutput",
    "ModelToolOutputCustom",
    "ModelToolOutputFunction",
    "ModelToolSpec",
    "ModelToolSpecFreeform",
    "ModelToolSpecFunction",
    "UnsupportedFeatureError",
]


def test_native_symbols_are_importable_from_the_package_root():
    missing = [name for name in NATIVE_MODEL_EXPORTS if not hasattr(motosan_ai, name)]
    assert missing == []


def test_native_symbols_are_listed_in_dunder_all():
    missing = [name for name in NATIVE_MODEL_EXPORTS if name not in motosan_ai.__all__]
    assert missing == []


def test_dunder_all_is_sorted_and_free_of_duplicates():
    assert motosan_ai.__all__ == sorted(set(motosan_ai.__all__))


def test_every_all_entry_actually_resolves():
    unresolved = [name for name in motosan_ai.__all__ if not hasattr(motosan_ai, name)]
    assert unresolved == []


P2_EXPORTS = ["collect_model_stream"]


def test_p2_symbols_are_importable_and_listed():
    for name in P2_EXPORTS:
        assert hasattr(motosan_ai, name), f"{name} is not importable from motosan_ai"
        assert name in motosan_ai.__all__, f"{name} is missing from __all__"


def test_collect_model_stream_is_the_native_collector():
    from motosan_ai._stream_collect import collect_model_stream

    assert motosan_ai.collect_model_stream is collect_model_stream


def test_capability_constructors_are_reachable_from_the_package_root():
    caps = motosan_ai.ProviderCapabilities
    assert caps.with_freeform_tools().supports_freeform_tools is True
    assert caps.with_image_and_freeform_tools().supports_image is True
    assert caps.full().supports_freeform_tools is False


def test_native_provider_methods_are_wired():
    from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
    from motosan_ai.providers.openai import OpenAIProvider

    for provider_cls in (ChatGptCodexProvider, OpenAIProvider):
        assert hasattr(provider_cls, "model_chat"), provider_cls.__name__
        assert hasattr(provider_cls, "model_stream"), provider_cls.__name__
    assert hasattr(motosan_ai.Client, "model_chat_with")
    assert hasattr(motosan_ai.Client, "model_stream_with")
    assert hasattr(motosan_ai.Client, "model_stream_collect_with")
