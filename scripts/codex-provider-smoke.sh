#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

CORE_PORT="${CODEX_SMOKE_CORE_PORT:-18080}"
WEB_PORT="${CODEX_SMOKE_WEB_PORT:-15173}"
CORE_URL="${CODEX_SMOKE_CORE_URL:-http://127.0.0.1:$CORE_PORT}"
WEB_URL="${CODEX_SMOKE_WEB_URL:-http://127.0.0.1:$WEB_PORT}"
STATE_DIR="${CODEX_SMOKE_STATE_DIR:-/private/tmp/uprava-codex-provider-smoke-$$}"
WORKSPACE_PATH="${CODEX_SMOKE_WORKSPACE_PATH:-$STATE_DIR/workspace}"
NODE_STATE_PATH="${CODEX_SMOKE_NODE_STATE_PATH:-$STATE_DIR/node.sqlite}"
CORE_DATABASE_URL="${CODEX_SMOKE_DATABASE_URL:-sqlite://$STATE_DIR/core.sqlite}"
EXPECTED_NODE="${CODEX_SMOKE_NODE_NAME:-Host Codex Node}"
SESSION_TITLE="${CODEX_SMOKE_SESSION_TITLE:-Codex provider smoke}"
TURN_CONTENT="${CODEX_SMOKE_TURN_CONTENT:-Reply exactly UPRAVA_CODEX_SMOKE_OK. Do not modify files.}"
EXPECTED_ASSISTANT_CONTENT="${CODEX_SMOKE_EXPECTED_ASSISTANT_CONTENT:-UPRAVA_CODEX_SMOKE_OK}"
CODEX_TIMEOUT_SECONDS="${CODEX_SMOKE_CODEX_TIMEOUT_SECONDS:-180}"
SMOKE_RETRIES="${SMOKE_RETRIES:-60}"
ENROLLMENT_RETRIES="${CODEX_SMOKE_ENROLLMENT_RETRIES:-180}"
SMOKE_DELAY_SECONDS="${SMOKE_DELAY_SECONDS:-1}"
CODEX_BINARY="${UPRAVA_CODEX_BINARY:-${CODEX_SMOKE_CODEX_BINARY:-}}"
WEB_PASSWORD="${CODEX_SMOKE_WEB_PASSWORD:-uprava-smoke-password}"
COOKIE_JAR="$STATE_DIR/cookies.txt"
CSRF_TOKEN=""
PIDS=""

case ",${NO_PROXY:-}," in
  *,127.0.0.1,*) ;;
  *) NO_PROXY="${NO_PROXY:+$NO_PROXY,}127.0.0.1" ;;
esac
case ",${NO_PROXY:-}," in
  *,localhost,*) ;;
  *) NO_PROXY="${NO_PROXY:+$NO_PROXY,}localhost" ;;
esac
case ",${no_proxy:-}," in
  *,127.0.0.1,*) ;;
  *) no_proxy="${no_proxy:+$no_proxy,}127.0.0.1" ;;
esac
case ",${no_proxy:-}," in
  *,localhost,*) ;;
  *) no_proxy="${no_proxy:+$no_proxy,}localhost" ;;
esac
export NO_PROXY no_proxy

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required for Codex provider smoke checks" >&2
    exit 1
  fi
}

fetch() {
  curl -fsS --noproxy 127.0.0.1,localhost "$1"
}

auth_get() {
  curl -fsS --noproxy 127.0.0.1,localhost -b "$COOKIE_JAR" "$1"
}

auth_post_json() {
  url="$1"
  body="$2"
  if [ -n "$CSRF_TOKEN" ]; then
    curl -fsS \
      --noproxy 127.0.0.1,localhost \
      -b "$COOKIE_JAR" \
      -c "$COOKIE_JAR" \
      -H "content-type: application/json" \
      -H "x-uprava-csrf: $CSRF_TOKEN" \
      -X POST \
      --data "$body" \
      "$url"
  else
    curl -fsS \
      --noproxy 127.0.0.1,localhost \
      -b "$COOKIE_JAR" \
      -c "$COOKIE_JAR" \
      -H "content-type: application/json" \
      -X POST \
      --data "$body" \
      "$url"
  fi
}

json_select() {
  selector="$1"
  shift
  node -e '
const selector = process.argv[1];
const args = process.argv.slice(2);
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
});
process.stdin.on("end", () => {
  try {
    const data = JSON.parse(input);
    let value;
    if (selector === "field") {
      value = data[args[0]];
    } else if (selector === "enrollment_id") {
      const [expectedNode] = args;
      const enrollment = data.find((candidate) =>
        candidate.display_name === expectedNode &&
        candidate.status === "pending_user_approval"
      );
      value = enrollment && enrollment.enrollment_id;
    } else if (selector === "has_node") {
      const [expectedNode] = args;
      value = data.nodes.some((candidate) => candidate.display_name === expectedNode)
        ? "true"
        : "false";
    } else {
      throw new Error(`unknown selector ${selector}`);
    }
    if (value === undefined || value === null || value === "") {
      throw new Error(`selector ${selector} returned no value`);
    }
    process.stdout.write(String(value));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
});
' "$selector" "$@"
}

