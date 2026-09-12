mod calculate;
mod echo;
mod server_time;

use crate::application::ports::ToolRegistry;
use crate::domain::Tool;

/// Fixed set of tools, built once at startup. `list` and `call` are both driven from this
/// vector, so they cannot drift apart.
pub struct StaticToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl Default for StaticToolRegistry {
    fn default() -> Self {
        Self {
            tools: vec![
                Box::new(echo::Echo),
                Box::new(server_time::ServerTime),
                Box::new(calculate::Calculate),
            ],
        }
    }
}

impl ToolRegistry for StaticToolRegistry {
    fn all(&self) -> &[Box<dyn Tool>] {
        &self.tools
    }
}
