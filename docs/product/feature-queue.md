# Uprava Feature Queue

Статус: `historical`

Итерация закрыта на baseline `0.2.26`. Эта очередь сохраняет историю реализации
и прежние направления. Новый scope и порядок работы определяются через
[ретроспективу](../retrospective/iteration-01/README.md) и её рекомендации.

Этот документ использует implementation queue вместо phase-based roadmap.

Очередь - это не calendar, milestone ladder or delivery promise. Это
ранжированный набор продуктовых и архитектурных срезов, упорядоченный по
dependency, complexity, risk and value. Позиции могут двигаться по мере
прояснения дизайна.

В колонке `Done`: `+` означает закрытый slice, `~` — поставленный, но ещё не
закрытый partial baseline, `-` — будущую работу.

## Правила очереди

Каждый элемент очереди должен фиксировать:

- **Value** - почему это важно пользователю или Uprava как системе.
- **Dependency** - что должно существовать раньше.
- **Complexity** - сложность реализации and surface area.
- **Risk** - unknowns, security concerns or product ambiguity.
- **First useful slice** - минимальная версия, которую стоит строить.
- **Target direction** - как механизм должен расти без overfitting под первую
  реализацию.

Используйте этот документ, чтобы отвечать на вопрос:

```text
Что строить следующим, и почему это раньше другого?
```

Не используйте его для ответа на вопрос:

```text
Что входит в первую версию?
```

