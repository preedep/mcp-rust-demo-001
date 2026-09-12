#!/bin/sh
# ROLLBACK PATH. The gateway now authenticates with Entra tokens
# (k8s/securitypolicy.yaml); this script manages the previous static-key policy in
# k8s/securitypolicy-apikey.yaml. Use it to switch back, or to rotate the key while
# key auth is live.
#
# Create (or rotate) the API key Secret the gateway checks, and apply the
# SecurityPolicy that enforces it.
#
# The key is read from .env, which is gitignored and never committed. Generate one
# with --generate if you do not have it yet.
#
# Usage:
#   scripts/apply-auth.sh              # apply the key from .env
#   scripts/apply-auth.sh --generate   # mint a new key into .env, then apply
#   scripts/apply-auth.sh --show       # print the current key (for pasting into Foundry)
#   scripts/apply-auth.sh --remove     # drop the policy, leaving the route open

set -eu

NAMESPACE=${NAMESPACE:-envoy-gateway}
SECRET=${SECRET:-mcp-rust-demo-apikey}
CLIENT=${CLIENT:-foundry}
ACTION=apply

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case $1 in
        --generate) ACTION=generate; shift ;;
        --show)     ACTION=show; shift ;;
        --remove)   ACTION=remove; shift ;;
        -h|--help)  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)          die "unknown argument '$1'" ;;
    esac
done

CDPATH=''
export CDPATH
cd -- "$(dirname -- "$0")/.."

ENV_FILE=${ENV_FILE:-.env}

if [ -z "${KUBECONFIG:-}" ]; then
    KUBECONFIG="$HOME/.kube/config${KUBECONFIG_EXTRA:+:$KUBECONFIG_EXTRA}"
    export KUBECONFIG
fi

new_key() { openssl rand -base64 32 | tr -d '/+=' | head -c 40; }

read_key() {
    [ -f "$ENV_FILE" ] || die "$ENV_FILE not found — run with --generate first"
    # Read without sourcing, so a stray line in .env cannot execute.
    _k=$(grep -E '^MCP_API_KEY=' "$ENV_FILE" | head -1 | cut -d= -f2-)
    [ -n "$_k" ] || die "MCP_API_KEY not set in $ENV_FILE"
    printf '%s' "$_k"
}

case $ACTION in
    generate)
        KEY=$(new_key)
        if [ -f "$ENV_FILE" ] && grep -qE '^MCP_API_KEY=' "$ENV_FILE"; then
            # Rewrite in place, preserving the rest of the file.
            _tmp=$(mktemp)
            sed "s|^MCP_API_KEY=.*|MCP_API_KEY=$KEY|" "$ENV_FILE" > "$_tmp"
            mv "$_tmp" "$ENV_FILE"
        else
            printf '\n# API key the Envoy Gateway requires on the Authorization header.\nMCP_API_KEY=%s\n' \
                "$KEY" >> "$ENV_FILE"
        fi
        chmod 600 "$ENV_FILE"
        printf 'generated a new key into %s\n' "$ENV_FILE"
        ;;
    show)
        printf '%s\n' "$(read_key)"
        exit 0
        ;;
    remove)
        kubectl -n "$NAMESPACE" delete securitypolicy mcp-rust-demo-auth --ignore-not-found
        kubectl -n "$NAMESPACE" delete secret "$SECRET" --ignore-not-found
        printf '\nauth removed — the route is now open to anyone who can reach the gateway.\n'
        exit 0
        ;;
esac

KEY=$(read_key)

# The Secret maps client-id -> api-key (that direction, per the CRD: "each API key
# is stored in the key representing the client id"). Recreated rather than patched,
# so rotation is a single idempotent step.
kubectl -n "$NAMESPACE" create secret generic "$SECRET" \
    --from-literal="$CLIENT=$KEY" \
    --dry-run=client -o yaml | kubectl apply -f -

kubectl apply -f k8s/securitypolicy-apikey.yaml

printf '\n==> waiting for the policy to be accepted\n'
i=0
until kubectl -n "$NAMESPACE" get securitypolicy mcp-rust-demo-auth \
        -o jsonpath='{.status.ancestors[0].conditions[?(@.type=="Accepted")].status}' \
        2>/dev/null | grep -q True; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || {
        kubectl -n "$NAMESPACE" get securitypolicy mcp-rust-demo-auth -o yaml | sed -n '/^status:/,$p'
        die "policy was not accepted"
    }
    sleep 1
done
printf '    accepted\n'

printf '\nAPI key auth is active. Callers must send:\n'
printf '    Authorization: <key>\n\n'
printf 'Get the key with:  scripts/apply-auth.sh --show\n'
printf 'Paste that value into Foundry as the Authorization credential.\n'
