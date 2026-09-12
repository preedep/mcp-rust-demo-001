use crate::domain::{DomainError, SessionId, Tool};

/// Outbound port for session persistence. The in-memory implementation lives in
/// infrastructure; swapping it for Redis would not touch this layer.
pub trait SessionStore: Send + Sync {
    fn create(&self) -> SessionId;
    fn exists(&self, id: &SessionId) -> bool;
    fn remove(&self, id: &SessionId) -> bool;
}

/// Outbound port for tool lookup.
pub trait ToolRegistry: Send + Sync {
    fn all(&self) -> &[Box<dyn Tool>];

    fn find(&self, name: &str) -> Result<&dyn Tool, DomainError> {
        self.all()
            .iter()
            .find(|t| t.descriptor().name == name)
            .map(AsRef::as_ref)
            .ok_or_else(|| DomainError::UnknownTool(name.to_owned()))
    }
}
