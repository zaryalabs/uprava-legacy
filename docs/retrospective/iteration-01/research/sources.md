# Источники и границы доказательств

Основной обзор источников выполнен 3 октября 2026 года. DeepSeek Harness
добавлен 7 октября; дата и revision указаны отдельно. Источники являются
первичными: документация разработчиков, спецификации, release notes и код.
Поисковые snippets использовались только для поиска страниц. Полные внешние
тексты не копировались в пакет; в тематических документах приведены краткие
пересказы, сопоставления и собственные рекомендации.

## Внутренний снимок

Implementation `0.2.26`, commit
`9d096834eadfacd9f04a551c5557b33194e7ca4b`. Для ссылок на код ниже подразумевается
именно этот commit. Локальные ссылки открывают текущий checkout; при дальнейших
изменениях сравнивайте с сохранённой ревизией.

| Источник | Что он подтверждает | Ограничение |
| --- | --- | --- |
| Объяснение владельца в разговоре 2026-10-03, сохранённое в [продуктовых уроках](../retrospective/01-product-and-lessons.md) | Причины завершения, неудобство, положительная оценка Rust/React, недостаточный опыт ToolHive, намерение пересмотреть модульность | Личный опыт, не performance benchmark |
| [README](../../../../README.md), [versioning](../../../versioning.md), [releases](../../../releases.md) | Baseline и порядок поставки | Release closure не доказывает удобство |
| [Vision](../../../vision.md), [inventory](../../../product/feature-inventory.md), [queue](../../../product/feature-queue.md) | Продуктовые идеи и история scope | Будущие возможности не считаются реализованными |
| [Architecture](../../../systems/architecture.md), [system areas](../../../systems/areas/README.md) | Архитектурные позиции | Сверяются с кодом |
| [Аудит 2026-07-09](../../../audit/audit-2026-07-09.md) | Исторические findings `0.1.8` | Не текущий аудит `.26` |
| [Core](../../../../crates/uprava-server/src/), [Node](../../../../crates/uprava-node/src/), [Protocol](../../../../crates/uprava-protocol/src/) | Выборочно прочитанные implementation boundaries и tests | Не полный аудит каждой строки |
| [Workspace UI](../../../../apps/web/src/features/workspace-inspector/), [plugins](../../../../apps/web/src/plugins/), [workbench](../../../../apps/web/src/workbench/) | Editor lifecycle, draft/save, contributions и state flow | Чтение кода не даёт latency |
| [Managed runbook](../../../runbooks/managed-agent-runtime.md), [Task runbook](../../../runbooks/task-sandbox-runtime.md), [ToolHive runbook](../../../runbooks/agent-tooling-toolhive-linear.md) | Поддерживаемые сценарии и границы ручной приёмки | Наличие инструкции не равно выполненному прогону |
| [Cargo.lock](../../../../Cargo.lock), [web lock](../../../../apps/web/package-lock.json), [Makefile](../../../../Makefile) | Закреплённые зависимости и quality workflow | Не означает актуальность внешних версий |
| [Browser golden tests](../../../../apps/web/e2e/golden.spec.ts), [Playwright config](../../../../apps/web/playwright.config.ts) | Сценарии выполненных browser проверок | Core API подменён fixture |
| [Журнал экспериментов](experiments.md) | Выполненные команды, результаты и ограничения | Единственный источник новых заявлений о прохождении проверок |

## Референсные панели

Начальное исследование документации дополнено source review четырёх систем.
Ревизии получены через GitHub API, публичные source archives скачаны для
чтения и проверки номеров строк. Исходники не собирались и не запускались.
Commit ветки обозначает исследованный snapshot, а не принятый production pin.

