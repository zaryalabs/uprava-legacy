# Системные направления Uprava

Статус: `active`

Этот раздел нужен для глубокой проработки **ключевых системных направлений**
Uprava.

Направление — это не список внутренних модулей, таблиц и API, а иерархическая
работа над большим продуктово-архитектурным решением:

1. Сначала сформулировать vision ключевой механики: суть идеи, продуктовую логику и почему это важно для Uprava.
2. Затем развернуть vision в architecture: сущности, границы, сценарии, lifecycle, контракты, artifacts/events, хранение, permissions, UI consequences и проверку качества.

Документ направления начинается с корневого vision-блока, а затем постепенно
разворачивается в architecture-блок. Внутри architecture глубина может расти от
концептуальной модели до технических контрактов.

## Что такое ключевая механика

Ключевая механика - это крупный продуктово-архитектурный механизм Uprava, который задает модель работы пользователя, агента, UI и backend-системы.

Это не отдельный модуль кода, не частная feature и не пользовательский flow. Внутри ключевой механики может быть много модулей, сценариев, UI-состояний, событий и технических решений, но сама механика описывает, **как работает важная часть системы**.

Примеры ключевых механик:

- distributed architecture: как Uprava реализует Core / Node Daemon / clients модель;
- distributed runtime coordination: как Core координирует runtime work на конкретных Node/workspace placements, dispatch, events, stale/offline, resource warnings and overrides;
- modular UI: как устроена модульная рабочая поверхность, блоки, панели и расширяемость интерфейса;
- Plugin Registry and Extension Host: как packages расширяют саму Uprava через
  themes, commands, views, renderers and other governed contributions;
- Tool Registry and integrations: как подключаются callable tools, MCP,
  native adapters and external systems;
- dynamic UI: как агент может вернуть форму, dashboard, graph, embedded view или другой интерактивный блок;
- visual rendering and artifact semantics: где и как Uprava рендерит visual objects, что является source-of-truth, и когда view становится artifact;
- go to source / causality navigation: как пользователь переходит от результата, diff, ошибки или artifact к источнику, evidence and причине;
- run mode: как Uprava запускает Agent через provider-native managed runtime
  или exec/resume compatibility path, а Tasks и Jobs — через отдельные one-shot
  contracts с собственной isolation and approval policy;
- human-agent dual interface: как человек и агент работают с одной видимой моделью, где agent является first-class citizen.

Внутри каждой ключевой механики может быть много технических решений, но сначала нужно выбрать саму модель.

## Как писать документы направлений

Файлы в этой директории лучше именовать по ключевой механике:

```text
docs/systems/areas/001-distributed-architecture.md
docs/systems/areas/002-run-mode.md
docs/systems/areas/003-distributed-runtime-coordination.md
docs/systems/areas/004-modular-ui-work-surface.md
docs/systems/areas/005-dynamic-ui-from-agents.md
docs/systems/areas/006-visual-rendering-and-artifact-semantics.md
docs/systems/areas/007-plugins-tool-registry-and-mcp-strategy.md
docs/systems/areas/008-go-to-source-and-causality-ux.md
docs/systems/areas/009-human-agent-dual-interface.md
docs/systems/areas/010-project-workspace-surface.md
docs/systems/areas/011-background-jobs.md
docs/systems/areas/012-plugin-contribution-resolution.md
docs/systems/areas/013-task-based-sandbox-runtime.md
```

Рекомендуемая структура документа:

```text
# Название ключевой механики

Статус: draft / working-position / accepted / superseded

## Vision

### Какую проблему решает механика
### Концептуально как реализуем
### Пользовательские сценарии
### Agent-facing сценарии
### Scope boundaries / release constraints, if useful

## Architecture

### Основные сущности
### Границы ответственности
### UI consequences
### Detailed lifecycle and state machines
### API/protocol contracts
### Artifact/event formats
### Storage implications
### Permissions and failure modes
### Tests/evals/checklist
```

Главное правило: сначала фиксируем **vision и суть механики**, потом разворачиваем это в **architecture**. Сценарии и UI появляются уже в `Vision`, но детальные состояния, контракты, storage and permissions живут в `Architecture`.

## Сквозные принципы

Эти принципы должны проходить через все ключевые механики:

- Uprava - Distributed Agent OS, а не agent chat with panels.
- Agent output не равен accepted work.
- Продукт должен снижать стоимость review, handoff, return и ownership decision.
- Модульность является архитектурным принципом, а не маркетплейсом поверх монолита.
- Visual artifacts и dynamic UI - способ понимания и управления работой, а не декоративный слой.
- Интеграции должны быть visible and traceable: не скрытые API calls внутри текста агента.
- Provider-native managed, stateless/ephemeral, externally sandboxed and
  sessionless strategies должны быть явными runtime profiles одного Run Mode,
  а не скрытыми вариациями общей session.
