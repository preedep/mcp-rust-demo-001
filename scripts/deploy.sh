#!/bin/sh
# Build, ship and deploy the MCP server to k3s on g1pro.
#
# There is no registry on the tailnet, so the image is imported straight into the
# node's containerd. That import needs sudo on g1pro, which is password-protected:
# this script uses `ssh -t` so the prompt reaches your terminal.
#
# Usage:
#   scripts/deploy.sh                  # build, import, apply, verify
#   scripts/deploy.sh -t v0.2.0        # deploy a specific tag
#   scripts/deploy.sh --skip-build     # reuse the image already built locally
#   scripts/deploy.sh --skip-import    # image is already on the node
#   scripts/deploy.sh --dry-run        # validate manifests server-side, change nothing

set -eu

IMAGE=${IMAGE:-mcp-rust-demo-001}
TAG=${TAG:-dev}
SSH_HOST=${SSH_HOST:-}
NAMESPACE=${NAMESPACE:-mcp-rust-demo}
DEPLOYMENT=${DEPLOYMENT:-mcp-rust-demo}
GATEWAY_NS=${GATEWAY_NS:-envoy-gateway}
PUBLIC_URL=${PUBLIC_URL:-}

DO_BUILD=1
DO_IMPORT=1
DRY_RUN=0

die() { printf 'error: %s\n' "$1" >&2; exit 1; }
step() { printf '\n==> %s\n' "$1"; }

while [ $# -gt 0 ]; do
    case $1 in
        -t|--tag)      [ $# -ge 2 ] || die "$1 needs a value"; TAG=$2; shift 2 ;;
        -i|--image)    [ $# -ge 2 ] || die "$1 needs a value"; IMAGE=$2; shift 2 ;;
        --skip-build)  DO_BUILD=0; shift ;;
        --skip-import) DO_IMPORT=0; shift ;;
        --dry-run)     DRY_RUN=1; shift ;;
        -h|--help)     sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)             die "unknown argument '$1'" ;;
    esac
done

REF="$IMAGE:$TAG"
CDPATH=''
export CDPATH
cd -- "$(dirname -- "$0")/.."

# Host and endpoint come from .env, not from defaults baked into this file: the repo is
# public and a tailnet hostname is an invitation to probe. See .env.example.
ENV_FILE=${ENV_FILE:-.env}
env_get() {
    [ -f "$ENV_FILE" ] || return 0
    grep -E "^$1=" "$ENV_FILE" | head -1 | cut -d= -f2-
}
[ -n "$SSH_HOST" ]   || SSH_HOST=$(env_get SSH_HOST)
[ -n "$PUBLIC_URL" ] || PUBLIC_URL=$(env_get MCP_URL)
[ -n "$SSH_HOST" ]   || die "SSH_HOST not set — put it in $ENV_FILE (see .env.example)"
[ -n "$PUBLIC_URL" ] || die "MCP_URL not set — put it in $ENV_FILE (see .env.example)"

command -v kubectl >/dev/null 2>&1 || die "kubectl not found in PATH"
# Merge the g1pro kubeconfig the way CLAUDE.md documents, unless the caller set one.
if [ -z "${KUBECONFIG:-}" ]; then
    KUBECONFIG="$HOME/.kube/config${KUBECONFIG_EXTRA:+:$KUBECONFIG_EXTRA}"
    export KUBECONFIG
fi

step "checking connectivity"
# A timeout here almost always means Tailscale is down on the Mac, not that the
# server is broken — say so rather than letting kubectl fail obscurely later.
ssh -o ConnectTimeout=8 -o BatchMode=yes "$SSH_HOST" true 2>/dev/null \
    || die "cannot ssh to $SSH_HOST — is Tailscale up?"
kubectl version -o json >/dev/null 2>&1 \
    || die "kubectl cannot reach the cluster — check KUBECONFIG and Tailscale"
printf '    ssh and kubectl ok\n'

if [ "$DRY_RUN" -eq 1 ]; then
    step "validating manifests (server dry-run, nothing applied)"
    # The namespace has to exist before the namespaced objects can be validated,
    # so anything depending on it is only checked client-side here.
    kubectl apply --dry-run=server -f k8s/namespace.yaml
    kubectl apply --dry-run=server -f k8s/httproute.yaml
    for f in k8s/deployment.yaml k8s/service.yaml k8s/referencegrant.yaml; do
        kubectl apply --dry-run=client -f "$f"
    done
    printf '\ndry run complete. No changes made.\n'
    exit 0
fi

if [ "$DO_BUILD" -eq 1 ]; then
    step "building and smoke testing $REF"
    scripts/build-image.sh -t "$TAG" -i "$IMAGE"
else
    docker image inspect "$REF" >/dev/null 2>&1 \
        || die "$REF not found locally; drop --skip-build"
    printf '\n==> reusing local image %s\n' "$REF"
fi

