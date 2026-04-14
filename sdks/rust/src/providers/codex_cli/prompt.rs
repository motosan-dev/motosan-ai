//! Prompt assembly for the Codex CLI provider.
//!
//! Codex `exec` takes a single prompt string and has **no native
//! system-prompt flag**, so the crate-level multi-message
//! [`ChatRequest`](crate::types::ChatRequest) shape has to be flattened
//! before it can be sent on stdin. The strategy:
//!
//! 1. [`messages_to_prompt`] extracts any system message and formats the
//!    remaining turns with role labels.
//! 2. [`compose_prompt`] prepends the system prompt as an `[system
//!    instructions]` block so the model still sees it.
//!
//! This keeps the wire format deterministic and round-trippable for tests
//! while matching how the rest of the crate represents multi-turn chats.

use crate::types::{Message, Role};

/// Flatten multi-turn messages into a single prompt string for `codex exec`.
///
/// Extracts the first system message (if any) into the first return
/// value, and formats all remaining messages into the second. When there
/// is exactly one non-system message its content is returned verbatim;
/// otherwise each turn is labeled with its role (`[user]`, `[assistant]`,
/// `[tool]`) and separated by blank lines.
///
/// Returns `(system_prompt, user_prompt)`. The system prompt is handled
/// separately because Codex CLI has no `--system` flag — callers feed it
/// through [`compose_prompt`] to interleave it into the final stdin
/// payload.
pub fn messages_to_prompt(messages: &[Message]) -> (Option<String>, String) {
    let system: Option<String> = messages
        .iter()
        .find(|m| m.role == Role::System)
        .map(|m| m.content.clone());

    let non_system: Vec<&Message> = messages.iter().filter(|m| m.role != Role::System).collect();

    let prompt = if non_system.len() <= 1 {
        non_system
            .first()
            .map(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        non_system
            .iter()
            .map(|m| {
                let label = match m.role {
                    Role::User => "[user]",
                    Role::Assistant => "[assistant]",
                    Role::Tool => "[tool]",
                    Role::System => unreachable!(),
                };
                format!("{}\n{}", label, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    (system, prompt)
}

/// Combine an optional system prompt with the user prompt.
///
/// When `system` is `None` or empty, returns `user` unchanged so trivial
/// prompts stay trivial. Otherwise wraps both in labeled blocks:
///
/// ```text
/// [system instructions]
/// <system>
///
/// [user]
/// <user>
/// ```
///
/// Codex CLI lacks a `--system` flag, so this is the closest we can get
/// to a structured system-prompt channel without depending on
/// `-c` config overrides or profile files.
pub fn compose_prompt(system: Option<&str>, user: &str) -> String {
    match system {
        Some(sp) if !sp.is_empty() => format!("[system instructions]\n{sp}\n\n[user]\n{user}"),
        _ => user.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    #[test]
    fn single_user_message() {
        let msgs = vec![msg(Role::User, "hello")];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys, None);
        assert_eq!(prompt, "hello");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            msg(Role::User, "hi"),
            msg(Role::Assistant, "hello"),
            msg(Role::User, "how are you?"),
        ];
        let (_, prompt) = messages_to_prompt(&msgs);
        assert!(prompt.contains("[user]\nhi"));
        assert!(prompt.contains("[assistant]\nhello"));
        assert!(prompt.contains("[user]\nhow are you?"));
    }

    #[test]
    fn system_message_extraction() {
        let msgs = vec![msg(Role::System, "you are helpful"), msg(Role::User, "hi")];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys.as_deref(), Some("you are helpful"));
        assert_eq!(prompt, "hi");
    }

    #[test]
    fn compose_prepends_system() {
        let out = compose_prompt(Some("be terse"), "hello");
        assert!(out.starts_with("[system instructions]\nbe terse"));
        assert!(out.contains("[user]\nhello"));
    }

    #[test]
    fn compose_without_system_is_passthrough() {
        assert_eq!(compose_prompt(None, "hello"), "hello");
        assert_eq!(compose_prompt(Some(""), "hello"), "hello");
    }
}
