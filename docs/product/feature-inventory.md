# Uprava Feature Inventory

Статус: `active`

Назначение: зафиксировать идеи фич, которые уже были придуманы в `README.md` и `docs/development/uprava-notes.md`, без приоритизации и без превращения в roadmap. Это сырой, но сгруппированный инвентарь для последующего отбора в vision, архитектуру и backlog.

Источники:

- `README.md`
- `docs/development/uprava-notes.md`

## Как читать

- Это не список обязательств.
- Повторяющиеся идеи сведены в один пункт.
- В `Источник` указаны места, откуда идея пришла или где она повторяется.
- Если идея пока выглядит как исследовательское направление, это явно отмечено.

## 1. Platform / Distributed Agent OS

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-001 | WorkOS общего назначения для агентов | Uprava как рабочая операционная среда для агентской работы, сначала для разработки, затем для аналитики, ресерча, финансов и других задач. | `README.md`, `docs/development/uprava-notes.md:24` |
| F-002 | Distributed Agent OS | Система не замыкается на одной машине или одном чате, а управляет агентами, нодами, окружениями и workflow. | `docs/development/uprava-notes.md:24` |
| F-003 | Core / control plane | Центральный слой управления агентами, проектами, нодами, задачами, артефактами и workflow. | `README.md`, `docs/development/uprava-notes.md:23`, `docs/development/uprava-notes.md:54` |
| F-004 | Node Daemon | Системный демон, который запускается на ноде, регистрирует ее в Core, запускает агентов и дает доступ к файлам и состоянию системы. | `README.md`, `docs/development/uprava-notes.md:23`, User clarification, 2026-06-15 |
| F-005 | Multi-node execution | Возможность запускать агентов на разных нодах: локальный компьютер, сервер, cloud workspace, sandbox. | `README.md`, `docs/development/uprava-notes.md:23`, `docs/development/uprava-notes.md:54` |
| F-006 | Подключение личного компьютера как ноды | Можно подключить личный комп и смотреть, что там делают агенты. | `docs/development/uprava-notes.md:23` |
| F-007 | Легковесный runtime для агентов | Среда, в которой можно быстро запускать агентов под конкретные задачи. | `docs/development/uprava-notes.md:28` |
| F-008 | Stable isolated environment | Агент работает в стабильном окружении с песочницей, файлами, кодом, bash и UI-видимостью. | `docs/development/uprava-notes.md:54` |
| F-009 | microVM для агентов | Все агенты могут работать в microVM. | `docs/development/uprava-notes.md:15` |
| F-010 | Отдельная git-ветка на агента/run | Агентская работа изолируется отдельной веткой. | `docs/development/uprava-notes.md:15` |
| F-011 | Stateless agent + sandbox | Агент не обязательно является долгоживущим процессом; работа идет через sandbox/workspace. | `docs/development/uprava-notes.md:112-126` |
| F-012 | Durable workflow state | Долгоживущим является состояние workflow, а не контейнер или конкретная агентская сессия. | `docs/development/uprava-notes.md:121-126` |
| F-013 | Event-driven state machine для агента | Агентская работа моделируется как event-driven state machine. | `docs/development/uprava-notes.md:109-110` |
| F-014 | State store + event log | Хранение состояния и истории событий через Postgres/Redis/S3/Vector DB или аналогичный слой. | `docs/development/uprava-notes.md:102-103` |
| F-015 | Deployment repo / compose generator | Репа или CLI для старта инфраструктуры: выбрать конфигурацию core/node и собрать compose/deploy config. | `docs/development/uprava-notes.md:73` |
| F-016 | Cloud product with accounts/projects | Коммерческий cloud-вариант с аккаунтами и проектами. | `docs/development/uprava-notes.md:55` |
| F-017 | Мультиплатформенность | Продукт должен работать между desktop и mobile. | `README.md`, `docs/development/uprava-notes.md:9`, `docs/development/uprava-notes.md:69` |
| F-018 | Execution-mode neutral core | Core не должен быть привязан только к task-based cloud-agent flow; persistent, task-based and hybrid режимы должны быть разными execution modes одной системы. | User clarification, 2026-06-15 |
| F-019 | Future Agent-to-Task delegation | Managed Agent позже может явным tool-вызовом порождать bounded Task Runs, не смешивая live session context с Task isolation, evidence and review contract. Не входит в exit criteria Managed Agent Work Loop. | User clarification, 2026-06-15; reframed 2026-07-22 |

