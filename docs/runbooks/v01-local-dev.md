# V01 Local Development Runbook

Статус: `active`

Этот runbook описывает текущий scaffold `0.1`: Rust Core Backend, Rust Node
Daemon, Vite Web Control Panel, SQLite persistence и первый минимальный Codex
provider adapter.

## Prerequisites

- Rust `1.88` or newer with `cargo`, `rustfmt` and `clippy`.
- Node.js and npm for `apps/web`.
- Docker Compose for the Core/Web/ToolHive/Generated UI Builder dev profile.
- `curl`, `grep` and `node` for `make dev-smoke`.

`make init` installs the Rust quality tools used by `make c` when they are
missing: `cargo-audit`, `cargo-deny` and `taplo-cli`.

## Local Processes

```sh
make init
make core-r
make node-r
make web-r
```

`make node-r` defaults `UPRAVA_NODE_WORKSPACES` to this repository root so the
Node has an explicit workspace allow-list without reopening the old unrestricted
workspace behavior. Set `UPRAVA_NODE_WORKSPACES` before running the target when
you want the host Node to manage a different local workspace tree.

Default ports and paths:

- Core API: `http://127.0.0.1:8080/api/v1`
- Web UI: `http://127.0.0.1:5173`
- Core SQLite: `.local/state/core.sqlite`
- Node local state: `~/.local/share/uprava-node/node.json`
- Core process log: `.local/logs/core.log`
- Node process log: `.local/logs/node.log`
- Browser client log accepted by Core: `.local/logs/client.log`
- Runtime idle expiry: `UPRAVA_RUNTIME_EXPIRY_SECONDS`, default `86400`
- Browser CORS origins: `UPRAVA_ALLOWED_ORIGINS`, default
  `http://127.0.0.1:5173,http://localhost:5173`
- Web auth mode: `UPRAVA_WEB_AUTH`, default `auto`
- Web session TTL: `UPRAVA_WEB_SESSION_TTL_SECONDS`, default `86400`
- Secure cookie flag: `UPRAVA_COOKIE_SECURE`, default `false` for local HTTP
- Public ingress window: `UPRAVA_PUBLIC_RATE_WINDOW_SECONDS`, default `60`
- Global ingress budget per window: `UPRAVA_PUBLIC_GLOBAL_RATE_LIMIT`, default
  `5000`
- Authenticated UI budget per peer and window:
  `UPRAVA_PUBLIC_PEER_RATE_LIMIT`, default `600`; auth, enrollment, Node и
  session stream используют независимые buckets

Core creates and migrates the SQLite schema on startup. Migration coverage
includes a clean empty database and previous dev `nodes` table shape without
`credential_hash`. For local-only recovery, stop Core/Node first, copy
`.local/state/core.sqlite` and `~/.local/share/uprava-node/node.json` aside if
you need evidence, then delete broken local state or use `make dev-reset`
for the Compose volume.

## Breaking Reset Contract 0.2.0

Existing unversioned paths выше являются sources 0.1.8. До первого запуска
0.2.0 их и effective environment нужно сохранить в зарезервированные local
slots:

| Release | Core state | Core config | Node state | Node config |
| --- | --- | --- | --- | --- |
| `0.1.8` | `.local/state/0.1.8/core.sqlite` | `.local/config/0.1.8/core.env` | `~/.local/share/uprava-node/0.1.8/node.json` | `~/.config/uprava-node/0.1.8/node.env` |
| `0.2.0` | `.local/state/0.2.0/core.sqlite` | `.local/config/0.2.0/core.env` | `~/.local/share/uprava-node/0.2.0/node.sqlite` | `~/.config/uprava-node/0.2.0/node.env` |

Это release-family slots, а не in-place migration. Implementation 0.2.0
обязана выбирать только state and config 0.2.0. Сохранённые slots 0.1.8
read-only во время работы 0.2.0 и остаются доступны для rollback. Если у
выбранного state неправильная schema или format, startup завершается actionable
incompatible-state error; state не импортируется, не reinterpret и не
удаляется.

Процедура clean reset 0.2.0:

1. остановить Core, Web and Node и скопировать нужную diagnostic evidence
   0.2.0;