- Distributed Runtime Coordination должен быть общим dispatch/resource-awareness слоем для interactive sessions, future task runs and sandboxed runtimes.
- Human UI и agent-readable UI должны развиваться вместе.
- Local development and UI verification являются частью проектирования системы: Docker
  Compose должен давать reproducible hardened Core/Web/ToolHive/Generated UI Builder setup, while host Node
  Daemon and Codex provider execution are covered by separate smoke paths.
  Playwright должен покрывать automated E2E checks и agent/operator inspection
  через `playwright-cli`.
- V01 должен быть маленьким, но не должен закрывать путь к plugins, visual blocks, task-based runtime, mobile and team/cloud.

## Карта ключевых механик

| ID | Механика | Ключевые вопросы механики | Ожидаемый результат |
| --- | --- | --- | --- |
| A-001 | Distributed architecture | Как именно реализуем distributed модель? Что является Core/control plane, что остается за Node Daemon/data plane, как клиенты работают через Core, где проходят границы host/node/workspace/session? | Рабочая позиция по Core / Node Daemon / clients модели, deployment profiles and responsibility boundaries. |
| A-002 | Run Mode | Как устроены provider-native Managed Agent Work Loop, явный exec/resume compatibility mode и отдельные one-shot contracts Tasks/Jobs? Где проходят execution policy, session, isolation and lifecycle boundaries? | Managed Agent как целевой основной режим; compatibility exec как явный fallback; Tasks и Jobs со своими provider policy, trace и lifetime contracts. |
| A-003 | Distributed Runtime Coordination | Как Core координирует runtime work между session thread, runtime session, workspace placement and Node? Как dispatch-ятся commands, как упорядочиваются events, как UI видит node/workspace tree, stale/offline, resource warning badges and overrides? Как git repo/branch signals подсвечивают возможные конфликты без lock-системы? | Рабочая модель coordination layer для V01: Nodes -> Projects/Workspaces tree, command proxy, idempotency, event ordering, resource signals, warning badges, override events and reuse by future task/sandbox runtimes. |
| A-004 | Modular UI and work surface | Что значит модульный UI для Uprava? Это Notion-like blocks, IDE/workbench panels, Obsidian-like navigation, plugin-rendered surfaces или гибрид? Где проходят границы pages, panels, blocks, artifacts, integration surfaces and extension points? | Модель рабочей поверхности и Plugin Registry: VS Code/Obsidian-like packages, manifests, Extension Host, activation/context keys, themes, typed contributions, trust/runtime levels and constraints for React/Vite UI. |
| A-005 | Dynamic UI from agents | Как агент должен возвращать форму, dashboard, chart, graph, embedded tool или custom block? Как исполнять generated React, не отдавая ему main workbench tree? | Plugin-first концепция dynamic UI: opt-in bundled Generated React plugin, sandboxed artifact runtime, Uprava UI SDK, layout/action contracts, safe fallback и путь к внешним plugins без privileged React path. |
| A-006 | Visual rendering and artifact semantics | Где и как Uprava рендерит visual objects: inline Markdown diagrams, editor/viewer enhancements, diff/terminal/test views, charts, dashboards, external previews and artifacts? Что является source-of-truth, когда visual view становится artifact, какие refs/actions/fallback нужны? | Сквозная visual semantics и plugin-first artifact delivery: generic base contracts плюс first-party/external artifact types, renderers and viewers через Plugin Registry. |
| **A-007** | Agent Tooling, Tool Registry and MCP strategy | Как Core-owned registry, Uprava MCP, progressive discovery, ToolHive runtime и observed native capabilities образуют единый agent tooling contract? | Модель managed tools and observed capabilities: Search/Inspect/Execute, Node/project/session availability, routing, permissions, events and trace. |
| A-008 | Go to source and causality UX | Как сделать аналог go to definition, но для агентской работы? Как из answer, diff line, failed check, artifact, decision, status или UI block перейти к source/evidence/cause: prompt/context/tool call/command/event/file change/raw log? Что является source/cause graph, а что просто log noise? | Модель UIUX причинности: навигация от результата к источнику, evidence and причине, минимальная модель source/cause links without dumping raw trace. |
| **A-009** | Human-agent dual interface and Agent as First-Class Citizen | Как сделать UI понятным и человеку, и агенту? Что такое machine-readable UI state, context entry points, internal Uprava agent, chat over UI element, agent identity, capabilities, status, memory, permissions and ownership? | Модель dual interface, где agent является видимым участником системы, а не скрытым процессом за текстовым чатом. |
| A-010 | Project Workspace Surface | Как пользователь видит и меняет конкретный workspace агента? Где живут file tree, file viewer/editor, terminal/PTY, command history, diff/check views and "open full IDE" sidecar? Как Core/Node Daemon обеспечивают permissions, path boundaries, edit lifecycle, trace and addressable workspace refs? | Модель post-V01 workspace surface: inspect-first, edit-light, terminal-capable, traceable, with optional full IDE sidecar later. |
| A-011 | Background Jobs | Как задать prompt-first unattended work, запускать его вручную или по расписанию, наблюдать runs и останавливать расписание после ошибок без преждевременного workflow engine? | Модель Job/Job Run для controlled deployment: current workspace, durable schedule, summary/output UX, stop-on-error и shared Codex quota admission. |
| A-012 | Plugin Contribution Resolution | Как несколько plugins воздействуют на один target, в каком порядке применяются contributions и как пользователь видит и разрешает exclusive conflicts? | Минимальная target-based модель с режимами `exclusive`/`ordered`, детерминированным изменяемым порядком и conflict controls в Plugin Panel. |
| A-013 | Task-based Sandbox Runtime | Как запускать bounded agent work в изолированном workspace, не реализуя Docker orchestration внутри Uprava и не вводя тяжёлую cloud platform? Как переиспользовать Codex login между runs? | Docker-first модель с отдельным OpenSandbox service, host-owned worktree/evidence, custom Codex image, persistent credential profiles and replaceable `TaskRuntimeBackend`. |

