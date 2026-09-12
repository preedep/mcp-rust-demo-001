use serde_json::Value;

use super::DomainError;

/// Behavioural hints a client can use to decide how freely to call a tool — some skip
/// confirmation entirely for read-only tools, and gate destructive ones behind approval.
///
/// These are **hints, not enforcement**: a client may ignore them. Real authorisation
/// belongs on the server (see `title` on the MCP spec's tool annotations).
#[derive(Debug, Clone, Copy)]
pub struct ToolAnnotations {
    /// Does not modify any state.
    pub read_only: bool,
    /// May delete or overwrite something. Meaningless when `read_only` is set.
    pub destructive: bool,
    /// Repeating the call with the same arguments has no additional effect.
    pub idempotent: bool,
    /// Interacts with systems beyond this server (network, other services).
    pub open_world: bool,
}

impl ToolAnnotations {
    /// A pure function of its arguments: safe to call freely and to repeat.
    pub const fn read_only() -> Self {
        Self {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        }
    }
}

/// What a tool advertises to the model. `input_schema` is JSON Schema, which is part of
/// the MCP contract rather than an implementation detail.
pub struct ToolDescriptor {
    pub name: &'static str,
    /// Written for the model that has to choose a tool, not for a developer reading
    /// docs: say when to prefer it, and whether calling it is cheap and safe.
    pub description: &'static str,
    pub input_schema: Value,
    pub annotations: ToolAnnotations,
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
