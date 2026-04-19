use percent_encoding::percent_decode_str;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::error::Error;

pub async fn wait_for_callback(port: u16) -> Result<(String, String), Error> {
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Error::Callback(format!(
                    "port {port} is already in use; close other instances and retry"
                ))
            } else {
                Error::Io(e)
            }
        })?;

    // Loop until we receive the actual callback request (browser may also send
    // a favicon request before or after the redirect).
    loop {
        let (mut stream, _) = listener.accept().await?;
        let buf = read_headers(&mut stream).await?;
        let request = String::from_utf8_lossy(&buf);

        if !is_callback_request(&request) {
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }

        let html = "<html><body>Login successful. You can close this tab.</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes()).await;

        return parse_callback(&request);
    }
}

async fn read_headers(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if buf.len() + n >= 16384 {
            return Err(Error::Callback("request too large".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    Ok(buf)
}

fn is_callback_request(request: &str) -> bool {
    let path = request
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("");
    path.starts_with("/auth/callback") && path.contains("code=")
}

fn parse_callback(request: &str) -> Result<(String, String), Error> {
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::Callback("malformed HTTP request".into()))?;

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut state = None;

    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let decoded = percent_decode_str(v)
                .decode_utf8()
                .map_err(|_| Error::Callback(format!("param '{k}' is not valid UTF-8")))?
                .into_owned();
            match k {
                "code" => code = Some(decoded),
                "state" => state = Some(decoded),
                _ => {}
            }
        }
    }

    let code = code.ok_or_else(|| Error::Callback("missing code param".into()))?;
    let state = state.ok_or_else(|| Error::Callback("missing state param".into()))?;
    Ok((code, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
    }

    #[test]
    fn parses_normal_callback() {
        let (code, state) = parse_callback(&get("/auth/callback?code=abc123&state=xyz")).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz");
    }

    #[test]
    fn decodes_percent_encoded_params() {
        let (code, state) =
            parse_callback(&get("/auth/callback?code=ab%2Bcd&state=x%3Dy")).unwrap();
        assert_eq!(code, "ab+cd");
        assert_eq!(state, "x=y");
    }

    #[test]
    fn extra_params_are_ignored() {
        let (code, state) =
            parse_callback(&get("/auth/callback?code=c&state=s&session_state=ignored")).unwrap();
        assert_eq!(code, "c");
        assert_eq!(state, "s");
    }

    #[test]
    fn missing_code_returns_error() {
        let err = parse_callback(&get("/auth/callback?state=s")).unwrap_err();
        assert!(err.to_string().contains("missing code"));
    }

    #[test]
    fn missing_state_returns_error() {
        let err = parse_callback(&get("/auth/callback?code=c")).unwrap_err();
        assert!(err.to_string().contains("missing state"));
    }

    #[test]
    fn no_query_string_returns_error() {
        let err = parse_callback(&get("/auth/callback")).unwrap_err();
        assert!(matches!(err, Error::Callback(_)));
    }

    #[test]
    fn non_callback_path_is_not_callback() {
        assert!(!is_callback_request(&get("/favicon.ico")));
        assert!(!is_callback_request(&get("/")));
    }

    #[test]
    fn callback_path_with_code_is_callback() {
        assert!(is_callback_request(&get(
            "/auth/callback?code=abc&state=xyz"
        )));
    }
}
