//! Gemini (Google Cloud Code Assist) OAuth provider config.
//!
//! The `client_id` and `client_secret` below are Google's public installed-application
//! credentials from the Gemini CLI open-source project
//! (<https://github.com/google-gemini/gemini-cli>). Per Google's OAuth2 documentation for
//! installed apps, these values are intentionally distributed in client software and are
//! not treated as confidential — embedding them in source code is the documented practice.

use crate::OAuthConfig;

pub fn gemini() -> OAuthConfig {
    OAuthConfig {
        client_id: "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
        client_secret: Some("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"),
        auth_url: "https://accounts.google.com/o/oauth2/auth",
        token_url: "https://oauth2.googleapis.com/token",
        scopes: &[
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ],
        redirect_port: None,
        extra_auth_params: &[("access_type", "offline")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_config_has_google_client_id() {
        let c = gemini();
        assert!(c.client_id.contains("681255809395"));
    }

    #[test]
    fn gemini_config_has_client_secret() {
        let c = gemini();
        assert!(c.client_secret.is_some());
    }

    #[test]
    fn gemini_config_redirect_port_is_dynamic() {
        let c = gemini();
        assert!(c.redirect_port.is_none());
    }

    #[test]
    fn gemini_config_auth_url_is_google() {
        let c = gemini();
        assert!(c.auth_url.contains("accounts.google.com"));
    }

    #[test]
    fn gemini_config_scopes_include_cloud_platform() {
        let c = gemini();
        assert!(c.scopes.iter().any(|s| s.contains("cloud-platform")));
    }
}
