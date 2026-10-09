# A-007 Agent Tooling, Tool Registry and MCP Strategy

Статус: `working-position`

Этот документ фиксирует архитектуру пункта очереди `11 Agent Tooling and Tool
Registry v1` и долговечное направление для plugins and integrations.

## Решение

Uprava строит agent tooling вокруг четырёх обязательных решений:

1. Core владеет Tool Registry, product policy, scope, routing, trace and audit.
2. Uprava MCP является основным machine interface агента к Core capabilities и
   внешним integrations.
3. Agent-facing каталог всегда использует progressive discovery
   `Search -> Inspect -> Execute`; полный catalog schemas не передаётся модели.
4. ToolHive является external MCP runtime provider: запускает, обнаруживает,
   агрегирует и проксирует внешние MCP servers, но не заменяет Core policy.

Отдельный Uprava CLI не входит в первый срез. Его можно добавить позже, если
появятся подтверждённые shell-composition, streaming or batch scenarios.

Provider-native и Node-local tools (`bash`, filesystem tools, `git`, `gh`,
`glab`) не дублируются и не проксируются через Uprava без отдельной продуктовой
причины. Uprava показывает их как observed capabilities, после чего агент
вызывает native tool напрямую.

Короткая формула:

```text
Core owns meaning, policy and trace.
Uprava MCP exposes a progressive agent interface.
ToolHive operates external MCP runtimes.
Native agent tools remain native.
```

Версионированный foundation contract для среза `0.2.11` находится в
[`agent-tooling-contracts.md`](../../development/agent-tooling-contracts.md).

## Реализованный baseline Core

Эпик 1 среза `0.2.11` реализует Core-owned часть решения:

- SQLite Tool Registry, atomic definition/search-document update и раздельную
  effective availability;
- bounded permission-first BM25 Search с подписанным scope/query/filter-bound
  cursor, Inspect одной current definition и Execute с fresh checks;
- пять явных native tools для Node, workspace, session, trace и capability
  inspection без автоматического экспорта HTTP routes;
- Uprava MCP Streamable HTTP endpoint `/mcp` на `rmcp 2.2.0` с постоянной
  surface из трёх discovery meta-tools;
- short-lived session-scoped lease, tool-call lifecycle journal, redacted
  summaries, hashes, refs и session tool snapshots;
- Web-authenticated Core read routes для definitions, availability и calls.

Baseline эпика 2 реализует observed Node inventory, durable typed
Tooling commands, desired/actual reconciliation после reconnect, отдельный
Compose ToolHive service с pinned CLI, bounded HTTP/MCP bridge, external
availability и terminal trace recovery. Ручная приёмка Linear OAuth callback,
`tools/list`, read-only call и disconnect/reconnect в этой топологии ожидается;
она не считается подтверждённой кодовыми и mock-проверками.

Baseline эпика 3 добавляет Web management surface для integration,
managed/observed capabilities и redacted tool-call trace. Primary Codex Managed
path получает Uprava MCP endpoint и session-scoped process lease через
authenticated Node transport, переиспользует его до bounded refresh window и
ротирует replacement app-server с provider-native resume. Token не входит в
durable command, process args, prompt или transcript и передаётся Codex через
одноразовое имя environment variable из очищенного allowlist; provider
`shell_snapshot` выключен, чтобы cached credential предыдущего process не мог
подменить lease. Codex approval для встроенного `uprava` MCP server считается
уже выданным, потому что Search/Inspect/Execute проходят Core-owned permission,
approval, audit и trace boundary; эта настройка не применяется к внешним MCP
servers. Connect и
Reconnect возвращают валидированный Linear authorization URL только текущему
Web-запросу, а heartbeat завершает actual-state reconciliation. Disconnect
немедленно закрывает effective availability; без отдельного upstream proof Core
консервативно не выдаёт локальное действие за подтверждённый remote revoke.
Operational детали находятся в
[`agent-tooling-toolhive-linear.md`](../../runbooks/agent-tooling-toolhive-linear.md).

## Vision

### Проблема

У агентской системы одновременно существуют:

- Uprava-native capabilities;
- внешние MCP integrations;
- provider-native tools;
- Node-local CLI tools;
- разные Node, workspaces, sessions and authorization states;
- разные agent providers с разными механизмами tool loading.