| Проект | Неизменяемая ревизия | Цепочки и реестр файлов |
| --- | --- | --- |
| T3 Code, `main` | [`ce90eec1ffc2087395227a610843091f4b260cf7`](https://github.com/pingdotgg/t3code/commit/ce90eec1ffc2087395227a610843091f4b260cf7) | [Разбор с code/test permalinks](07-t3-remote-agent-implementation.md): remote auth, provider ownership, orchestration, client recovery, scoped MCP |
| OpenCode, `dev` | [`907b3bc518fa48e90e8ec24dd327d13eee71c36c`](https://github.com/anomalyco/opencode/commit/907b3bc518fa48e90e8ec24dd327d13eee71c36c) | [Разбор с code/test permalinks](08-opencode-agent-tooling-implementation.md): legacy remote/MCP отдельно от нового SessionV2/ToolRegistry |
| ToolHive, `main` | [`3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c`](https://github.com/stacklok/toolhive/commit/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c) | [Разбор с code/test permalinks](09-toolhive-control-implementation.md): workload/proxy, admission, identity, credentials, retry и telemetry |
| DeepSeek Harness, `master`, чтение 2026-10-07 | [`5badb15009ae1756c3afe0ae0cef1faafc290ccc`](https://github.com/deepseek-ai/deepseek-harness/commit/5badb15009ae1756c3afe0ae0cef1faafc290ccc) | [Модульность](11-deepseek-harness-modularity.md), [developer tools](12-deepseek-harness-developer-tools.md), [перенос в Uprava](13-deepseek-harness-for-v2.md) |

В каждом разборе перечислены конкретные функции, границы гарантий и
рекомендации. Тесты прочитаны как описание инвариантов, их прохождение не
заявляется. [Синтез](10-remote-agents-and-tools-for-v2.md) сопоставляет эти
механизмы с baseline Uprava; рекомендации не являются фактами об upstream.

Для Harness ref `master` получен через GitHub API 2026-10-07. Commit от
2026-10-03 содержит root package `0.2.1-alpha.1`; root metadata не означает,
что все vendored packages имеют ту же версию. Прочитаны целевые реализации
Cordis/Loader, scope/presets, client modules/chat, Session/Inspector,
SSH/native subagents и browser/MCP projections. Диапазоны 84 code/test links
проверены по source archive. Удалённые capability adapters и диагностические
очереди не считаются готовым distributed session control plane.

Обзорные документы, прочитанные на первом этапе:

| Источник | Версия или ref | Прочитанный материал |
| --- | --- | --- |
| [T3 Architecture](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/overview.md) | `ce90eec1` | Ownership и orchestration |
| [T3 Connection runtime](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/connection-runtime.md) | `ce90eec1` | Connection и freshness |
| [T3 Environment authentication](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/environment-auth.md) | `ce90eec1` | Trust boundaries и scope |
| [T3 Providers](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/providers.md) | `ce90eec1` | Provider ownership |
| [T3 supervisor](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/packages/client-runtime/src/connection/supervisor.ts) | `ce90eec1` | Выбранный код reconnect |
| [T3 thread state](https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/packages/client-runtime/src/state/threads.ts) | `ce90eec1` | Выбранный код sequence и snapshots |
| [OpenCode CLI](https://opencode.ai/v2/docs/cli) | Docs v2 | Shared service |
| [OpenCode Web](https://opencode.ai/v2/docs/cli/web/) | Docs v2 | Web/server access |
| [OpenCode API](https://opencode.ai/v2/docs/api) | Docs v2 | Events, log, session control |

## Интерактивные протоколы

| Источник | Версия или ref | Прочитанный материал |
| --- | --- | --- |
| [Codex App Server](https://learn.chatgpt.com/docs/app-server) | Rolling official docs; новая CLI version не зафиксирована | Lifecycle, schema, transports; это не проверка старого pin `0.144.1` |
| [ACP overview](https://agentclientprotocol.com/protocol/v1/overview) | Protocol v1 | Роли и сообщения |
| [ACP initialization](https://agentclientprotocol.com/protocol/v1/initialization) | Protocol v1 | Version и capability negotiation |
| [ACP session setup](https://agentclientprotocol.com/protocol/v1/session-setup) | Protocol v1 | Создание и восстановление |
| [ACP prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn) | Protocol v1 | Updates и cancel |

## Безопасность и модульность

| Источник | Версия или ref | Прочитанный материал |
| --- | --- | --- |
| [OWASP ASVS](https://owasp.org/projects/asvs) | Страница указывает `5.0.0` | Назначение стандарта и версия идентификаторов; полный compliance assessment не проводился |
| [OWASP WebSocket Security](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html) | Rolling docs | Origin, authorization, session, limits, logs |
| [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285) | Issued 2026-09-14 | Advisory, выявленный новым `make c`; rustls patched `>=0.23.45` |
| [RUSTSEC-2026-0221](https://rustsec.org/advisories/RUSTSEC-2026-0221) | Issued 2026-07-31 | Warning нового audit; event-listener patched `>=5.4.2` |
| [MCP Security Best Practices](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices) | Docs `2026-07-28` | Proxy, credentials и OAuth risks |
| [Neovim API](https://github.com/neovim/neovim/blob/master/runtime/doc/api.txt) | `master`, дата доступа; SHA не зафиксирован | Functions, events, object handles и compatibility |
| [VS Code Extension Host](https://github.com/microsoft/vscode-docs/blob/main/api/advanced-topics/extension-host.md) | `main`, front matter `DateApproved: 9/30/2026`; SHA не зафиксирован | Local/web/remote hosts и activation |

## Редакторы и remote workspace

| Источник | Версия или ref | Прочитанный материал |
| --- | --- | --- |
| [Monaco README](https://github.com/microsoft/monaco-editor/blob/main/README.md) | `main`, дата доступа; SHA не зафиксирован | Model/view/URI/dispose; установленная версия Uprava отдельно `0.53.0` |
| [CodeMirror system guide](https://github.com/codemirror/website/blob/main/site/docs/guide/index.md) | `main`, дата доступа; SHA не зафиксирован | State, transactions, view и extensions; не benchmark |
| [VS Code Remote](https://code.visualstudio.com/docs/remote/remote-overview) | Rolling docs, страница от `9/30/2026` | Размещение workspace execution |

## Tools и сборка

| Источник | Версия или ref | Прочитанный материал |
| --- | --- | --- |
| [MCP specification](https://modelcontextprotocol.io/specification/2026-07-28) | `2026-07-28` | Актуальная на дату чтения protocol model |
| [MCP changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog) | `2026-07-28` | Изменения контрактов |
| [ToolHive overview](https://docs.stacklok.com/toolhive/) | Rolling docs | Runtime/registry/gateway |
| [ToolHive authorization](https://docs.stacklok.com/toolhive/reference/authz-policy-reference) | Rolling docs | Policy semantics |
| [ToolHive 0.40.0](https://github.com/stacklok/toolhive/releases/tag/v0.40.0) | Tag `v0.40.0`, short commit `505df83` | Release, совпадающий с pin проекта |
| [ToolHive 0.51.4](https://github.com/stacklok/toolhive/releases/tag/v0.51.4) | Tag `v0.51.4`, short commit `61aaf42` | Latest redirect при чтении; рекомендация upgrade не делалась |
| [Cargo timings](https://doc.rust-lang.org/cargo/reference/timings.html) | Rolling Cargo Book | Профилирование compilation |
| [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) | Rolling Cargo Book | Параметры профиля |
| [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html) | Rolling Cargo Book | Feature graph |

## Как продолжать проверку

Перед экспериментом зафиксировать конкретный upstream tag/commit, toolchain,
dependency lock и параметры стенда. Если moving-документ изменился, пересмотреть
зависящий от него вывод. Старая версия может оставаться рабочим вариантом, но
её совместимость и безопасность проверяются отдельно от возможностей latest.