## 2. Agents

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-020 | Codex как default agent | Начальный агент по умолчанию. | `README.md` |
| F-021 | Agent orchestrator | Оркестратор, который управляет несколькими агентами и распределяет задачи. | `docs/development/uprava-notes.md:4`, `docs/development/uprava-notes.md:14` |
| F-022 | Internal Uprava agent | Агент внутри самого UI как first-class citizen для работы над Uprava и помощи пользователю. | `README.md` |
| F-023 | Internal agent orchestrates node agents | Внутренний агент может оркестрировать агентов, запущенных на нодах. | `README.md` |
| F-024 | Multi-chat with agents | Возможность одновременно общаться с несколькими агентами. | `docs/development/uprava-notes.md:13` |
| F-025 | Pluggable agent providers | Единый интерфейс поверх разных агентских провайдеров, как метаинструмент для агентов. | `docs/development/uprava-notes.md:76` |
| F-026 | CLI-коннекторы агентов | Поддержка CLI-коннекторов для разных агентских инструментов. | `docs/development/uprava-notes.md:32` |
| F-027 | Agent server | Собственный или готовый agent server, способный выполнять задачи уровня code actions. | `docs/development/uprava-notes.md:32` |
| F-028 | Specialized agents | Разные классы агентов: coding, support, retrieval, browser, finance. | `docs/development/uprava-notes.md:95-100` |
| F-029 | Агенты как исполнители коммитов | По максимуму делать агентами даже коммиты. | `docs/development/uprava-notes.md:27` |
| F-030 | Self-creation as eval | Самосоздание первой версии Uprava как оценочный сценарий. | `docs/development/uprava-notes.md:8` |
| F-031 | Агент с доступным UI-контекстом | Интерфейс проектируется так, чтобы агент понимал, что видит пользователь. | `docs/development/uprava-notes.md:83` |
| F-032 | Человек/агент co-working model | Возможная иерархия: люди, управляющие, работяги, агенты; либо упрощенная модель человек/агент. | `docs/development/uprava-notes.md:10` |
| F-033 | Agents in tool environments | Направление: давать агентам окружения в виде инструментов, а не только процесс/машину. | `docs/development/uprava-notes.md:29` |
| F-034 | Persistent agent session | Агент запускается как живой процесс или подключается как внешний interactive agent; пользователь может продолжать диалог, смотреть состояние и управлять процессом. | User clarification, 2026-06-15 |
| F-035 | Attach/detach to live agent | Возможность подключаться к уже живому агентскому процессу и отключаться без потери состояния. | User clarification, 2026-06-15 |
| F-036 | Task-based agent server mode | Агент вызывается как сервер/исполнитель задачи: ему передаются tools, context package and sandbox, а он возвращает bounded result. | User clarification, 2026-06-15 |
| F-037 | Agent CP-like connection | Возможность подключаться к агенту как к управляемому процессу/контрольной плоскости, а не только запускать отдельную задачу. | User clarification, 2026-06-15 |

