from __future__ import annotations

from motosan_ai.oauth._flow import OAuthConfig


def gemini_config() -> OAuthConfig:
    return OAuthConfig(
        client_id="681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
        client_secret="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",
        auth_url="https://accounts.google.com/o/oauth2/auth",
        token_url="https://oauth2.googleapis.com/token",
        scopes=(
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ),
        callback_path="/auth/callback",
        redirect_uri_host="127.0.0.1",
        extra_auth_params=(("access_type", "offline"),),
    )
