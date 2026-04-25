from __future__ import annotations

import base64
import hashlib

from motosan_ai.oauth._pkce import Pkce


def test_verifier_is_base64url_no_pad():
    p = Pkce.generate()
    assert all(c.isalnum() or c in "-_" for c in p.verifier)
    assert "=" not in p.verifier
    assert len(p.verifier) == 86


def test_challenge_matches_s256_of_verifier():
    p = Pkce.generate()
    expected = (
        base64.urlsafe_b64encode(hashlib.sha256(p.verifier.encode()).digest()).rstrip(b"=").decode()
    )
    assert p.challenge == expected


def test_challenge_is_base64url_no_pad():
    p = Pkce.generate()
    assert all(c.isalnum() or c in "-_" for c in p.challenge)
    assert "=" not in p.challenge


def test_each_generate_is_unique():
    assert Pkce.generate().verifier != Pkce.generate().verifier