## 3. Tasks, Workflows, Harness

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-040 | Долгие agent tasks | Возможность ставить агенту задачи на часы и возвращаться к результату. | `README.md` |
| F-041 | Harness для долгих задач | Слой практик и механизмов, помогающий агентам выполнять длинные задачи управляемо. | `README.md` |
| F-042 | Полудетерминированные pipelines | Workflow вида реализация -> review -> исправление, где часть этапов может быть детерминирована. | `README.md` |
| F-043 | Agent self-review / tests внутри pipeline | Внутри реализации агент может делать свой review и запускать тесты, но отдельный review-блок остается явным. | `README.md` |
| F-044 | Schedules / n8n-like automation | Пайплайны и расписания в стиле n8n, но для агентской работы. | `docs/development/uprava-notes.md:11` |
| F-045 | Библиотека guides | Гайды, которые агент может применять, например setup Python project. | `README.md` |
| F-046 | Библиотека guidelines | Правила по code style, архитектуре, тестам, review и частым ошибкам агентов. | `README.md` |
| F-047 | Skills, tools, pipelines из коробки | Базовые готовые наборы, чтобы не настраивать каждый workflow с нуля. | `README.md` |
| F-048 | Task -> MR/PR flow | Сценарий, где агентская работа заканчивается merge request / pull request. | `README.md`, `docs/development/uprava-notes.md:20`, `docs/development/uprava-notes.md:120` |
| F-049 | Git webhook wakes workflow | Workflow может засыпать и просыпаться от GitHub webhook, например после CI. | `docs/development/uprava-notes.md:121-124` |
| F-050 | CI follow-up loop | Агент проверяет CI и обновляет внешнюю задачу после webhook. | `docs/development/uprava-notes.md:121-124` |
| F-051 | One-shot vs dialogue mode experiment | Исследовать, когда лучше "1 задача = 1 запрос", а когда диалог. | `docs/development/uprava-notes.md:57` |
| F-052 | Иерархический подход к планированию | Двигаться от масштаба к деталям, чтобы снизить когнитивную нагрузку при проектировании. | `docs/development/uprava-notes.md:78-81` |
| F-053 | C4 + Activity Diagram + UML State Machine | Использовать C4, activity diagrams и UML state machines для проектирования Uprava и агентских процессов. | `docs/development/uprava-notes.md:81` |
| F-054 | Отложенные сообщения в сессии | Долговечный одноразовый будущий turn существующей сессии; он проходит обычную runtime/session admission, а не становится повторяющейся автоматизацией. | User clarification, 2026-07-12 |
| F-055 | Background Jobs и scheduled agent runs | Долговечное Job definition с task prompt, параметрами, schedules, stop-on-error policy и наблюдаемыми runs для unattended agent work в текущем workspace. | User clarification, 2026-07-12 |
| F-056 | Provider quota admission | Общая best-effort проверка пятичасового и недельного Codex limits перед chat и Job starts; порог 5%, typed rejection, explicit force override и честное состояние `unknown`. | User clarification, 2026-07-12 |

## 4. Developer Workflow

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-060 | Project/file browser | Возможность смотреть весь проект и файлы, а не только измененные файлы. | `README.md`, `docs/development/uprava-notes.md:31`, `docs/development/uprava-notes.md:59` |
| F-061 | Terminal view / agent output screen | Экран с выводом агентов, желательно как терминалы. | `docs/development/uprava-notes.md:12`, `docs/development/uprava-notes.md:54` |
| F-062 | Bash/tool call visibility | Видеть, какие команды и tool calls выполнялись в окружении. | `docs/development/uprava-notes.md:54` |
| F-063 | Diff viewer | Удобная работа с diff как обязательный ранний слой. | `README.md`, `docs/development/uprava-notes.md:22` |
| F-064 | Git integration | Интеграция с git providers и рабочими ветками. | `README.md`, `docs/development/uprava-notes.md:20`, `docs/development/uprava-notes.md:120` |
| F-065 | PR/MR comments import | Загрузка комментариев к PR/MR в Uprava. | `docs/development/uprava-notes.md:20` |
| F-066 | Fix PR comments with agents | Возможность отправить PR/MR comments агенту на исправление. | `docs/development/uprava-notes.md:20` |
| F-067 | Mobile review | Удобное review с телефона. | `README.md`, `docs/development/uprava-notes.md:69` |
| F-068 | Test/check reports | Агент запускает tests через bash tool, а система показывает результат как часть run. | `docs/development/uprava-notes.md:119` |
| F-069 | API-level regression for UI/system evals | Для benchmark UI тестировать сложно, но API можно покрывать большим regression набором. | `docs/development/uprava-notes.md:41` |
| F-070 | Project state view | Видеть состояние проекта/задачи/агентов, а не только chat transcript. | `README.md`, `docs/development/uprava-notes.md:31`, `docs/development/uprava-notes.md:59` |

