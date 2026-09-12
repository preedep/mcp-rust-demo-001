use serde_json::{json, Value};

use crate::domain::{DomainError, Tool, ToolAnnotations, ToolDescriptor, ToolOutput};

pub struct Echo;

impl Tool for Echo {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "echo",
            description: "Echo a message back unchanged. Call this to check that the \
                          connection to the MCP server is working. Safe, instant and \
                          read-only — there is no reason not to call it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Text to echo back." }
                },
                "required": ["message"],
                "additionalProperties": false
            }),
            annotations: ToolAnnotations::read_only(),
        }
    }

    fn invoke(&self, args: &Value) -> Result<ToolOutput, DomainError> {
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| DomainError::InvalidArgument("'message' must be a string".into()))?;
        Ok(ToolOutput::ok(message))
    }
}