Если каждый provider самостоятельно читает локальные MCP configs и получает
полный список всех tools, Core теряет ответы на вопросы:

- какие capabilities агент действительно видел;
- почему tool был доступен или недоступен;
- на какой Node и через какой backend он исполнялся;
- какие permissions and approvals применились;
- какая schema/version использовалась;
- как вызов связан с trace, artifact or external entity;
- как отключить integration для одного project/session;
- как не заполнить context сотнями schemas.

Если Uprava начнёт оборачивать `bash`, `git`, `gh` и другие знакомые агенту
инструменты, появятся дублирующие contracts, лишние шаги и худшая composability.

Нужна общая модель, которая управляет тем, чем Uprava действительно владеет,
наблюдает остальное и раскрывает агенту только релевантные capabilities.

### Целевая схема

```text
Agent provider
  built-in tools: bash, files, git, browser
  direct Node CLI: gh, glab, other registered binaries
  Uprava MCP: small stable discovery surface
                     |
                     v
Core Tool Registry and Policy
  definitions, effective availability, permissions, routing, trace
             |                         |
             v                         v
Uprava-native backend             Node MCP bridge
Core commands/API                      |
                                      v
                                   ToolHive
                                      |
                                      v
                         external MCP servers/integrations
```

## Термины и границы

### Managed tool

Callable capability, contract and execution path которой находятся под
Uprava policy.

Примеры:

- `uprava.session.inspect`;
- `uprava.trace.resolve`;
- `uprava.artifact.describe`;
- `linear.search_issues` через ToolHive-backed MCP;
- `notion.get_page` через ToolHive-backed MCP.

### Observed capability

Capability, наличие которой Uprava может обнаружить и показать, но execution
contract которой не принадлежит Uprava.

Примеры:

- `bash` у agent provider;
- `git` на Node;
- авторизованный `gh`;
- `glab` определённой версии;
- provider-native web search.

Observed capability не должна притворяться managed tool. Для неё Core может
знать version, health, Node placement and safe auth status, но не обещает
Uprava schema, routing, audit coverage or result capture.

### Tool definition

Версионируемое описание managed tool независимо от его текущей доступности.

### Effective availability

Результат вычисления доступности definition для конкретных actor, Node,
project/workspace, session and policy version.

### MCP integration

Настроенное подключение внешней системы, tools которой доступны через MCP
server. Примеры: Linear, Notion, Atlassian/Jira.

### Dependency

Runtime artifact, необходимый для capability: MCP server package/image,
endpoint, binary or provider runtime.

### Runtime provider

Механизм, управляющий lifecycle dependency. В первом срезе external MCP
dependencies обслуживает ToolHive.

## Разделение ответственности

### Core

Core владеет:

- Tool Registry definitions;
- integration configuration and desired state;
- actor/project/workspace/session scope;
- permissions, risk and approval policy;
- progressive discovery index and filtering;
- effective availability projection;
- routing decisions;
- tool-call identity, trace and causality;
- safe product-level audit metadata;
- session tool snapshots where required for reproducibility;
- UI projection and explanations of unavailable state.

### Node Daemon

Node владеет:

- actual Node capability inventory;
- workspace-local execution context;
- adapter к private ToolHive bridge и его bounded contract;
- desired/actual reconciliation for enabled MCP dependencies;
- desired/actual lifecycle orchestration, делегированным отдельному ToolHive service;
- reporting discovered tools, schema hashes, health and failures;
- bounded materialization of credentials where local runtime requires it;
- local enforcement that complements Core policy.

### ToolHive

ToolHive владеет MCP runtime concerns:

- starting and stopping MCP servers;
- remote MCP proxying;
- workload/group/vMCP management;
- tool discovery and upstream metadata;
- namespacing/filtering primitives;
- runtime health and logs;
- runtime-level audit;
- supported secrets and isolation mechanisms.

ToolHive не встраивается в Node image/process. В локальном профиле это
отдельный Compose service; pinned `thv` и OAuth state находятся в его XDG
volume, а Node и Codex работают на bare metal. Bridge принимает только один
зафиксированный Linear workload и методы `tools/list`/`tools/call`; MCP proxy
не публикуется на host.

