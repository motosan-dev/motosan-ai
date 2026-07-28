from __future__ import annotations

import pytest

from motosan_ai.error import InvalidRequestError, MotosanError, UnsupportedFeatureError


def test_unsupported_feature_error_subclasses_invalid_request_error():
    assert issubclass(UnsupportedFeatureError, InvalidRequestError)
    assert issubclass(UnsupportedFeatureError, MotosanError)


def test_existing_invalid_request_handlers_still_catch_it():
    with pytest.raises(InvalidRequestError):
        raise UnsupportedFeatureError("provider does not support native freeform tools")


def test_callers_can_distinguish_the_subclass():
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        raise UnsupportedFeatureError("provider does not support native freeform tools")


def test_carries_the_motosan_error_metadata_fields():
    err = UnsupportedFeatureError("nope")
    assert err.status_code is None
    assert err.retry_after is None
    assert err.request_id is None
    assert str(err) == "nope"
