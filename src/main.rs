mod application;
mod domain;
mod infrastructure;

use std::env;

use actix_web::{middleware, web, App, HttpServer};
use tracing_subscriber::EnvFilter;

use application::McpService;
use infrastructure::{http, MemorySessionStore, StaticToolRegistry};

/// Composition root: the only place that knows every layer. Wiring happens here so the
/// inner layers stay free of framework and construction concerns.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_tracing();

    let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let server_name =
        env::var("MCP_SERVER_NAME").unwrap_or_else(|_| "mcp-rust-demo-001".to_string());

    tracing::info!(%bind, %server_name, "starting MCP server");

    let service = web::Data::new(McpService::new(
        Box::new(MemorySessionStore::default()),
        Box::new(StaticToolRegistry::default()),
        server_name,
    ));

    HttpServer::new(move || {
        App::new()
            .app_data(service.clone())
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
