# Релизы Uprava

Статус: `active`

Current release baseline: `0.2.26`.

Этот ledger фиксирует implementation baselines. Он не заменяет
[`feature-queue.md`](product/feature-queue.md), где остается ранжированная очередь
future work.

## Release Ledger

| Version | Date | Status | Completed Slice |
| --- | --- | --- | --- |
| `0.1.0` | 2026-07-06 | shipped | V01 Distributed Agent Control Panel |
| `0.1.1` | 2026-07-06 | shipped | Security baseline |
| `0.1.2` | 2026-07-06 | shipped | Runtime/session hardening |
| `0.1.3` | 2026-07-06 | shipped | Workspace shell and reference model |
| `0.1.4` | 2026-07-06 | shipped | Read-only Project Workspace Inspector |
| `0.1.5` | 2026-07-06 | shipped | Workspace intervention layer |
| `0.1.6` | 2026-07-06 | shipped | Unified audit hardening |
| `0.1.7` | 2026-07-06 | shipped | Workspace renderer and PTY terminal layer |
| `0.1.8` | 2026-07-08 | shipped | CI/CD deployment automation and self-hosted Codex execution posture |
| `0.2.0` | 2026-07-11 | shipped | Protocol v2 quality foundation, durable Core/Node state, workspace workbench and stable deployment paths |
| `0.2.1` | 2026-07-11 | shipped | Zarya 0.1 Web Control Panel alignment, flat work-sheet shell, system overview, agent work phases and visual regression gates |
| `0.2.2` | 2026-07-12 | shipped | Automatic main delivery, bounded CI workspaces, coordinated state epoch reset, scoped Node enrollment and functional production smoke |
| `0.2.3` | 2026-07-12 | shipped | Clean-bootstrap four-phase CI/CD, containerized prepare, explicit deploy/finalize boundaries and state-neutral delivery |
| `0.2.4` | 2026-07-12 | shipped | Отложенные сообщения в сессии: долговечные Core-owned одноразовые будущие turn с guarded dispatch |
| `0.2.5` | 2026-07-12 | shipped | Background Jobs и scheduled agent runs с наблюдаемыми per-run sessions и quota admission |
| `0.2.6` | 2026-07-13 | shipped | Workspace-centered Web UI: Node/Workspace navigation и workspace Agent, Workbench, Jobs surfaces |
| `0.2.7` | 2026-07-19 | shipped | Causality/Trace UX, raw event/ref resolution и isolated structured Deduction |
| `0.2.8` | 2026-07-19 | shipped | Модульные Core/Node runtime boundaries, capability-oriented tests и автоматический architecture gate |
| `0.2.9` | 2026-07-19 | shipped | Прозрачный agent timeline: Conversation/Trace modes, сгруппированные live-события и stalled activity state |
| `0.2.10` | 2026-07-19 | shipped | Git-aware Review: branch/worktree snapshots, scoped diffs, risk signals и traceable check results |
| `0.2.11` | 2026-07-19 | shipped | Agent Tooling and Tool Registry v1: progressive Uprava MCP, scoped registry, ToolHive-backed Linear integration и traceable execution |
| `0.2.12` | 2026-07-19 | shipped | Plugin Registry v1: Core-owned lifecycle, manifest-driven Web Extension Host и bundled data-only Dark Theme |
| `0.2.13` | 2026-07-20 | shipped | CI visual baseline и SQLite migration reliability: синхронизированные Linux golden snapshots, per-connection busy timeout и изолированный concurrent migration test |
| `0.2.14` | 2026-07-20 | shipped | ToolHive Compose topology: отдельный pinned runtime, bounded host Node bridge, persistent OAuth state, smoke и ручной Linear acceptance runbook |
| `0.2.15` | 2026-07-21 | shipped | Bundled Markdown renderer plugin: typed `visual.renderer` contribution, безопасный Streamdown для static/streaming chat content, lazy Web activation и plain-text fallback |
| `0.2.16` | 2026-07-21 | shipped | Plugin contribution resolution: normalized targets, deterministic exclusive chains, revisioned order/disable preferences, visible conflicts и bundled Markdown/Plain Text fallback chain |
| `0.2.17` | 2026-07-21 | shipped | Plugin-driven Visual Artifact System: generic versioned artifacts, source matchers, inline/block/artifact viewers и bundled color, diagram, review and trace plugins с mandatory fallbacks |
| `0.2.18` | 2026-07-21 | shipped | Dynamic UI from Agents: opt-in Generated React plugin, controlled builder, sandboxed iframe, Uprava UI SDK, persisted state, permissioned actions и safe fallbacks |
| `0.2.19` | 2026-07-21 | shipped | Task-based sandbox runtime mechanics: durable Task Runs, isolated Git worktrees, Docker/OpenSandbox execution, cancel/recovery, bounded evidence, Tasks UI и immutable Codex runtime image; credential profiles отложены |
| `0.2.20` | 2026-07-22 | shipped | Managed Agent Work Loop protocol gate: Codex app-server 0.144.1 spike, process-per-attempt architecture, scrubbed fixtures и measured approval/input/interrupt/reconnect/resume policy evidence |
| `0.2.21` | 2026-07-22 | shipped | Managed Agent Work Loop stage 1: shared profiles/attempts/interactions, immutable effective policy/hash, migration 18, Rust/Web fixtures и typed capability admission без fallback |
| `0.2.22` | 2026-07-22 | shipped | Managed Agent Work Loop stage 2: Node-owned Codex app-server supervisor, live semantic stream, interactions, interrupt/stop, resume descriptors и restart reconciliation |
| `0.2.23` | 2026-07-22 | shipped | Managed Agent Work Loop stage 3: Core policy preview/admission, durable interaction state machine, ordered projection, actual-state reconnect, recovery audit и metrics |
| `0.2.24` | 2026-07-22 | shipped | Managed Agent Work Loop stage 4: explicit profile/policy start UX, semantic timeline, typed interaction cards, runtime diagnostics и capability-aware lifecycle |
| `0.2.25` | 2026-07-22 | shipped | Managed Agent Work Loop stage 5: recovery/acceptance gate, capability-aware Managed default, explicit compatibility isolation и provider environment allowlist |
| `0.2.26` | 2026-07-22 | current | Managed Agent Work Loop deployment hardening: renewable process MCP lease, canonical workspace enforcement, bounded interrupt/stop, real idle teardown, provider version pinning и complete host acceptance |

