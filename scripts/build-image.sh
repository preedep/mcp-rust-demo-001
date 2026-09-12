#!/bin/sh
# Build the MCP server image for the k3s node and verify it before it leaves the Mac.
#
# The node is x86_64 while this machine is ARM, so the platform is always explicit —
# a native ARM build will not start on g1pro.
#
# Usage:
#   scripts/build-image.sh                 # build + smoke test
#   scripts/build-image.sh -t v0.2.0       # tag explicitly
#   scripts/build-image.sh --no-test       # build only
#   scripts/build-image.sh --save out.tar  # also write a tarball for transfer

set -eu

IMAGE=${IMAGE:-mcp-rust-demo-001}
TAG=${TAG:-dev}
PLATFORM=${PLATFORM:-linux/amd64}
PORT=${PORT:-18080}
RUN_TEST=1
SAVE_TO=""

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case $1 in
        -t|--tag)    [ $# -ge 2 ] || die "$1 needs a value"; TAG=$2; shift 2 ;;
        -i|--image)  [ $# -ge 2 ] || die "$1 needs a value"; IMAGE=$2; shift 2 ;;
        -p|--platform) [ $# -ge 2 ] || die "$1 needs a value"; PLATFORM=$2; shift 2 ;;
        --save)      [ $# -ge 2 ] || die "$1 needs a value"; SAVE_TO=$2; shift 2 ;;
        --no-test)   RUN_TEST=0; shift ;;
        -h|--help)   sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)           die "unknown argument '$1'" ;;
    esac
done

REF="$IMAGE:$TAG"
# Run from the repo root regardless of where the script was invoked from.
CDPATH=''
export CDPATH
cd -- "$(dirname -- "$0")/.."

command -v docker >/dev/null 2>&1 || die "docker not found in PATH"
docker buildx version >/dev/null 2>&1 || die "docker buildx is required"

printf '==> building %s for %s\n' "$REF" "$PLATFORM"
# --provenance/--sbom off: the attestation manifests triple the reported image size and
# k3s has no use for them here.
docker buildx build \
    --platform "$PLATFORM" \
    --provenance=false \
    --sbom=false \
    -t "$REF" \
    --load \
    .

# docker image ls counts uncompressed layers plus attestations; docker save is the number
# that matters, since that is what crosses the wire to g1pro.
SIZE=$(docker save "$REF" | wc -c | awk '{printf "%.2f MB", $1/1048576}')
printf '==> image size: %s (docker save)\n' "$SIZE"

# The image metadata can claim one architecture while the binary inside is another
# (a builder stage pinned to $BUILDPLATFORM does exactly that), and the node then
# fails with "exec format error". Check the ELF itself, not the label.
case $PLATFORM in
    */amd64) WANT='x86-64' ;;
    */arm64) WANT='aarch64' ;;
    *)       WANT='' ;;
esac
if [ -n "$WANT" ] && command -v file >/dev/null 2>&1; then
    CID=$(docker create --platform "$PLATFORM" "$REF" 2>/dev/null) || CID=''
    if [ -n "$CID" ]; then
        docker cp "$CID:/mcp-rust-demo-001" "/tmp/mcp-arch-check.$$" >/dev/null 2>&1 \
            && ARCH_DESC=$(file -b "/tmp/mcp-arch-check.$$") \
            || ARCH_DESC=''
        docker rm "$CID" >/dev/null 2>&1 || true
        rm -f "/tmp/mcp-arch-check.$$"
        if [ -n "$ARCH_DESC" ]; then
            case $ARCH_DESC in
                *"$WANT"*) printf '==> binary arch ok (%s)\n' "$WANT" ;;
                *) die "binary is not $WANT: $ARCH_DESC" ;;
            esac
        fi
    fi
fi

if [ "$RUN_TEST" -eq 1 ]; then
    CNAME="mcp-smoke-$$"
    # Clean up the container however we exit, including on failure.
    trap 'docker rm -f "$CNAME" >/dev/null 2>&1 || true' EXIT INT TERM

    printf '==> starting container on port %s\n' "$PORT"
    docker run --rm -d --name "$CNAME" -p "$PORT:8080" "$REF" >/dev/null \
        || die "container failed to start"

    # The server binds in well under a second, but an emulated amd64 image on ARM is slower.
    i=0
    until curl -sf "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1; do
        i=$((i + 1))
        [ "$i" -lt 60 ] || {
            docker logs "$CNAME" 2>&1 | tail -20
            die "server did not become healthy"
        }
        sleep 0.5
    done
    printf '    healthz ok\n'

    # initialize must return a session id; tools/list must report the three demo tools.
    HDRS=$(curl -s -D - -o /tmp/mcp-init.$$ \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        -X POST "http://127.0.0.1:$PORT/mcp" \
        -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}')
    SID=$(printf '%s' "$HDRS" | tr -d '\r' | awk 'tolower($1) == "mcp-session-id:" {print $2}')
    grep -q '"protocolVersion"' /tmp/mcp-init.$$ || {
        cat /tmp/mcp-init.$$ >&2
        rm -f /tmp/mcp-init.$$
        die "initialize did not return a protocol version"
    }
    rm -f /tmp/mcp-init.$$
    [ -n "$SID" ] || die "initialize did not issue an Mcp-Session-Id"
    printf '    initialize ok (session %s)\n' "$SID"

    TOOLS=$(curl -s \
        -H 'Content-Type: application/json' \
        -H "Mcp-Session-Id: $SID" \
        -X POST "http://127.0.0.1:$PORT/mcp" \
        -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}')
    for tool in echo get_server_time calculate; do
        printf '%s' "$TOOLS" | grep -q "\"$tool\"" || {
            printf '%s\n' "$TOOLS" >&2
            die "tools/list is missing '$tool'"
        }
    done
    printf '    tools/list ok (echo, get_server_time, calculate)\n'

    # Exercise one tool end to end rather than trusting the listing alone.
    CALC=$(curl -s \
        -H 'Content-Type: application/json' \
        -H "Mcp-Session-Id: $SID" \
        -X POST "http://127.0.0.1:$PORT/mcp" \
        -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"calculate","arguments":{"expression":"(2+3)*4.5"}}}')
    printf '%s' "$CALC" | grep -q '22.5' || {
        printf '%s\n' "$CALC" >&2
        die "calculate returned an unexpected result"
    }
    printf '    tools/call ok\n'

    docker rm -f "$CNAME" >/dev/null 2>&1 || true
    trap - EXIT INT TERM
    printf '==> smoke test passed\n'
fi

if [ -n "$SAVE_TO" ]; then
    docker save "$REF" -o "$SAVE_TO"
    printf '==> saved %s\n' "$SAVE_TO"
fi

printf '\n%s is ready.\n' "$REF"
printf 'To deploy it:  scripts/deploy.sh --skip-build\n'
# Not a single `docker save | ssh -t ...` pipeline: ssh will not allocate a TTY when
# stdin is a pipe, and sudo on the node needs one. Stage first, then import.
printf 'Or by hand, in two steps (the second prompts for sudo on the node):\n'
printf '  docker save %s | ssh nickmsft@nixhome-linux-g1pro '"'"'cat > /tmp/%s.tar'"'"'\n' \
    "$REF" "$IMAGE-$TAG"
printf "  ssh -t nickmsft@nixhome-linux-g1pro 'sudo k3s ctr images import /tmp/%s.tar'\n" \
    "$IMAGE-$TAG"
