use actix_web::{delete, get, http::header, post, web, HttpRequest, HttpResponse, Responder};
use serde_json::{json, Value};

use super::jsonrpc::{
    code, error_code_for, tool_descriptor_to_json, tool_output_to_json, Request, Response,
    PROTOCOL_VERSION,
};
use crate::application::McpService;
use crate::domain::SessionId;
use crate::infrastructure::PrometheusMetrics;

const SESSION_HEADER: &str = "Mcp-Session-Id";
/// Set by the gateway from the verified token; absent when running without one.
const CLIENT_ID_HEADER: &str = "X-Client-Id";

type Service = web::Data<McpService>;

#[get("/healthz")]
async fn healthz() -> impl Responder {
    HttpResponse::Ok().content_type("text/plain").body("ok")
}

/// Prometheus scrape target. Deliberately NOT published through the gateway: it is
/// reachable on the ClusterIP service only, so it needs no credential and never
/// leaves the cluster.
#[get("/metrics")]
async fn metrics(exporter: web::Data<std::sync::Arc<PrometheusMetrics>>) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4; charset=utf-8")
        .body(exporter.encode())
}

#[post("/mcp")]
async fn mcp_post(req: HttpRequest, body: web::Bytes, svc: Service) -> HttpResponse {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            // The body never parsed, so there is no method name to attribute it to.
            return HttpResponse::Ok().json(rpc_err(
                &svc,
                Value::Null,
                code::PARSE_ERROR,
                "unparsed",
                e.to_string(),
            ));
        }
    };

    // A batch is a JSON array; handle it before reading a single envelope.
    if let Some(items) = parsed.as_array() {
        if items.is_empty() {
            return HttpResponse::Ok().json(rpc_err(
                &svc,
                Value::Null,
                code::INVALID_REQUEST,
                "batch",
                "empty batch",
            ));
        }
        let replies: Vec<Response> = items
            .iter()
            .filter_map(|item| dispatch(&req, item, &svc).map(|d| d.body))
            .collect();
        return if replies.is_empty() {
            HttpResponse::Accepted().finish()
        } else {
            HttpResponse::Ok().json(replies)
        };
    }

    match dispatch(&req, &parsed, &svc) {
        // Notifications get 202 with no body, per the transport spec.
        None => HttpResponse::Accepted().finish(),
        Some(d) => {
            let mut builder = HttpResponse::Ok();
            if let Some(id) = d.new_session.as_deref() {
                builder.insert_header((SESSION_HEADER, id));
            }
            builder.json(d.body)
        }
    }
}

/// Opens the server-to-client SSE stream. This server never initiates messages, so the
/// stream is accepted and held open with periodic comments.
#[get("/mcp")]
async fn mcp_get(req: HttpRequest, svc: Service) -> HttpResponse {
    match session_header(&req) {
        None => HttpResponse::BadRequest().body("missing Mcp-Session-Id"),
        Some(id) if svc.validate_session(&id).is_err() => {
            HttpResponse::NotFound().body("unknown session")
        }
        Some(_) => HttpResponse::Ok()
            .content_type("text/event-stream")
            .insert_header((header::CACHE_CONTROL, "no-cache"))
            .streaming(keepalive_stream()),
    }
}

#[delete("/mcp")]
async fn mcp_delete(req: HttpRequest, svc: Service) -> HttpResponse {
    match session_header(&req) {
        Some(id) if svc.end_session(&id) => HttpResponse::NoContent().finish(),
        _ => HttpResponse::NotFound().finish(),
    }
}

struct Dispatched {
    body: Response,
    new_session: Option<String>,
}

