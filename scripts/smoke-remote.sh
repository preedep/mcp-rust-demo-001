#!/bin/sh
# Exercise a running MCP server over HTTP and print each response.
#
# Defaults to the deployment behind the Envoy Gateway on g1pro. Override with the
# first argument or $URL, e.g. to test a local `cargo run`:
#   scripts/smoke-remote.sh http://localhost:8080/mcp
#
# Note the scheme is http: the gateway listener is plain HTTP, so https fails with
# "wrong version number".

set -eu

URL=${1:-${URL:-http://nixhome-linux-g1pro.tail1f1e30.ts.net:30800/mcp-rust-demo}}

post() {
    curl -s -X POST "$URL" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        ${SID:+-H "Mcp-Session-Id: $SID"} \
        -d "$1"
}

pretty() { python3 -m json.tool 2>/dev/null || cat; }

printf '==> %s\n\n' "$URL"

# initialize is the only call that issues a session id, so read it from the headers.
SID=''
SID=$(curl -s -D - -o /dev/null -X POST "$URL" \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke-remote","version":"1.0"}}}' \
    | grep -i '^mcp-session-id' | tr -d '\r' | awk '{print $2}')
[ -n "$SID" ] || { printf 'no Mcp-Session-Id returned; is the URL right?\n' >&2; exit 1; }
printf 'session: %s\n' "$SID"

printf '\n-- tools/list --\n';  post '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | pretty
printf '\n-- echo --\n';        post '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"message":"hello"}}}'
printf '\n-- get_server_time --\n'; post '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_server_time","arguments":{"timezone":"Asia/Bangkok"}}}'
printf '\n-- calculate --\n';   post '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"calculate","arguments":{"expression":"(2+3)*4.5"}}}'
printf '\n-- ping --\n';        post '{"jsonrpc":"2.0","id":6,"method":"ping"}'
# A tool that fails semantically returns isError: true, not a JSON-RPC error.
printf '\n-- calculate 1/0 (expect isError) --\n'; post '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"calculate","arguments":{"expression":"1/0"}}}'

printf '\n\n-- DELETE session --\n'
curl -s -o /dev/null -w 'status=%{http_code}\n' -X DELETE "$URL" -H "Mcp-Session-Id: $SID"