## Current Baseline

`0.2.26` включает baseline `0.2.25`, protocol-v2 baseline `0.2.0`, завершённое Zarya 0.1 Web UI/UX
alignment и clean-bootstrap four-phase delivery path. Текущая реализация включает
первый working distributed
control panel, двадцать шесть implementation slices после `0.1.0`, unified audit
hardening slice и workspace
renderer/PTY terminal layer, а также первый deployable self-hosted release path:

- Start Agent выбирает Managed по умолчанию на capable Node, показывает target
  Node/workspace и effective policy preview; compatibility остаётся явным
  выбором и требует отдельного unrestricted acknowledgement;
- Agent surface постоянно показывает execution profile, sandbox/approval
  policy, policy hash, provider/driver version, current attempt, recovery и
  last activity; compatibility имеет persistent unsafe warning;
- semantic timeline различает bounded provider activity, provider approvals и
  questions. Typed cards используют отдельные Core endpoints, блокируют replay
  в `resolving` и сохраняются после reload через durable projection;
- lifecycle controls следуют projected capabilities: managed-only Interrupt не
  показывается для Exec compatibility, Detach не останавливает provider, Stop
  сохраняет session history, а Resume показывает policy/recovery context;
- Managed process MCP credential переиспользуется до refresh window и
  ротируется через replacement app-server + provider-native resume; первый
  turn больше не отзывает credential живого процесса; одноразовое имя
  secret-env и отключённый `shell_snapshot` защищают от cached credentials,
  а встроенный server не дублирует Core-owned tool approvals;
