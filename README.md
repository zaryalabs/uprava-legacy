# Uprava

Uprava — распределённая агентская операционная система для масштабной работы с
ИИ-агентами.

Продукт начинается со сценариев разработки ПО, а затем может расшириться на
аналитику, исследования, финансы, документы и другие виды интеллектуального
труда. Первое практическое направление — рабочая среда разработчика для живой
работы агентов на нодах через управляемый Persistent Runtime и слой
распределённой координации runtime.

## Продуктовая идея

Большинство агентских инструментов всё ещё устроены как чат. Они могут запустить
агента и показать результат, но дают мало видимости в файлы, терминал, состояние
workspace, изменения, проверки, trace, визуальные артефакты и процесс review.

Uprava должна стать control plane и рабочей поверхностью для агентских нагрузок:

- **Core Backend** — control plane.
- **Node Daemon** — data plane на локальных машинах, серверах, devbox,
  sandbox-окружениях и облачных нодах.
- **Web Control Panel** — первый клиент.
- **Workspace-centered Web Control Panel** — общий workspace с поверхностями
  Agent, IDE-like Workbench и Jobs, условным Context Inspector, деревом файлов,
  лёгким редактированием текста, workspace-терминалами и diff.
- **Managed Agent Work Loop и Agent Provider Adapter** — provider-native live
  Codex session как основной Agent mode; текущий `codex exec/resume` остаётся
  явным compatibility mode, а Tasks/Jobs используют отдельные one-shot
  execution contracts.
- **Distributed Runtime Coordination** — dispatch, порядок событий, размещение
  node/workspace и предупреждения о ресурсах между Core, Node Daemon и runtime.
- **Agent Tooling, Tool Registry и Plugin Registry** — Core-owned capabilities,
  Uprava MCP с progressive discovery и ToolHive-backed integrations как основа
  модульности.
- **Визуальные артефакты и трассируемость** — принципы первого уровня.

## Текущее состояние

Текущий baseline репозитория — `0.2.26`. Он доводит Managed Agent Work Loop до
deployment-ready состояния:
Managed является capability-aware default для новых Agent sessions, Core не
переходит в unrestricted Exec молча, existing sessions сохраняют stored
profile, а Jobs и Tasks остаются на отдельных execution contracts. Host-only
acceptance проверяет safe policy, approve/deny/input, interrupt, MCP, два
Managed turn с detach/reattach, stop/resume и reload, а также явный Exec
compatibility path. Managed MCP credential обновляется через native resume,
idle expiry завершает Node process и отзывает lease, а workspace повторно
проверяется на Node. Provider child process получает только allowlisted
environment. Baseline наследует Web work
surface `0.2.24`, Core orchestration `0.2.23`, Node-managed Codex runtime
`0.2.22`, shared foundation `0.2.21`, protocol gate `0.2.20` и `0.2.19` с
durable Task Runs, isolated Git worktrees, Docker/OpenSandbox lifecycle,
bounded checks/evidence, cancellation/recovery и отдельная Tasks surface.
Persistent Codex credential profile и OpenSandbox API key намеренно отложены
до ручной acceptance-проверки. Baseline также включает opt-in Dynamic UI from
Agents: bundled Generated React plugin, Core-owned versioned source/build/state/action
lifecycle, отдельный allowlisted builder, sandboxed iframe runtime, Uprava UI
SDK и обязательные fallback поверх plugin-driven Visual Artifact System,
target-based plugin contribution resolution и Markdown/Plain Text chain,
отдельным Compose ToolHive runtime, host Node/Codex и bounded
private bridge поверх Core-owned Plugin Registry v1, manifest-driven Web
Extension Host и bundled Dark Theme, Agent Tooling,
Git-aware Review, прозрачного live agent timeline, модульных Core/Node runtime
boundaries, workspace-centered Web UI, Background Jobs и protocol v2.
`V01` обозначает первый продуктовый срез, выпущенный как `0.1.0`. После него
реализовано двадцать шесть implementation slices, workspace-centered UI follow-up, единый
hardening-аудит, renderer/PTY-срез workspace и первый baseline self-hosted
CI/CD deployment и синхронизированными Linux visual baselines.

