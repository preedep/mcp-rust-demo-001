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

It prints the full request — method, URL, headers, pretty-printed body — next to each
response and HTTP status, so any step can be lifted out and re-run by hand. Covers
`initialize`, `tools/list`, all three tools, `ping`, a deliberate tool failure, an unknown
method, and `DELETE`.

```
scripts/smoke-remote.sh --quiet   # responses only
scripts/smoke-remote.sh --raw     # no pretty-printing
scripts/smoke-remote.sh --help
```

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

## Authentication

The gateway requires an **Entra (Azure AD) access token** on every request; the server
itself is unauthenticated, so the check happens entirely at the edge. Envoy validates the
token against the tenant's JWKS and forwards the caller's identity as plain headers
(`X-Client-Id`, `X-Caller-Oid`), so the server never parses a token.

```bash
scripts/get-token.sh --check    # does the token carry the app role?
scripts/get-token.sh --claims   # full decoded claims
MCP_AUTH_MODE=jwt scripts/smoke-remote.sh
```

Credentials live in `.env`, which is gitignored — copy `.env.example` to start.

### Setting up the app registrations

Two registrations: one identifies the API, one identifies each caller.

**1. The API — `mcp-rust-demo-api`**

| Step | Where | What |
|---|---|---|
| Create | App registrations → New | Single tenant is fine |
| Expose | Expose an API | Set the Application ID URI (defaults to `api://<client-id>`) |
| **App role** | **App roles** → Create | Value `mcp.invoke`, **Allowed member types: Applications** |

**Use an app role, not a scope.** The "Expose an API → Add a scope" flow creates a
*delegated* permission, which appears in a token as `scp` and only for user sign-ins. A
client-credentials token carries `roles` and never `scp`, so a scope-only setup returns a
token with no `roles` claim and the gateway rejects every call. The portal steers you
toward the scope, and the failure is silent.

**2. Each caller — e.g. `mcp-rust-demo-client`**

| Step | Where | What |
|---|---|---|
| Create | App registrations → New | One per caller, so they can be told apart and revoked separately |
| Secret | Certificates & secrets | Note the value — it is shown once |
| Permission | API permissions → Add → My APIs → the API | **Application permissions** → `mcp.invoke` |
| **Consent** | API permissions | **Grant admin consent** |

**Admin consent is required and easy to miss.** Adding an application permission only
declares intent; until an admin consents, the Status column reads "Not granted" and the
role is silently absent from every token.

**3. Fill in `.env`**

```bash
TENANT_ID=<Directory (tenant) ID from the API registration>
CLIENT_ID=<Application (client) ID of the *client* registration>
CLIENT_SECRET=<the secret value>
API_AUDIENCE=api://<Application (client) ID of the *API* registration>
```

**4. Verify before touching the cluster**

```bash
scripts/get-token.sh --check
```

Expect `roles: ['mcp.invoke']`. If it prints `(none)`, work back through consent → permission
type → member types. Entra caches tokens for a few minutes after a consent change, so wait
and retry before assuming it failed.

Note the `iss` that `--check` reports. A registration without `accessTokenAcceptedVersion: 2`
in its manifest mints **v1** tokens (`https://sts.windows.net/<tenant>/`), not the v2.0
`login.microsoftonline.com` issuer the docs suggest — and `k8s/securitypolicy.yaml` must
match exactly, or every call fails as a bare 401 with nothing to explain it.

### Connecting a Microsoft Foundry agent

A Foundry agent authenticates with its own **Entra Agent Identity** — a per-agent service
principal — so no secret is shared with it and each agent can be authorised separately.

**1. Configure the connection** (Foundry → the MCP connection → Edit):

| Field | Value |
|---|---|
| Remote MCP Server endpoint | the public HTTPS URL, e.g. `https://<machine>.<tailnet>.ts.net/mcp-rust-demo` |
| Authentication | **Microsoft Entra** |
| Type | **Agent Identity** |
| Audience | the API's Application ID URI, e.g. `api://<api-client-id>` |

The audience must match `spec.jwt.providers[].audiences` in `k8s/securitypolicy.yaml`
exactly. Choose *Microsoft Entra*, not *OAuth Identity Passthrough*: passthrough represents
the signed-in **user** and yields a `scp` claim, while the policy requires the `roles` claim
that only an application token carries — and in a programmatic flow there is no interactive
user to pass through anyway.

