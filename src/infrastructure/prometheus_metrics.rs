use std::sync::atomic::AtomicI64;
use std::sync::Mutex;

use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::application::Metrics;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolLabels {
    tool: String,
    client_id: String,
    is_error: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolOnly {
    tool: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct MethodLabels {
    method: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ErrorLabels {
    code: String,
    method: String,
}

/// The method name arrives from the caller, so recording it verbatim would let
/// anyone inflate the series count by inventing names. Only the methods this
/// server implements become labels; everything else collapses to `other`.
fn known_method(method: &str) -> String {
    match method {
        "initialize" | "notifications/initialized" | "tools/list" | "tools/call" | "ping" => {
            method.to_owned()
        }
        _ => "other".to_owned(),
    }
}

pub struct PrometheusMetrics {
    registry: Mutex<Registry>,
    tool_calls: Family<ToolLabels, Counter>,
    tool_duration: Family<ToolOnly, Histogram>,
    requests: Family<MethodLabels, Counter>,
    rpc_errors: Family<ErrorLabels, Counter>,
    sessions: Gauge<i64, AtomicI64>,
    sessions_created: Counter,
    sessions_closed: Counter,
}

impl Default for PrometheusMetrics {
    fn default() -> Self {
        let mut registry = <Registry>::default();

        let tool_calls = Family::<ToolLabels, Counter>::default();
        registry.register(
            "mcp_tool_calls",
            "Tool invocations, by tool, calling client and outcome",
            tool_calls.clone(),
        );

        // Tools here are sub-millisecond, but the buckets leave room for one that
        // does real work later.
        let tool_duration = Family::<ToolOnly, Histogram>::new_with_constructor(|| {
            Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
        });
        registry.register(
            "mcp_tool_duration_seconds",
            "Time spent inside a tool invocation",
            tool_duration.clone(),
        );

        let requests = Family::<MethodLabels, Counter>::default();
        registry.register(
            "mcp_requests",
            "MCP methods handled, by method name",
            requests.clone(),
        );

        let rpc_errors = Family::<ErrorLabels, Counter>::default();
        registry.register(
            "mcp_rpc_errors",
            "JSON-RPC errors returned, by code and method. These leave as HTTP 200, \
             so they are invisible to the gateway",
            rpc_errors.clone(),
        );

        let sessions_created = Counter::default();
        registry.register(
            "mcp_sessions_created",
            "Sessions opened since start",
            sessions_created.clone(),
        );

        let sessions_closed = Counter::default();
        registry.register(
            "mcp_sessions_closed",
            "Sessions closed since start; a growing gap against created means \
             clients are not sending DELETE",
            sessions_closed.clone(),
        );

        let sessions = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "mcp_sessions_active",
            "Sessions currently held in memory",
            sessions.clone(),
        );

        Self {
            registry: Mutex::new(registry),
            tool_calls,
            tool_duration,
            requests,
            rpc_errors,
            sessions,
            sessions_created,
            sessions_closed,
        }
    }
}

impl PrometheusMetrics {
    /// Render the Prometheus text exposition format.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        match self.registry.lock() {
            Ok(r) => {
                let _ = encode(&mut out, &r);
                out
            }
            // A poisoned registry must not take the whole endpoint down.
            Err(_) => String::new(),
        }
    }
}

impl Metrics for PrometheusMetrics {
    fn tool_call(&self, tool: &str, client_id: &str, is_error: bool, seconds: f64) {
        self.tool_calls
            .get_or_create(&ToolLabels {
                tool: tool.to_owned(),
                client_id: client_id.to_owned(),
                is_error: is_error.to_string(),
            })
            .inc();
        self.tool_duration
            .get_or_create(&ToolOnly {
                tool: tool.to_owned(),
            })
            .observe(seconds);
    }

    fn request(&self, method: &str) {
        self.requests
            .get_or_create(&MethodLabels {
                method: known_method(method),
            })
            .inc();
    }

    fn rpc_error(&self, code: i32, method: &str) {
        self.rpc_errors
            .get_or_create(&ErrorLabels {
                code: code.to_string(),
                method: known_method(method),
            })
            .inc();
    }

    fn sessions(&self, count: usize) {
        self.sessions.set(count as i64);
    }

    fn session_opened(&self) {
        self.sessions_created.inc();
    }

    fn session_closed(&self) {
        self.sessions_closed.inc();
    }
}
