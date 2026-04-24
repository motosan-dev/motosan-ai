from motosan_ai.types import SystemBlock, system_block_to_dict


def test_system_block_new_defaults_cache_false():
    block = SystemBlock.new("Hello")
    assert block.text == "Hello"
    assert block.cache_control is False


def test_system_block_cached_sets_cache_true():
    block = SystemBlock.cached("Cached prompt")
    assert block.text == "Cached prompt"
    assert block.cache_control is True


def test_system_block_to_dict_plain_omits_cache_control():
    block = SystemBlock.new("plain")
    assert system_block_to_dict(block) == {"type": "text", "text": "plain"}


def test_system_block_to_dict_cached_includes_ephemeral():
    block = SystemBlock.cached("cached")
    assert system_block_to_dict(block) == {
        "type": "text",
        "text": "cached",
        "cache_control": {"type": "ephemeral"},
    }
