use crate::domain::{DomainError, SessionId, Tool};

/// Outbound port for session persistence. The in-memory implementation lives in
/// infrastructure; swapping it for Redis would not touch this layer.
pub trait SessionStore: Send + Sync {
    fn create(&self) -> SessionId;
    fn exists(&self, id: &SessionId) -> bool;
    fn remove(&self, id: &SessionId) -> bool;
    fn count(&self) -> usize;
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

/// Outbound port for telemetry. Keeps the Prometheus client out of this layer, so
/// the use-cases stay testable without a metrics registry.
pub trait Metrics: Send + Sync {
    /// One finished tool invocation. `client_id` is the caller the gateway
    /// authenticated, so usage can be attributed per agent.
    fn tool_call(&self, tool: &str, client_id: &str, is_error: bool, seconds: f64);

    /// One MCP method handled, e.g. `initialize` or `tools/list`.
    fn request(&self, method: &str);

    /// A JSON-RPC error returned to the caller. These leave as HTTP 200, so the
    /// gateway cannot see them — without this, a client sending malformed
    /// requests is invisible.
    fn rpc_error(&self, code: i32, method: &str);

    /// Current number of live sessions, plus the cumulative open/close counts.
    /// The gauge alone cannot reveal a slow leak; the difference between the
    /// counters can.
    fn sessions(&self, count: usize);
    fn session_opened(&self);
    fn session_closed(&self);
}

/// Discards everything. Used by tests that do not care about telemetry.
#[cfg(test)]
pub struct NoMetrics;

#[cfg(test)]
impl Metrics for NoMetrics {
    fn tool_call(&self, _: &str, _: &str, _: bool, _: f64) {}
    fn request(&self, _: &str) {}
    fn rpc_error(&self, _: i32, _: &str) {}
    fn sessions(&self, _: usize) {}
    fn session_opened(&self) {}
    fn session_closed(&self) {}
}
