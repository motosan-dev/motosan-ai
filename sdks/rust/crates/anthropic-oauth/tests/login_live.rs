//! Live login smoke test.
//!
//! `#[ignore]`'d by default — running it pops open a browser window,
//! requires a Claude Pro/Max account, and binds to port 53692 on
//! localhost. It is intended to be run manually before releasing a new
//! version of `anthropic-oauth`:
//!
//! ```bash
//! cargo test -p anthropic-oauth --test login_live -- --ignored
//! ```
//!
//! Success criterion: the returned `Token.access_token` starts with the
//! `sk-ant-oat01-` prefix and the refresh token is non-empty.

#[tokio::test]
#[ignore = "interactive: opens a browser and requires Claude Pro/Max account"]
async fn live_login_returns_setup_token() {
    let token = anthropic_oauth::login()
        .await
        .expect("interactive login must succeed");

    assert!(
        token.access_token.starts_with("sk-ant-oat01-"),
        "access_token did not have expected setup-token prefix; got: {}",
        &token.access_token.chars().take(20).collect::<String>(),
    );

    assert!(
        !token.refresh_token.is_empty(),
        "refresh_token must be non-empty for subsequent refresh()"
    );
    assert!(
        token.expires_in > 0,
        "expires_in must be positive; got {}",
        token.expires_in
    );

    eprintln!("Live login OK. expires_in={}s", token.expires_in);
}