if [ "$DO_IMPORT" -eq 1 ]; then
    step "importing $REF into containerd on the node"
    # This cannot be a single `docker save | ssh -t` pipeline: ssh refuses to allocate
    # a TTY when stdin is a pipe, and sudo on the node requires a TTY to prompt. So
    # stage the tarball first (pipe, no TTY), then import it (TTY, no pipe).
    REMOTE_TAR="/tmp/$IMAGE-$TAG.tar"
    printf '    staging image on the node (%s)\n' "$REMOTE_TAR"
    # shellcheck disable=SC2029  # the path is chosen here, so expanding locally is intended
    docker save "$REF" | ssh "$SSH_HOST" "cat > $REMOTE_TAR" \
        || die "could not copy the image to $SSH_HOST"

    printf '    importing; enter your sudo password for the node when prompted\n'
    if [ -t 0 ]; then
        # shellcheck disable=SC2029  # same: REMOTE_TAR is a local decision
        ssh -t "$SSH_HOST" "sudo k3s ctr images import $REMOTE_TAR && rm -f $REMOTE_TAR" \
            || die "image import failed"
    else
        # No controlling terminal here (CI, or a pipeline), so the sudo prompt has
        # nowhere to go. Leave the staged tarball and hand over the exact command.
        printf '\n'
        printf 'No terminal available for the sudo prompt. The image is staged on the node.\n'
        printf 'Run this, then re-run with --skip-build --skip-import:\n\n'
        printf "  ssh -t %s 'sudo k3s ctr images import %s && rm -f %s'\n\n" \
            "$SSH_HOST" "$REMOTE_TAR" "$REMOTE_TAR"
        exit 1
    fi
fi

step "applying manifests"
# Namespace first: everything else is namespaced or references the Service in it.
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/referencegrant.yaml
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml
kubectl apply -f k8s/httproute.yaml

# Auth is applied separately: the SecurityPolicy needs a Secret built from the key in
# .env, which is not a plain `kubectl apply`. Without it the endpoint is wide open, and
# nothing else would say so — hence the explicit check rather than a silent skip.
if kubectl -n "$GATEWAY_NS" get securitypolicy mcp-rust-demo-auth >/dev/null 2>&1; then
    printf '    auth policy present\n'
else
    printf '    %sno auth policy — the endpoint is UNAUTHENTICATED%s\n' "${RED:-}" "${RST:-}"
    printf '    run: kubectl apply -f k8s/securitypolicy.yaml\n'
fi

# Pin the running container to the tag just deployed, in case the manifest default
# differs from --tag.
kubectl -n "$NAMESPACE" set image "deploy/$DEPLOYMENT" "$DEPLOYMENT=$REF" >/dev/null
# A tag that was re-imported keeps the same name, so the pod spec may be unchanged
# and no rollout would happen; force one so the new image is actually picked up.
kubectl -n "$NAMESPACE" rollout restart "deploy/$DEPLOYMENT" >/dev/null

step "waiting for rollout"
if ! kubectl -n "$NAMESPACE" rollout status "deploy/$DEPLOYMENT" --timeout=120s; then
    printf '\n--- pods ---\n'
    kubectl -n "$NAMESPACE" get pods
    printf '\n--- recent events ---\n'
    kubectl -n "$NAMESPACE" get events --sort-by=.lastTimestamp 2>/dev/null | tail -15
    printf '\n--- logs ---\n'
    kubectl -n "$NAMESPACE" logs "deploy/$DEPLOYMENT" --tail=30 2>/dev/null || true
    die "rollout did not complete"
fi

step "checking the route"
# RefNotPermitted here means the ReferenceGrant is missing or mismatched.
if ! kubectl -n "$GATEWAY_NS" get httproute "$DEPLOYMENT" \
        -o jsonpath='{.status.parents[0].conditions[?(@.type=="ResolvedRefs")].status}' \
        2>/dev/null | grep -q True; then
    kubectl -n "$GATEWAY_NS" get httproute "$DEPLOYMENT" -o yaml | sed -n '/^status:/,$p'
    die "route did not resolve its backend"
fi
printf '    accepted, backend resolved\n'

step "verifying through the gateway"
# The gateway enforces Entra tokens, so fetch one to verify with. MCP_AUTH_MODE=key
# falls back to the static key for when the rollback policy is live.
ENV_FILE=${ENV_FILE:-.env}
if [ "${MCP_AUTH_MODE:-jwt}" = key ]; then
    API_KEY=${MCP_API_KEY:-}
    if [ -z "$API_KEY" ] && [ -f "$ENV_FILE" ]; then
        API_KEY=$(grep -E '^MCP_API_KEY=' "$ENV_FILE" | head -1 | cut -d= -f2-)
    fi
else
    API_KEY="Bearer $(scripts/get-token.sh)"
fi
# Envoy can still be draining the old endpoint for a moment after the rollout
# reports complete, so a single probe here is flaky. Retry briefly.
RESP=''
i=0
while [ "$i" -lt 10 ]; do
    RESP=$(curl -s --max-time 10 "$PUBLIC_URL" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        ${API_KEY:+-H "Authorization: $API_KEY"} \
        -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' 2>/dev/null || true)
    printf '%s' "$RESP" | grep -q '"protocolVersion"' && break
    i=$((i + 1))
    sleep 2
done
if printf '%s' "$RESP" | grep -q '"protocolVersion"'; then
    printf '    handshake ok\n'
else
    printf '%s\n' "$RESP" >&2
    printf 'the gateway requires a credential; check scripts/get-token.sh --check\n' >&2
    die "gateway did not return a valid initialize response"
fi

printf '\n%s deployed.\n' "$REF"
printf '  endpoint   %s\n' "$PUBLIC_URL"
printf '  in-cluster http://%s.%s.svc.cluster.local:8080/mcp\n' "$DEPLOYMENT" "$NAMESPACE"
printf '  logs       kubectl -n %s logs -f deploy/%s\n' "$NAMESPACE" "$DEPLOYMENT"
printf '\nAuth: Entra token required — scripts/get-token.sh --check\n'
