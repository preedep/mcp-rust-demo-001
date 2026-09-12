use serde_json::Value;

use super::DomainError;

/// What a tool advertises to the model. `input_schema` is JSON Schema, which is part of
/// the MCP contract rather than an implementation detail.
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A successful tool run. A tool that fails *semantically* still returns Ok(ToolOutput)
/// with `is_error` set, because the model must be able to read and react to the message.
#[derive(Debug)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn failed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// A capability the server exposes. Implemented in the infrastructure layer; the
/// application layer depends only on this trait.
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;

    fn invoke(&self, args: &Value) -> Result<ToolOutput, DomainError>;
}