2. удалить или переинициализировать только Core and Node state slots 0.2.0;
3. запустить Core с empty Core slot 0.2.0 и matching config;
4. запустить Node с empty SQLite slot 0.2.0 и matching config;
5. создать и явно approve новое enrollment, затем заново bind Projects and
   Placements;
6. выполнить clean-state smoke flow.

Old Node JSON state не импортируется, поэтому re-enrollment обязателен. Reset
никогда не удаляет и не переписывает state/config slot 0.1.8. `make dev-reset`
можно расширить для initialize или clear выбранного development slot 0.2.0,
но он обязан сохранять все slots 0.1.8. Rollback означает совместный выбор
binaries 0.1.8, Core config/state и Node config/state; работа, созданная только
в 0.2.0, после rollback отсутствует.

## Security Profile

`controlled_dev` is the only supported V01 development profile. Browser auth is
enabled by default with `UPRAVA_WEB_AUTH=auto`; `local_trusted` and
`UPRAVA_WEB_AUTH=disabled` are rejected at startup.

- Web shows first-run local password setup, then requires login.
- Core issues an `HttpOnly`, `SameSite=Lax` session cookie and a CSRF cookie.
- Browser mutations must send `x-uprava-csrf`; the Web client does this from
  the CSRF cookie.
- Core checks configured browser origins and records security audit events for
  setup/login/logout, rejected auth, CSRF failures, enrollment, revoke and
  rotate.
- Node heartbeat and control-channel auth use bearer credentials. Core stores
  only credential hashes and verifies them with constant-time comparison.

For HTTPS or a TLS-terminating proxy, set `UPRAVA_COOKIE_SECURE=true`. For
local HTTP development, leave it disabled or the browser will not return the
session cookie. Node enrollment always requires explicit approval; the old
`UPRAVA_AUTO_APPROVE_ENROLLMENTS=true` development shortcut is rejected.

## Docker Compose

```sh
make dev-smoke
make dev-logs
make dev-down
```

Use `make dev-up` when you want Core/Web/ToolHive/Generated UI Builder attached in the foreground for
interactive debugging. `make dev-smoke` starts or rebuilds the dev profile in
detached mode before running checks. Set `SMOKE_SKIP_COMPOSE_UP=1` to probe an
already running non-default profile.

Reset local Compose state intentionally:

```sh
make dev-reset
```

The `compose.dev.yaml` profile starts Core, Web, a separate ToolHive bridge and
the network-isolated Generated UI Builder.
Host ports are bound to `127.0.0.1`; Core and ToolHive state use resettable
Docker volumes. Run `make node-r` for a real local Node Daemon that can access
host workspaces, Codex and user credentials. Neither Node nor Codex runs in a
container in this profile.

ToolHive publishes its private Node bridge at `127.0.0.1:18081` and the Linear
OAuth callback at `127.0.0.1:18765`. Its MCP proxy stays inside the service.
The service mounts `/var/run/docker.sock`, because ToolHive `0.40.0` initializes
its workload manager even for remote MCP. This socket is intentionally confined
to the ToolHive service and is never mounted into Node, Core or Web.

Core rejects browser CORS origins outside `UPRAVA_ALLOWED_ORIGINS`; the default
allows the local Vite Web UI on `127.0.0.1` and `localhost`. For a controlled
development host or forwarded port, set `UPRAVA_ALLOWED_ORIGINS` to the exact
comma-separated browser origins that should reach Core. Wildcard origins are
rejected.

`make dev-smoke` starts the profile and then runs `scripts/dev-smoke.sh`. The
script bypasses localhost HTTP proxies and
checks:

- Core health at `http://127.0.0.1:8080/api/v1/health`;
- Web entrypoint at `http://127.0.0.1:5173`;
- ToolHive bridge version at `http://127.0.0.1:18081/api/v1/version`;
- local web auth setup/login and CSRF-protected client requests;
- authenticated Core inventory access.

Override `CORE_URL`, `WEB_URL`, `TOOLHIVE_URL`, `SMOKE_WEB_PASSWORD`, `SMOKE_RETRIES` or
`SMOKE_DELAY_SECONDS`, чтобы запускать тот же smoke check against a non-default
local profile. Set `SMOKE_SKIP_COMPOSE_UP=1` when those endpoints are already
running and should not be started by the Make target.

## Node Enrollment