- Node канонизирует Managed workspace и immutable policy root перед spawn;
  interrupt доступен параллельно active socket loop, имеет bounded provider
  confirmation и TERM/KILL escalation; неизвестный provider callback даёт
  typed failure и teardown вместо бесконечного зависания;
- idle expiry записывает system StopRuntime, завершает Node process, отзывает
  lease и переводит pending interactions в expired; provider version из Node
  capability закрепляется в effective policy; SIGINT/SIGTERM наблюдаются на
  всём supervisor loop, поэтому service shutdown не оставляет app-server orphan;
- host acceptance автоматически проверяет approve, deny, user input,
  interrupt, MCP, stop/resume, reload и explicit Exec compatibility;
- Managed composer и Send Turn API имеют explicit `default | plan` mode; Plan
  прокидывает effective provider model в app-server и включает typed
  user-input interaction, каждый следующий turn явно восстанавливает Default,
  а Exec compatibility получает typed rejection;

- Core рассчитывает policy preview до start, атомарно сохраняет выбранный
  profile, immutable policy/hash, Start command и audit. Explicit Exec
  compatibility selection получает отдельный unsafe audit record;
- provider interaction проходит `requested -> resolving -> approved | denied |
  answered` либо terminal expiry/cancellation/supersession. HTTP acceptance и
  command intent атомарны, но только provider event завершает interaction и
  возвращает active turn/runtime в running;
- reconnect handshake несёт secret-free actual attempt identity, policy hash и
  state. Core принимает только current generation/attempt, не оживляет stale
  report и показывает отсутствующий managed process как provider-resumable;
- Node владеет process-per-attempt Codex app-server v2 supervisor: проверяет
  immutable policy/hash до запуска, сохраняет socket/process/request ids только
  in-memory, сериализует один active turn, нормализует bounded deltas/activity,
  approvals и user input и не выполняет silent Exec fallback;
- native `turn/interrupt`, scoped process-group stop и opaque thread resume
  reference сохраняют lifecycle конкретного attempt; после Node restart
  непроверяемый live descriptor становится explicit `lost/stale`, после чего
  допускается только отдельный provider-native Resume;
- deterministic fake app-server regression доказывает два turns в одном live
  thread, approval/input continuation и stop; существующий Exec compatibility
  и OpenSandbox Task paths остаются отдельными;
- shared Managed Agent contracts различают `managed` и `exec_compatibility`,
  `RuntimeSession` и конкретный `RuntimeAttempt`, approval и user-input
  interactions; Core сохраняет immutable effective policy JSON/hash, recovery
  projection и attempt/interaction lifecycle через migration 18;
- Core допускает managed start только при полном наборе раздельных Node
  capabilities и возвращает typed unavailable reason без Exec fallback;
  отсутствие profile выбирает Managed, existing sessions сохраняют stored
  profile, а Jobs продолжают выбирать `exec_compatibility` явно;
- provider protocol gate для Managed Agent Work Loop выбирает experimental
  Codex app-server v2 из `codex-cli 0.144.1` over loopback WebSocket;
  disposable Rust probe подтверждает два live turns, typed activity,
  approval/input continuation, interrupt, reconnect/resume, safe/unrestricted
  policy echo, Uprava-shaped MCP bearer boundary and process recovery;
- каноническая модель разделяет Core-owned `SessionThread`/`RuntimeSession` и
  Node-owned process-per-attempt `RuntimeAttempt`; `0.2.25` закрывает stages
  0–5, ограничивает provider child environment allowlist и добавляет host-only
  acceptance для Managed default, recovery и explicit compatibility;