Не все важные темы являются отдельными ключевыми механиками. Некоторые стоит
держать как пользовательские сценарии или срезы внутри документов направлений:

- Distributed Agent Control Panel - главный V01 сценарий для `A-002 Run Mode`,
  `A-003 Distributed Runtime Coordination` and `A-004 Modular UI and work
  surface`. Developer workbench surfaces начинаются как post-V01 feature queue
  slices, начиная с workspace references and read-only inspector.
- Workflow and harness - сценарный срез для длинной работы, который проверяет `A-002 Run Mode` and `A-003 Distributed Runtime Coordination`, но не заменяет их.
- Integration UX - частный случай модульности, plugins/tools and visual blocks; его нужно раскрывать внутри `A-004`, `A-005`, `A-006` and `A-007`.
- Security, permissions and trust - обязательный architecture-срез для execution
  modes, plugins/tools, dynamic UI and agent identity. Security baseline является
  первым post-V01 implementation slice, но не отдельной key mechanism на карте
  map.
- Metrics, observability and evals - quality/feedback-срез для проверки механик, а не самостоятельная ключевая механика.
- Mobile continuity, deployment/bootstrap and beyond software development - важные constraints/product horizons, но не ключевые механики текущей проработки системных направлений.

## Глубина проработки

Для каждой ключевой механики сначала нужен `Vision`. Это не summary, а корневой смысловой блок: какую проблему решаем, какую модель предлагаем, какие человеческие и agent-facing сценарии считаем ключевыми, and какие scope/release constraints уже понятны, если они полезны для конкретного направления.

`Architecture` можно заполнять постепенно. Не каждая ключевая механика сразу
должна доходить до подробных state machines, API contracts, storage implications
or tests/evals. На текущем этапе важно создать документ по каждой механике из
карты, зафиксировать в каждом `Vision`, а затем углублять `Architecture` там,
где решение критично для V01 или блокирует соседние механики.

## Что считать готовым результатом этой фазы

Проработка системных направлений будет полезной, если после неё станет понятно:

- по каждой ключевой механике из карты есть документ направления с корневым
  `Vision` и заготовкой `Architecture`;
- как устроены ключевые механики Uprava: distributed architecture, run mode, distributed runtime coordination, modular UI, plugins/tools, dynamic UI and visual rendering/artifact semantics;
- какие решения обязательны для V01, а какие только накладывают constraints на архитектуру;
- какие идеи из Notion/Obsidian/IDE/Grafana/MCP мы берем, а какие не берем;
- где проходит граница между product concept, architecture and implementation detail;
- какие решения созрели для переноса в общую архитектуру, продуктовые документы
  или очередь реализации.

## Открытые вопросы верхнего уровня

- Должен ли dynamic UI быть частью modular UI system или отдельным artifact/runtime слоем?
- Нужно ли начинать с набора фиксированных block types или сразу проектировать plugin-rendered blocks?
- Как развивать split между Core-owned MCP policy/discovery, host Node и
  отдельным Compose ToolHive bridge, сохраняя единый agent-facing endpoint?
- Где граница между plugin, integration, tool, block and artifact?
- Как изучить Notion-like modularity practically: как data model, как UI composition, как plugin model или как interaction pattern?
- Где visual representation должен быть inline/viewer enhancement, где отдельным block, где artifact, а где external preview/embed?
- Какой минимальный source/cause graph нужен для go to source, чтобы помогать review, но не превращаться в шумный trace log?
- Как именно agent должен быть представлен в UI как first-class citizen: identity, status, permissions, memory, capabilities или отдельный work object?
- Как развивать bounded task contract после принятого Docker/OpenSandbox
  baseline, не смешивая его с interactive session и не перенося Core-owned
  workflow/evidence state во внешний runtime?
- Как не сделать слишком абстрактную платформу до появления рабочего developer workbench?
