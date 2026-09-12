# mcp-rust-demo-001

A demo **MCP (Model Context Protocol) server** in Rust, served over HTTP so a remote agent
can call it as a tool provider. Built with actix-web, arranged in clean-architecture layers,
and shipped as a 2.4 MB container image.

Transport is **Streamable HTTP** (MCP spec `2025-03-26`): JSON-RPC 2.0 over a single
endpoint.

## Quick start

```bash
cargo run
```

Then, in another shell:

```bash
curl -s localhost:8080/healthz

curl -s localhost:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Connect a local MCP client by pointing it at `http://localhost:8080/mcp` with transport
type `http`.

To exercise every method against a running server — local or deployed — use:

```bash
scripts/smoke-remote.sh http://localhost:8080/mcp
```

It runs `initialize`, `tools/list`, each tool, `ping`, a deliberate tool failure, and
`DELETE`, printing each response.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/mcp` | JSON-RPC requests and notifications |
| `GET` | `/mcp` | SSE stream for server-initiated messages |
| `DELETE` | `/mcp` | End the session |
| `GET` | `/healthz` | Liveness probe (not part of MCP) |

Supported methods: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`,
`ping`.

`initialize` returns an `Mcp-Session-Id` header; echo it on later requests. A session id the
server did not issue is rejected, which normally means the server restarted — call
`initialize` again.

## Tools

| Tool | Input | Returns |
|---|---|---|
| `echo` | `message: string` | The same message — connectivity check |
| `get_server_time` | `timezone?: string` (IANA) | Current time, RFC 3339 |
| `calculate` | `expression: string` | Result of `+ - * / %`, parentheses, unary minus |

A tool that fails *semantically* (bad timezone, division by zero) returns a normal result
with `isError: true`, so the calling model can read the reason and react. JSON-RPC error
codes are reserved for protocol faults.

## Configuration

Environment variables only, so the same image runs locally and in a cluster.

| Variable | Default | Purpose |
|---|---|---|
| `BIND_ADDR` | `0.0.0.0:8080` | Listen address |
| `MCP_SERVER_NAME` | `mcp-rust-demo-001` | Name reported in `initialize` |
| `RUST_LOG` | `info` | `tracing` filter, e.g. `debug`, `warn` |

Logs are human-readable in debug builds and JSON in release builds.

## Container

```bash
scripts/build-image.sh          # build for linux/amd64, then smoke test it
scripts/build-image.sh --help
```

The script builds the image, starts it, and verifies `/healthz`, an `initialize` handshake,
`tools/list`, and a real `tools/call` before reporting success. It exits non-zero if any
step fails.

The image is an Alpine/musl build with a `scratch` runtime — the binary is statically
linked, so the image carries nothing else (**2.4 MB**). Note that this means **no shell**:
`docker exec`/`kubectl exec` will not work, so debug from logs or use `kubectl debug` with
an ephemeral container. Switch the final stage to `alpine:3.21` (~5.6 MB) if you want a
shell available.

## Deploying to Kubernetes

Manifests are in `k8s/`. The image is imported directly into the node's containerd rather
than pulled from a registry, so `imagePullPolicy` is `IfNotPresent` and the tag must already
exist on the node.

```bash
scripts/deploy.sh --dry-run   # validate manifests, change nothing
scripts/deploy.sh             # build, import, apply, verify
```

The script builds and smoke tests the image, copies it to the node over ssh, imports it into
containerd (this needs sudo on the node, so it prompts), applies the manifests, forces a
rollout, and verifies the ingress route resolves and answers an MCP handshake.

The image is imported in two ssh steps rather than one piped command: ssh will not allocate a
TTY when stdin is a pipe, and sudo needs a TTY to prompt. The script stages the tarball, then
imports it.

**Building for a different architecture than your machine?** The build verifies the compiled
binary's real ELF architecture, not just the image label — a builder stage pinned to
`$BUILDPLATFORM` produces a host-arch binary inside an image *labelled* for the target, which
only fails once it reaches the real hardware (`exec format error`).

## Architecture

Clean architecture; dependencies point inward only, so the domain has no knowledge of
actix, HTTP, or JSON-RPC.

```
src/
  main.rs              composition root — the only place that wires the layers
  domain/              Tool trait, ToolOutput, SessionId, DomainError. No framework, no I/O
  application/         use-cases (McpService) + outbound ports (SessionStore, ToolRegistry)
  infrastructure/      actix handlers, JSON-RPC framing, tool impls, in-memory store
k8s/                   namespace, deployment, service, httproute, referencegrant
scripts/               build-image.sh, deploy.sh, smoke-remote.sh
```

Consequences worth knowing before you extend it:

- `domain` and `application` must never import `actix_web`. Such an import means logic has
  leaked outward.
- Adding a tool: implement `domain::Tool` under `infrastructure/tools/`, then register it in
  `StaticToolRegistry`. The registry drives both `tools/list` and `tools/call`, so they
  cannot drift apart.
- Replacing the in-memory session store (with Redis, say) means one new adapter plus one
  line in `main.rs` — no change to `application` or `domain`.

## Development

```bash
cargo test                              # unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Sessions are held in memory, so a restart invalidates them. The server is otherwise
stateless.

## Status

The server is implemented, verified locally, and deployed to a k3s cluster behind an Envoy
Gateway. It has **no TLS and no authentication** — add both before exposing it
outside a trusted network.