- RuntimeStopped атомарно закрывает current attempt и pending interactions;
  Managed resume получает MCP lease, scoped к non-terminal Resume command, а
  SQLite lease/event writes резервируют writer до read snapshot, не оставляя
  recovery или compatibility runtime без terminal projection;

- Core хранит отдельный `TaskRun` и dispatch-ит его на capability-compatible
  Node без создания interactive session; Node создаёт linked Git worktree,
  управляет pinned OpenSandbox Docker lifecycle и возвращает summary, bounded
  diff, checks, hashes artifacts, terminal reason и независимый cleanup state;
- Tasks surface создаёт, отслеживает и отменяет bounded runs. Node сохраняет
  transient run-to-sandbox mappings для restart cleanup, а release manifest
  digest-pin-ит Codex task image и third-party OpenSandbox server image.
  OpenSandbox API key и persistent Codex
  credential profile намеренно отложены и остаются manual acceptance debt;

- opt-in bundled `uprava.generated-react` публикует Generated UI runtime, SDK,
  action bridge, artifact type and viewer contributions через общий target
  resolver; native `uprava.dynamic_ui.create` даёт агенту session-scoped path;
- migration 16 хранит content-addressed TypeScript and bundles, atomic artifact
  build/state records и idempotent action requests; controlled builder имеет
  strict import/size policy, dependency lock и отдельный network-isolated
  container в dev and production topology;
- Web исполняет готовый bundle в opaque sandboxed iframe с nonce CSP и
  MessageChannel. Persisted state, send-agent-input, open-reference and layout
  requests проходят bounded protocol and Core authorization; PNG/WebP snapshot,
  markdown fallback, diagnostics and source review остаются при любой ошибке;

- Core хранит generic artifact identity и immutable versions с type, scope,
  source/evidence/cause/trace refs, provenance и обязательным readable fallback;
  migration 15 сохраняет совместимость с существующими causality narratives;
- `visual.renderer` v2 и `artifact.type` v1 проходят manifest validation,
  compatibility, permissions и общий exclusive target resolver, включая
  пользовательский order/disable для конфликтующих artifact types;
- bundled `uprava.content-enhancements`, `uprava.diagrams`,
  `uprava.review-artifacts` и `uprava.trace-artifacts` лениво активируют color
  tokens, Mermaid/ограниченный PlantUML, timeline/review blocks и artifact
  viewers; diagram, diff/check и trace snapshots можно закрепить и открыть по
  стабильной artifact reference, а disable/render failure сохраняет source или
  raw fallback;

- Linux visual regression baselines синхронизированы с pinned Playwright CI
  environment; SQLite busy timeout применяется к каждой Core connection, а
  concurrent migration test проверяет migration boundary без конкурирующего
  application bootstrap;

- Core-owned Plugin Registry отделён от Tool Registry и хранит immutable package
  metadata, installation state, compatibility, configuration revisions,
  permission grants и effective contribution projection;
- bundled trusted plugin `uprava.markdown` публикует typed `visual.renderer`
  contribution и безопасно обогащает assistant messages через Streamdown;
  renderer поддерживает static и streaming content, лениво загружается только
  для подходящего target после effective activation и остаётся default winner;
- target-based resolver публикует contribution provenance, детерминированный
  order и exclusive conflict metadata; Plugin Panel сохраняет per-target order
  and disable preferences, а bundled `uprava.plain-text` служит управляемой
  alternative перед обязательным raw fallback;
- manifest-driven Web Extension Host применяет только валидированные typed
  `ui.theme` и `visual.renderer` contributions; bundled data-only
  `uprava.theme-dark` переключает semantic tokens, Monaco и xterm с безопасным
  `core.light` fallback;
- Core-owned Tool Registry, permission-first progressive discovery
  `Search -> Inspect -> Execute` и session-scoped Uprava MCP leases;