print_logs() {
  for log in "$STATE_DIR/core.log" "$STATE_DIR/node.log" "$STATE_DIR/web.log"; do
    if [ -f "$log" ]; then
      printf '\n%s\n' "==> $log" >&2
      tail -n 120 "$log" >&2 || true
    fi
  done
}

cleanup() {
  for pid in $PIDS; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  for pid in $PIDS; do
    wait "$pid" >/dev/null 2>&1 || true
  done
}

fail() {
  echo "$1" >&2
  print_logs
  exit 1
}

trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

wait_for_contains() {
  label="$1"
  url="$2"
  expected="$3"
  attempt=1

  while [ "$attempt" -le "$SMOKE_RETRIES" ]; do
    body="$(fetch "$url" 2>/dev/null || true)"
    if printf '%s' "$body" | grep -F "$expected" >/dev/null 2>&1; then
      printf '%s ok\n' "$label"
      return 0
    fi
    sleep "$SMOKE_DELAY_SECONDS"
    attempt=$((attempt + 1))
  done

  if [ -n "${body:-}" ]; then
    printf '%s\n' "$body" >&2
  fi
  fail "$label failed: did not find '$expected' at $url"
}

wait_for_auth_contains() {
  label="$1"
  url="$2"
  expected="$3"
  attempt=1

  while [ "$attempt" -le "$SMOKE_RETRIES" ]; do
    body="$(auth_get "$url" 2>/dev/null || true)"
    if printf '%s' "$body" | grep -F "$expected" >/dev/null 2>&1; then
      printf '%s ok\n' "$label"
      return 0
    fi
    sleep "$SMOKE_DELAY_SECONDS"
    attempt=$((attempt + 1))
  done

  if [ -n "${body:-}" ]; then
    printf '%s\n' "$body" >&2
  fi
  fail "$label failed: did not find '$expected' at $url"
}

authenticate() {
  status="$(fetch "$CORE_URL/api/v1/auth/status")"
  setup_required="$(printf '%s' "$status" | json_select field setup_required)"
  authenticated="$(printf '%s' "$status" | json_select field authenticated)"
  if [ "$setup_required" = "true" ]; then
    response="$(auth_post_json "$CORE_URL/api/v1/auth/setup" "{\"password\":\"$WEB_PASSWORD\"}")"
    CSRF_TOKEN="$(printf '%s' "$response" | json_select field csrf_token)"
    printf '%s ok\n' "codex core auth setup"
    return 0
  fi
  if [ "$authenticated" != "true" ]; then
    response="$(auth_post_json "$CORE_URL/api/v1/auth/login" "{\"password\":\"$WEB_PASSWORD\"}")"
    CSRF_TOKEN="$(printf '%s' "$response" | json_select field csrf_token)"
    printf '%s ok\n' "codex core auth login"
    return 0
  fi
  fail "codex core auth already authenticated but CSRF token is unavailable"
}

approve_pending_enrollment() {
  attempt=1
  # A clean host may still be compiling the Node binary while Core is already
  # healthy. Give enrollment its own startup budget instead of coupling it to
  # the shorter HTTP readiness budget.
  while [ "$attempt" -le "$ENROLLMENT_RETRIES" ]; do
    body="$(auth_get "$CORE_URL/api/v1/node-enrollments" 2>/dev/null || true)"
    enrollment_id="$(
      printf '%s' "$body" |
        json_select enrollment_id "$EXPECTED_NODE" 2>/dev/null || true
    )"
    if [ -n "$enrollment_id" ]; then
      auth_post_json "$CORE_URL/api/v1/node-enrollments/$enrollment_id/approve" "{}" >/dev/null
      printf '%s ok\n' "codex node enrollment approved"
      return 0
    fi
    sleep "$SMOKE_DELAY_SECONDS"
    attempt=$((attempt + 1))
  done
  fail "codex node enrollment approval failed for $EXPECTED_NODE"
}

wait_for_node_inventory() {
  attempt=1
  while [ "$attempt" -le "$SMOKE_RETRIES" ]; do
    body="$(auth_get "$CORE_URL/api/v1/inventory" 2>/dev/null || true)"
    has_node="$(
      printf '%s' "$body" |
        json_select has_node "$EXPECTED_NODE" 2>/dev/null || true
    )"
    if [ "$has_node" = "true" ]; then
      printf '%s ok\n' "codex node inventory"
      return 0
    fi
    sleep "$SMOKE_DELAY_SECONDS"
    attempt=$((attempt + 1))
  done
  fail "codex node inventory failed"
}

require_command cargo
require_command curl
require_command git
require_command grep
require_command node
require_command npm

if [ -z "$CODEX_BINARY" ]; then
  CODEX_BINARY="$(command -v codex || true)"
