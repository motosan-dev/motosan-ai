use std::path::PathBuf;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::MotosanError;
use crate::types::Usage;

/// Configuration for spawning the `claude` CLI process.
pub struct SpawnConfig {
    pub binary_path: PathBuf,
    pub agent_mode: bool,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

const TIMEOUT_SECS: u64 = 300;

/// Returns `true` if `model` should be forwarded to the CLI via `--model`.
///
/// Skips empty strings, whitespace-only strings, and the sentinel value `"default"`.
pub(crate) fn should_forward_model(model: &str) -> bool {
    let trimmed = model.trim();
    !trimmed.is_empty() && trimmed != "default"
}

/// Invoke the `claude` CLI with `--print` and return `(text, usage)`.
pub async fn invoke_cli(
    config: &SpawnConfig,
    prompt: &str,
) -> Result<(String, Usage), MotosanError> {
    let mut cmd = Command::new(&config.binary_path);
    cmd.arg("--print");

    if config.agent_mode {
        cmd.arg("--dangerously-skip-permissions");
        cmd.arg("--output-format").arg("json");
    }

    if let Some(ref model) = config.model {
        if should_forward_model(model) {
            cmd.arg("--model").arg(model);
        }
    }

    if let Some(ref sp) = config.system_prompt {
        if !sp.is_empty() {
            cmd.arg("--append-system-prompt").arg(sp);
        }
    }

    // Read prompt from stdin
    cmd.arg("-");

    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| MotosanError::ProviderError(format!("failed to spawn claude CLI: {e}")))?;

    // Write prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
            MotosanError::ProviderError(format!("failed to write to claude stdin: {e}"))
        })?;
        // Drop stdin to close it so the child reads EOF.
    }

    let result = tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), child.wait_with_output())
        .await
        .map_err(|_| {
            MotosanError::ProviderError(format!(
                "claude CLI timed out after {TIMEOUT_SECS} seconds"
            ))
        })?
        .map_err(|e| MotosanError::ProviderError(format!("claude CLI process error: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(MotosanError::ProviderError(format!(
            "claude CLI exited with {}: {}",
            result.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();

    if config.agent_mode {
        parse_agent_json(&stdout)
    } else {
        Ok((
            stdout.trim().to_string(),
            Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        ))
    }
}

/// Parse the JSON output from agent mode, extracting `result` and `usage`.
fn parse_agent_json(raw: &str) -> Result<(String, Usage), MotosanError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        MotosanError::ProviderError(format!("failed to parse claude JSON output: {e}"))
    })?;

    let text = v
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    let usage = match v.get("usage") {
        Some(u) => Usage {
            input_tokens: u.get("input_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
            cache_creation_input_tokens: u
                .get("cache_creation_input_tokens")
                .and_then(|t| t.as_u64())
                .map(|t| t as u32),
            cache_read_input_tokens: u
                .get("cache_read_input_tokens")
                .and_then(|t| t.as_u64())
                .map(|t| t as u32),
        },
        None => Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };

    Ok((text, usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_forward_model_forwards_named_models() {
        assert!(should_forward_model("sonnet"));
        assert!(should_forward_model("opus"));
        assert!(should_forward_model("claude-sonnet-4-6"));
    }

    #[test]
    fn should_forward_model_skips_default() {
        assert!(!should_forward_model("default"));
    }

    #[test]
    fn should_forward_model_skips_empty() {
        assert!(!should_forward_model(""));
    }

    #[test]
    fn should_forward_model_skips_whitespace() {
        assert!(!should_forward_model("   "));
        assert!(!should_forward_model("\t"));
    }
}