fn dispatch(http: &HttpRequest, raw: &Value, svc: &McpService) -> Option<Dispatched> {
    let req: Request = match serde_json::from_value(raw.clone()) {
        Ok(r) => r,
        Err(e) => {
            let id = raw.get("id").cloned().unwrap_or(Value::Null);
            let method = raw
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("unparsed");
            return Some(plain(rpc_err(
                svc,
                id,
                code::INVALID_REQUEST,
                method,
                e.to_string(),
            )));
        }
    };

    if req.jsonrpc != "2.0" {
        let id = req.id.clone().unwrap_or(Value::Null);
        return Some(plain(rpc_err(
            svc,
            id,
            code::INVALID_REQUEST,
            &req.method,
            "jsonrpc must be \"2.0\"",
        )));
    }

    if req.is_notification() {
        return None;
    }

    let id = req.id.clone().unwrap_or(Value::Null);
    let params = req.params.clone().unwrap_or(Value::Null);
    svc.record_request(&req.method);

    match req.method.as_str() {
        "initialize" => {
            let session = svc.initialize();
            let result = json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": svc.server_name(),
                    "version": env!("CARGO_PKG_VERSION"),
                },
            });
            Some(Dispatched {
                body: Response::result(id, result),
                new_session: Some(session.to_string()),
            })
        }
        "ping" => Some(plain(Response::result(id, json!({})))),
        "tools/list" => Some(plain(match guard_session(http, svc) {
            Err(e) => rpc_err(svc, id, error_code_for(&e), "tools/list", e.to_string()),
            Ok(()) => {
                let tools: Vec<Value> = svc
                    .list_tools()
                    .iter()
                    .map(tool_descriptor_to_json)
                    .collect();
                Response::result(id, json!({ "tools": tools }))
            }
        })),
        "tools/call" => Some(plain(call_tool(http, svc, id, &params))),
        other => Some(plain(rpc_err(
            svc,
            id,
            code::METHOD_NOT_FOUND,
            other,
            format!("unknown method '{other}'"),
        ))),
    }
}

fn client_id(req: &HttpRequest) -> String {
    req.headers()
        .get(CLIENT_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_owned()
}

fn call_tool(http: &HttpRequest, svc: &McpService, id: Value, params: &Value) -> Response {
    if let Err(e) = guard_session(http, svc) {
        return rpc_err(svc, id, error_code_for(&e), "tools/call", e.to_string());
    }
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_err(
            svc,
            id,
            code::INVALID_PARAMS,
            "tools/call",
            "missing 'name'",
        );
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match svc.call_tool(name, &client_id(http), &args) {
        Ok(out) => Response::result(id, tool_output_to_json(&out)),
        Err(e) => rpc_err(svc, id, error_code_for(&e), "tools/call", e.to_string()),
    }
}

fn session_header(req: &HttpRequest) -> Option<SessionId> {
    req.headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(SessionId::new)
}

/// Clients that omit the header are tolerated; an id the server never issued is rejected,
/// which usually means the server restarted and the client must re-initialize.
fn guard_session(req: &HttpRequest, svc: &McpService) -> Result<(), crate::domain::DomainError> {
    match session_header(req) {
        Some(id) => svc.validate_session(&id),
        None => Ok(()),
    }
}

/// Build an error response and count it. Every JSON-RPC error goes through here,
/// so the counter cannot drift from what is actually returned.
fn rpc_err(
    svc: &McpService,
    id: Value,
    code: i32,
    method: &str,
    message: impl Into<String>,
) -> Response {
    svc.record_rpc_error(code, method);
    Response::error(id, code, message)
}

fn plain(body: Response) -> Dispatched {
    Dispatched {
        body,
        new_session: None,
    }
}

fn keepalive_stream() -> impl futures_core::Stream<Item = Result<web::Bytes, actix_web::Error>> {
    use std::time::Duration;
    // Periodic comments stop intermediaries closing an idle stream.
    let ticks = tokio::time::interval(Duration::from_secs(15));
    futures_util::stream::unfold(ticks, |mut t| async move {
        t.tick().await;
        Some((Ok(web::Bytes::from_static(b": keepalive\n\n")), t))
    })
}

pub fn configure(c: &mut web::ServiceConfig) {
    c.service(healthz)
        .service(metrics)
        .service(mcp_post)
        .service(mcp_get)
        .service(mcp_delete);
}