fi
if [ -z "$CODEX_BINARY" ] || ! command -v "$CODEX_BINARY" >/dev/null 2>&1; then
  fail "codex binary is not available; set UPRAVA_CODEX_BINARY or CODEX_SMOKE_CODEX_BINARY"
fi

mkdir -p "$STATE_DIR" "$WORKSPACE_PATH" "$(dirname "$NODE_STATE_PATH")"
if [ ! -d "$WORKSPACE_PATH/.git" ]; then
  git -C "$WORKSPACE_PATH" init >/dev/null 2>&1 ||
    fail "failed to initialize disposable git workspace at $WORKSPACE_PATH"
fi

(
  cd "$ROOT_DIR"
  UPRAVA_CORE_BIND="127.0.0.1:$CORE_PORT" \
    UPRAVA_DATABASE_URL="$CORE_DATABASE_URL" \
    UPRAVA_DEPLOYMENT_PROFILE="controlled_dev" \
    UPRAVA_ALLOWED_ORIGINS="$WEB_URL,http://localhost:$WEB_PORT" \
    RUST_LOG="info,uprava_server=debug" \
    cargo run -p uprava-server
) >"$STATE_DIR/core.log" 2>&1 &
PIDS="$PIDS $!"

wait_for_contains "codex core health" "$CORE_URL/api/v1/health" '"status":"ok"'
authenticate

(
  cd "$ROOT_DIR"
  UPRAVA_CORE_URL="$CORE_URL" \
    UPRAVA_NODE_DISPLAY_NAME="$EXPECTED_NODE" \
    UPRAVA_NODE_HEARTBEAT_SECONDS="1" \
    UPRAVA_NODE_STATE_PATH="$NODE_STATE_PATH" \
    UPRAVA_NODE_WORKSPACES="$WORKSPACE_PATH" \
    UPRAVA_CODEX_BINARY="$CODEX_BINARY" \
    UPRAVA_CODEX_IGNORE_USER_CONFIG="true" \
    UPRAVA_CODEX_TIMEOUT_SECONDS="$CODEX_TIMEOUT_SECONDS" \
    RUST_LOG="info,uprava_node=debug" \
    cargo run -p uprava-node
) >"$STATE_DIR/node.log" 2>&1 &
PIDS="$PIDS $!"
approve_pending_enrollment

(
  cd "$ROOT_DIR/apps/web"
  VITE_UPRAVA_API_BASE="$CORE_URL/api/v1" \
    npm exec vite -- --host 127.0.0.1 --port "$WEB_PORT" --strictPort
) >"$STATE_DIR/web.log" 2>&1 &
PIDS="$PIDS $!"

wait_for_contains "codex web entrypoint" "$WEB_URL/" '<title>Uprava</title>'
wait_for_node_inventory
wait_for_auth_contains "codex provider capability" "$CORE_URL/api/v1/inventory" '"provider.codex"'
wait_for_auth_contains "codex managed capability" "$CORE_URL/api/v1/inventory" '"provider.codex.managed"'

(
  cd "$ROOT_DIR"
  UPRAVA_E2E_REAL_API=1 \
    UPRAVA_E2E_CORE_URL="$CORE_URL" \
    UPRAVA_E2E_EXPECTED_NODE="$EXPECTED_NODE" \
    UPRAVA_E2E_WORKSPACE_PATH="$WORKSPACE_PATH" \
    UPRAVA_E2E_PROVIDER="codex" \
    UPRAVA_E2E_SESSION_TITLE="$SESSION_TITLE" \
    UPRAVA_E2E_TURN_CONTENT="$TURN_CONTENT" \
    UPRAVA_E2E_EXPECTED_ASSISTANT_CONTENT="$EXPECTED_ASSISTANT_CONTENT" \
    UPRAVA_E2E_WEB_PASSWORD="$WEB_PASSWORD" \
    UPRAVA_E2E_LIFECYCLE=1 \
    UPRAVA_E2E_EXEC_COMPATIBILITY=1 \
    UPRAVA_E2E_TURN_TIMEOUT_MS="$((CODEX_TIMEOUT_SECONDS * 1000))" \
    UPRAVA_E2E_TEST_TIMEOUT_MS="$(((CODEX_TIMEOUT_SECONDS * 3 + 90) * 1000))" \
    PLAYWRIGHT_BASE_URL="$WEB_URL" \
    make web-e2e
) || fail "codex provider Playwright E2E failed"

{
  printf 'accepted_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  printf 'codex_version=%s\n' "$("$CODEX_BINARY" --version | head -n 1)"
  printf 'os=%s\n' "$(uname -srm)"
  printf 'transport=codex-app-server-v2-loopback-websocket\n'
  printf 'managed_default=true\n'
  printf 'exec_compatibility=true\n'
} >"$STATE_DIR/acceptance.env"
printf '%s\n' "acceptance metadata: $STATE_DIR/acceptance.env"

echo "codex provider smoke passed"