## 5. UI, Visual Artifacts, Interaction

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-080 | Notion-like block UI | Интерфейс с блоками, в которые можно встраивать разные типы данных и действий. | `README.md`, `docs/development/uprava-notes.md:34` |
| F-081 | Obsidian-like knowledge/navigation model | Вдохновение от Obsidian: связность, дерево, docs, ссылки, knowledge base. | `docs/development/uprava-notes.md:34`, `docs/development/uprava-notes.md:72` |
| F-082 | Dynamic block | Агент может выдать форму, график или целый dashboard. | `README.md` |
| F-083 | Dynamic UI in chats | В чатах агент может показывать Grafana, другие инструменты и интерактивные UI-блоки. | `docs/development/uprava-notes.md:21`, `docs/development/uprava-notes.md:54` |
| F-084 | Visual plugins | Плагины, которые отображают действия и результаты в интерфейсе. | `README.md` |
| F-085 | Forms instead of text | Иногда агент должен показывать форму, а не просить/отвечать текстом. | `README.md` |
| F-086 | Graphs / charts | Визуализации данных, графики и другие chart artifacts. | `README.md` |
| F-087 | Dashboards | Агент или plugin может создавать целые dashboards. | `README.md`, `docs/development/uprava-notes.md:54` |
| F-088 | Embedded external views | Встроенные views и ссылки на инструменты вроде Grafana, сервисов и внешних систем. | `docs/development/uprava-notes.md:31`, `docs/development/uprava-notes.md:54` |
| F-089 | UML visualization | Минимум просмотр UML. | `README.md` |
| F-090 | UML editor | Расширение UML visualization до редактора. | `README.md` |
| F-091 | Canvas / dynamic interfaces | Канвас и динамические интерфейсы как важное направление. | `docs/development/uprava-notes.md:33` |
| F-092 | @mentions | Возможность упоминать файл, инструмент, агента и другие сущности через `@`. | `README.md` |
| F-093 | Dual UI | У каждого элемента есть визуальное представление для человека и машинно-читаемое представление для агента. | `README.md` |
| F-094 | Long press to internal agent chat | Долгое нажатие открывает чат с внутренним агентом для гибкого взаимодействия с UI. | `README.md`, `docs/development/uprava-notes.md:83` |
| F-095 | UI доступен агенту | Агент понимает, что видит пользователь, и может действовать с учетом интерфейсного контекста. | `docs/development/uprava-notes.md:83` |
| F-096 | Visual stack integration | Интеграции должны проявляться визуально, а не только как текст "я сделал X" или ссылка. | `README.md`, `docs/development/uprava-notes.md:31` |

