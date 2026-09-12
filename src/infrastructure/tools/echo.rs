use serde_json::{json, Value};

use crate::domain::{DomainError, Tool, ToolDescriptor, ToolOutput};

pub struct Echo;

impl Tool for Echo {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "echo",
            description: "Return the supplied message unchanged. Use this to verify that \
                          the connection to the MCP server is working.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Text to echo back." }
                },
                "required": ["message"],
                "additionalProperties": false
            }),
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