**2. Find the agent's identity.** Agent → **Details** → *Identity & access* → **Entra agent
identity**, and copy the full ID. Use the *agent identity*, not the *agent blueprint*: the
blueprint is the template it was created from and never authenticates.

**3. Grant it the app role.** The portal cannot assign app roles to an agent identity, so use
Graph. Each agent needs its own assignment:

```bash
az login --tenant <tenant-id>

AGENT_ID=<entra agent identity id>
API_APP_ID=<api registration client id>

AGENT_OID=$(az ad sp show --id "$AGENT_ID" --query id -o tsv 2>/dev/null \
  || az ad sp list --filter "appId eq '$AGENT_ID'" --query '[0].id' -o tsv)
API_SP=$(az ad sp list --filter "appId eq '$API_APP_ID'" --query '[0].id' -o tsv)
ROLE_ID=$(az ad app show --id "$API_APP_ID" \
  --query "appRoles[?value=='mcp.invoke'].id | [0]" -o tsv)

az rest --method POST \
  --uri "https://graph.microsoft.com/v1.0/servicePrincipals/$AGENT_OID/appRoleAssignments" \
  --body "{\"principalId\":\"$AGENT_OID\",\"resourceId\":\"$API_SP\",\"appRoleId\":\"$ROLE_ID\"}"
```

A `201` with `principalDisplayName` naming your agent means it worked. Entra caches tokens
for a few minutes, so allow a short delay before retrying.

**4. Verify** by asking the agent what tools it has. A healthy session looks like this in the
gateway's access log — handshake, SSE stream, notification, listing, teardown:

```
POST   200  via_upstream                  initialize
GET    200  downstream_remote_disconnect  SSE stream
POST   202  via_upstream                  notifications/initialized
POST   200  via_upstream                  tools/list
DELETE 204  via_upstream                  session closed
```

#### Reading a failure

The status code says which half of the check failed, which narrows it immediately:

| Result | Meaning |
|---|---|
| **401**, `Jwt is missing` | No token sent — the connection is not set to Microsoft Entra |
| **401**, `Jwt verification fails` | Issuer or JWKS mismatch (see the v1/v2 note above) |
| **401**, audience not allowed | The Audience field does not match the policy |
| **403**, `rbac_access_denied_matched_policy[DENY]` | **Token is valid**; the principal lacks `mcp.invoke` — do step 3 |

A 403 is good news: it means signature, issuer and audience all passed, and only the role
assignment is missing. Read the log with:

```bash
kubectl -n envoy-gateway-system logs deploy/envoy-<gateway> | grep AzureAIFoundryAgentRuntime
```

#### Per-agent authorisation

Because the identity is per agent, a second agent gets a 403 until it is granted the role
too. That is the access-control model working as intended rather than an obstacle: define
narrower roles (`mcp.read` alongside `mcp.invoke`, say) and assign each agent only what it
needs, and the gateway enforces the split without the server changing.

### Switching back to a static key

`k8s/securitypolicy-apikey.yaml` is the previous key-based policy, kept as a rollback. Both
files use the same resource name, so applying either replaces the other:

```bash
kubectl apply -f k8s/securitypolicy-apikey.yaml   # back to the static key
scripts/apply-auth.sh --show                      # print the key
```

**The switch is all-or-nothing.** A `SecurityPolicy` accepts `apiKeyAuth` and `jwt` as
sibling fields and reports `Accepted=True` with both set, but at runtime the combination
rejects everything — including credentials that worked a moment earlier. Verified on
2026-09-12: key-only works, JWT-only works, both together returns 401 for each. Confirm
every caller can present a token before cutting over.

## Deploying to Kubernetes

Manifests are in `k8s/`. The image is imported directly into the node's containerd rather
than pulled from a registry, so `imagePullPolicy` is `IfNotPresent` and the tag must already
exist on the node.

```bash
scripts/deploy.sh --dry-run   # validate manifests, change nothing
scripts/deploy.sh             # build, import, apply, verify
scripts/apply-auth.sh         # API-key Secret + SecurityPolicy
```