- Node-owned observed capability inventory and desired/actual reconciliation;
  pinned ToolHive runtime вынесен в отдельный Compose service с bounded private
  bridge к host Node;
- Web management для integration lifecycle, availability и redacted tool-call
  trace; Linear OAuth URL передаётся только эфемерно и не сохраняется в durable
  state, audit или logs;
- Linear OAuth callback, discovery, read-only call и disconnect/reconnect
  подготовлены к ручной opt-in приёмке, но пока не являются подтверждённым E2E;
  `remote_revocation_confirmed = false` остаётся консервативным без upstream proof;

- coarse session trace с precision markers и типизированными
  source/evidence/cause/result/raw links;
- глобальный cursor-based event log, raw event detail и server-side reference
  resolver с явными unavailable states;
- workspace file/command/check/diff causality events, сохраняемые через
  Core-owned event log;
- isolated Deduction execution без resume live session, в ephemeral read-only
  Codex process со structured output schema;
- bounded evidence snapshots, Core-side ref allowlist/provenance validation,
  raw fallback, cancellation и explicit persistence в versioned
  `CausalityNarrative`;
- Web trace, aspect-based Context Inspector, raw event log и Deduction panel.
- Conversation и Trace разделены на URL-addressable режимы одной session
  surface; runtime bootstrap и события каждого agent turn сгруппированы в
  компактные раскрываемые блоки.
- Provider activity поступает в Core во время выполнения, а не только после
  завершения process; отсутствие новых событий у running turn становится
  видимым stalled attention state.
- Session SSE применяется как push-first read-model stream: каждый event
  немедленно обновляет session timeline, inventory summary, открытые trace,
  evidence и raw-event caches без каскада snapshot GET. Полные server
  projections остаются bootstrap/recovery boundary и обновляются только на
  значимых lifecycle/message границах.
- Public ingress разделяет authenticated UI, Node, stream, auth, enrollment и
  client-log buckets; UI/global budgets конфигурируются environment и имеют
  controlled-development defaults `600/5000` на минутное окно.
- Trace остаётся session-wide подробной летописью, Deduction и raw payload
  убраны на дополнительный уровень раскрытия, а reference actions используют
  явные inspect/copy controls.
- Core и Node composition roots отделены от application, transport,
  persistence, workspace, terminal и provider modules; прежние runtime и test
  монолиты разложены по capability boundaries без изменения protocol или state
  schema.
- Runtime architecture gate ограничивает рост composition roots и запрещает
  transport/process dependencies внутри persistence; logging policy рекурсивно
  проверяет все production-модули после декомпозиции.
- Node-owned Git snapshot различает branch, detached/unborn HEAD, primary и
  linked worktree, локальный upstream drift, staged/unstaged/untracked/conflict
  state и выполняющуюся Git operation; Core сохраняет snapshot на Placement и
  показывает warning для active runtime на том же repo/branch.
- Workspace Review предоставляет bounded `all`/`staged`/`unstaged` diff,
  per-file Monaco/raw preview, стабильные `WorkspaceDiff`/`DiffHunk` refs и
  долговечную историю traceable `make l`/`make c` check results.

- controlled-development security baseline;
- runtime/session hardening;
- stable workspace/reference model;
- read-only Project Workspace Inspector;
- workspace intervention layer с text save, bounded command runner, command
  history and diff/check entry points.
- daily-use hardening and deployment readiness для sustained-use Web workbench
  и controlled self-hosted delivery path.
- quality gate honesty and Rust `1.88` MSRV alignment;
- Node allow-list enforcement, atomic local state writes, no-follow workspace
  writes and bounded command output during execution;
- ACK-after-reconnect command redispatch and session projection cursors для
  cross-scope event streams;
- visible web error states, send draft preservation and terminal enrollment
  status handling;
- healthcheck and logging failure hardening.
- Monaco-backed file editing and diff rendering в Web Control Panel;
- Core workspace terminal APIs для open, list, attach/stream, resize, input and
  close;
