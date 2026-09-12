use serde_json::Value;

use super::ports::{SessionStore, ToolRegistry};
use crate::domain::{DomainError, SessionId, ToolDescriptor, ToolOutput};

/// Use-cases behind the MCP methods. Knows nothing about HTTP or JSON-RPC framing.
pub struct McpService {
    sessions: Box<dyn SessionStore>,
    tools: Box<dyn ToolRegistry>,
    server_name: String,
}

impl McpService {
    pub fn new(
        sessions: Box<dyn SessionStore>,
        tools: Box<dyn ToolRegistry>,
        server_name: String,
    ) -> Self {
        Self {
            sessions,
            tools,
            server_name,
        }
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub fn initialize(&self) -> SessionId {
        self.sessions.create()
    }

    pub fn end_session(&self, id: &SessionId) -> bool {
        self.sessions.remove(id)
    }

    pub fn validate_session(&self, id: &SessionId) -> Result<(), DomainError> {
        if self.sessions.exists(id) {
            Ok(())
        } else {
            Err(DomainError::UnknownSession)
        }
    }

    pub fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.tools.all().iter().map(|t| t.descriptor()).collect()
    }

    /// A missing tool is a protocol-level error; a tool that runs and fails reports
    /// through `ToolOutput::is_error` instead.
    pub fn call_tool(&self, name: &str, args: &Value) -> Result<ToolOutput, DomainError> {
        self.tools.find(name)?.invoke(args)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{Tool, ToolAnnotations, ToolDescriptor};

    /// Stub adapters: the use-cases are testable without actix or a real store.
    struct FakeStore;
    impl SessionStore for FakeStore {
        fn create(&self) -> SessionId {
            SessionId::new("fixed")
        }
        fn exists(&self, id: &SessionId) -> bool {
            id.as_str() == "fixed"
        }
        fn remove(&self, id: &SessionId) -> bool {
            id.as_str() == "fixed"
        }
    }

    struct Noop;
    impl Tool for Noop {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "noop",
                description: "d",
                input_schema: json!({}),
                annotations: ToolAnnotations::read_only(),
            }
        }
        fn invoke(&self, _: &Value) -> Result<ToolOutput, DomainError> {
            Ok(ToolOutput::ok("done"))
        }
    }

    struct OneTool(Vec<Box<dyn Tool>>);
    impl ToolRegistry for OneTool {
        fn all(&self) -> &[Box<dyn Tool>] {
            &self.0
        }
    }

    fn service() -> McpService {
        McpService::new(
            Box::new(FakeStore),
            Box::new(OneTool(vec![Box::new(Noop)])),
            "test-server".into(),
        )
    }

    #[test]
    fn validates_sessions() {
        let s = service();
        assert!(s.validate_session(&s.initialize()).is_ok());
        assert_eq!(
            s.validate_session(&SessionId::new("other")),
            Err(DomainError::UnknownSession)
        );
    }

    #[test]
    fn calls_known_tool_and_rejects_unknown() {
        let s = service();
        assert_eq!(s.call_tool("noop", &json!({})).unwrap().text, "done");
        assert_eq!(
            s.call_tool("ghost", &json!({})).unwrap_err(),
            DomainError::UnknownTool("ghost".into())
        );
    }

    #[test]
    fn lists_tools() {
        assert_eq!(service().list_tools().len(), 1);
    }
}