A first-time deploy needs both. `deploy.sh` handles the image and the routing manifests
(namespace, deployment, service, httproute, referencegrant); `apply-auth.sh` handles the
SecurityPolicy, which is separate because its Secret is built from the key in `.env` rather
than applied from a file. `deploy.sh` reports whether the auth policy is present, so a
deployment left unauthenticated is visible rather than silent. Both are idempotent.

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

## Using it from an agent

Point any MCP client at your deployment's endpoint with transport type `http`. The scripts
read it from `MCP_URL` in `.env`; this repo deliberately does not record a live endpoint. For
a Microsoft Foundry agent, see **Connecting a Microsoft Foundry agent** above — it
authenticates with its own Entra Agent Identity rather than a shared credential. For Microsoft Foundry,
create a connection with `Key-based` authentication, header `Authorization`, and the value
from `scripts/apply-auth.sh --show`.

Once connected, these prompts exercise each tool. The agent decides to call a tool from its
description, so phrasing that implies live server state works best.

**Enumerate what is available**

```
What tools do you have available?
```

**Call each tool**

```
Use the echo tool to send back exactly: hello from Foundry
```

```
What time is it right now on the MCP server, in Asia/Bangkok?
```

The model cannot know this — any answer has to come from the server, which makes it a good
proof that the tool really ran.

```
Use the calculate tool to work out (2+3)*4.5, then 100/7, and tell me both results.
```

**Chain all three in one turn**

```
Do these three things using the MCP tools, and show each result:
1. echo the message "MCP demo"
2. get the server time in Asia/Bangkok
3. calculate (2+3)*4.5
```

**Show error handling**

A tool that fails semantically returns `isError: true` with the reason, rather than a
protocol error, so the model can read it and respond:

```
Use the calculate tool to compute 1/0. What does the server say?
```

```
Get the server time in the timezone "Mars/Olympus".
```

Expect `cannot evaluate '1/0': division by zero` and `unknown time zone 'Mars/Olympus';
expected an IANA name such as 'UTC'`.

**Agent instructions**

A default system prompt tends to make the agent *describe* the tools instead of calling
them. Something like this works better:

```
You are an assistant with access to MCP tools on a remote server.

When a user asks for the current time, always call get_server_time — never answer
from your own knowledge, as only the server knows its real clock. For arithmetic,
use the calculate tool rather than computing it yourself.

After each tool call, state which tool you used and show the raw result. If a tool
returns an error, report the server's message verbatim.
```

## Per-tool access control (design sketch)

**Not implemented.** This records where per-tool authorisation would go, and — as
importantly — what the protocol does and does not make possible.

### What each layer can decide

```mermaid
flowchart TD
    A[Agent] -->|"POST /mcp-rust-demo<br/>Authorization: Bearer …"| B[Envoy Gateway]

    B --> C{"Valid token?<br/>+ mcp.invoke role?"}
    C -->|no| D["401 / 403"]
    C -->|yes| E["Forward + X-Client-Id"]

    E --> F[MCP server]
    F --> G{Which method?}
    G -->|tools/list| H[Return only tools<br/>this client may call]
    G -->|tools/call| I{Client allowed<br/>this tool?}

    I -->|no| J["JSON-RPC error -32601<br/>(indistinguishable from<br/>'no such tool')"]
    I -->|yes| K[Invoke tool]
    K --> L[Result]

    style B fill:#e8eaf6,color:#000
    style F fill:#e0f2f1,color:#000
    style D fill:#ffebee,color:#000
    style J fill:#ffebee,color:#000
```

The gateway sees only `POST /mcp-rust-demo`. Every `tools/call` is that same URL with a
different JSON body, so **the gateway cannot make per-tool decisions** — it answers "may
this caller in?", never "may this caller run `delete_dag`?". Per-tool authorisation has to
live in the server, the only component that knows which tool was named.

Envoy's `apiKeyAuth` has a `forwardClientIDHeader` field, so the client id matched against
the Secret can be passed downstream. That is what gives the server a caller identity to
authorise against, without it having to verify credentials itself.

### Where it fits in the layers

