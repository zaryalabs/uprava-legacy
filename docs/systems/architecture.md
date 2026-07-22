# Uprava Architecture

## Public invariants и compatibility версии 0.2.0

- Core — единственный authority для resource identity, legal state transitions,
  command state и projected event state; publication выполняется только после
  commit durable transaction.
- Node — единственный authority для local SQLite state, workspaces, provider
  processes и PTY; bounded owner tasks сохраняют progress до ACK/send и
  завершаются через join при shutdown.
- HTTP failures используют typed envelope `ApiError` и `x-correlation-id`;
  control failures возвращают typed protocol results вместо free-form success.
- Protocol v2, Core state slot `0.2.0` и Node state slot `0.2.0` образуют одну
  coordinated breaking boundary. Версия 0.2.0 не открывает retained state 0.1.8
  in place; rollback одновременно выбирает matching binary, config и state.
- Core владеет durable Background Job schedules и atomic occurrence claim.
  Каждый Job Run сохраняет snapshot effective configuration и использует
  отдельную обычную placement-bound session/runtime; Job не представляется
  бессмертным provider process.
- Scheduled overlap сохраняется как typed skipped Job Run. Failed start или
  turn ставят automatic scheduling на паузу, если Job явно не включает
  continue-after-error.

## Границы модулей реализации 0.2.10

Core и Node используют небольшие `runtime.rs` только как composition roots.
Они собирают зависимости, запускают owner tasks и recovery loops, но не
являются местом для накопления transport, persistence, workspace или provider
реализации.

Core разделён следующим образом:

- `runtime/transport/` — Axum router, HTTP/control ingress, process-local
  connection registry, command waiters и terminal fan-out;
- `runtime/application/` — capability-oriented orchestration sessions,
  workspace, scheduling, projections и distributed coordination;
- `persistence/` — numbered migrations и durability-heavy command, event,
  enrollment, heartbeat и placement operations;
- `runtime/support.rs` — единый error/serialization/security boundary.

Node разделён следующим образом:

- `runtime/persistence/state.rs` — единственный owner локального versioned state
  и SQLite actor;
- `runtime/transport/` — outbound enrollment, heartbeat и control channel;
- `runtime/application/` — bounded dispatch, cancellation, execution и event
  outbox;
- `runtime/workspace.rs`, `terminal.rs` и `provider.rs` — отдельные data-plane
  boundaries для filesystem/process, PTY и provider adapter.

Feature-local orchestration может оставаться рядом с транзакцией, если
искусственное выделение универсального repository разорвало бы один durable
use case. При этом persistence modules не импортируют Axum/WebSocket, а Node
state persistence не импортирует HTTP, control socket, PTY или provider
execution. Инвариант `event -> projection -> publication outbox` остаётся одной
Core-owned транзакцией. Эти правила, наличие модулей и предел размера
composition roots проверяет `scripts/check_runtime_boundaries.py`.

Статус: `active`

Этот документ фиксирует первую архитектурную позицию по client/server модели Uprava.

## Короткое решение

Uprava должен иметь отдельный **Core Backend** как control plane. Клиенты работают через Core, а работа с конкретными машинами, файлами, терминалами, процессами, sandbox и AI-agent lifecycle выполняется через **Node Daemons**.

Важно: Core - обязательная архитектурная абстракция, но не обязательно удаленный SaaS backend в каждом deployment. В локальном режиме Core может запускаться на той же машине, что UI и Node Daemon.

```text
Clients
web / desktop / mobile / CLI
        |
        v
Core Backend / Control Plane
API, web control panel, auth, discovery, registry, workflows, events, artifacts
        |
        v
Node Daemons / Data Plane
files, terminal, processes, sandboxes, agent lifecycle, local capabilities
        |
        v
AI Agents / Tools / Workspaces
persistent sessions, task runs, hybrid flows
```

## Термины

### Core Backend

Центральный backend и control plane Uprava.

Core отвечает за глобальную модель системы: проекты, пользователей, права, ноды, capabilities, agent sessions, agent runs, workflows, artifacts, event log, trace, tool registry, routing и web control panel.

Core не должен становиться процессом, который сам напрямую работает с файловыми системами всех машин. Работа с конкретным окружением должна оставаться на стороне Node Daemon.

### Control Panel