For a host-running Node that should manage a workspace outside this repository,
set `UPRAVA_NODE_WORKSPACES` to one or more explicit allowed workspace roots
before starting the Node:

```sh
export UPRAVA_NODE_WORKSPACES=/path/to/workspace-root
make node-r
```

Node writes local development state to
`~/.local/share/uprava-node/0.2.0/node.sqlite` by default and logs the short-lived
`enrollment_id`. That state includes a stable `daemon_installation_id` used for
local diagnostics; the pairing code stays in local Node state for the claim
request and is not logged. Approve enrollment through Core:

```sh
curl -X POST http://127.0.0.1:8080/api/v1/node-enrollments/{enrollment_id}/approve
```

Node retries the claim, stores returned development credential in its local
state file with private file permissions where the OS supports them, and then
heartbeats with that credential in the `Authorization: Bearer` header.
Heartbeats include bounded
Node diagnostics with the daemon installation id, local event outbox count and
command cache count so the Core node detail can distinguish local state files
during troubleshooting. Revoking a node clears the Core-side credential hash and
rejects future heartbeats from that node. Rotating a node credential returns a
new credential once; update the Node state before restarting that daemon or the
old credential will be rejected.

## Node и ToolHive runtime

Node probes the separate ToolHive bridge and observed native CLI capabilities
on heartbeat. Default bridge URL:

```sh
export UPRAVA_TOOLHIVE_URL=http://127.0.0.1:18081
export UPRAVA_TOOLHIVE_TIMEOUT_SECONDS=300
make node-r
```

`thv` на host устанавливать не нужно: pinned ToolHive `0.40.0` находится только
в `Dockerfile.toolhive`. Недоступный bridge остаётся поддержанным diagnostic
state: definitions сохраняются в Core, availability становится false с
`toolhive_missing`, direct Linear fallback отсутствует. Private bridge принимает
только `https://mcp.linear.app/mcp`, workload `uprava-linear`, fixed callback /
proxy ports и `tools/list`/`tools/call`. Desired state живёт в private Node state
и повторно reconciles после control-channel reconnect.

OAuth URL возвращается bridge через Node и Core только текущему Web request;
bridge валидирует HTTPS Linear host и не пишет URL в logs. Disconnect сначала
закрывает Core availability, затем Node просит ToolHive остановить и удалить
workload. Полная ручная приёмка описана в отдельном tooling runbook.

## Control Channel

After enrollment, Core asks Node to open `/api/v1/node/control` when pending
commands exist. Node connects outbound over WebSocket with its development
credential in the authorization header, sends a `control.hello` frame, receives
`control.hello_ack` after protocol compatibility is checked, then receives
`command.dispatch` frames and returns `command.ack`, `event_batch` and
`command.result` frames. If either side receives a control frame with an
unsupported protocol version, it replies with `control.error` using
`control.protocol_incompatible` and does not execute that command batch.

Для 0.2.0 это protocol v2 как единый coordinated breaking release для Core,
Node and Web. Compatibility с protocol-v1 API/schema/state не требуется, и
in-place migration 0.1.x отсутствует.

Core records node-routed commands before dispatch. The command envelope remains
stored as JSON for replay, and the command table also keeps queryable actor,
correlation, source refs, cause refs, payload and dedupe-key fields for
attribution, inspection and future idempotency checks.
Accepted events follow the same shape: Core stores the full event JSON and
queryable actor, scope, correlation, source/evidence/cause/result refs and
payload fields for ordered projections and inspection.

Codex provider path executes through the Node-side Provider Adapter boundary
for `StartRuntime`, `ResumeRuntime`, `SendTurn`, `ResolveApproval`,
`InterruptRuntime` and `StopRuntime`. Core persists accepted Node events and
rebuilds assistant messages from `provider.message.completed`. Core also
stores a durable `turns` row for every accepted user turn and advances that row
from `turn.started`, `approval.requested`, `turn.completed`,
`turn.interrupted` and runtime error events. Approval request and resolution
events are also projected into the durable `approvals` table. Node keeps
generated events in a local outbox until Core acknowledges their event ids, so
reconnects can replay unaccepted provider events without regenerating command
output.
Node command dedupe stores the terminal `command.result` status: normal event
batches complete the command, while provider/runtime error batches fail it and
replay the same failed status for duplicate command delivery.
The outbox is bounded; if retention is exceeded, Node drops the oldest unacked
events and emits a runtime-scoped `runtime.error` with
`node.event_outbox_retention_exceeded` so Core and UI see an explicit degraded
history condition instead of silent loss.