Следующее крупное продуктовое направление — team/cloud model. Отдельным
runtime follow-up остаётся sessionless sandboxed Job Run; Task Run сохраняет
собственный OpenSandbox contract.

Основные продуктовые и архитектурные документы:

- [Документация](docs/README.md)
- [Vision](docs/vision.md)
- [Архитектура](docs/systems/architecture.md)
- [Системные направления](docs/systems/areas/)
- [Task-based Sandbox Runtime](docs/systems/areas/013-task-based-sandbox-runtime.md)
- [Task Sandbox Runtime Runbook](docs/runbooks/task-sandbox-runtime.md)
- [Managed Agent Runtime Runbook](docs/runbooks/managed-agent-runtime.md)
- [Версионирование](docs/versioning.md)
- [Релизы](docs/releases.md)
- [Очередь фич](docs/product/feature-queue.md)
- [Эволюция продукта и V01](docs/product/product-evolution.md)
- [Историческая модель стадий](docs/product/product-stages.md)
- [Технический стек](docs/development/tech-stack.md)
- [Инвентарь фич](docs/product/feature-inventory.md)
- [Project Workspace Surface](docs/systems/areas/010-project-workspace-surface.md)
- [Self-Hosting Golden Path](docs/development/self-hosting-golden-path.md)
- [Единый аудит архитектуры и качества](docs/audit/audit-2026-07-09.md)
- [Исходные заметки](docs/development/uprava-notes.md)
- [Polish handoff для 0.2.0](docs/polish/README.md)
- [Временные планы](docs/tmp-plans/)

## Первая версия продукта

V01 — **Distributed Agent Control Panel**:

- Rust Core Backend;
- Rust Node Daemon;
- web control panel;
- управляемый Persistent Runtime как первый Run Mode и Codex через provider
  adapter;
- распределённая координация с деревом
  `Nodes -> Projects/Workspaces -> Sessions`, dispatch команд, порядком событий
  и предупреждениями о ресурсах и offline-состоянии;
- привязка project/workspace как контекст размещения;
- chat/session view как первая основная рабочая поверхность;
- жизненный цикл постоянной сессии: start, attach, detach, interrupt, stop,
  resume и возврат позже, если это поддерживает провайдер;
- базовое хранение nodes, projects, runtimes, sessions, messages и events;
- UI shell и типизированные command/event envelopes, позволяющие позже добавить
  workspace inspector, editor, terminal, tools, plugins, trace и artifacts без
  перестройки продуктовой модели.

Первый Codex-адаптер трактует persistent runtime как управляемую Core сессию с
сохранённым состоянием, упорядоченными событиями и provider resume references.
Непрерывность Codex использует стабильные пути `codex exec` и
`codex exec resume`, когда доступен provider session id. Владение живым
процессом, streaming вывода и настоящее interrupt escalation остаются
последующей работой.

V01 должна ощущаться как небольшая панель управления распределённой агентской
системой: сначала лишь немного прозрачнее чата, но уже организованная вокруг
нод, проектов, сессий, состояния runtime и долговечной истории событий.
Project Workspace Inspector, tools, plugins, dynamic UI и визуальные артефакты
перенесены в очередь следующих срезов.

V01 рассчитана на доверенное локальное, однопользовательское или контролируемое
development-развёртывание, а не на production security. Security baseline —
первый hardening-срез после V01.

