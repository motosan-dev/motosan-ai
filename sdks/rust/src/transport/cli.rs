//! Shared helpers for the CLI backends (claude-code / codex-cli / gemini-cli),
//! compiled ONCE behind the private `_cli` umbrella feature.

use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::process::ExitStatus;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::process::{Child, ChildStderr};

use crate::error::MotosanError;

const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const STDERR_RETAIN_BYTES: usize = 8_000;
const STDERR_EXCERPT_CHARS: usize = 2_000;

/// Owns a piped child stderr as soon as the subprocess is spawned.
///
/// [`poll_with_stderr`] cooperatively drains this pipe alongside stdin writes,
/// stdout reads, and child waits so verbose diagnostics cannot block progress.
/// Only a small prefix is retained for errors.
pub(crate) struct StderrCapture {
    pipe: Option<ChildStderr>,
    retained: Vec<u8>,
}

impl StderrCapture {
    #[cfg(test)]
    pub(crate) fn start(child: &mut Option<Child>) -> Self {
        match child.as_mut() {
            Some(child) => Self::start_child(child),
            None => Self {
                pipe: None,
                retained: Vec::new(),
            },
        }
    }

    pub(crate) fn start_child(child: &mut Child) -> Self {
        Self {
            pipe: child.stderr.take(),
            retained: Vec::with_capacity(STDERR_RETAIN_BYTES),
        }
    }

    async fn finish(&mut self) -> String {
        if self.pipe.is_some() {
            let _ =
                tokio::time::timeout(STDERR_DRAIN_TIMEOUT, poll_fn(|cx| self.poll_drain(cx))).await;
        }
        self.pipe = None;

        String::from_utf8_lossy(&self.retained)
            .trim()
            .chars()
            .take(STDERR_EXCERPT_CHARS)
            .collect()
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(pipe) = self.pipe.as_mut() else {
            return Poll::Ready(());
        };

        let mut chunk = [0_u8; 8_192];
        for _ in 0..16 {
            let mut read_buf = ReadBuf::new(&mut chunk);
            match Pin::new(&mut *pipe).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) if read_buf.filled().is_empty() => {
                    self.pipe = None;
                    return Poll::Ready(());
                }
                Poll::Ready(Ok(())) => {
                    let read = read_buf.filled().len();
                    let available = STDERR_RETAIN_BYTES.saturating_sub(self.retained.len());
                    self.retained
                        .extend_from_slice(&read_buf.filled()[..available.min(read)]);
                }
                Poll::Ready(Err(_)) => {
                    self.pipe = None;
                    return Poll::Ready(());
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Poll a subprocess operation and its stderr pipe together.
pub(crate) async fn poll_with_stderr<F>(future: F, stderr: &mut StderrCapture) -> F::Output
where
    F: Future,
{
    tokio::pin!(future);
    poll_fn(|cx| {
        let _ = stderr.poll_drain(cx);
        future.as_mut().poll(cx)
    })
    .await
}

enum ChildExit {
    NotTracked,
    Exited(ExitStatus),
    WaitFailed(String),
}

/// Wait for and validate the child before a successful terminal event escapes.
pub(crate) async fn validate_terminal_exit(
    child: &mut Option<Child>,
    stderr: &mut StderrCapture,
    cli_label: &str,
) -> Result<(), MotosanError> {
    let exit = wait_bounded(child, stderr, false).await;
    let stderr_excerpt = stderr.finish().await;

    match exit {
        ChildExit::NotTracked => Ok(()),
        ChildExit::Exited(status) if status.success() => Ok(()),
        ChildExit::Exited(status) => Err(unexpected_exit(
            cli_label,
            &status_text(&status),
            stderr_excerpt,
            Some("process exited after a terminal event".to_string()),
        )),
        ChildExit::WaitFailed(detail) => Err(unexpected_exit(
            cli_label,
            "unknown",
            stderr_excerpt,
            Some(detail),
        )),
    }
}

/// Kill/reap a child and finish draining stderr for timeout/parser-error paths.
pub(crate) async fn terminate(child: &mut Option<Child>, stderr: &mut StderrCapture) {
    let _ = wait_bounded(child, stderr, true).await;
    let _ = stderr.finish().await;
}

/// Build the established premature-EOF/read-error contract after bounded reap.
pub(crate) async fn abnormal_exit_error(
    child: &mut Option<Child>,
    stderr: &mut StderrCapture,
    cli_label: &str,
    read_err: Option<std::io::Error>,
) -> MotosanError {
    let exit = wait_bounded(child, stderr, false).await;
    let stderr_excerpt = stderr.finish().await;

    match exit {
        ChildExit::NotTracked => unexpected_exit(
            cli_label,
            "unknown",
            stderr_excerpt,
            read_err.map(|err| format!("stdout read error: {err}")),
        ),
        ChildExit::Exited(status) => unexpected_exit(
            cli_label,
            &status_text(&status),
            stderr_excerpt,
            read_err.map(|err| format!("stdout read error: {err}")),
        ),
        ChildExit::WaitFailed(detail) => {
            unexpected_exit(cli_label, "unknown", stderr_excerpt, Some(detail))
        }
    }
}

async fn wait_bounded(
    child: &mut Option<Child>,
    stderr: &mut StderrCapture,
    kill_first: bool,
) -> ChildExit {
    let Some(mut child) = child.take() else {
        return ChildExit::NotTracked;
    };

    if kill_first {
        let _ = child.start_kill();
    }

    match tokio::time::timeout(CHILD_EXIT_TIMEOUT, poll_with_stderr(child.wait(), stderr)).await {
        Ok(Ok(status)) => ChildExit::Exited(status),
        Ok(Err(err)) => ChildExit::WaitFailed(format!("failed to wait for child: {err}")),
        Err(_) => {
            let _ = child.start_kill();
            match tokio::time::timeout(CHILD_EXIT_TIMEOUT, poll_with_stderr(child.wait(), stderr))
                .await
            {
                Ok(Ok(status)) => ChildExit::WaitFailed(format!(
                    "child did not exit within {}s; killed with status {}",
                    CHILD_EXIT_TIMEOUT.as_secs(),
                    status_text(&status)
                )),
                Ok(Err(err)) => {
                    ChildExit::WaitFailed(format!("failed to reap timed-out child: {err}"))
                }
                Err(_) => ChildExit::WaitFailed(format!(
                    "child did not exit after kill within {}s",
                    CHILD_EXIT_TIMEOUT.as_secs()
                )),
            }
        }
    }
}

fn status_text(status: &ExitStatus) -> String {
    status
        .code()
        .map_or_else(|| status.to_string(), |code| code.to_string())
}

fn unexpected_exit(
    cli_label: &str,
    status: &str,
    stderr_excerpt: String,
    fallback_detail: Option<String>,
) -> MotosanError {
    let detail = if !stderr_excerpt.is_empty() {
        stderr_excerpt
    } else {
        fallback_detail.unwrap_or_else(|| "stream ended before a terminal event".to_string())
    };

    MotosanError::Stream(format!(
        "{cli_label} exited unexpectedly (status {status}): {detail}"
    ))
}