Node also keeps a small local runtime-state projection from its own emitted
events. Heartbeats report `active_runtime_count`, and `control.hello` reports
active runtime ids from that projection. `runtime.ready`, `runtime.running`,
`runtime.blocked`, `runtime.resuming` and interrupted runtimes count as active;
`runtime.stopped`, `runtime.error` and `runtime.expired` do not.

## Logs And Redaction

Use `RUST_LOG=info,uprava_server=debug,uprava_node=debug` for local diagnosis.
Core and Node write the same tracing stream to stderr and append local files by
default:

```sh
tail -f .local/logs/core.log
tail -f .local/logs/node.log
tail -f .local/logs/client.log
```

Override file locations when running outside the repository root:

```sh
export UPRAVA_CORE_LOG_FILE=/tmp/uprava-core.log
export UPRAVA_NODE_LOG_FILE=/tmp/uprava-node.log
export UPRAVA_CLIENT_LOG_FILE=/tmp/uprava-client.log
```

Web client installs global `error` and `unhandledrejection` handlers and logs
failed Core API calls plus session-stream errors to `POST /api/v1/client/logs`.
Core stores those records as JSONL in `UPRAVA_CLIENT_LOG_FILE`, including
browser route, user agent, client timestamp, source, level, message and bounded
diagnostic detail.

Core emits structured logs for enrollment create/approve/claim, heartbeat
acceptance, control-channel connect/disconnect, command record/dispatch/result,
event append, stream gaps and runtime-state changes. These logs use IDs,
states, counts and command `correlation_id` values; they intentionally avoid
command payloads, bearer tokens, node credentials and pairing codes.
HTTP command APIs use `x-correlation-id` when supplied, fall back to
`x-request-id`, and otherwise generate a fresh correlation id before recording
the command. Command-generated events copy that value, and Core backfills it
from `command_id` when accepting older Node event payloads.

Host-running Node logs the short-lived `enrollment_id` during first-run
enrollment so the operator can approve it in Core. The pairing code and
persistent development credential are kept only in the local Node state file
and are not logged. Node local-state debug formatting redacts both `credential`
and `pairing_code`.

Workspace validation is also routed through the control channel. Placement API
creates a pending placement and records a `ValidateWorkspace` command. Node
validates the path on the machine that owns the workspace, emits a
`workspace.validated` event scoped to the placement, and Core projects that
event into durable placement state and resource badges. Git workspaces also get
lightweight resource badges from `git status --porcelain=v1 --branch`,
including branch, dirty workspace, ahead and behind state when available. Core
adds a computed `same_workspace_active` warning badge when a workspace already
has live session work; the warning stays non-blocking in `0.1`.

Core runs command preflight before recording node-routed work. Validation,
session start, turn send, approval resolution, stop, interrupt and resume reject
revoked or offline nodes. Runtime start, turn send and resume also reject
placements that are not validated or have hard-blocking resource badges. The
runtime start, turn send, approval resolution and resume paths require the node
to advertise the selected `provider.*` capability. Heartbeats replace Core's
normalized `node_capabilities` snapshot, and provider routing reads that
snapshot while the public node API keeps the compatibility capability list.
Runtime-state preflight allows new turns only while the runtime is ready or
running, approval resolution only for a pending approval while the runtime is
blocked, interrupt only while running or blocked, resume only from stopped,
expired, stale, error or interrupted runtimes, and stop only before stopped or
expired. Agent
projection uses the same basic signals when advertising available commands, and
adds `node_stale`, `node_offline`, `node_revoked` or `provider_unavailable`
warnings when heartbeat or capability state requires it.

Placement resource snapshots can be refreshed with
`POST /api/v1/placements/{placement_id}/resource-snapshot/refresh`. Core records
a `RefreshResourceSnapshot` command, Node emits `resource.snapshot.updated` with
the current workspace state and resource badges, and Core projects that event
back onto the placement.

Resource and runtime warnings can be acknowledged through Core with
`POST /api/v1/sessions/{session_thread_id}/warnings/{warning_kind}/acknowledge`.
Core records a session-scoped `coordination.warning_acknowledged` event, stores
the acknowledgement in `warning_acknowledgements`, and removes that warning kind
from the session's active warning projection.