ToolHive не владеет:

- Uprava actor/project/session permissions;
- product-level tool identity;
- cross-provider progressive discovery UX;
- Uprava trace and causality;
- artifact/UI semantics;
- final routing authorization.

### Agent provider

Provider:

- подключается к Uprava MCP;
- показывает модели минимальную discovery surface;
- может dynamically mount inspected definitions, если умеет;
- продолжает предоставлять собственные native tools;
- не получает полный Uprava catalog в model context.

## Core Tool Registry

### ToolDefinition

Минимальная модель:

```text
tool_id
version
display_name
short_description
documentation optional
source_kind
source_ref optional
input_schema
output_schema optional
schema_hash
risk_level
required_permissions
execution_kind
routing_policy
required_capabilities
approval_policy
audit_policy
renderer_contract optional
artifact_contract optional
status
created_at
updated_at
```

`source_kind`:

```text
uprava_native
external_mcp
plugin
```

`execution_kind`:

```text
core_native
node_native
toolhive_mcp
external_provider
hybrid
```

`risk_level` initial vocabulary:

```text
read_only
workspace_write
external_read
external_write
credentialed_action
destructive
privileged_local
network_broad
```

### ToolAvailability

```text
tool_id
actor_scope
node_id optional
project_id optional
project_placement_id optional
session_id optional
status
reason_code optional
backend_ref optional
dependency_instance_ref optional
schema_hash
policy_version
observed_at
```

`status`:

```text
available
unavailable
degraded
approval_required
```

`reason_code` examples:

```text
node_offline
capability_missing
dependency_missing
dependency_unhealthy
not_authenticated
permission_denied
policy_blocked
project_not_enabled
session_not_enabled
schema_changed
backend_unreachable
```

Definition не исчезает при временной недоступности backend. Registry должен
объяснять unavailable state, а не превращать runtime failure в исчезновение
capability.

### ObservedCapability

```text
capability_id
kind: provider_native | node_cli
display_name
short_description
node_id optional
provider optional
version optional
health
authentication_status optional
invocation_hint optional
scope
observed_at
```

Нельзя публиковать secret values, raw credential paths или чрезмерные account
metadata. Для `gh` достаточно, например, host, authenticated boolean and safe
account label, если policy это разрешает.

## Progressive tool discovery

### Обязательная модель

Полный catalog никогда не передаётся модели. Даже если Core, Node or ToolHive
получили upstream `tools/list`, schemas остаются host-side.

```text
Search
  returns names and one-line descriptions

Inspect
  returns the selected full contract

Execute
  invokes after fresh validation and authorization
```

### Agent-facing meta-tools

```text
search_tools(query, filters?, cursor?)
inspect_tool(tool_id)
execute_tool(tool_id, arguments)
```

Это минимальный стабильный набор, который можно держать в provider context
постоянно.

### Search

Search принимает natural-language query и optional filters:

```text
source
risk
integration
node
project/workspace
availability
```

Ответ содержит только bounded results:

```text
tool_id
name
one_line_description
source/server label optional
availability hint optional
```

Search pipeline:

```text
resolve actor/session/project/node scope
-> filter by permission visibility
-> filter or annotate effective availability
-> retrieve and rank relevant tools
-> return bounded summaries
```

Policy filtering происходит до retrieval output. Search не должен раскрывать
существование secret/private tools неавторизованному actor.

Первый retrieval engine может быть keyword/BM25. Contract должен позволять
добавить embeddings, hybrid ranking or provider-native tool search позже.

### Inspect

Inspect возвращает один выбранный contract:

```text
full description
input/output schemas
examples optional
permissions and risk
approval behavior
effective availability and reason
source/integration
schema version/hash
result and artifact semantics
```

Inspect не является authorization grant и не резервирует backend.

### Execute

Execute всегда заново выполняет:

```text
resolve current scope
load current definition/version
validate arguments against schema
recompute effective availability
authorize actor and approval policy
choose route/backend
record tool_call.requested/authorized
dispatch
record completion/failure and result refs
```

Это защищает от stale Inspect, policy changes, Node disconnect and upstream
schema changes.