- Node Daemon PTY lifecycle management с workspace cwd enforcement,
  shell-profile policy, resize/input handling, status/exit frames and bounded
  replay;
- xterm.js terminal tabs with WebSocket attach, fit resize and interactive
  input/output streaming;
- retained bounded command runner для traceable controlled checks вроде
  `make l` and `make c`;
- GitHub Actions CI/CD для Core/Web/Generated UI Builder images, Node artifact publishing, release
  manifest generation and deploy activation;
- server Make targets and scripts для release manifests, host Node artifact
  extraction, activation and deploy;
- self-hosted Codex adapter launch flags для noninteractive execution на
  production server, где effective boundary остается Unix user `uprava`,
  workspace allow-list and deployment ACLs.
- six-token monochrome foundation Web Control Panel с square geometry,
  semantic risk/notice roles и static enforcement против legacy raw styles;
- flat App Shell, Dashboard system overview and runtime pipeline, phased
  Session/Agent Chat, 2.5D source/evidence disclosure и согласованный chrome
  Workspace Inspector;
- component, keyboard and responsive visual regression coverage для Zarya UI
  vocabulary.
- automatic immutable release publication и production activation после
  успешного обновления `main`;
- bounded per-job CI workspaces с unconditional cleanup, orphaned-workspace GC и
  free-space preflight;
- digest-pinned Core/Web/Generated UI Builder/Node manifests, temporary registry credentials и
  project-scoped image/release retention;
- phase boundaries GitHub Actions `prepare -> build -> deploy -> finalize` с
  immutable manifest artifact handoff;
- containerized source checks без Docker socket и host-only execution для
  build, deploy и finalize;
- clean host bootstrap из root-owned inputs `/etc/uprava`, state-neutral
  ordinary deploys и read-only interface Node readiness;
- отдельный health/SHA/heartbeat/version finalize и bounded Uprava-only
  retention releases, images и stale runner workspaces.
- Core-owned записи отложенных сообщений с UTC due time и явной IANA timezone,
  lifecycle `scheduled -> sending -> sent | failed | cancelled`, списком и UI
  внутри сессии, edit/reschedule/send-now/cancel/retry и typed guard failures;
  отправка в срок использует обычный send-turn admission path.
- Core-owned paused Job definitions и наблюдаемые Job Runs с сохранёнными
  snapshots effective configuration, manual starts и interval/daily/weekly IANA
  schedules;
- atomic due-occurrence claim, restart-safe session correlation, один active run
  на Job с typed overlap skips и stop-on-error по умолчанию;
- отдельные обычные managed sessions для Job Runs, final provider message как
  summary, typed failures и ссылки на полный session output/evidence;
- общая provider-quota admission для chat/Job с порогом 5%, audited force
  override и честным `unknown`, когда Codex не даёт надёжного usage source;
- Web surfaces Job list/detail/run для create/edit, schedule control, manual
  tests, force override, history, summaries и configuration snapshots.
- workspace-centered navigation с единственным глобальным Dashboard, деревом
  `Nodes -> Workspaces`, агрегированным Node Overview и независимым sidebar
  toggle;
- canonical workspace routes и compatibility redirects для session, Job и Job
  Run deep links с ownership guards и сохранением Inspector query;
- workspace Agent с session list, start flow и прежним lifecycle, Workbench с
  IDE-like grid `file tree | editor/diff` над PTY terminal и workspace-scoped
  Jobs list/create/detail/run;
- условный Context Inspector, который не резервирует колонку без reference и
  переходит в drawer на узком desktop;
- четыре Dashboard metrics, честная Recent Activity projection, единые status
  dimensions и unit/component/Playwright golden coverage desktop, narrow и
  mobile regression states.

Новые аудиты и temporary plans должны считать это фактами текущей реализации.
Они могут ссылаться на `V01`, когда обсуждают исторический первый продуктовый
срез.
