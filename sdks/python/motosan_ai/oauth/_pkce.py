from __future__ import annotations

import base64
import hashlib
import secrets
from dataclasses import dataclass


@dataclass(frozen=True)
class Pkce:
    verifier: str
    challenge: str

    @classmethod
    def generate(cls) -> Pkce:
        verifier_bytes = secrets.token_bytes(64)
        verifier = base64.urlsafe_b64encode(verifier_bytes).rstrip(b"=").decode("ascii")
        challenge = (
            base64.urlsafe_b64encode(hashlib.sha256(verifier.encode("ascii")).digest())
            .rstrip(b"=")
            .decode("ascii")
        )
        return cls(verifier=verifier, challenge=challenge)