Scope первой версии сохранён в разделе [V01](docs/product/product-evolution.md#v01).
Дальнейшая работа ведётся как очередь реализации в
[Feature Queue](docs/product/feature-queue.md), а не как
фиксированный roadmap по фазам. История версий и выпущенных срезов находится в
[Versioning](docs/versioning.md) и [Releases](docs/releases.md).

## Предварительный технический стек

```text
Rust workspace
Axum Core Backend
Rust Node Daemon
SQLite
HTTP + WebSocket/SSE
Docker Compose local development profile
React 19 + TypeScript + Vite
Tailwind CSS v4
shadcn/ui conventions
lucide-react
TanStack Query
TanStack Table
React Hook Form + Zod
Vitest
Playwright UI testing and agent verification
Rust tooling: cargo, rust-analyzer, rustfmt, clippy, bacon, nextest, audit, deny, taplo
```

Next.js не является обязательным runtime для V01. Он остаётся вариантом для
cloud/web frontend, BFF, SSR, публичных страниц или SaaS, если для этого появятся
достаточные основания.

Локальная разработка должна иметь Docker Compose dev profile со стабильным
путём Core/Web/ToolHive/Generated UI Builder, предсказуемыми портами, volumes состояния, healthcheck
и понятным reset. Node Daemon и Codex запускаются как host processes, когда им
нужен реальный доступ к локальному workspace и пользовательской авторизации. UI проверяется Playwright в
двух режимах: автоматические E2E-тесты и agent/operator inspection через
`playwright-cli` на том же локальном окружении.

## Локальная разработка

Текущий implementation baseline включает:

- Rust workspace с общими protocol/domain contracts, Core Backend и Node
  Daemon;
- Vite React Web Control Panel в `apps/web`;
- SQLite-backed Core с health, inventory, heartbeat, placement, session, Codex
  provider, artifact tree и agent projection API;
- workspace-centered shell с деревом `Nodes -> Workspaces`, Node Overview,
  поверхностями `Agent / Workbench / Jobs` и условным Context Inspector;
- IDE-like Workbench с безопасным чтением и сохранением текста, file tree,
  editor, Git-aware Review и PTY terminal; Review показывает branch/worktree,
  changed files, scoped Monaco diff и traceable `make l` / `make c` results;
- Monaco для файлов и diff, xterm.js для интерактивных PTY-сессий workspace;
- hardening для quality gates, безопасности состояния и файлов Node, retry
  команд, stream cursors, healthcheck и web error states;
- долговечные отложенные сообщения сессии с явными timezone и guarded dispatch;
- выключенные по умолчанию Background Jobs с ручными и плановыми запусками,
  наблюдаемыми run-сессиями, stop-on-error, overlap skipping и общей admission
  по квоте провайдера;
- GitHub Actions release automation, deploy manifests и server activation;
- Docker Compose dev profile для Core/Web/ToolHive/Generated UI Builder и host
  Node Daemon/Codex.

Запуск локального стека в отдельных терминалах:

```sh
make init
make core-r
make node-r
make web-r
```

`make node-r` по умолчанию разрешает Node работать с корнем этого репозитория.
Для другого дерева workspace задайте
`UPRAVA_NODE_WORKSPACES=/path/to/workspace-root`.

Core/Web/ToolHive/Generated UI Builder можно запустить через Docker Compose:

```sh
make dev-up
```

Актуальный runbook: [локальная разработка V01](docs/runbooks/v01-local-dev.md).

## Работа с документацией

- `docs/` — единое каноническое русскоязычное дерево.
- `docs/systems/architecture.md` — общая архитектура системы.
- `docs/systems/areas/` — глубокая проработка отдельных системных направлений.
- `docs/polish/` и `docs/tmp-plans/` — рабочие исключения, которые временно
  могут оставаться на английском.

Если временный документ фиксирует долговечное продуктовое, архитектурное или
процессное решение, его нужно перенести в соответствующий русский документ в
`docs/`. Дублирующее языковое зеркало поддерживать не нужно.

## Основания

Uprava опирается на практики harness engineering и Superadditivity Theory. Цель
не в максимальной автономности ИИ любой ценой, а в системе «человек + агент»,
где вместе растут скорость, качество, понимание, трассируемость, способность к
review и безопасному делегированию.