Web UI, который разворачивается рядом с Core Backend и дает доступ к управлению Uprava из браузера.

На раннем этапе это может быть основной клиент. Позже рядом могут появиться desktop, mobile and CLI clients.

### Client

Пользовательский интерфейс к Core.

Типы клиентов:

- web;
- desktop;
- mobile;
- CLI.

Клиент не должен быть обязан напрямую подключаться к каждой ноде. Базовая модель: клиент общается с Core, Core маршрутизирует команды, события и состояние между клиентом и нодами.

### Node

Зарегистрированное вычислительное окружение, где может выполняться работа.

Node может быть:

- локальным компьютером;
- сервером;
- devbox;
- cloud workspace;
- sandbox;
- microVM host;
- будущей managed cloud node.

Термин `host` можно использовать как техническое пояснение, но продуктовая сущность лучше называется `Node`, потому что Uprava ближе к distributed/cloud модели, чем к простому списку машин.

### Node Daemon

Системный демон, запущенный на Node.

Это не AI-агент. Node Daemon - инфраструктурный процесс, который:

- регистрирует Node в Core;
- сообщает capabilities;
- запускает и останавливает AI agents;
- управляет persistent agent sessions;
- выполняет task-based runs в sandbox/workspace;
- дает доступ к файлам;
- открывает terminal/PTY или command execution;
- стримит логи, events and outputs;
- применяет изменения;
- запускает checks/tests;
- управляет локальными workspace, env, credentials and runtime limits.

Node Daemon - основной data plane Uprava.

### AI Agent

AI-agent workload, который запускается через Node Daemon или подключается как внешний provider.

AI Agent может работать в разных execution modes:

- provider-native managed Agent session;
- Agent exec/resume compatibility session;
- task-based sandbox run;
- sessionless one-shot Job Run.

Опциональная будущая композиция позволяет managed Agent запускать bounded
`TaskRun`, но она не является отдельным основным режимом и не определяет
контракт живой Agent surface.

## Почему Core Backend нужен

### Discovery

Node Daemon должен регистрироваться в одном месте:

```text
node id
status / heartbeat
available projects
available agents
available tools
runtime capabilities
sandbox capabilities
security limits
```

Без Core каждый клиент должен был бы сам искать ноды, держать соединения, понимать capabilities and синхронизировать состояние. Это быстро ломается на mobile, distributed and team scenarios.

### Mobile and web access

Телефон или браузер не должны напрямую подключаться к ноутбуку, devbox or sandbox. Им нужен стабильный endpoint.

Core дает этот endpoint и позволяет:

- открыть web UI;
- посмотреть состояние задач;
- подключиться к agent session;
- прочитать trace;
- посмотреть diff/artifacts;
- принять review decision;
- остановить or продолжить работу.

### Workflow state

Task-based режим, future Agent-to-Task delegation, CI callbacks and long-running
work требуют долговечного состояния:

- что было запущено;
- где работа остановилась;
- какие checks прошли;
- какой webhook пришел;
- какой следующий step;
- кто должен принять решение.

Это состояние должно жить в Core, а не в конкретном клиенте.

### Trace and event log

Traceability должна быть общей для всех клиентов и нод. Core хранит event log, trace metadata, artifacts metadata, review decisions and workflow state.

Node Daemon может хранить локальные raw logs или большие файлы, но Core должен знать, что существует, где лежит, кто имеет доступ и как это связано с workflow.

### Security and permissions

Core должен быть местом, где принимаются и проверяются системные решения:

- кто видит project;
- кто видит Node;
- кто может открыть terminal;
- кто может запускать agent run;
- кто может использовать tool;
- кто может читать artifact;
- кто может принять diff;
- кто может остановить or удалить session.

Node Daemon должен enforce локальные ограничения, но policy и routing должны быть согласованы через Core.

## Agent Tooling and Tool Registry

Tool Registry должен жить в Core.

Причина: tools являются частью общей системы capabilities, permissions, UI, trace and routing. Если registry будет только на нодах или клиентах, Core не сможет нормально отвечать на вопросы:

- какие tools доступны в проекте;
- какие tools доступны на конкретной Node;
- какие tools разрешены конкретному пользователю or агенту;
- какие tools можно показать в UI;
- как tool отображается как visual block or artifact;
- какие вызовы tools нужно трассировать;
- куда маршрутизировать tool call;
- какие schemas, permissions and risk levels у tool.