```mermaid
flowchart LR
    subgraph infra["infrastructure"]
        H[mcp_handler]
        P[(policy source<br/>ConfigMap / env)]
        PA[PolicyAdapter]
    end

    subgraph app["application"]
        S[McpService]
        PP[/"Policy port"/]
    end

    subgraph dom["domain"]
        T[Tool trait]
        D[DomainError]
    end

    H -->|client id + tool name| S
    S --> PP
    PA -.implements.-> PP
    PA --> P
    S --> T

    style dom fill:#e0f2f1,color:#000
    style app fill:#fff8e1,color:#000
    style infra fill:#e8eaf6,color:#000
```

`Policy` becomes a third outbound port beside `SessionStore` and `ToolRegistry`. The rule
source stays swappable — a static table today, a ConfigMap or an external service later —
without `application` or `domain` changing. Enforcement sits in `McpService::call_tool`, the
single chokepoint every tool invocation already passes through, and `list_tools` filters by
the same policy so a caller is never shown a tool it cannot use.

### Deny vs. require-approval

These are different things, and only one is a server concern:

| | Deny | Require approval |
|---|---|---|
| Effect | Tool never runs | Tool pauses for a human decision |
| Decided by | **The server** | **The client** |
| Mechanism | Filter from `tools/list`; error on `tools/call` | Client's own approval UI |

**MCP has no protocol-level approval flow.** A server cannot tell a client "run this only
after a human agrees". Approval is configured in the client — Claude Code and Claude Desktop
prompt per call, and Foundry has a require-approval setting on MCP tools. Do not try to build
it server-side; there is nowhere to put it in the protocol.

What a server *can* do is declare risk, via MCP tool annotations:

| Annotation | Means |
|---|---|
| `readOnlyHint` | Does not modify anything |
| `destructiveHint` | May delete or overwrite |
| `idempotentHint` | Safe to repeat |

These are **hints, not enforcement** — a client is free to ignore them. `ToolDescriptor` has
no annotations field today; adding one would be the smallest useful step, since it lets any
client gate on risk without this server changing again.

### Honest caveat

Per-tool RBAC only means something when the key identifies a *real* caller. There is one key
and one client here, so a policy table would have exactly one subject — the pattern without
the benefit. This becomes worth building when there are several callers at different trust
levels, or a tool that can destroy something. The three demo tools are pure functions over
their arguments and touch nothing.

## Architecture

### Request path, edge to tool

```mermaid
flowchart LR
    A["Agent / client<br/>(Entra identity)"]
    E["Microsoft Entra<br/>issues token"]

    subgraph edge["public edge"]
        F["Tailscale Funnel<br/>TLS :443<br/>scoped to one path"]
    end

    subgraph k3s["k3s on g1pro"]
        G["Envoy Gateway<br/>HTTPRoute + SecurityPolicy"]
        S["Service<br/>ClusterIP :8080"]
        P["Pod<br/>MCP server"]
    end

    A -.->|"1. client credentials"| E
    E -.->|"token, roles: mcp.invoke"| A
    A -->|"2. HTTPS /mcp-rust-demo<br/>Authorization: Bearer …"| F
    F -->|"http :30800"| G

    G -.->|"fetch JWKS"| E
    G -->|"401 — bad signature,<br/>issuer or audience"| X1[rejected]
    G -->|"403 — valid token,<br/>no mcp.invoke role"| X2[rejected]
    G -->|"URLRewrite /mcp-rust-demo → /mcp<br/>+ X-Client-Id, X-Caller-Oid"| S

    S --> P

    style edge fill:#e3f2fd,color:#000
    style k3s fill:#e8f5e9,color:#000
    style E fill:#fff8e1,color:#000
    style X1 fill:#ffebee,color:#000
    style X2 fill:#ffebee,color:#000
```

Authentication stops at the gateway: Envoy validates the token against Entra's JWKS and the
server never sees a credential. It forwards the caller's identity as plain headers
(`X-Client-Id`, `X-Caller-Oid`), so per-caller logic needs no token parsing in the app.

The two rejection paths are worth telling apart when debugging — **401** means the token
itself failed (signature, issuer, audience), **403** means it was valid but the principal
lacks the `mcp.invoke` app role. The URL rewrite means the app always serves `/mcp` and
never learns its public path.

### Modules, and which way dependencies point

