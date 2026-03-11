import pytest

from motosan_ai.error import AuthError, InvalidRequestError, ProviderError, RateLimitError
from motosan_ai.providers.minimax import MinimaxProvider


def test_error_mapping():
    with pytest.raises(AuthError):
        MinimaxProvider._raise_for_status(401, "unauthorized")
    with pytest.raises(RateLimitError):
        MinimaxProvider._raise_for_status(429, "rate")
    with pytest.raises(InvalidRequestError):
        MinimaxProvider._raise_for_status(400, "bad")
    with pytest.raises(ProviderError):
        MinimaxProvider._raise_for_status(500, "oops")
