#[cfg(feature = "claude-code")]
#[test]
fn claude_default_timeout_is_public_api() {
    assert_eq!(
        motosan_ai::claude_code::DEFAULT_TIMEOUT,
        std::time::Duration::from_secs(300)
    );
}

#[cfg(feature = "codex-cli")]
#[test]
fn codex_default_timeout_is_public_api() {
    assert_eq!(
        motosan_ai::codex_cli::DEFAULT_TIMEOUT,
        std::time::Duration::from_secs(600)
    );
}

#[cfg(feature = "gemini-cli")]
#[test]
fn gemini_default_timeout_is_public_api() {
    assert_eq!(
        motosan_ai::gemini_cli::DEFAULT_TIMEOUT,
        std::time::Duration::from_secs(600)
    );
}
