//! Anthropic Claude Pro/Max OAuth provider config.
//!
//! The `client_id` below is extracted from Anthropic's Claude Code CLI
//! authentication flow; the same value is used by the reference
//! implementation `@earendil-works/pi-ai`. Anthropic has not published this
//! client_id as a public app registration for third-party use — see the ToS
//! disclosure in the top-level README.

use crate::{OAuthConfig, TokenBodyFormat};

pub fn claude_pro_max() -> OAuthConfig {
    OAuthConfig {
        client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        client_secret: None,
        auth_url: "https://claude.ai/oauth/authorize",
        token_url: "https://platform.claude.com/v1/oauth/token",
        scopes: &[
            "org:create_api_key",
            "user:profile",
            "user:inference",
            "user:sessions:claude_code",
            "user:mcp_servers",
            "user:file_upload",
        ],
        redirect_port: Some(53692),
        callback_path: "/callback",
        redirect_uri_host: "localhost",
        token_body: TokenBodyFormat::Json,
        extra_auth_params: &[("code", "true")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_pro_max_has_expected_client_id() {
        assert_eq!(
            claude_pro_max().client_id,
            "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
        );
    }

    #[test]
    fn claude_pro_max_has_no_client_secret() {
        assert!(claude_pro_max().client_secret.is_none());
    }

    #[test]
    fn claude_pro_max_redirect_port_is_53692() {
        assert_eq!(claude_pro_max().redirect_port, Some(53692));
    }

    #[test]
    fn claude_pro_max_callback_path_is_slash_callback() {
        assert_eq!(claude_pro_max().callback_path, "/callback");
    }

    #[test]
    fn claude_pro_max_redirect_uri_host_is_localhost() {
        // Anthropic registers redirect_uri with literal "localhost", not "127.0.0.1".
        assert_eq!(claude_pro_max().redirect_uri_host, "localhost");
    }

    #[test]
    fn claude_pro_max_token_body_is_json() {
        assert_eq!(claude_pro_max().token_body, TokenBodyFormat::Json);
    }

    #[test]
    fn claude_pro_max_extra_auth_params_include_code_true() {
        let params = claude_pro_max().extra_auth_params;
        assert!(params.iter().any(|(k, v)| *k == "code" && *v == "true"));
    }

    #[test]
    fn claude_pro_max_scopes_include_claude_code_session() {
        assert!(claude_pro_max()
            .scopes
            .contains(&"user:sessions:claude_code"));
    }

    #[test]
    fn claude_pro_max_auth_url_is_claude_ai() {
        assert!(claude_pro_max().auth_url.contains("claude.ai"));
    }
}