### Dynamic mounting

Если provider умеет безопасно добавлять tool definitions в текущую session,
после Inspect выбранный definition может быть mounted как direct tool. Это
оптимизация provider adapter, а не новый authority path.

Provider-neutral fallback всегда использует `execute_tool`. Direct calls and
fallback calls проходят одну Core policy and trace pipeline.

### Catalog maintenance

Core/host:

- индексирует полные definitions вне model context;
- группирует их по source/server/integration;
- caches by `tool_id + version + schema_hash`;
- обновляет индекс на upstream `tools/list_changed`;
- поддерживает pagination and bounded detail levels;
- хранит session snapshot, когда это нужно для reproducibility;
- не меняет значение существующей версии/schema hash молча.

## Uprava-native MCP tools

Первый срез должен показывать через тот же progressive contract небольшой набор
Core capabilities. Предварительные группы:

```text
uprava.node.inspect
uprava.workspace.inspect
uprava.session.inspect
uprava.trace.resolve
uprava.ref.resolve
uprava.artifact.describe
uprava.capability.inspect
```

Точный набор определяется implementation plan. Не следует автоматически
экспортировать каждый HTTP endpoint или внутренний command как MCP tool.
Agent-facing tools должны быть устойчивыми domain operations с bounded output.

Uprava-native tool call переводится в существующие Core application services,
commands and events. MCP не создаёт параллельную domain model.

## External MCP integrations

Основной scope первого направления:

- Linear;
- Notion;
- Atlassian/Jira;
- другие готовые MCP servers.

Первый implementation slice выбирает один реальный server для end-to-end
проверки. ToolHive должен:

1. materialize configured MCP dependency;
2. report actual state;
3. discover tools and schemas;
4. expose backend through Node bridge;
5. return health and audit refs;
6. survive restart/reconciliation according to desired state.

Core namespaces upstream tools and сохраняет stable mapping:

```text
Uprava tool_id
upstream server/workload
upstream tool name
schema hash
integration/connection ref
Node/runtime provider
```

Direct agent-to-ToolHive exposure не является целевой моделью v1, потому что
обходит Core progressive discovery, permissions and trace. Все agent-visible
calls должны проходить через Uprava MCP policy boundary.

## Desired and actual state

Core хранит desired integration state:

```text
integration enabled
connection ref
allowed projects/sessions
preferred Node/pool
tool allow/deny policy
runtime profile/version
```

Node сообщает actual state:

```text
not_installed
installing
starting
running
degraded
failed
blocked_by_policy
missing_auth
stopped
```

UI и discovery должны различать configured, running and available-to-this-
session. Desired state не считается применённым до Node report.

## Authentication and secrets

### Uprava MCP access

Agent runtime получает scoped identity/credential для Uprava MCP. Core
проверяет actor, session/project binding, token audience, expiry and scopes.
Credential не должен становиться частью model prompt.

Точный enrollment/token lifecycle определяется security implementation plan,
но authority остаётся в Core.

### External integration auth

Connection model хранит authorization state и secret refs, но не secret values
в registry output.

Для remote MCP integrations предпочтителен стандартный MCP/OAuth flow. Для
third-party authorization допускается out-of-band browser flow; credentials
хранит соответствующая trusted runtime boundary и не возвращает агенту.

Если local ToolHive workload требует credential, Node получает только право на
bounded materialization для конкретной dependency instance.

Trace сохраняет:

```text
connection ref
materialization event/ref
scope and policy version
outcome
```

Trace не сохраняет token, cookie, API key or raw secret-bearing environment.

## Permissions, approvals and security

### Permission decision

```text
actor
tool
project/workspace/session
Node
arguments/target summary
risk level
connection
runtime mode
policy version
-> allow | deny | require_approval
```

Filtering tool out of Search не является единственной security boundary.
Inspect and Execute independently enforce visibility and authorization.
Calling a hidden/denied upstream name through crafted input must fail.

### Threats

Security model учитывает:

- malicious tool descriptions and metadata poisoning;
- schema drift;
- supply-chain provenance of MCP packages/images;
- overbroad network/filesystem access;
- secret exfiltration and token passthrough;
- cross-tool data forwarding;
- unsafe result content;
- confused-deputy routing;
- stale policy or availability snapshots;
- excessive catalog disclosure;
- destructive or credentialed calls without approval.