Registry различает два класса:

```text
managed tools
  Uprava-native MCP tools и внешние MCP integrations под Core policy

observed capabilities
  provider-native и Node-local tools, наличие которых Core может показывать,
  но execution contract которых не принадлежит Uprava
```

`bash`, file tools, `git`, `gh`, `glab` и похожие средства не нужно
дублировать или проксировать через Uprava. Node сообщает их version, health and
safe authentication status, Core учитывает Node/project/session scope, а агент
вызывает native CLI напрямую.

Выполнение managed tool не обязано происходить в Core.

Модель:

```text
Core Tool Registry
metadata, schema, permissions, effective availability, routing, audit policy

Uprava MCP
agent-facing Search -> Inspect -> Execute

Node Tool Runtime
host-local execution, capability inventory, workspace context, reconciliation

ToolHive MCP Runtime
отдельный Compose service: external MCP lifecycle, OAuth, discovery, proxying

External Tool Provider
Linear, Notion, Atlassian/Jira, Grafana and other MCP-backed systems
```

Core знает, что tool существует, кому и в каком scope он доступен, где его
исполнять и как связать вызов с trace. Bare-metal Node не содержит и не
запускает `thv`: он управляет отдельным ToolHive service через закрытый
loopback HTTP bridge. Codex остаётся host dependency для interactive Persistent
Runtime; bounded sandbox runs используют Codex из versioned runtime image.
ToolHive или external provider выполняют действие там, где находятся
credentials and runtime.

Реализованный management baseline показывает человеку integration
desired/auth/actual state, effective availability, managed Inspect detail,
observed native inventory и redacted recent calls. Primary Codex adapter
получает session-scoped Uprava MCP lease через authenticated Node transport
непосредственно перед turn и передаёт credential процессу только через
environment. Durable command, provider arguments, prompt and transcript не
содержат lease. Linear Connect/Reconnect возвращает authorization URL только
эфемерному Web-запросу; heartbeat подтверждает actual runtime state, не
подменяя его optimistic UI состоянием.

### Uprava MCP

MCP является основным machine interface агента как first-class citizen к
Uprava-native capabilities и внешним integrations. Внутренний source of truth
остаётся в Core domain/API/command contracts; MCP является управляемой
agent-facing проекцией этих contracts, а не отдельной authority boundary.

Отдельный Uprava CLI не входит в первый Agent Tooling slice. Он нужен только
если появятся подтверждённые shell-composition, streaming or batch scenarios,
для которых MCP неудобен.

### Progressive tool discovery

Полный tool catalog не должен попадать в model context. Uprava MCP обязательно
следует трёхуровневой модели:

```text
Search
  query + scope -> tool name/id + one-line description

Inspect
  tool id -> full schema, docs, permissions, risk, source and availability

Execute
  tool id + arguments -> fresh authorization, validation, routing and result
```

Минимальная стабильная agent-facing поверхность:

```text
search_tools(query, filters?, cursor?)
inspect_tool(tool_id)
execute_tool(tool_id, arguments)
```

Core/host может получать полный upstream `tools/list`, но использует его только
для policy-filtered index, ranking and schema cache. Full schema раскрывается
только после Inspect. Execute не доверяет предыдущему Inspect и заново проверяет
schema, permission, availability and approval policy. Upstream
`tools/list_changed` обновляет index and effective availability.

Provider-specific dynamic tool mounting допустим после Inspect, если host умеет
добавлять definition безопасно. Stable `execute_tool` остаётся
provider-neutral fallback.

### ToolHive boundary

ToolHive является обязательным external MCP runtime provider первого среза и
отвечает за server lifecycle, discovery, grouping/aggregation, proxying, health
and runtime-level audit.

В локальной топологии ToolHive — отдельный Compose service с pinned CLI
`0.40.0`, собственным persistent XDG volume и доступом к container runtime.
Внешне публикуются только loopback bridge `127.0.0.1:18081` и OAuth callback
`127.0.0.1:18765`. MCP proxy остаётся внутри service. Core и Web к bridge не
обращаются; единственный клиент — host Node через `UPRAVA_TOOLHIVE_URL`.

ToolHive не заменяет Core. Uprava продолжает владеть:

- product-level Tool Registry;
- Node/project/session scope;
- permissions and approvals;
- routing decisions;
- tool-call trace, causality and safe audit metadata;
- integration configuration and UI exposure.

### Task sandbox runtime boundary

Первый task-based sandbox backend использует отдельный OpenSandbox service с
обычным Docker runtime. Core не обращается к нему напрямую: Node создаёт
host-side worktree, вызывает lifecycle and execution OpenAPI, переводит
upstream stream/status в Uprava events и собирает git/check/artifact evidence.
OpenSandbox владеет только container lifecycle, TTL, mounts, limits and
in-container command transport.

Rust Node интегрируется с service напрямую по private HTTP contract через
replaceable `TaskRuntimeBackend`; дополнительный JS/Python runner или локальный
самописный Docker orchestrator не входят в baseline. Codex CLI и общие tools
поставляются custom versioned image. Baseline `0.2.19` реализует runtime,
worktree, lifecycle и evidence mechanics; read-write cached login из persistent
host credential profile и OpenSandbox API key намеренно отложены и не
запекаются в image.

Полный scope, lifecycle and spike criteria определены в
[`A-013 Task-based Sandbox Runtime`](areas/013-task-based-sandbox-runtime.md).

## Plugins and Integrations

Плагины и интеграции - один из главных механизмов модульности Uprava.

Uprava не должен реализовывать все внешние системы сам. Вместо этого Core должен иметь расширяемую модель, через которую можно подключать:

- task trackers: Linear, Jira, GitHub Issues;
- knowledge and docs systems: Notion, Obsidian-like repos, Google Docs;
- git providers: GitHub, GitLab;
- observability and dashboards: Grafana, LangSmith, Langfuse, OpenTelemetry, Phoenix;
- runtimes and infrastructure: Docker, sandbox providers, devboxes, Kubernetes-like environments;
- ML/experiment systems: MLflow and similar tools;
- custom internal company tools;
- MCP servers.

### Plugin Registry

Plugin Registry должен жить в Core рядом с Tool Registry.

Tool Registry и Plugin Registry являются разными authority boundaries:

```text
Tool Registry
  реестр callable capabilities, schemas, routing, availability and trace

Plugin Registry
  реестр package-level extensions самой Uprava and their contributions
```

Plugin может не предоставлять ни одного tool. Theme, renderer, Workbench view,
Inspector aspect or link handler остается полноценным plugin. И наоборот,
external MCP integration может дать tools без права менять UI shell.

Plugin Registry отвечает за:

- installed plugins;
- plugin versions;
- plugin configuration;
- package provenance, trust and compatibility;
- enable/disable and activation lifecycle;
- manifest-driven UI contributions;
- themes, commands, views, actions, renderers, Inspector aspects and link
  handlers;
- exposed tools;
- visual blocks;
- artifact types;
- workflow templates;
- permissions requested by plugin;
- integration accounts/connections;
- compatibility with Core and Node Daemon versions.

Tool Registry отвечает за конкретные callable capabilities. Plugin Registry
отвечает за package-level extension: какие extension points пакет расширяет,
откуда contribution пришел, как он активируется, конфигурируется, отключается
and обновляется. Связь plugin с tool является явной ссылкой между registries, а
не объединением их моделей.

Core хранит packages, installations, permissions and effective contribution
projection. Web Extension Host монтирует только разрешенные contributions в
известные typed surfaces. Node Plugin Runtime появляется только для plugins,
которым действительно нужен local execution рядом с workspace or credentials.

Если несколько contributions воздействуют на один target, их разрешение не
зависит от порядка загрузки. Каждый extension point задаёт bounded target и
режим `exclusive` или `ordered`; Host применяет стабильный пользовательски
изменяемый порядок и показывает одинаковые exclusive targets как конфликты в
Plugin Panel. Общий минимальный contract определён в
[`A-012 Plugin Contribution Resolution`](areas/012-plugin-contribution-resolution.md).

Первый slice использует bundled data-only Dark Theme. Arbitrary plugin
JavaScript, global CSS injection and direct DOM mutation не допускаются;
последующие executable plugins требуют trusted bundled boundary или отдельный
sandbox.