## 6. Integrations and Plugins

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-100 | Plugin system | Система расширений для инструментов, визуализаций, агентов, integrations и workflows. | `README.md`, `docs/development/uprava-notes.md:63` |
| F-101 | Notion integration | Подключение Notion через plugin/integration. | `README.md` |
| F-102 | GitLab integration | Подключение GitLab. | `README.md` |
| F-103 | Linear integration | Linear как основной task tracker на старте. | `README.md` |
| F-104 | Grafana integration | Встроенные views, dashboards и monitoring через Grafana. | `README.md`, `docs/development/uprava-notes.md:21`, `docs/development/uprava-notes.md:30-31` |
| F-105 | Docker integration | Интеграция с Docker/deployment/runtime слоем. | `README.md`, `docs/development/uprava-notes.md:73` |
| F-106 | MLflow integration | Подключение MLflow как возможного plugin. | `README.md` |
| F-107 | MCP integration | Подключение MCP, но с выводом результатов в визуальный UI, а не только в текст. | `README.md`, `docs/development/uprava-notes.md:100` |
| F-108 | Progressive agent tool access | Основной machine interface агента к Uprava и внешним integrations строится через MCP с `Search -> Inspect -> Execute`; отдельный Uprava CLI откладывается до подтверждённых composition/streaming/batch scenarios. | User clarification, 2026-07-19 |
| F-109 | External task trackers instead of own tracker first | На старте не делать свой task tracker, а использовать готовые. | `README.md` |
| F-110 | Task tracker provider abstraction | Позже можно сделать метаинструмент поверх разных task trackers. | `docs/development/uprava-notes.md:76` |
| F-111 | External memory instead of own memory first | На старте не делать свою memory system. | `README.md` |
| F-112 | Memory provider abstraction | Метаинструмент поверх разных memory providers. | `docs/development/uprava-notes.md:5`, `docs/development/uprava-notes.md:19`, `docs/development/uprava-notes.md:76` |
| F-113 | Git provider integration | Интеграция с git для PR/MR, comments, branches, checks. | `docs/development/uprava-notes.md:20`, `docs/development/uprava-notes.md:120-124` |
| F-114 | Observability provider integration | LangSmith, Langfuse, OpenTelemetry, Phoenix или аналогичные инструменты. | `docs/development/uprava-notes.md:105-106` |
| F-115 | Sandbox/devbox providers | Возможные внешние sandbox providers из useful links: Daytona, E2B, Sandcastle и аналоги. | `docs/development/uprava-notes.md:132`, `docs/development/uprava-notes.md:138-139` |
| F-116 | Core Tool Registry | Реестр managed tools и observed capabilities в Core: metadata, schemas, permissions, Node/project/session availability, routing, UI contracts and audit policy. Выполнение managed tool может происходить на Node, через ToolHive-backed MCP provider, во внешнем provider или в Core. Native Node/provider tools не проксируются и вызываются агентом напрямую. | User clarification, 2026-07-19 |
| F-117 | Plugin Registry and Extension Host | Core-owned package/install lifecycle и VS Code/Obsidian-like manifest-driven extensions самой Uprava: themes, commands, Workbench views/tabs, Inspector aspects, actions, renderers, links, artifact types, optional tools/services, configuration, permissions, compatibility and governed trust/runtime levels. Первый plugin — bundled data-only Dark Theme с semantic-token contract и safe light fallback. | User clarification, 2026-07-19 |
| F-118 | MCP and integration runtime model | MCP является основным agent-facing interface для Uprava-native tools и внешних integrations; ToolHive управляет external MCP runtime. Native API/Node-local/hybrid adapters допустимы как execution backends за Core-owned policy and registry. | User clarification, 2026-07-19 |
| F-119 | First-class integration UX | Интеграции должны давать UI blocks, artifacts, workflow hooks, trace and permissions, а не только скрытый tool call внутри текстового ответа агента. | User clarification, 2026-06-15 |