Это описано в разделе [V01](product-evolution.md#v01).

## Обзор очереди

Current release baseline: `0.2.26`. Закрытые пункты `0` through `14` и `16`, unified
audit hardening release и `5a` workspace renderer release соответствуют shipped
versions, зафиксированным в [`releases.md`](../releases.md). Пункт `6` включает
workbench alignment, первый стабильный self-hosted deployment path и
workspace-centered UI follow-up `0.2.6` и Causality/Trace/Deduction slice
`0.2.7`. Runtime boundary refactor зафиксирован implementation baseline
`0.2.9`. Git and review basics зафиксирован implementation baseline `0.2.10`,
Agent Tooling and Tool Registry v1 — `0.2.11`, Plugin Registry v1 — `0.2.12`,
CI/SQLite reliability fix slice — `0.2.13`, отдельная ToolHive Compose topology
для ручного Linear acceptance — `0.2.14`, bundled Markdown renderer plugin —
`0.2.15`, Plugin contribution resolution — `0.2.16`, Visual Artifact System —
`0.2.17`, Dynamic UI from Agents — `0.2.18`. Runtime mechanics пункта `15`
поставлены в `0.2.19`; пункт остаётся частично открытым до persistent Codex auth,
OpenSandbox API key и ручной Docker acceptance.

| Order | Done | Mechanism / Feature Slice | First Useful Slice | Dependency | Complexity |
| --- | --- | --- | --- | --- | --- |
| 0 | + | V01 Distributed Agent Control Panel | Multi-node chat/session control panel | Current design baseline | High |
| 1 | + | Security baseline | Trusted-dev warning, node auth, local web auth, credential handling, audit minimum | V01 control path | High |
| 2 | + | Runtime/session hardening | Robust lifecycle, resume, stop, blocked, stale states | V01 runtime path | Medium |
| 3 | + | Workspace shell and reference model | Stable refs and routes for future workspace evidence | V01 entity/session model | Medium |
| 4 | + | Read-only Project Workspace Inspector | File tree, metadata, safe text viewer | Workspace refs, Node file reads | Medium |
| 5 | + | Workspace intervention layer | Lightweight editor, terminal, command history, diff/check entry points | Read-only inspector, events | High |
| 5a | + | Workspace renderer and PTY terminal layer | Monaco file/diff renderers and xterm-backed interactive PTY sessions | Workspace intervention, Core/Node control channel | High |
| 6 | + | Daily-use hardening and deployment readiness | Stable panel layout, product polish, server deploy path, CI/CD baseline | `0.1.8` deployable workbench, security baseline | High |
| 7 | + | Отложенные сообщения в сессии | Долговечные одноразовые будущие turn существующей сессии | Runtime/session guards, Core-owned persistence | Medium |
| 8 | + | Background Jobs и scheduled agent runs | Долговечные определения unattended agent work, расписания и наблюдаемые runs | Placements, provider runtime, durable events | High |
| 9 | + | Causality and trace UX | Coarse source/cause links with raw fallback | Workspace refs, event log | Medium |
| 10 | + | Git and review basics | Better diff, branch/worktree awareness, check results | Workspace intervention, trace | Medium |
| 11 | + | Agent Tooling and Tool Registry v1 | Uprava MCP, progressive discovery, ToolHive runtime, scoped registry and trace | V01 capability model, events | High |
| 12 | + | Plugin Registry v1 | Core registry, manifest-driven Web Extension Host and bundled Dark Theme plugin | Stable workbench shell, design tokens | High |
| 12a | + | Markdown renderer plugin | Typed `visual.renderer` contribution, safe Streamdown rendering and plain-text fallback for assistant chat content | Plugin Registry v1 | Medium |
| 12b | + | Plugin contribution resolution | Target-based `exclusive`/`ordered` resolution, configurable contribution order and visible conflicts in Plugin Panel | Plugin Registry v1, Markdown renderer plugin | High |
| 13 | + | Visual artifact system as plugins | Plugin-driven content enhancements for code, colors and diagrams plus artifact viewers for reports, diffs and timelines | Trace, Plugin contribution resolution | High |
| 14 | + | Dynamic UI from agents as plugins | Opt-in bundled Generated React plugin with sandboxed runtime, Uprava UI SDK, safe fallbacks and permissioned actions | Plugin-delivered visual artifact system | High |
| 15 | ~ | Task-based sandbox runtime | Docker/OpenSandbox bounded run, isolated worktree, persistent Codex auth and expected evidence | Runtime, workspace, trace | Very high |
| 16 | + | Managed Agent Work Loop | Provider-native live Codex session with real approvals, interruption, reconnect/resume and an explicit exec compatibility mode | Agent/session runtime baseline, provider-native managed protocol | Very high |
| 17 | - | Team/cloud model | Users, roles, shared projects, managed Core/nodes | Mature personal workflow | Very high |
| 18 | - | Beyond software development | Research, analytics, documents, finance, knowledge workflows | Mature artifact/plugin model | Very high |
| 19 | - | Audit follow-up refactors | Core/Node module split, generated protocol contracts, async workspace command API | `0.1.6` audit hardening | Medium |

## Детали очереди

### 0. V01 Distributed Agent Control Panel

**Value:** Дает первый осязаемый продукт: пользователь может запустить Core,
подключить одну или несколько nodes, bind projects/workspaces, start persistent
Codex-backed sessions and control those sessions from a web UI.

**First useful slice:** Описан в разделе [V01](product-evolution.md#v01).

**Target direction:** Сохранить первый продукт маленьким, но не закрывать system
model для workspaces, providers, tools, plugins, visual artifacts, task runs,
mobile and team/cloud modes.

### 1. Security baseline

**Value:** Делает V01 control path достаточно безопасным для использования за
пределами полностью trusted local prototype, не притворяясь full team/cloud
security.

**First useful slice:** Explicit deployment profiles, visible non-production
warning until hardened mode is enabled, node enrollment/auth, credential storage
rules, revoke/rotate basics, local web auth/session handling, origin/CSRF checks
where relevant, token redaction and minimal security/audit events.

**Current implementation note:** `controlled_dev` with `UPRAVA_WEB_AUTH=auto`
is the supported V01 profile. It enables local password setup/login, session
and CSRF cookies, protected browser routes, origin checks, node bearer
credentials for heartbeat/control, node revoke/rotate, private Node state-file
permissions where supported, token redaction and minimal
`security_audit_events` records. `local_trusted`, disabled browser auth and
auto-approved enrollment are rejected at startup.

**Target direction:** Дорасти до permissions, secrets handling, stronger audit,
mTLS or request signing, keychain-backed credentials, team RBAC and managed
cloud security без изменения Core/Node responsibility split.

### 2. Runtime/session hardening

**Value:** Делает live agent work надежной, а не похожей на wrapped CLI.

**First useful slice:** Clear lifecycle states, explicit expiry/resume behavior,
blocked approvals, interrupt/stop semantics, stale node handling and degraded
resume messaging.

**Current implementation note:** Core and Node now persist and project
start/ready/running/blocked/resuming/stopped/error/expired runtime state,
bounded provider resume refs, idle expiry, stale/offline/revoked node warnings,
detached-session gates, approval request/resolution state and command preflight.
The Web Control Panel and agent projection only advertise send-turn and
approval-resolution commands when those commands match Core runtime/session
preflight, and resolved historical approval blocks no longer expose approval
actions.

**Target direction:** Поддержать несколько runtime strategies and provider
adapters без изменения Core/UI concepts.

### 3. Workspace shell and reference model

**Value:** Позволяет будущим chat, trace, artifacts, review and agents
ссылаться на одну и ту же workspace evidence, не затаскивая full inspector в
V01.

**First useful slice:** Stable ids, routes and reference shapes for project,
workspace, session, turn, message, runtime event and reserved future workspace
objects such as file, file range, edit, terminal session, command, output range,
diff hunk, check result, artifact and trace event.

**Current implementation note:** Shared Rust and Web protocol contracts now
define stable Uprava refs for project, placement, workspace, session, runtime,
turn, message, block, artifact, event, command, approval, warning, tool call,
file/file range, terminal/command/output range, diff hunk, check result,
workspace edit, trace event, external entity and unknown future refs. Web
Control Panel has stable project, workspace, placement, node and session route
helpers, a project route, a workspace route alias, inspector stack URL encoding
and explicit fallback handling for reserved future workspace refs.

**Target direction:** Shared addressability for UI navigation, agent prompts,
review decisions, plugin blocks and task-run packages.

### 4. Read-only Project Workspace Inspector

**Value:** Дает пользователю увидеть, где работает агент, до добавления прямых
intervention tools.

**First useful slice:** Workspace file tree, file metadata, safe text file
viewer, readable states for large/binary/ignored/generated/permission-denied
files and node-side workspace boundary enforcement.

**Current implementation note:** Core exposes authenticated placement workspace
tree and file-read routes, dispatching read-only commands to the Node Daemon and
waiting for typed command results. Node Daemon normalizes relative paths,
enforces workspace and allowed-root boundaries, avoids symlink traversal, caps
tree and text reads, and returns explicit states for large, binary, generated,
ignored, missing, symlink and permission-denied paths. Web Control Panel mounts
file tree and safe text viewer on workspace routes.

**Target direction:** Project surface, который позже сможет принять editor,
terminal, diff, checks, artifacts and trace links.

### 5. Workspace intervention layer

**Value:** Дает человеку narrow control, когда прямое действие быстрее, чем
просить агента описать или исправить собственное окружение.

**First useful slice:** Controlled text writes or patch applies, workspace
terminal/PTY or command runner, command/output history, session-level diff and
basic check/test entry points.

**Current implementation note:** Первый intervention slice расширяет Project
Workspace Inspector явным save для text files, bounded workspace command runner,
отображением command/check results, persisted command result history и git diff
snapshot entry point. Core routes these actions через placement-scoped commands
and persists command-result payloads; Node enforces allowed workspace roots,
path normalization, protected generated/ignored paths, text-size caps, no-shell
command execution, timeout limits and bounded output. Web Control Panel exposes
save, `make l`, `make c`, custom command, diff and history controls в workspace
surface.

**Target direction:** Lightweight developer workbench ergonomics без превращения
в full browser IDE.

### 5a. Workspace renderer and PTY terminal layer

**Value:** Делает workspace surface похожей на настоящий developer workbench:
Monaco рендерит code and diffs, а xterm рендерит interactive PTY вместо
имитации terminal через command-runner output.

**First useful slice:** Monaco-backed file editor and diff viewer; Core APIs для
terminal open/list/stream/input/resize/close; Node Daemon PTY lifecycle scoped
to the validated workspace; xterm terminal tabs with attach, resize, input,
output, status and close handling. Bounded command runner остается отдельным
механизмом для traceable controlled checks.

**Current implementation note:** `0.1.7` добавляет shared protocol contracts для
workspace terminal commands and stream frames, Core routes all terminal traffic
через node control channel and WebSocket client stream, Node owns PTY creation
and cleanup внутри workspace cwd, а Web uses Monaco plus xterm.js as
first-class renderers.

**Target direction:** Добавить durable replay endpoints, terminal output refs,
search/copy ergonomics, review decorations, selection/range actions and richer
diff/review workflows без ослабления Core/Node authority boundary.

### 6. Daily-use hardening and deployment readiness

**Value:** Core workbench path уже достаточно полезен, чтобы следующий срез
сделал его удобным и надежным для постоянной работы, а не только
feature-complete в отдельных flows.

**Dependency:** `0.1.7` workspace renderer/PTY baseline, controlled-dev security
baseline, current Core/Web dev profile and host Node run path.

**First useful slice:** Переработать Web Control Panel под длительное
использование: расположение панелей, информационную плотность, навигационный
ритм, переключение workspace/session, terminal/editor/diff ergonomics and
empty/loading/error states. Сделать визуальный design pass, чтобы текущая
функциональность ощущалась связной и пригодной для continuous use. Добавить
реальный server deployment path с documented environment settings,
reverse-proxy/TLS assumptions, persistent volumes, logs, backup/restore
expectations and CI/CD baseline, который запускает quality gates and can deploy
the controlled instance.

**Current implementation note:** `0.2.1` Zarya Web Control Panel alignment
принес flat work-sheet shell, system overview, phased agent-work surface,
workspace/session chrome и visual regression coverage. Релизы `0.2.2` и
`0.2.3` завершили deployable server path: automatic immutable delivery из
`main`, bounded CI workspaces, явные
`prepare -> build -> deploy -> finalize` gates, root-owned deployment inputs,
state-neutral ordinary deploys, production health/SHA/Node finalization и
bounded release retention. Environment controlled instance, TLS/reverse-proxy
assumptions, persistent paths, logging и backup/restore operations описаны в
deployment и CI/CD guides.

UI follow-up `0.2.6` заменил глобальные Nodes/Jobs экраны на workspace-centered
навигацию: sidebar хранит `Nodes -> Workspaces`, workspace содержит известные
поверхности `Agent / Workbench / Jobs`, а Context Inspector появляется только
для выбранного reference. Workbench использует IDE-like file/editor/terminal
композицию, сохраняя Core/Node authority и lazy Monaco/xterm loading.

**Risk:** Этот срез легко расползается в redesign будущих поверхностей или в
притворство, что продукт уже является multi-user production release. Scope
нужно держать вокруг текущего single-user or controlled deployment, а детальный
checklist уточнять по actual daily use.

**Target direction:** Создать стабильный personal/server operating mode,
которым можно пользоваться постоянно, пока строятся trace, git/review,
registries, plugins, artifacts and task-runtime work.

### 7. Отложенные сообщения в сессии

**Value:** Позволяет человеку подготовить follow-up turn, не прерывая активного
агента и не удерживая browser открытым. Отложенное сообщение — один будущий
turn конкретной существующей сессии, а не повторяющаяся автоматизация и не
Job Run.

**Dependency:** Runtime/session admission guards и durable Core persistence;
фактическая отправка должна проходить обычным send-turn path.

**First useful slice:** Core-owned records с explicit timezone, lifecycle
`scheduled -> sending -> sent | failed | cancelled`, список внутри сессии,
edit/reschedule/send-now/cancel, пока запись остаётся `scheduled`. В назначенное
время Core проверяет обычные session/runtime guards. Если turn не принят,
запись остаётся видимой с typed reason и явным действием retry или reschedule,
а не повторяется скрытно.

**Delivered в `0.2.4`:** Core хранит запись и запускает durable dispatcher.
Перед отправкой он атомарно claim'ит запись, вызывает обычный send-turn
admission path и сохраняет typed failure для ручного retry или reschedule. UI
сессии поддерживает создание, edit/reschedule, send-now, cancel и retry.

**Target direction:** Delivery policies вроде exact-time или
not-before-when-ready, видимая history и notifications о failure. Recurrence,
запуск новой сессии, обход approvals и цепочки автоматизации остаются за
пределами этого среза.

### 8. Background Jobs и scheduled agent runs

**Value:** Добавляет управляемый unattended-work mode для повторяемой bounded
agent work, не объявляя бессмертный process или непрозрачный workflow graph
продуктовой моделью.

**Dependency:** Project/workspace placements, обычный provider execution path и
durable Core events. Provider-native managed Agent runtime из пункта `16` не
блокирует этот срез: Jobs являются unattended one-shot work contract, а не
долговечными интерактивными сессиями. Для поставленного controlled-deployment
baseline сознательно приняты изоляция отдельным OS user и/или VM и риски
unrestricted provider execution.

**First useful slice:** Paused-by-default Job definition с одним target
placement, prompt/task description и параметрами запуска, manual test run и
простыми interval/daily/weekly schedules с explicit IANA timezone. Job работает
только в текущем placement workspace; worktree и isolated task runtime
отложены. Каждый запуск сохраняется как наблюдаемый Job Run. UI показывает
конфигурацию, run history, итоговый summary, доступный provider output/logs,
typed skipped/failed outcomes и переход к run trace/evidence.
Default overlap policy — `skip`, не больше одного active run на Job.

Расписание по умолчанию использует stop-on-error policy: failed или не
стартовавший из-за runtime/admission error run приостанавливает дальнейшие
автоматические запуски Job до явного действия человека. Это opt-out параметр:
пользователь может разрешить расписанию продолжаться после ошибки. Manual run
остаётся доступен независимо от паузы расписания.

Перед автоматическим и обычным interactive start Core по возможности проверяет
provider usage limits. Если Codex сообщает, что у пятичасового или недельного
лимита осталось `5%` или меньше, новый chat/session и Job Run не запускаются с
typed reason. Пользователь может сделать explicit force start. Если provider не
даёт надёжных machine-readable данных, состояние quota должно быть `unknown`, а
не выдуманным числом; отсутствие данных само по себе не блокирует запуск.

**Target direction:** Каждый `JobRun` использует sessionless one-shot
`codex exec`: provider sandbox включён, `workspace-write` является обычным
write-профилем, а unattended approval policy не ждёт человека и возвращает
запрещённые действия модели как execution failure. Dangerous bypass не является
default и требует отдельного explicit unsafe override, если такая возможность
будет добавлена. Дальше возможны immutable configuration revisions, event и
task-tracker triggers, budgets, notifications, richer summaries/evidence,
review/PR loops, worktrees и isolated task runtimes. Первый срез исключает
visual workflow canvas, arbitrary multi-step pipelines и unlimited backfill.

**Delivered в `0.2.5`:** Core хранит paused Job definitions и Job Runs со
snapshot конфигурации, атомарно claim-ит interval/daily/weekly IANA schedule
occurrences, использует обычный placement/session/runtime path, показывает
typed overlap/failure outcomes, по умолчанию ставит schedule на pause после
ошибки и применяет общую quota admission с audited force override. Web Control
Panel показывает конфигурацию Job/run, history, summary, ссылки на session
evidence и schedule controls. Codex quota честно остаётся `unknown`, когда CLI
не даёт стабильного machine-readable usage source.

**UI follow-up в `0.2.6`:** Jobs показываются и создаются внутри текущего
workspace. Web фильтрует глобальный Core `/jobs` read endpoint по
`project_placement_id`; Core API и scheduler semantics не менялись. Legacy Job
и Job Run links разрешаются в nested workspace routes с ownership guards.

### 9. Causality and trace UX

**Реализовано в `0.2.7`:** Core хранит типизированные workspace causality
events и отдает глобальный cursor-based event log, coarse
`SessionTraceProjection`, raw event detail и permission-aware ref resolver.
Web показывает trace steps, aspect-based Inspector и фильтруемый raw fallback.
Явный Deduction запускается на Node отдельным ephemeral/read-only provider
process, получает bounded evidence snapshot, проходит Core validation по схеме
и allowlist refs, поддерживает cancel и может быть сохранён как versioned
`CausalityNarrative`.

**Value:** Снижает стоимость review, связывая result с evidence без выгрузки raw
logs в пользовательский интерфейс.

**First useful slice:** Coarse links from answers, commands, diffs, checks and
artifacts to source events, with explicit unknown/missing-cause states and raw
fallbacks.

**Target direction:** Более богатый cause graph and trace timeline после
стабилизации event quality and artifact semantics.

### 10. Git and review basics

**Value:** Developer work требует changed-file awareness and review ergonomics.

**First useful slice:** Branch/worktree snapshot, changed-file list, diff view,
check entry points, warning badges for risky workspace state.

**Current implementation note:** `0.2.10` добавляет Node-owned porcelain-v2 Git
snapshot, persisted Placement git facts, same-repo/branch coordination warning,
changed-file scopes `all / staged / unstaged`, bounded per-file Monaco diff с
binary/raw fallback и resolvable diff/hunk refs. Workbench Review запускает
`make l` и `make c` через существующий async bounded command path, показывает
progress/cancel и долговечную типизированную историю check results.

**Target direction:** Git provider integration, PR/MR comment import, review
queues, CI follow-up loops and review-ready task outputs.

### 11. Agent Tooling and Tool Registry v1

Рабочий план реализации:
[`0.2.11-agent-tooling-tool-registry.md`](../tmp-plans/0.2.11-agent-tooling-tool-registry.md).

**Value:** Агент как first-class citizen получает единый machine interface к
Uprava и внешним integrations, а tools становятся системными capabilities с
permissions, Node/project/session scope, routing, schemas, trace and audit
policy вместо скрытого agent behavior.

**First useful slice:** Core-owned registry для managed tools и observed Node
capabilities; Uprava MCP как основной agent-facing interface; обязательный
progressive discovery `Search -> Inspect -> Execute`; ToolHive-backed runtime с
одним реальным внешним MCP server; effective availability, permissions,
routing and end-to-end tool-call trace.

Uprava не передаёт модели полный каталог schemas. Core/host индексирует upstream
`tools/list`, применяет policy и возвращает через Search только имена и краткие
описания. Inspect раскрывает полную схему одного выбранного tool. Execute заново
проверяет schema, permission and availability перед routing.

Provider-native и Node-local инструменты (`bash`, file tools, `git`, `gh`,
`glab`) не оборачиваются и не проксируются без отдельной продуктовой причины.
Uprava сообщает их version, health and safe authentication status как observed
capabilities, а агент вызывает native CLI напрямую.

**Current implementation note:** `0.2.11` добавляет Core-owned scoped registry,
permission-first `Search -> Inspect -> Execute`, Streamable HTTP Uprava MCP с
краткоживущими session leases, Node inventory и desired/actual reconciliation,
pinned отдельный Compose ToolHive bridge к официальному Linear MCP, Web connect/reconnect/
disconnect и redacted tool-call trace. Linear authorization URL существует
только в эфемерном ответе текущему Web-клиенту; OAuth callback, discovery и
read-only execution готовы к ручной opt-in приёмке, но ещё не подтверждены.

**Target direction:** Более богатый MCP catalog, dynamic server selection,
programmatic tool calling/code mode, дополнительные runtime providers,
approval policies and first-class integration UX. Отдельный Uprava CLI
добавляется только при подтверждённых shell-composition, streaming or batch
сценариях.

### 12. Plugin Registry v1

Рабочий план реализации:
[`0.2.12-plugin-registry-dark-theme.md`](../tmp-plans/0.2.12-plugin-registry-dark-theme.md).

**Value:** Uprava становится extensible без hardcoding каждого tool, block and
integration внутри workbench. Plugin Registry расширяет саму Uprava и не
является разновидностью Tool Registry или integration catalog.

**First useful slice:** Core-owned packages/installations, versioned manifest,
compatibility, configuration and permissions; permission-filtered contribution
projection; Web Extension Host с first-class `ui.theme` contribution; bundled
data-only `uprava.theme-dark`, который можно enable, выбрать, disable и безопасно
заменить на `core.light` без reload or broken UI.

Theme меняет только allowlisted semantic tokens, Monaco theme and terminal
palette. Plugin не получает arbitrary CSS, DOM access or JavaScript execution в
main React tree. Первый slice также приводит first-party UI к theme-safe tokens
и добавляет light/dark visual and contrast gates.

**Current implementation note:** `0.2.12` добавляет отдельные protocol,
persistence and application boundaries Plugin Registry, migration 13,
идемпотентный bundled-package bootstrap, enable/disable and compatibility
lifecycle, permission-filtered effective projection, Plugins/Appearance UI и
versioned preference с безопасным light fallback. `uprava.theme-dark@1.0.0`
является data-only package; arbitrary CSS and executable plugin code в этот
срез не входят.

**Target direction:** VS Code/Obsidian-like package lifecycle, local/team
installation, signed catalogs, activation/context keys, plugin-provided
commands, Workbench views/tabs, Inspector aspects, renderers, link handlers,
artifact types, workflow templates, services and governed sandboxed extension
surfaces. Следующие функциональные направления должны расширять Uprava как
bundled first-party plugins через те же versioned contracts, которые позже
будут доступны внешним plugins. После data-only theme следующими доказательствами
платформы становятся artifact plugins и dynamic UI plugin; Git Review остаётся
кандидатом отдельного functional bundled plugin.

### 12b. Plugin contribution resolution

**Value:** Несколько plugins могут расширять один target без зависимости от
порядка загрузки и без скрытого выбора победителя. Конфликты остаются возможны,
но становятся детерминированными, видимыми и управляемыми пользователем.

**First useful slice:** Каждый поддерживаемый extension point определяет
bounded normalized target и platform-owned mode: `exclusive`, где применяется
первая доступная contribution, или `ordered`, где применяются все contributions
по порядку. Host использует стабильный default order, позволяет пользователю
переставлять и отдельно отключать contributions для target и показывает в
Plugin Panel plugins, конфликтующие на одинаковом exclusive target.

Первый acceptance case — два content renderer-а для
`chat.assistant_message` на `session.timeline`: текущий победитель и fallback
alternatives видны, порядок можно изменить, а render result не зависит от
порядка effective projection, lazy loading or React mounting.

**Scope boundary:** В первый contract не входят универсальная scope algebra,
dependency/conflict graph, `before`/`after`, arbitrary numeric priorities,
constraint solver, heuristic specificity ranking и runtime collision history.
Подробная модель зафиксирована в
[`A-012 Plugin Contribution Resolution`](../systems/areas/012-plugin-contribution-resolution.md).

**Current implementation note:** `0.2.16` добавляет versioned contribution
identity and provenance, normalized `visual.renderer` targets, migration 14 с
revisioned per-target order/disable preferences, deterministic exclusive
resolver and conflict metadata. Plugin Panel показывает winner and alternatives,
позволяет менять порядок, отключать отдельную contribution и сбрасывать order.
Bundled `uprava.markdown` и `uprava.plain-text` образуют первый acceptance chain;
ошибка или недоступность renderer-а переводит Host к следующему candidate, затем
к обязательному raw-text fallback.

### 13. Visual artifact system as plugins

**Value:** Results such as diffs, checks, timelines, reports, diagrams and
dashboards должны быть inspectable UI objects, а не только chat text.

**Delivery rule:** Пользовательская функциональность поставляется как один или
несколько bundled first-party plugins поверх Plugin Registry and Web Extension
Host. Базовая система владеет generic artifact identity, storage, refs,
permissions, contribution validation, renderer isolation and fallback, но не
hardcode-ит каждый artifact type или его UI. Bundled plugins используют тот же
versioned extension contract, который предназначен для будущих local/team/
community plugins.

**Content pickup rule:** Обычный путь не требует просить агента выдать
специальный artifact descriptor или заранее выбранный UI. Агент пишет обычный
Markdown и использует естественный формат данных; Extension Host подхватывает
его зарегистрированными content/inline renderers. Например fenced code block
получает syntax highlighting, строгий color literal становится активным color
token, а Mermaid/PlantUML fence — diagram preview. Сохраненный текст и его
source range остаются source-of-truth и обязательным fallback. Такое
обогащение само по себе не создает durable artifact; pin/save/export или
review-valued tool result могут отдельно превратить visual object в artifact.

**First useful slice:** Generic visual/artifact contract плюс bundled content
and artifact plugins для Markdown/code rendering, color tokens,
Mermaid/PlantUML diagrams, diff/check reports and trace timeline with source
references, viewers and readable fallbacks. Plugin можно disable или сделать
incompatible без поломки App Shell и без потери доступа к исходному тексту,
raw metadata or evidence.

**Plugin platform increment:** Активировать manifest contributions для
content/inline renderers and source matchers, `artifact_types`,
`block_renderers`, artifact viewers, commands/actions and related context keys;
провести их через Core-owned lifecycle, compatibility, permissions and
effective projection. Acceptance требует одновременно полезной visual/artifact
функции и переиспользуемого контракта, на котором следующий plugin может
подхватить новый source format или добавить свой artifact type без изменения
базового Web shell.

**Target direction:** Artifact gallery, richer visual review, dashboards, UML,
forms and embedded external views, предоставляемые first-party и внешними
plugins. Новые artifact families должны преимущественно добавляться packages,
а generic artifact kernel и Extension Host оставаться небольшими и стабильными.

**Current implementation note:** `0.2.17` добавляет `visual.renderer` v2,
declarative fenced-language/strict-color matchers, `artifact.type` v1 и общий
exclusive resolver для renderer and artifact-type targets. Migration 15 и
Core API хранят immutable artifact versions, scope, provenance, typed refs и
readable fallback. Bundled plugins подхватывают color literals и
Mermaid/ограниченный PlantUML из обычного Markdown, монтируют review/trace
blocks и viewers, а Web позволяет закреплять diagram, diff/check и trace
snapshots. Disabled/incompatible/unknown or failed renderer не лишает
пользователя исходного текста, metadata или raw evidence.

### 14. Dynamic UI from agents as plugins

**Value:** Agents and tools могут возвращать structured interactive surfaces там,
где text имеет неправильную форму.

**Delivery rule:** Dynamic UI реализуется как bundled first-party plugin поверх
artifact и renderer contracts пункта `13`, а не как привилегированная ветка
основного React tree. Базовая система владеет validation, persistence,
permissions, command/event routing, sandbox boundary and fallback. Plugin
предоставляет versioned Generated React runtime, Uprava React SDK, design
tokens, layout contract, renderers and UI-specific contributions через
общие extension points. Generated code не монтируется в main React
tree даже в trusted mode.

**First useful slice:** Выключенный по умолчанию bundled Generated React
UI plugin. Агент создает версионируемый React/TypeScript artifact,
который проходит controlled build/validation pipeline и исполняется в
sandboxed iframe с жестким CSP/capability policy. Plugin предоставляет
Uprava UI SDK, responsive layout primitives, permissioned action bridge,
persisted state, sanitized/static snapshot and markdown/table/raw fallback. Его
disable, version mismatch, build или render failure оставляют reviewable
artifact and fallback вместо broken surface.

**Plugin platform increment:** Добавить versioned contributions для
generated UI runtimes, React SDK/API compatibility, dynamic renderers, layout
intents, permissioned action bridge, plugin-owned configuration/context keys
and sandbox capabilities. Acceptance требует, чтобы другой plugin мог
переиспользовать эти contracts для нового generated UI family без
изменения App Shell или обхода Core authorization. Declarative component
schema может остаться опциональным fast path для простых forms/cards,
но не является главным expressive model.

**Target direction:** Plugin-rendered blocks, Generated React artifacts,
controlled embeds, capability-scoped runtimes and agent-readable UI state.
Каждый новый dynamic UI family должен
одновременно улучшать пользовательскую функцию и приближать package lifecycle,
extension points, isolation and interoperability к уровню Obsidian/VS Code.

**Current implementation note:** `0.2.18` поставляет выключенный по умолчанию
`uprava.generated-react` с versioned `generated_ui.runtime`,
`generated_ui.sdk` and `generated_ui.action_bridge` contributions. Migration 16
атомарно связывает artifact version, content-addressed source, build, persisted
state и idempotent action requests. Native tool `uprava.dynamic_ui.create`
принудительно создаёт session-scoped proposal; отдельный network-isolated
builder принимает только React and `@uprava/ui-sdk`, фиксирует dependency lock
и не исполняет generated source. Web загружает готовый bundle только в opaque
`sandbox="allow-scripts"` iframe с nonce CSP and MessageChannel bridge. State,
agent input и declared reference actions повторно авторизуются Core; build,
runtime, plugin disable or render failure сохраняют markdown fallback,
ограниченный PNG/WebP snapshot, diagnostics и reviewable source.

### 15. Task-based sandbox runtime

**Value:** Uprava может запускать bounded background work with explicit scope,
isolation, evidence and review-ready output.

**Delivery rule:** Первый runtime backend — отдельный pinned OpenSandbox service
с Docker runtime. Node управляет им напрямую по HTTP/OpenAPI через replaceable
`TaskRuntimeBackend`; JavaScript/Python SDK runner, Kubernetes and microVM не
входят в baseline. Uprava сохраняет authority над `TaskRun`, worktree, events,
evidence and review, а OpenSandbox отвечает за container/command lifecycle,
TTL, mounts and resource limits.

**First useful slice:** Task contract, host-owned isolated worktree/branch,
custom versioned Codex image, persistent file-based Codex credential profile,
context package, streamed event log, cancellation/TTL cleanup, expected
evidence and result package. Перед реализацией короткий spike должен подтвердить
Compose/Docker path handling, повторное использование auth, SSE execution,
process cancellation and целевой idle budget.

Полный принятый baseline и spike criteria:
[`A-013 Task-based Sandbox Runtime`](../systems/areas/013-task-based-sandbox-runtime.md).

**Target direction:** Durable workflow state, queues, CI/webhook wakeups, PR/MR
flow and reproducible review packages. VM-grade isolation and external/managed
providers могут появиться позже за тем же backend contract, когда возникнет
подтверждённая потребность.

**Current implementation note:** `0.2.19` реализует durable Core contract и
migration 17, capability-gated dispatch, Node-owned linked worktree,
OpenSandbox lifecycle/exec adapter, readiness polling, cancel/timeout, restart
reconciliation, bounded diff/check/artifact evidence, digest-pinned task image
и Tasks UI. API key OpenSandbox и persistent Codex `CODEX_HOME/auth.json` не
реализованы по явному решению текущего среза; до их добавления runtime profile
остаётся controlled-development only, а пункт отмечен частично выполненным.

Для Tasks `--dangerously-bypass-approvals-and-sandbox` является осознанной
provider policy: Codex исполняется unrestricted **внутри** внешней границы
OpenSandbox. Итоговая безопасность определяется изолированным worktree,
container mounts, credentials, network, resource limits, timeout and TTL, а не
внутренним sandbox Codex. Trace/evidence должен показывать оба слоя явно.

### 16. Managed Agent Work Loop

**Value:** Agent становится основной поверхностью живой совместной работы с
Codex, а не chat wrapper над последовательностью `codex exec`. Пользователь
ведёт долговечную рабочую сессию, видит structured activity, отвечает на
approvals и provider questions, вмешивается в выполнение, отключается и
возвращается без потери session/workspace context.

Речь идёт о TUI-equivalent возможностях через provider-native protocol, а не о
встраивании Codex TUI или эмуляции terminal UI внутри Web Control Panel.

**Dependency:** Существующая Agent/session surface, Core/Node command and event
path, durable session state и доказанный provider-native managed protocol,
способный поддерживать двустороннее выполнение, approvals, interruption and
reconnect/resume. Task-based sandbox runtime не является зависимостью этого
Agent slice.

**First useful slice and exit criteria:**

1. создание Agent session по умолчанию запускает provider-native managed Codex
   runtime, а не отдельный `codex exec` на каждый turn;
2. Core и Node поддерживают live streaming, structured tool/command activity,
   provider questions, real approval round-trip, interrupt, stop,
   detach/reattach и controlled resume/recovery;
3. provider sandbox является safe default, а effective sandbox/approval policy
   видна до start, во время работы и в trace/evidence;
4. unrestricted execution доступна только как явный unsafe choice и никогда не
   включается через скрытый fallback;
5. текущий `codex exec/resume` с
   `--dangerously-bypass-approvals-and-sandbox` сохраняется как отдельный
   надёжный **Exec compatibility mode** для Agent;
6. потеря Web/Core connection не уничтожает session thread; потеря provider
   process приводит к observable reconnect/resume или честному degraded state.

**Execution profiles:**

- **Agent / Managed** — provider-native live runtime, safe sandbox, interactive
  approvals and user input; основной режим;
- **Agent / Exec compatibility** — `codex exec/resume`, unrestricted provider,
  без обещания настоящего approval continuation; явный fallback с постоянным
  warning/effective-policy badge;
- **Tasks** — one-shot `codex exec`, unrestricted provider внутри OpenSandbox;
  Task contract и внешний sandbox остаются отдельной поверхностью;
- **Jobs** — целевой sessionless one-shot `codex exec`, sandbox enabled и
  non-interactive approval policy по умолчанию; переход Jobs на этот contract
  является отдельным follow-up и не блокирует поставку Agent runtime.

**Accepted risk before delivery:** Audit finding P0-3 остаётся accepted risk для
controlled deployment и Exec compatibility mode. Текущие normalized
`approval.requested` events не считаются реальным enforcement: managed mode
должен продолжать тот же provider execution только после решения Core/User/Node.

**Current implementation note:** `0.2.20` закрыл stage 0 architecture gate:
выбран experimental Codex app-server v2 из `codex-cli 0.144.1` over local
WebSocket, подтверждены bidirectional approvals/input, interrupt,
reconnect/resume, policy echo and MCP credential boundary, приняты
process-per-`RuntimeAttempt` topology and typed degraded recovery. Production
`0.2.21` закрыл stage 1 foundation: shared Rust/Web execution profiles,
attempt/interaction identity, immutable effective policy/hash, migration 18 и
profile-aware admission без silent fallback. `0.2.22` закрыл stage 2:
Node-owned process-per-attempt app-server driver, semantic stream,
approval/input continuation, interrupt/stop, resume descriptor и restart
reconciliation. `0.2.23` закрывает stage 3: Core policy preview/admission,
атомарный `requested -> resolving -> terminal` interaction lifecycle, ordered
turn/runtime projection, attempt-aware reconnect, recovery audit and bounded
metrics. `0.2.24` закрывает stage 4: Web явно выбирает profile, показывает
policy preview и persistent effective-policy diagnostics, рендерит semantic
timeline и typed approval/question cards, а interrupt/stop/detach/resume
следуют projected capabilities. `0.2.25` закрывает stage 5: Managed стал
default для новых Agent sessions на capable Node, missing profile больше не
включает Exec fallback, existing sessions сохраняют stored profile, Jobs и
Tasks остаются изолированными, provider process environment ограничен
allowlist, а real-host acceptance и recovery matrix оформлены отдельным
[runbook](../runbooks/managed-agent-runtime.md).
`0.2.26` закрывает post-completion deployment hardening: Managed MCP lease
сохраняет process continuity и ротируется через provider-native resume,
workspace повторно канонизируется на Node, interrupt имеет bounded
TERM/KILL escalation, idle expiry действительно останавливает Node process и
отзывает lease, а host gate автоматически проверяет approve, deny, input,
interrupt и MCP.

**Target direction:** Provider-neutral managed runtime contract, richer
TUI-equivalent interaction, runtime recovery, checkpoints and handoff. Agent
позже может получить tool для делегирования bounded `TaskRun`, но такое task
spawning больше не является headline или exit criterion этого пункта.

Крупные этапы и release gates разложены во временном
[implementation plan](../tmp-plans/16-managed-agent-work-loop.md).

### 17. Team/cloud model

**Value:** Uprava расширяется от personal workbench до shared distributed Agent
OS.

**First useful slice:** Multi-user projects, roles, shared node visibility, team
audit trail and managed Core deployment path.

**Target direction:** Managed cloud nodes, node pools, organization-level
plugin/integration governance, stronger secrets model and billing if needed.

### 18. Beyond software development

**Value:** Та же node, agent, tool, artifact, trace and workflow model может
поддерживать broader knowledge work.

**First useful slice:** Выбрать одну non-code vertical только после того, как
developer artifact/plugin model станет достаточно сильной для переноса.

**Target direction:** Research, analytics, documents, presentations, finance,
monitoring and knowledge-base workflows.

### 19. Audit follow-up refactors

**Value:** Сохраняет `0.1.6` audit fixes reviewable, contract-backed and ready
for longer-running tools, не смешивая broad mechanical work с behavior
hardening release.

**First useful slice:** Разделить Core command/event/session code и Node
state/command-runner code into focused modules under current public interfaces;
add generated or schema-checked web protocol contracts; design async workspace
command API для команд, которые перерастают bounded synchronous execution.

**Target direction:** Сделать command lifecycle, session projection, Node state
store, workspace command execution and web protocol shapes independently
testable до того, как Tool Registry and external integrations увеличат surface
area.

## Открытые вопросы очереди

- Насколько строгим должен быть первый security baseline, прежде чем
  рекомендовать non-local node?
- Какие daily-use hardening items обязательны до первого continuously used
  server deployment, а что может подождать следующих feature slices?
- Какой реальный external MCP server лучше взять для ToolHive-backed acceptance
  scenario после обязательного Uprava-native proof?
- Насколько маленькой может быть первая visual artifact system, чтобы при этом
  уже изменить product experience beyond text?
- Какой Codex managed protocol даёт достаточно стабильные lifecycle,
  approval, interrupt and reconnect contracts для основного Agent mode?
- Нужна ли отдельная `RuntimeAttempt` identity для перезапусков managed
  provider process или достаточно одной `RuntimeSession` с attempt events?
- Какой workspace concurrency guard должен не позволять sessionless Job Run
  конфликтовать с живой Agent session на том же placement?
