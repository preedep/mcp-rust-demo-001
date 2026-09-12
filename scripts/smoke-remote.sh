#!/bin/sh
# Exercise a running MCP server over HTTP, printing the full request and response
# for every call so each step can be copied and re-run by hand.
#
# Defaults to the deployment behind the Envoy Gateway on g1pro. Override with the
# first argument or $URL, e.g. to test a local `cargo run`:
#   scripts/smoke-remote.sh http://localhost:8080/mcp
#
# Options:
#   -q, --quiet     responses only, no request echo
#   -r, --raw       do not pretty-print JSON responses
#   --no-color      plain output (also honoured via NO_COLOR)
#
# Note the scheme is http: the gateway listener is plain HTTP, so https fails the
# TLS handshake with "wrong version number".

set -eu

URL=''
SHOW_REQ=1
PRETTY=1

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case $1 in
        -q|--quiet)    SHOW_REQ=0; shift ;;
        -r|--raw)      PRETTY=0; shift ;;
        --no-color)    NO_COLOR=1; export NO_COLOR; shift ;;
        -h|--help)     sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*)            die "unknown option '$1'" ;;
        *)             URL=$1; shift ;;
    esac
done

URL=${URL:-${URL_DEFAULT:-${MCP_URL:-http://nixhome-linux-g1pro.tail1f1e30.ts.net:30800/mcp-rust-demo}}}

# Colour only when writing to a terminal and NO_COLOR is unset.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    B=$(printf '\033[1m'); DIM=$(printf '\033[2m')
    GRN=$(printf '\033[32m'); RED=$(printf '\033[31m'); CYA=$(printf '\033[36m')
    RST=$(printf '\033[0m')
else
    B=''; DIM=''; GRN=''; RED=''; CYA=''; RST=''
fi

CT='Content-Type: application/json'
ACCEPT='Accept: application/json, text/event-stream'

pretty() {
    if [ "$PRETTY" -eq 1 ]; then
        python3 -m json.tool 2>/dev/null || cat
    else
        cat
    fi
}

# Echo the exact curl that is about to run, so any step can be lifted out and
# re-run by hand.
show_request() {
    _method=$1 _body=$2
    [ "$SHOW_REQ" -eq 1 ] || return 0
    printf '%s  request%s\n' "$DIM" "$RST"
    printf '    %s %s\n' "$_method" "$URL"
    printf '    %s\n' "$CT"
    printf '    %s\n' "$ACCEPT"
    [ -n "$SID" ] && printf '    Mcp-Session-Id: %s\n' "$SID"
    if [ -n "$_body" ]; then
        printf '    %s\n' "$(printf '%s' "$_body" | pretty | sed '2,$s/^/    /')"
    fi
    printf '%s  response%s\n' "$DIM" "$RST"
}

# Separate body from status so a failure is visible even with an empty body.
post() {
    _body=$1
    show_request POST "$_body"
    _out=$(curl -s -w '\n%{http_code}' -X POST "$URL" \
        -H "$CT" -H "$ACCEPT" \
        ${SID:+-H "Mcp-Session-Id: $SID"} \
        -d "$_body" 2>&1) || { printf '%s    request failed%s\n' "$RED" "$RST"; return 1; }
    _code=$(printf '%s' "$_out" | tail -n1)
    printf '%s' "$_out" | sed '$d' | pretty | sed 's/^/    /'
    status_line "$_code"
}

status_line() {
    case $1 in
        2*) printf '    %sHTTP %s%s\n' "$GRN" "$1" "$RST" ;;
        *)  printf '    %sHTTP %s%s\n' "$RED" "$1" "$RST" ;;
    esac
}

step() { printf '\n%s── %s%s\n' "$B$CYA" "$1" "$RST"; }

printf '%s==> %s%s\n' "$B" "$URL" "$RST"

# ---------------------------------------------------------------- initialize
# The only call that issues a session id, so the headers are needed, not just the body.
SID=''
INIT_BODY='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke-remote","version":"1.0"}}}'
step 'initialize'
show_request POST "$INIT_BODY"
if ! INIT_RAW=$(curl -s -S -D "/tmp/smoke-hdr.$$" -X POST "$URL" \
        -H "$CT" -H "$ACCEPT" -d "$INIT_BODY" 2>&1); then
    rm -f "/tmp/smoke-hdr.$$"
    printf '    %s%s%s\n' "$RED" "$INIT_RAW" "$RST"
    die "could not reach $URL — check the host, port and that the scheme is http"
fi
printf '%s' "$INIT_RAW" | pretty | sed 's/^/    /'
SID=$(tr -d '\r' < "/tmp/smoke-hdr.$$" | awk 'tolower($1) == "mcp-session-id:" {print $2}')
STATUS=$(awk 'NR==1 {print $2}' "/tmp/smoke-hdr.$$" | tr -d '\r')
rm -f "/tmp/smoke-hdr.$$"
status_line "${STATUS:-000}"
[ -n "$SID" ] || die "no Mcp-Session-Id returned — check the URL and that the server is up"
printf '    %ssession: %s%s\n' "$GRN" "$SID" "$RST"

# ---------------------------------------------------------------- the rest
step 'tools/list'
post '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

step 'tools/call — echo'
post '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"message":"hello"}}}'

step 'tools/call — get_server_time'
post '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_server_time","arguments":{"timezone":"Asia/Bangkok"}}}'

step 'tools/call — calculate'
post '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"calculate","arguments":{"expression":"(2+3)*4.5"}}}'

step 'ping'
post '{"jsonrpc":"2.0","id":6,"method":"ping"}'

# A tool that fails semantically returns isError: true, not a JSON-RPC error, so the
# model can read the reason and react.
step 'tools/call — calculate 1/0 (expect isError: true)'
post '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"calculate","arguments":{"expression":"1/0"}}}'

# An unknown method is a protocol fault, so this one *is* a JSON-RPC error (-32601).
step 'unknown method (expect JSON-RPC error -32601)'
post '{"jsonrpc":"2.0","id":8,"method":"no/such/method"}'

step 'DELETE session'
[ "$SHOW_REQ" -eq 1 ] && {
    printf '%s  request%s\n' "$DIM" "$RST"
    printf '    DELETE %s\n' "$URL"
    printf '    Mcp-Session-Id: %s\n' "$SID"
    printf '%s  response%s\n' "$DIM" "$RST"
}
status_line "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$URL" -H "Mcp-Session-Id: $SID")"

printf '\n%sdone%s\n' "$GRN" "$RST"