## 7. Traceability, Monitoring, Metrics

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-120 | Traceability / process visibility | Видеть, что агент сделал, какие файлы смотрел, какие изменения внес, какие команды запускал и что проверял. | `README.md`, `docs/development/uprava-notes.md:31`, `docs/development/uprava-notes.md:54` |
| F-121 | Review-friendly trace | Система должна уменьшать стоимость review, а не просто копить logs. | `README.md`, `docs/development/uprava-notes.md:31`, `docs/development/uprava-notes.md:61` |
| F-122 | Monitoring layer | Мониторинг агентской работы и состояния системы. | `docs/development/uprava-notes.md:30`, `docs/development/uprava-notes.md:61`, `docs/development/uprava-notes.md:105-106` |
| F-123 | Explainable AI direction | Исследовать explainability/интерпретируемость агентских решений как часть будущей системы. | `docs/development/uprava-notes.md:61` |
| F-124 | Stats screen | Экран статистики по агентам/задачам/системе. | `docs/development/uprava-notes.md:16` |
| F-125 | LLM proxy | Возможный proxy для LLM, чтобы собирать статистику, контролировать расходы и наблюдать вызовы. | `docs/development/uprava-notes.md:16` |
| F-126 | Hard metrics | "Железные" метрики вместо ощущения прогресса. | `docs/development/uprava-notes.md:17`, `docs/development/uprava-notes.md:67` |
| F-127 | Edits per iteration | Мерить число правок в одной итерации. | `docs/development/uprava-notes.md:25` |
| F-128 | Iterations to merge | Мерить число итераций до merge. | `docs/development/uprava-notes.md:25` |
| F-129 | Scalability/support metrics | Размеры diff, число измененных строк на функцию, число измененных модулей на функцию, число fix commits. | `docs/development/uprava-notes.md:26` |
| F-130 | Attention budgeting / token economics | Метрики и оптимизация затрат внимания/токенов: retrieval cost, context entropy, cache stability, semantic locality. | `docs/development/uprava-notes.md:65` |
| F-131 | Workflow provenance / audit trail | Сохранение происхождения действий, событий, проверок и решений. | `docs/development/uprava-notes.md:102-106`, `docs/development/uprava-notes.md:126` |
| F-132 | Review debt visibility | Видимость накопленного долга review/integration, чтобы агентская скорость не превращалась в скрытую нагрузку. | `README.md`, `docs/development/uprava-notes.md:25-26`, `docs/development/uprava-notes.md:67` |

## 8. Mobile and Collaboration

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-140 | Desktop/mobile continuity | Можно начать работу на компьютере и продолжить с телефона. | `README.md`, `docs/development/uprava-notes.md:9`, `docs/development/uprava-notes.md:69` |
| F-141 | Mobile task monitoring | С телефона видно состояние задач, агентов и их результатов. | `README.md`, `docs/development/uprava-notes.md:69` |
| F-142 | Mobile review | С телефона можно ревьюить изменения, читать trace, смотреть diff и принимать решения. | `README.md` |
| F-143 | Multi-user control / co-working | Исследовать коллективные сценарии управления агентами по аналогии с Figma/Zed. | `docs/development/uprava-notes.md:10` |
| F-144 | Agent work surface for teams | Общая рабочая поверхность, где видны агентские runs, задачи, результаты и статусы. | `docs/development/uprava-notes.md:10`, `docs/development/uprava-notes.md:54` |

## 9. Knowledge Base, Documentation, Research

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-160 | Knowledge base mode | Uprava может быть не только runtime, но и layer для базы знаний. | `docs/development/uprava-notes.md:72` |
| F-161 | Git repo + Obsidian model | База знаний как git repo с docs, indexes, links and tree navigation. | `docs/development/uprava-notes.md:72` |
| F-162 | Docs as code | Документы развиваются как код: создаются по потребности, делятся при росте, не плодятся заранее. | `docs/development/uprava-notes.md:44-51` |
| F-163 | README as project/key feature source | Описание проекта и ключевых фич должно жить в основном README. | `docs/development/uprava-notes.md:45-46` |
| F-164 | Architecture tree | Расширенная схема/дерево с коротким описанием каждого большого модуля. | `docs/development/uprava-notes.md:51` |
| F-165 | Zotero-inspired research features | Взять полезные идеи из Zotero для research/document workflows, не превращая Uprava в Zotero. | `docs/development/uprava-notes.md:74` |
| F-166 | Research/article workflows | Не замыкаться на разработке: статьи и исследования тоже должны поддерживаться. | `docs/development/uprava-notes.md:24` |