```mermaid
flowchart TD
    M[main.rs<br/>composition root]

    subgraph infra["infrastructure — adapters"]
        H[http::mcp_handler<br/>actix routes]
        J[http::jsonrpc<br/>envelopes, error codes]
        R[tools::StaticToolRegistry]
        T1[echo]
        T2[server_time]
        T3[calculate]
        ST[MemorySessionStore]
    end

    subgraph app["application — use-cases"]
        SVC[McpService]
        PORTS[/"ports:<br/>SessionStore, ToolRegistry"/]
    end

    subgraph dom["domain — no framework, no I/O"]
        TR[Tool trait<br/>ToolDescriptor, ToolAnnotations]
        OUT[ToolOutput]
        SID[SessionId]
        ERR[DomainError]
    end

    M -.wires.-> SVC
    M -.wires.-> R
    M -.wires.-> ST
    M --> H

    H --> J
    H --> SVC
    SVC --> PORTS
    R -.implements.-> PORTS
    ST -.implements.-> PORTS
    R --> T1 & T2 & T3
    T1 & T2 & T3 -.implement.-> TR
    SVC --> OUT
    SVC --> ERR
    H --> SID

    style dom fill:#e0f2f1,color:#000
    style app fill:#fff8e1,color:#000
    style infra fill:#e8eaf6,color:#000
```

Arrows only ever point inward — `infrastructure` → `application` → `domain`. Nothing in
`domain` or `application` imports `actix_web`; an import like that is the signal that logic
has leaked outward.

### A tool call, end to end

```mermaid
sequenceDiagram
    participant C as Client
    participant G as Envoy Gateway
    participant H as mcp_handler
    participant S as McpService
    participant R as ToolRegistry
    participant T as calculate

    C->>G: POST /mcp-rust-demo (initialize)
    G->>G: verify JWT, check mcp.invoke role
    G->>H: POST /mcp + X-Client-Id
    H->>S: initialize()
    S-->>H: SessionId
    H-->>C: 200 + Mcp-Session-Id

    C->>G: tools/call {name, arguments}
    G->>H: forward
    H->>H: parse JSON-RPC envelope
    H->>S: call_tool(name, args)
    S->>R: find(name)
    alt unknown tool
        R-->>S: Err(UnknownTool)
        S-->>H: DomainError
        H-->>C: JSON-RPC error -32601
    else found
        R-->>S: &dyn Tool
        S->>T: invoke(args)
        T-->>S: ToolOutput{text, is_error}
        S-->>H: ToolOutput
        H-->>C: result + isError
    end
```

Note the two failure modes are deliberately different. An unknown *method or tool* is a
protocol fault and returns a JSON-RPC error; a tool that runs and fails — `1/0`, a bad
timezone — returns a normal result with `isError: true`, so the model can read the reason
and react instead of seeing a transport failure.

### Layout

```
src/
  main.rs              composition root — the only place that wires the layers
  domain/              Tool trait, ToolAnnotations, ToolOutput, SessionId, DomainError
  application/         use-cases (McpService) + outbound ports (SessionStore, ToolRegistry)
  infrastructure/      actix handlers, JSON-RPC framing, tool impls, in-memory store
k8s/                   namespace, deployment, service, httproute, referencegrant,
                       securitypolicy (JWT), securitypolicy-apikey (rollback)
scripts/               build-image.sh, deploy.sh, smoke-remote.sh, get-token.sh,
                       apply-auth.sh (rollback)
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

The server is implemented, deployed to a k3s cluster behind an Envoy Gateway with API-key
authentication, published over HTTPS via Tailscale Funnel, and verified end to end from a
Microsoft Foundry agent.

TLS is terminated by Tailscale (Let's Encrypt); the Gateway listener behind it is plain HTTP.
The public tunnel is scoped to this service's path alone, so nothing else on the shared
gateway is reachable from the internet.

Callers authenticate with a short-lived Entra token carrying the `mcp.invoke` app role, so
there is no shared secret at rest and revocation happens at the identity provider. A
Microsoft Foundry agent connects with its own per-agent Entra Agent Identity, verified end to
end.

There is still no rate limiting and no per-tool authorisation — every caller holding
`mcp.invoke` can invoke every tool. See the access-control sketch below for where per-tool
rules would go.