Последующие функциональные UI-направления развиваются plugin-first. Visual
Artifact System и Dynamic UI from Agents поставляются как bundled first-party
plugins поверх общих versioned contributions, а base system владеет только
generic contracts, lifecycle, persistence, permissions, isolation and
fallback. Каждый slice обязан не только реализовать пользовательскую функцию,
но и расширить переиспользуемый Plugin Registry/Extension Host API. Bundled
plugin не получает скрытого hardcoded пути: enable/disable, compatibility,
effective projection and failure fallback проходят через общую package model,
приближая платформу к extension ecosystems Obsidian и VS Code.

Dynamic UI использует opt-in Generated React artifacts как основной
expressive path. Generated code не монтируется в main Web React tree: bundled
plugin регистрирует controlled build, sandboxed iframe runtime, Uprava React
SDK, layout contract and permissioned action bridge. Optional declarative blocks
остаются fast path для простых interactions, а не закрытым UI language.

Baseline `0.2.18` реализует этот путь через disabled-by-default
`uprava.generated-react`, Core-owned content-addressed source/build/state/action
lifecycle и отдельный internal-network builder service. Web получает только
готовый bundle и исполняет его в opaque iframe; MessageChannel requests не
обходят Core permissions, idempotency, confirmation or event trace.

Полный contribution, activation, trust and theme contract определен в
[`A-004 Modular UI and Work Surface`](areas/004-modular-ui-work-surface.md#plugin-registry-и-extension-host).

### Integration adapters

Интеграция может подключаться разными способами:

- **MCP adapter** - основной agent-facing путь для Uprava-native tools и внешних integrations.
- **Native API adapter** - если нужен контроль над auth, pagination, webhooks, rate limits, domain objects or visual UX.
- **Node-local adapter** - если tool должен выполняться рядом с файлами, терминалом, локальными credentials or runtime.
- **External provider adapter** - если tool исполняется во внешнем SaaS/provider.
- **Hybrid adapter** - metadata and permissions живут в Core, execution идет через Node or external provider.

MCP является основным agent-facing protocol, но execution backend не обязан
быть MCP. Для Uprava важно не только вызвать tool, но и:

- показать его в UI;
- трассировать вызовы;
- связать результат с artifact/workflow;
- применить permissions;
- поддержать review;
- встроить визуализации;
- сделать результат понятным человеку and агенту.

### Integration contract

Каждая интеграция должна описывать:

```text
identity
capabilities
tool schemas
auth requirements
permission scopes
risk level
routing target
execution location
event/audit policy
artifact types
visual blocks
workflow hooks
```

Это нужно, чтобы интеграции были first-class частью системы, а не набором скрытых API calls за текстовым ответом агента.

## Разделение ответственности

### Core Backend отвечает за

- API for clients;
- web control panel;
- auth and user/session management;
- projects;
- Node registry and discovery;
- Node capabilities;
- agent session/run registry;
- workflow state;
- event log;
- trace metadata;
- artifact metadata;
- Tool Registry;
- Plugin Registry;
- integration registry and configuration;
- MCP search/inspect/execute index and gateway policy;
- permissions and policies;
- routing commands to Node Daemons;
- webhooks from GitHub/GitLab/Linear/CI;
- review queue and decisions;
- global dashboards.

### Node Daemon отвечает за

- Node registration and heartbeat;
- local capability probing;
- native CLI capability inventory without proxying execution;
- ToolHive-backed MCP runtime lifecycle and actual-state reporting;
- workspace management;
- file access;
- terminal/PTY;
- process lifecycle;
- provider-native managed Agent sessions;
- Agent exec/resume compatibility executions;
- sessionless Job provider executions;
- task-based sandbox runs;
- sandbox/microVM integration;
- local tool execution;
- local logs and output streaming;
- checks/tests execution;
- local resource limits;
- local secret/env access;
- applying patches and file changes.

Provider protocol gate `0.2.20` уточняет managed boundary. Node запускает один
loopback-only Codex app-server process на `RuntimeAttempt`, владеет endpoint,
live request correlation, cancellation, process group and provider secrets и
нормализует ordered events до отправки в Core. Core хранит `SessionThread`,
`RuntimeSession`, immutable effective policy and bounded opaque provider resume
reference, но не process/socket/request ids. Потеря managed process создаёт
новый attempt и explicit provider resume or typed degraded recovery; она
никогда не переключает runtime на Exec compatibility молча.

Foundation `0.2.21` материализует эту границу в shared Rust/Web contracts и
Core SQLite migration 18. Execution profile, policy snapshot/hash,
RuntimeAttempt identity, recovery и pending provider interactions входят в
session read model; capability admission остаётся Core-owned. До появления
Node-managed driver baseline `0.2.22` реализует process-per-attempt supervisor,
policy/hash gate, typed provider interactions, native interrupt/stop, bounded
event normalization и explicit lost/stale reconciliation после Node restart.
Managed capabilities публикуются при доступном Codex binary с распознанной
совместимой версией. Core orchestration baseline `0.2.23` добавляет единый policy
preview/admission resolver, атомарный interaction decision intent,
provider-event confirmation и attempt-aware actual-state reconciliation. Stale
Node report не может воскресить superseded attempt, а отсутствие live managed
process даёт explicit provider-resumable projection. Web baseline `0.2.24`
добавляет явный profile/policy start contract, semantic activity and interaction
cards, persistent runtime diagnostics и capability-aware lifecycle. Closure
baseline `0.2.25` переводит новые Agent sessions на Managed default при
подтверждённых Node capabilities; отсутствующий profile на incapable Node
получает typed rejection без Exec fallback. Stored profiles не мигрируют,
internal Jobs остаются на явном Exec compatibility path, а Task Run contract
не меняется.

### Client отвечает за

- human interaction;
- visualization;
- review UX;
- command initiation;
- session attach/detach;
- artifact browsing;
- mobile/desktop/web ergonomics.

Client should not own durable workflow state.

### AI Agent отвечает за

- reasoning;
- tool use within granted scope;
- producing changes and artifacts;
- reporting expected evidence;
- exposing unresolved risks;
- following mode-specific contract.

## Контракты Quality Foundation 0.2.0

### Identity Project, Placement И Workspace

- `Project` — логический aggregate под authority Core. Его identity не зависит
  от Node или локального path.
- `ProjectPlacement` — физическая привязка одной Node к одному canonical local
  workspace path. Core persistence обязана обеспечивать uniqueness пары
  `(node_id, canonical_workspace_path)`.
- `Workspace` — пользовательский workbench поверх Placement, а не ещё одна
  persisted identity. Core resource route — `/placements/:id`, Web workbench
  route — `/workspaces/:placement_id`.
- Один Project может владеть Placements на нескольких Nodes. Core создаёт
  identifiers Project и Placement; Node canonicalizes and validates paths и
  сообщает local facts.
- Heartbeat discovery и explicit binding сходятся к одному Placement для пары
  Node/path. Discovery создаёт или обновляет один unbound Placement; explicit
  binding привязывает его к выбранному или новому Project. Один path никогда не
  задаёт cross-node Project identity автоматически.

### Authority Durable State

Core владеет durable product state, legal domain transitions и глобальной
event record. Схему задают numbered SQLx migrations with checksums. Неизвестные
persisted enum или state values — corruption or compatibility errors, а не
fallback к initial state. У каждого duplicated index или projection есть одна
документированная authority и rebuild rule; normalized capability rows
authoritative; immutable event envelopes и searchable projections фиксируются
в одной transaction; у session/runtime links одна authoritative relation.
Enrollment claim, Project/Placement binding, session creation, turn submission
и event ingestion являются units of work, а in-memory notification происходит
только после commit.

Node владеет одним transactional local state store и long-lived local
resources, которыми управляет. Daemon-level `NodeSupervisor` владеет
registration, heartbeat snapshots, command deduplication, runtime metadata,
outbox state и shutdown. Единственный state-store actor является единственным
writer durable state. Store 0.2.0 использует SQLite с отдельными tables для
identity, command cache, outbox, runtime metadata, transcripts и provider
resume references. `RuntimeSupervisor` владеет provider processes,
cancellation, transcripts и resume state; `TerminalSupervisor` владеет PTY
children независимо от control connection. Retention completed commands,
stopped runtimes, transcripts и acknowledged outbox entries задаётся явно и
имеет bounds.

### Protocol V2 И Compatibility

Protocol v2 — единый coordinated breaking release для Core, Node и Web. Rust
types в `uprava-protocol` являются source of truth; tracked JSON Schema,
TypeScript types, runtime validators и canonical fixtures генерируются для
Web-facing roots и проверяются на drift. Built-in commands and events используют
tagged typed payloads; known kinds не используют arbitrary JSON, а
extensibility остаётся explicit extension variant. Node-only control contracts
не попадают в browser bundle, а ingress validation доказывает связи scope,
target и identifiers.

Compatibility с API, schemas и state 0.1.x не требуется. In-place migration
0.1.x отсутствует: incompatible state должна явно останавливать startup с
инструкцией по reset и re-enrollment, а не молча переинтерпретироваться или
удаляться. Первый запуск 0.2.0 использует отдельные versioned Core and Node
state/config slots; сохранённые Core database 0.1.8, Node JSON state и matching
configuration остаются доступны для rollback. Rollback выбирает old binaries,
configuration и state вместе и не переносит в 0.1.8 работу, созданную только в
0.2.0.

## Connection model

Базовая безопасная модель: Node Daemon сам устанавливает outbound connection к Core.

Это упрощает:

- NAT/firewall scenarios;
- подключение личного компьютера;
- временные devboxes;
- cloud nodes;
- mobile/web access.

Core затем маршрутизирует команды и streams через это соединение.

Прямое client-to-node подключение можно рассматривать позже как optimization для локального режима, но не как обязательную базовую архитектуру.

## Deployment profiles

### Local single-user

```text
same machine:
Core Backend + Web Control Panel + Node Daemon
```

Подходит для раннего MVP и локальной разработки.

### Personal distributed

```text
server/VPS/cloud:
Core Backend + Web Control Panel

personal machines/devboxes:
Node Daemons

clients:
web/mobile/desktop/CLI
```

Подходит для сценария "работаю с компьютера и телефона, агенты бегут на разных машинах".

### Team/cloud

```text
managed Core Backend
multiple users
multiple projects
multiple Node Daemons
shared workflows
role-based access
```

Подходит для коммерческого/team продукта.

## Открытые вопросы

- Окончательно ли называем сущность `Node`, а не `Host`?
- Где хранить большие artifacts: в Core storage, на Node или во внешнем object storage?
- Какие secrets можно хранить в Core, а какие должны оставаться только на Node?
- Как описывать tool capabilities: через MCP schema, собственный contract или adapter model?
- Какими package signing, catalog and sandbox mechanisms расширить bundled
  Plugin Registry до local/team/community plugins?
- Какие интеграции стоит делать через MCP, а какие требуют native adapter?
- Как versioning tools/plugins влияет на воспроизводимость trace?
- Должен ли Core уметь выполнять lightweight tools сам, или любой execution должен идти через Node/Provider?
- Какой минимальный протокол нужен между Core and Node Daemon для MVP?
- Какой transport выбрать сначала: HTTP polling, WebSocket, gRPC, message queue?
- Как изолировать команды terminal/filesystem в persistent session mode?
- Как показывать пользователю разницу между Core-level tool и Node-local tool?

## Текущая позиция

На текущем уровне vision наиболее сильная архитектурная позиция такая:

- `Core Backend` обязателен как control plane.
- `Web Control Panel` можно сразу разворачивать вместе с Core.
- `Node Daemon` является системным агентом на ноде и основным data plane.
- `AI Agents` являются workloads, а не инфраструктурными демонами.
- `Tool Registry` живет в Core.
- `Plugin Registry` живет в Core рядом с Tool Registry.
- `Web Extension Host` применяет permission-filtered contributions к известным
  typed surfaces и всегда сохраняет safe fallback.
- Uprava MCP является основным machine interface агента к Core capabilities и
  внешним integrations.
- Agent-facing tool access обязательно использует progressive discovery
  `Search -> Inspect -> Execute`; полный catalog schemas не передаётся модели.
- ToolHive является external MCP runtime provider; Core сохраняет policy,
  scope, routing, trace and audit.
- Provider-native и Node-local CLI tools учитываются как observed capabilities
  и вызываются агентом напрямую.
- Native API, Node-local, external provider and hybrid adapters остаются
  допустимыми execution backends за MCP-facing/Core-owned contract.
- Tool execution может происходить на Node, во внешнем provider или позднее в самом Core для безопасных lightweight tools.
- Clients должны работать через Core, а не напрямую владеть distributed state.