## 10. Benchmarks and Evals

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-180 | Self-build benchmark | Оценивать Uprava через попытку создать систему заново с нуля. | `docs/development/uprava-notes.md:8`, `docs/development/uprava-notes.md:36-42` |
| F-181 | Detailed spec input benchmark | Бенчмарк с детальной спецификацией на входе. | `docs/development/uprava-notes.md:38` |
| F-182 | Business case coverage | Большое покрытие бизнес-кейсами в eval/regression наборе. | `docs/development/uprava-notes.md:39` |
| F-183 | Autonomous progress metric | Считать, насколько система смогла продвинуться без вмешательства человека. | `docs/development/uprava-notes.md:40` |
| F-184 | API regression benchmark | Делать большой regression на уровне API, потому что UI тестировать сложнее. | `docs/development/uprava-notes.md:41` |
| F-185 | Agent mode benchmark | Сравнивать single-shot, dialogue, hierarchical и pipeline режимы агентской работы. | `docs/development/uprava-notes.md:57`, `docs/development/uprava-notes.md:78-81` |
| F-186 | Execution mode comparison | Сравнивать Managed Agent, Agent exec compatibility, task-based sandbox run и sessionless Job Run по review cost, autonomy, latency, trace quality and user control. | User clarification, 2026-06-15; updated 2026-07-22 |
| F-187 | Managed Agent Work Loop | Сделать provider-native live Codex runtime основным Agent mode: streaming, approvals, questions, interrupt, detach/reattach and resume через semantic protocol, а не встроенный TUI. | User clarification, 2026-07-22 |
| F-188 | Surface-specific provider policy | Сохранить unrestricted exec для Agent compatibility и externally sandboxed Tasks; Jobs перевести на sessionless sandboxed exec с non-interactive approvals by default. | User clarification, 2026-07-22 |

## 11. Domains and Expansion

| ID | Фича / направление | Смысл | Источник |
| --- | --- | --- | --- |
| F-200 | Software development first | Первый фокус - разработка, потому что там есть project, files, git, tests, diff, review, MR/PR. | `README.md`, `docs/development/uprava-notes.md:22`, `docs/development/uprava-notes.md:59` |
| F-201 | Analytics workflows | Дальнейшее расширение на аналитику. | `README.md` |
| F-202 | Research workflows | Дальнейшее расширение на ресерч, статьи и исследования. | `README.md`, `docs/development/uprava-notes.md:24` |
| F-203 | Finance workflows | Дальнейшее расширение на финансы. | `README.md`, `docs/development/uprava-notes.md:98` |
| F-204 | Personal tasks branch | Возможно отдельное ответвление для личных задач. | `README.md` |
| F-205 | Site/email generators | Легковесный агентский runtime может упростить генерацию сайтов и писем. | `docs/development/uprava-notes.md:28` |
| F-206 | Knowledge workflows | Процессы базы знаний, docs, indexes, research library и handoff через repo/tree. | `docs/development/uprava-notes.md:72`, `docs/development/uprava-notes.md:74` |

## 12. Candidate Foundation Cut

Если из этого инвентаря выделять первый практический слой, наиболее связная минимальная версия выглядит так:

- Core/control plane.
- Node Daemon.
- Project + workspace.
- Codex as default agent.
- Persistent agent session.
- Chat plus non-chat views.
- File browser.
- Terminal/output view.
- Diff view.
- Basic session event log and trace.
- Basic git/diff awareness.
- Mobile-readable run/review state.
- Minimal dynamic artifact/block API.
- Minimal plugin boundary.
- Minimal Tool Registry and Plugin Registry shape.

Task-based sandbox runs, durable workflows and full MR/PR flow are intentionally outside the first foundation cut. The first product should prove the persistent Node-based developer workbench and leave architectural space for task-based mode later.

Эта версия проверяет главный тезис: Uprava дает больше управляемости, визуализации и traceability, чем обычный agent chat.
