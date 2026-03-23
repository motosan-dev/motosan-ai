#[cfg(feature = "agent-tool")]
use motosan_agent_tool::ToolDef;

use crate::types::Tool;

#[cfg(feature = "agent-tool")]
impl From<ToolDef> for Tool {
    fn from(def: ToolDef) -> Self {
        Self {
            name: def.name,
            description: Some(def.description),
            input_schema: Some(def.input_schema),
        }
    }
}

#[cfg(feature = "agent-tool")]
impl From<Tool> for ToolDef {
    fn from(t: Tool) -> Self {
        Self {
            name: t.name,
            description: t.description.unwrap_or_default(),
            input_schema: t
                .input_schema
                .unwrap_or_else(|| serde_json::json!({"type":"object"})),
        }
    }
}

#[cfg(all(test, feature = "agent-tool"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_from_tooldef() {
        let def = ToolDef {
            name: "get_weather".into(),
            description: "Fetch current weather".into(),
            input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        };

        let tool: Tool = def.into();

        assert_eq!(tool.name, "get_weather");
        assert_eq!(tool.description.as_deref(), Some("Fetch current weather"));
        assert_eq!(
            tool.input_schema.unwrap(),
            json!({"type": "object", "properties": {"city": {"type": "string"}}})
        );
    }

    #[test]
    fn test_tooldef_from_tool_with_none_fields() {
        let tool = Tool {
            name: "search".into(),
            description: None,
            input_schema: None,
        };

        let def: ToolDef = tool.into();

        assert_eq!(def.name, "search");
        assert_eq!(def.description, "");
        assert_eq!(def.input_schema, json!({"type": "object"}));
    }

    #[test]
    fn test_tooldef_from_tool_with_some_fields() {
        let tool = Tool {
            name: "calc".into(),
            description: Some("Calculator".into()),
            input_schema: Some(
                json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
            ),
        };

        let def: ToolDef = tool.into();

        assert_eq!(def.name, "calc");
        assert_eq!(def.description, "Calculator");
        assert_eq!(
            def.input_schema,
            json!({"type": "object", "properties": {"expr": {"type": "string"}}})
        );
    }
}
