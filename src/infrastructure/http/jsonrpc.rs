use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{DomainError, ToolDescriptor, ToolOutput};

pub const PROTOCOL_VERSION: &str = "2025-03-26";

pub mod code {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
}

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    /// Absent for notifications.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl Request {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Map a domain error onto the JSON-RPC error code that best describes it.
pub fn error_code_for(err: &DomainError) -> i32 {
    match err {
        DomainError::UnknownTool(_) => code::METHOD_NOT_FOUND,
        DomainError::InvalidArgument(_) => code::INVALID_PARAMS,
        DomainError::UnknownSession => code::INVALID_REQUEST,
    }
}

pub fn tool_descriptor_to_json(d: &ToolDescriptor) -> Value {
    let a = &d.annotations;
    serde_json::json!({
        "name": d.name,
        "description": d.description,
        "inputSchema": d.input_schema,
        // Hint names are fixed by the MCP spec; a client matches on them exactly.
        "annotations": {
            "readOnlyHint": a.read_only,
            "destructiveHint": a.destructive,
            "idempotentHint": a.idempotent,
            "openWorldHint": a.open_world,
        },
    })
}

pub fn tool_output_to_json(o: &ToolOutput) -> Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": o.text }],
        "isError": o.is_error,
    })
}