Session attachment is Core-only state. Use
`POST /api/v1/sessions/{session_thread_id}/detach` to mark a session detached
without stopping the runtime, and
`POST /api/v1/sessions/{session_thread_id}/attach` to reattach it. Detached
sessions remain readable and manageable, but Core rejects new turns and approval
responses until they are attached again. Healthy runtime activity does not
reattach a detached session; only the explicit attach endpoint does.

For deterministic local checks, send `/approval <prompt>` to emit an
approval request and block the runtime, or `/error <message>` to emit a runtime
error. Approval requests can be resolved through Core as `ResolveApproval`
commands.

## Codex Provider

Node can also run a minimal Codex adapter when a session is created with
`provider: "codex"`. Configure it with:

```sh
export UPRAVA_CODEX_BINARY=codex
export UPRAVA_CODEX_TIMEOUT_SECONDS=86400
```

Node advertises `provider.codex` as available only when `UPRAVA_CODEX_BINARY`
is either an existing path or resolves through `PATH`; otherwise Core preflight
can reject Codex session start or turn commands with a missing provider
capability instead of waiting for runtime execution to fail.

Adapter stores runtime provider, workspace path, bounded node-local transcript
and provider resume ref from `StartRuntime` or `RuntimeReady` in Node local
state. When no provider session id is known, `SendTurn` builds a prompt from
the recent local transcript plus the latest user turn, then runs:

```text
codex exec --cd <workspace_path> --json --output-last-message <temp_file> <turn>
```

When a bounded `provider_resume_ref` includes a Codex session id, `SendTurn`
uses the provider-native non-interactive resume path and sends only the latest
user turn:

```text
codex exec resume --json --output-last-message <temp_file> <session_id> <turn>
```

Node normalizes the result into `runtime.running`, `turn.started`,
`provider.output.delta`, `provider.message.completed`, `turn.completed` and
`runtime.ready`. JSON stdout events that look like provider approval or
user-input requests are normalized into `approval.requested` plus
`runtime.blocked` without storing raw provider payloads. JSON stdout session
or cursor identifiers are captured as a bounded `provider_resume_ref` and Core
persists that object on the runtime projection. `ResumeRuntime` carries the
persisted provider resume ref back to Node when available so the next turn can
use `codex exec resume`; without that ref, the adapter falls back to bounded
node-local transcript context. Missing binary, startup failure, timeout,
non-zero exit and empty final output map to `runtime.error` with user-safe
provider codes:
`provider.workspace_missing`, `provider.missing_binary`,
`provider.start_failed`, `provider.execution_timeout`, `provider.exec_failed` and
`provider.empty_output`.

