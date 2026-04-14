use crate::types::{Message, Role};

/// Flatten multi-turn messages into a single prompt string for `claude --print`.
///
/// Returns `(system_prompt, user_prompt)`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    fn user_msg(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    fn assistant_msg(content: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: content.to_string(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    fn system_msg(content: &str) -> Message {
        Message {
            role: Role::System,
            content: content.to_string(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    #[test]
    fn single_user_message() {
        let msgs = vec![user_msg("hello")];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys, None);
        assert_eq!(prompt, "hello");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            user_msg("hi"),
            assistant_msg("hello"),
            user_msg("how are you?"),
        ];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys, None);
        assert!(prompt.contains("[user]\nhi"));
        assert!(prompt.contains("[assistant]\nhello"));
        assert!(prompt.contains("[user]\nhow are you?"));
    }

    #[test]
    fn system_message_extraction() {
        let msgs = vec![system_msg("you are helpful"), user_msg("hello")];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys.as_deref(), Some("you are helpful"));
        assert_eq!(prompt, "hello");
    }

    #[test]
    fn empty_messages() {
        let msgs: Vec<Message> = vec![];
        let (sys, prompt) = messages_to_prompt(&msgs);
        assert_eq!(sys, None);
        assert_eq!(prompt, "");
    }
}
