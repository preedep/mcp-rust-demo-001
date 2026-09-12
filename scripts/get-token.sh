#!/bin/sh
# Fetch an Entra access token for the MCP API using client credentials.
#
# Reads TENANT_ID, CLIENT_ID, CLIENT_SECRET and API_AUDIENCE from .env, which is
# gitignored. Prints the raw token so it can be used directly:
#
#   curl -H "Authorization: Bearer $(scripts/get-token.sh)" ...
#
# Options:
#   --claims   decode and print the token's claims instead of the token
#   --check    report whether the token carries the mcp.invoke app role

set -eu

ENV_FILE=${ENV_FILE:-.env}
ACTION=token

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case $1 in
        --claims) ACTION=claims; shift ;;
        --check)  ACTION=check; shift ;;
        -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument '$1'" ;;
    esac
done

CDPATH=''
export CDPATH
cd -- "$(dirname -- "$0")/.."

env_get() {
    [ -f "$ENV_FILE" ] || return 0
    grep -E "^$1=" "$ENV_FILE" | head -1 | cut -d= -f2-
}

TENANT_ID=${TENANT_ID:-$(env_get TENANT_ID)}
CLIENT_ID=${CLIENT_ID:-$(env_get CLIENT_ID)}
CLIENT_SECRET=${CLIENT_SECRET:-$(env_get CLIENT_SECRET)}
API_AUDIENCE=${API_AUDIENCE:-$(env_get API_AUDIENCE)}

for v in TENANT_ID CLIENT_ID CLIENT_SECRET API_AUDIENCE; do
    eval "val=\$$v"
    [ -n "$val" ] || die "$v not set — put it in $ENV_FILE (see .env.example)"
done

# Client credentials asks for every app role already granted, via /.default —
# individual roles are not named in the request.
RESP=$(curl -s --max-time 20 -X POST \
    "https://login.microsoftonline.com/$TENANT_ID/oauth2/v2.0/token" \
    --data-urlencode "client_id=$CLIENT_ID" \
    --data-urlencode "client_secret=$CLIENT_SECRET" \
    --data-urlencode "scope=$API_AUDIENCE/.default" \
    --data-urlencode "grant_type=client_credentials")

TOKEN=$(printf '%s' "$RESP" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("access_token",""))' 2>/dev/null || true)
if [ -z "$TOKEN" ]; then
    printf '%s\n' "$RESP" | python3 -m json.tool 2>/dev/null >&2 || printf '%s\n' "$RESP" >&2
    die "no access_token returned"
fi

case $ACTION in
    token) printf '%s\n' "$TOKEN" ;;
    claims)
        # Decode the payload locally. This does NOT verify the signature — it is for
        # inspecting what the gateway will see, nothing more.
        printf '%s' "$TOKEN" | cut -d. -f2 | python3 -c '
import sys, json, base64
p = sys.stdin.read().strip()
p += "=" * (-len(p) % 4)
print(json.dumps(json.loads(base64.urlsafe_b64decode(p)), indent=2, sort_keys=True))
'
        ;;
    check)
        printf '%s' "$TOKEN" | cut -d. -f2 | python3 -c '
import sys, json, base64
p = sys.stdin.read().strip()
p += "=" * (-len(p) % 4)
c = json.loads(base64.urlsafe_b64decode(p))
roles = c.get("roles", [])
print("iss  :", c.get("iss"))
print("aud  :", c.get("aud"))
print("appid:", c.get("appid"))
print("roles:", roles or "(none)")
if "mcp.invoke" in roles:
    print("\nOK: token carries the mcp.invoke app role")
else:
    print("\nMISSING mcp.invoke.")
    print("  Add an App role (member type: Applications) on the API registration,")
    print("  grant it to this client under API permissions, then grant admin consent.")
    print("  A delegated scope appears as scp, not roles, and will not work here.")
    sys.exit(1)
'
        ;;
esac