This is the accepted V01 Codex protocol after the local CLI spike: exec-mode
adapter, not the final provider-native managed runtime. Пункт
[`16 Managed Agent Work Loop`](../product/feature-queue.md#16-managed-agent-work-loop)
закрыт baseline `0.2.25`: новые Agent sessions используют provider-native
Managed mode на capable Node. `codex exec/resume` сохранён как явный
unrestricted compatibility mode; настоящий Interrupt доступен только Managed.

When the host has an authenticated Codex CLI, run the host Codex smoke:

```sh
make codex-smoke
```

This starts a disposable Core on `127.0.0.1:18080`, Web on
`127.0.0.1:15173`, and a host Node with a temporary git workspace under
`/private/tmp`. It verifies the Managed API default and safe policy, performs
two turns around detach/attach and stop/resume, reloads the Web projection, and
then verifies a separate explicit Exec compatibility turn. Override
`CODEX_SMOKE_CORE_PORT`, `CODEX_SMOKE_WEB_PORT`, `CODEX_SMOKE_STATE_DIR`,
`CODEX_SMOKE_WORKSPACE_PATH`, `CODEX_SMOKE_CODEX_BINARY`,
`CODEX_SMOKE_TURN_CONTENT`, `CODEX_SMOKE_EXPECTED_ASSISTANT_CONTENT` or
`CODEX_SMOKE_CODEX_TIMEOUT_SECONDS` for a non-default host profile. The script
uses real Codex CLI auth/state and model access; keep it separate from
deterministic `make c` and Compose smoke checks.
Полная recovery/interaction checklist находится в
[`managed-agent-runtime.md`](managed-agent-runtime.md).

Core deduplicates replayed events by `event_id`. A conflicting `seq` is
rejected, and a detected sequence gap marks the session degraded and the
runtime stale with a degraded reason for UI/read-model consumers.

The session evidence projection and agent projection are rebuilt from Core
persistence. The projection includes current turn, pending approvals, active
warnings, recent message refs, available commands and a safe resume context.
Runtime-scoped events update `last_runtime_step_at`; healthy runtime events
such as `runtime.ready` clear degraded runtime/session read-model state.
Core expires ready, running, blocked or stale runtimes after
`UPRAVA_RUNTIME_EXPIRY_SECONDS` without runtime activity by recording a
system-authored `runtime.expired` event. Expired runtimes reject new turns but
remain resumable when the provider adapter supports resume.
The session SSE endpoint sends persisted historical events first and then keeps
the connection open for future accepted events through Core's in-process event
bus. If the stream falls behind the bounded bus, Core emits a `uprava.reload`
SSE event so the client can refetch the snapshot.

## Golden Path

1. Start Core and Web, then start a Node Daemon.
2. Open `http://127.0.0.1:5173`.
3. Open the reachable node.
4. Register a workspace placement with an explicit path.
5. Start a Codex session where the Codex CLI is available.
6. Send a turn and verify that user, assistant and runtime event blocks appear.
7. Stop or resume the runtime from the session header controls.
8. Reload the browser and verify that inventory, placement, session messages,
   evidence projection and agent projection reload from Core.

## Checks

```sh
make c
make dev-smoke
make web-e2e
make codex-smoke
```

`make dev-smoke` covers the deterministic Core/Web/ToolHive/Generated UI Builder infrastructure path through
Compose: hardened local auth setup/login and authenticated Core inventory
access plus pinned ToolHive version. It does not start a Node Daemon, perform
Linear OAuth or start a provider session.

`make web-e2e` starts the Vite web server when `PLAYWRIGHT_BASE_URL` is not set.
Set `PLAYWRIGHT_BASE_URL` to run the same checks against an already running
local profile.
Use `PLAYWRIGHT_BASE_URL=http://127.0.0.1:5173 make web-e2e` for automated
browser checks against the dev Compose Web service. Default E2E run uses mocked
Core snapshots for deterministic UI warning/degraded-state assertions. To run
the real Core/Web/Node browser path against a host Node with an available Codex
provider, run:

```sh
UPRAVA_E2E_REAL_API=1 \
PLAYWRIGHT_BASE_URL=http://127.0.0.1:5173 \
make web-e2e
```

For agent/operator inspection, run the dev Compose profile, start `make node-r`
when workspace access is needed, open `http://127.0.0.1:5173` with
`playwright-cli`, verify hardened profile banner, inventory tree,
workspace/session flow, warning/degraded states and inspector actions, then
collect `make dev-logs` output if a defect needs debugging.

`make codex-smoke` starts host Core/Web/Node with a disposable writable
workspace and runs both Managed default and explicit Exec compatibility paths.
Run it only where Codex CLI is installed and authenticated.

## Known Limits

- Managed app-server transport не обещает lossless replay provider activity
  при gap во время active turn; такой gap становится typed degraded recovery.
  Exec compatibility continuity остаётся bounded reconstruction/resume path и
  не используется как автоматический recovery Managed session.
- Warning acknowledgements are scoped by session and warning kind; future
  resource-specific acknowledgement expiry is not implemented yet.
- Node enrollment credentials are development credentials only; this remains a
  controlled-development profile.
- Host Node workspace summaries are still reported through heartbeat snapshots
  for paths in `UPRAVA_NODE_WORKSPACES`; explicit UI-created placement
  validation now runs through a Node `ValidateWorkspace` command.
- Workspace command execution is no-shell and limited to controlled-dev
  allow-list `cargo`, `git`, `make`, `node`, `npm`, `pnpm`, `bun` and `rustc`;
  другие executables отклоняются Node policy.
- Codex provider adapter is the V01 exec/resume mode with bounded local
  transcript continuity plus provider-native non-interactive resume when a
  provider session id is available. Persistent interactive process ownership,
  streaming output and real interrupt escalation are post-V01 work.
