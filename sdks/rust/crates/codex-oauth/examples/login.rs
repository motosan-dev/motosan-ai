#[tokio::main]
async fn main() {
    match codex_oauth::login().await {
        Ok(token) => {
            println!("access_token : {}", &token.access_token[..40]);
            println!("refresh_token: {}", &token.refresh_token[..20]);
            println!("expires_in   : {}s", token.expires_in);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
