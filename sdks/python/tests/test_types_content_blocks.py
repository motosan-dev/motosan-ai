from __future__ import annotations

import base64

from motosan_ai.types import (
    DocumentBlock,
    DocumentSourceBase64,
    DocumentSourceUrl,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    Role,
    TextBlock,
    content_block_to_dict,
    document_source_to_dict,
    image_source_to_dict,
)


def test_image_source_base64_to_dict():
    src = ImageSourceBase64(media_type="image/png", data="JVBER")
    assert image_source_to_dict(src) == {
        "type": "base64",
        "media_type": "image/png",
        "data": "JVBER",
    }


def test_image_source_url_to_dict():
    src = ImageSourceUrl(url="https://example.com/pic.png")
    assert image_source_to_dict(src) == {"type": "url", "url": "https://example.com/pic.png"}


def test_document_source_base64_to_dict():
    src = DocumentSourceBase64(media_type="application/pdf", data="JVBERi0xLjQK")
    assert document_source_to_dict(src) == {
        "type": "base64",
        "media_type": "application/pdf",
        "data": "JVBERi0xLjQK",
    }


def test_document_source_url_to_dict():
    src = DocumentSourceUrl(url="https://example.com/doc.pdf")
    assert document_source_to_dict(src) == {"type": "url", "url": "https://example.com/doc.pdf"}


def test_text_block_to_dict():
    block = TextBlock(text="hello")
    assert content_block_to_dict(block) == {"type": "text", "text": "hello"}


def test_image_block_to_dict_base64():
    block = ImageBlock(source=ImageSourceBase64(media_type="image/png", data="abc"))
    assert content_block_to_dict(block) == {
        "type": "image",
        "source": {"type": "base64", "media_type": "image/png", "data": "abc"},
    }


def test_document_block_to_dict_url():
    block = DocumentBlock(source=DocumentSourceUrl(url="https://x.com/d.pdf"))
    assert content_block_to_dict(block) == {
        "type": "document",
        "source": {"type": "url", "url": "https://x.com/d.pdf"},
    }


def test_user_with_image_sets_content_blocks():
    msg = Message.user_with_image("look at this", "JVBER", "image/png")
    assert msg.role == Role.user
    assert msg.content == "look at this"
    assert len(msg.content_blocks) == 2
    assert isinstance(msg.content_blocks[0], TextBlock)
    assert msg.content_blocks[0].text == "look at this"
    assert isinstance(msg.content_blocks[1], ImageBlock)
    assert isinstance(msg.content_blocks[1].source, ImageSourceBase64)
    assert msg.content_blocks[1].source.media_type == "image/png"


def test_user_with_pdf_base64():
    msg = Message.user_with_pdf_base64("summarize", "JVBERi0xLjQK")
    assert len(msg.content_blocks) == 2
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceBase64)
    assert doc.source.media_type == "application/pdf"
    assert doc.source.data == "JVBERi0xLjQK"


def test_user_with_pdf_url():
    msg = Message.user_with_pdf_url("analyze", "https://example.com/d.pdf")
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceUrl)
    assert doc.source.url == "https://example.com/d.pdf"


def test_user_with_pdf_bytes_auto_encodes():
    raw = b"%PDF-1.4\n"
    msg = Message.user_with_pdf_bytes("read", raw)
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceBase64)
    assert base64.b64decode(doc.source.data) == raw


def test_user_with_blocks_extracts_text():
    blocks = [
        TextBlock(text="describe"),
        ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png")),
    ]
    msg = Message.user_with_blocks(blocks)
    assert msg.content == "describe"
    assert msg.content_blocks == blocks


def test_message_default_content_blocks_is_empty_list():
    msg = Message.user("hello")
    assert msg.content_blocks == []
