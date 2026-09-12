mod application;
mod domain;
mod infrastructure;

use std::env;

use actix_web::{middleware, web, App, HttpServer};
use tracing_subscriber::EnvFilter;

use application::McpService;
use infrastructure::{http, MemorySessionStore, PrometheusMetrics, StaticToolRegistry};

/// Composition root: the only place that knows every layer. Wiring happens here so the
/// inner layers stay free of framework and construction concerns.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_tracing();

    let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let server_name =
        env::var("MCP_SERVER_NAME").unwrap_or_else(|_| "mcp-rust-demo-001".to_string());

    tracing::info!(%bind, %server_name, "starting MCP server");

    // The exporter is shared: the service records through the port, the /metrics
    // route renders from the same registry.
    let metrics = std::sync::Arc::new(PrometheusMetrics::default());
    let service = web::Data::new(McpService::new(
        Box::new(MemorySessionStore::default()),
        Box::new(StaticToolRegistry::default()),
        Box::new(SharedMetrics(metrics.clone())),
        server_name,
    ));
    let exporter = web::Data::new(metrics);

    HttpServer::new(move || {
        App::new()
            .app_data(service.clone())
            .app_data(exporter.clone())
            .wrap(middleware::Logger::default())
            .wrap(middleware::NormalizePath::trim())
            .configure(http::configure)
    })
    .bind(&bind)?
    .run()
    .await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if cfg!(debug_assertions) {
        builder.init();
    } else {
        builder.json().init();
    }
}

/// Lets the service and the /metrics route share one registry: McpService owns a
/// Box<dyn Metrics>, while the route needs the concrete exporter to encode it.
struct SharedMetrics(std::sync::Arc<PrometheusMetrics>);

impl application::Metrics for SharedMetrics {
    fn tool_call(&self, tool: &str, client_id: &str, is_error: bool, seconds: f64) {
        self.0.tool_call(tool, client_id, is_error, seconds);
    }
    fn request(&self, method: &str) {
        self.0.request(method);
    }
    fn rpc_error(&self, code: i32, method: &str) {
        self.0.rpc_error(code, method);
    }
    fn sessions(&self, count: usize) {
        self.0.sessions(count);
    }
    fn session_opened(&self) {
        self.0.session_opened();
    }
    fn session_closed(&self) {
        self.0.session_closed();
    }
}
