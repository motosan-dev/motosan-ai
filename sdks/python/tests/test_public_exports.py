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