ToolHive isolation and audit помогают, но не заменяют Core authorization.

## Tool call, trace and audit

### ToolCall

```text
tool_call_id
tool_id
tool_version/schema_hash
actor_ref
scope_ref
session/run refs optional
source integration/backend refs
arguments metadata/redaction refs
policy decision and version
approval ref optional
route
status
command/event/result/artifact refs
started_at
completed_at optional
```

### Events

Минимальные product events:

```text
tool_call.requested
tool_call.authorized
tool_call.approval_required
tool_call.started
tool_call.completed
tool_call.failed
tool_call.denied

tool_definition.discovered
tool_definition.changed
tool_availability.changed

mcp_dependency.reconcile_started
mcp_dependency.running
mcp_dependency.failed
mcp_dependency.stopped
```

Search and Inspect можно агрегировать как telemetry, но privileged discovery
denials and schema changes должны оставлять reviewable evidence.

### Trace versus security audit

Trace отвечает, что произошло в работе и к чему привело.

Security audit отвечает, кто запросил чувствительное действие, какое policy
решение было принято и почему.

Не каждый read-only call обязан дублироваться в security audit. Audit policy
может быть:

```text
none
failures
mutations
all_calls
```

Arguments/results сохраняются только согласно redaction policy. Предпочтительны
refs, hashes, sizes, bounded summaries and redaction flags вместо raw content.

## Core API and storage boundaries

Концептуальные read models/endpoints:

```text
GET /tools
GET /tools/:tool_id
GET /placements/:id/tools
GET /sessions/:id/tools
GET /nodes/:id/capabilities
GET /integrations
GET /integrations/:id/status
GET /tool-calls/:tool_call_id
```

Uprava MCP вызывает application services, а не эти HTTP endpoints через
loopback. HTTP и MCP transports должны использовать общие domain/application
boundaries.

Минимальные persistence concepts:

```text
tool_definitions
tool_sources
tool_availability_snapshots or projection
observed_capabilities
integration_connections
mcp_dependency_instances
session_tool_snapshots
tool_calls
```

Конкретная normal form определяется implementation plan. Upstream raw metadata
можно хранить отдельно от normalized product definition.

## UI consequences

UI первого среза не должен становиться marketplace. Достаточно:

- integration list and desired state;
- connect/authenticate, reconnect and disconnect;
- ToolHive dependency status and failure reason;
- effective availability per Node/project/session;
- observed native CLI inventory and safe auth status;
- recent tool calls and trace links;
- Inspect detail для definition, permissions, source and schema version.

UI не обязан показывать модели полный catalog. Human catalog view и
agent-facing progressive discovery являются разными projections одного Core
registry.

## First implementation slice

Пункт `11` включает целостный, но узкий vertical slice.

### Обязательно

- Core Tool Registry domain model and persistence;
- managed tools versus observed capabilities;
- effective availability by Node/project/session/actor;
- Uprava MCP endpoint/bridge and scoped authentication;
- stable `search_tools`, `inspect_tool`, `execute_tool` surface;
- permission-filtered keyword/BM25 search with bounded results;
- несколько Uprava-native inspection/action tools;
- Node adapter к отдельному ToolHive bridge;
- один реальный external MCP server/integration;
- desired/actual dependency status;
- schema hash/version and list-changed refresh;
- tool-call identity, routing, trace and safe audit metadata;
- compact integration/capability UI.

### Не входит

- отдельный Uprava CLI;
- большой integration marketplace;
- Plugin Registry implementation;
- автоматическая обёртка `bash`, files, `git`, `gh` or `glab`;
- remote installation/authentication management для native CLI tools;
- enterprise RBAC;
- универсальная secrets platform;
- embeddings as required search engine;
- programmatic tool calling/code mode;
- rich renderer/artifact contract для каждого tool;
- direct agent-to-ToolHive exposure.

## Acceptance scenario

Срез считается доказанным, когда проходит следующий сценарий:

1. Core хранит definitions Uprava-native and external tools.
2. Agent session подключена к Uprava MCP с project/session scope.
3. Модель видит только три discovery meta-tools, а не полный catalog.
4. Search находит релевантный Uprava-native tool и возвращает краткое описание.
5. Inspect раскрывает одну schema and availability.
6. Execute проходит fresh authorization и оставляет tool-call trace.
7. Node через ToolHive запускает один внешний MCP server и сообщает actual state.
8. Тот же Search -> Inspect -> Execute path вызывает внешний tool.
9. Отключение Node, integration or permission меняет availability и блокирует
   crafted Execute.
10. Agent может узнать, что на Node доступны `git`/`gh`, но вызывает их напрямую.
11. UI показывает integration/auth/health state и ссылку на trace без secrets.

## Relationship with other areas

### A-001 Distributed Architecture

```text
Core: definitions, desired state, policy, progressive index, routing, trace
Node: actual capabilities, ToolHive reconciliation, bridge client and enforcement
```

Clients and agents не обходят Core policy при managed tool calls.

### A-002 Run Mode

Run Mode определяет lifecycle agent workload. A-007 определяет effective
toolset этой session/run. Managed Agent, exec compatibility, sandboxed Task и
sessionless Job profiles могут иметь
разные Node placements and integration policies.

### A-003 Distributed Runtime Coordination

Tool availability становится placement constraint. Session, которой нужен
определённый external MCP backend, размещается только там, где dependency
running and policy allows it.

### A-004 Modular UI and Work Surface

A-007 владеет Tool Registry and agent-facing capability contract. Plugin
Registry, package lifecycle, Web Extension Host, themes, views, commands and
other UI contributions принадлежат A-004 и реализуются пунктом `12`.

Registries связаны явными references: plugin может contribute tool definition,
renderer or action for tool output, но Plugin Registry не становится source of
truth для tool schema/routing, а Tool Registry не активирует UI plugins.

### A-005 Dynamic UI from Agents

Tool result может ссылаться на registered renderer/artifact contract. Agent-
generated UI не получает authority bypass через MCP output.

### A-006 Visual Rendering and Artifact Semantics

A-006 определяет visual objects and fallbacks. A-007 связывает tool output,
source, schema, trace and renderer contract.

### A-008 Go to Source and Causality UX

```text
artifact/result -> tool call -> definition -> integration/backend -> events
```

### A-009 Human-Agent Dual Interface

Uprava MCP является primary machine interface агента к Core semantic context,
refs and commands. Progressive discovery не заменяет agent-readable UI model;
он является способом безопасно навигировать её capabilities.

### A-010 Project Workspace Surface

Observed `bash`, files and git capabilities остаются native. Managed tools and
external MCP integrations уважают workspace boundaries, Node placement and
project-scoped trace.

## Оставшиеся implementation questions

Архитектурный vision определён; implementation plan должен закрыть:

- первый реальный external MCP server для acceptance scenario;
- ToolHive CLI/API integration boundary and supported versions;
- session snapshot retention policy;
- safe native CLI auth probes for `gh`/`glab`;
- direct dynamic mounting support в первом Codex adapter или только
  `execute_tool` fallback;
- schema drift behavior during long persistent sessions.

Эти вопросы не меняют ключевые решения: MCP-first, ToolHive-backed,
Core-governed and progressively discovered.

## Quality questions

Для каждого capability:

- Managed tool это или observed capability?
- Кто владеет definition и version?
- Где находится actual runtime?
- Как вычисляется Node/project/session availability?
- Может ли неавторизованный actor узнать о существовании tool?
- Почему model context не получает лишние schemas?
- Что возвращают Search and Inspect?
- Проверяет ли Execute policy заново?
- Как обрабатываются schema changes and `tools/list_changed`?
- Какие secrets нужны и где они materialized?
- Какие trace/audit refs остаются?
- Можно ли отключить integration для одного scope?
- Что видит пользователь при missing auth, unhealthy backend or denied call?
- Не дублирует ли capability уже доступный native agent tool?

## References

- [MCP Client Best Practices](https://modelcontextprotocol.io/docs/develop/clients/client-best-practices)
- [MCP Authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization)
- [MCP Security Best Practices](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices)
- [ToolHive documentation](https://docs.stacklok.com/toolhive)
