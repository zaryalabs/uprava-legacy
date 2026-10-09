# OpenCode: удалённое исполнение, состояние сессии и инструменты

Срез исходников: **2026-10-03**, `anomalyco/opencode`, ветка `dev`, commit
[`907b3bc518fa48e90e8ec24dd327d13eee71c36c`](https://github.com/anomalyco/opencode/commit/907b3bc518fa48e90e8ec24dd327d13eee71c36c).
Это чтение реализации и тестовых сценариев; upstream не устанавливался и не
запускался. Выводы ниже — рекомендации для Uprava, не результаты измерений.

## Сначала разделить две реализации

В этом commit сосуществуют два поколения. В `packages/opencode` находятся
действующие legacy session/MCP services и экспериментальная маршрутизация
удалённых workspace. В `packages/core` и `packages/server` развивается новая
модель `SessionV2`. Клиент явно определяет `v1 | v2` и выбирает соответствующий
event API. Следовательно, нельзя объединять возможности разных путей в одну
«готовую архитектуру OpenCode v2».
[Выбор протокола в клиенте][oc-client].

Ограничение видно непосредственно в коде: `SessionExecutionLocal` исполняет
сессию в текущем процессе, а remote placement обозначен будущей работой.
`SessionV2.shell`, `skill`, `compact`, `wait` возвращают
`OperationUnavailableError`; это не означает отсутствия таких возможностей во
всём OpenCode, но исключает вывод об их готовности именно в новом service API.
[Local execution][oc-local], [SessionV2][oc-session].

Для Uprava полезны четыре конкретные цепочки: remote workspace с синхронизацией;
приём и исполнение долговечного ввода; MCP lifecycle; materialization и вызов tools.

## 1. Remote workspace: ответ исполнителя и видимость результата

В legacy-пути middleware сначала разрешает workspace с учётом сохранённой
сессии, затем `WorkspaceAdapterRuntime.target()` возвращает локальную directory
или remote target. Некоторые запросы остаются на control plane; остальные
пересылаются HTTP/WebSocket proxy. При пересылке удаляются host-local
`directory` и `workspace` query parameters: путь машины клиента не должен
интерпретироваться как путь внутри удалённого sandbox.
[Маршрутизация][oc-routing], [граница путей][oc-paths].

Состояние возвращается отдельным каналом. `syncWorkspaceLoop` открывает SSE и
затем вызывает `syncHistory`: отправляет карту последних локальных sequence по
сессиям и воспроизводит недостающую историю через `events.replay(...,
{ownerID: workspace.id})`. После этого обрабатываются live `sync` events.
Переподключение имеет exponential backoff до 120 секунд. Открытие stream до
загрузки истории и повторяемый replay — полезная конструкция против разрыва
между snapshot/history и live delivery. Условия replay проверяет event store:
совпадение id/type/data допускает дубликат, расхождение или gap отклоняются.
[Workspace sync][oc-sync], [проверки replay][oc-event-commit].

Самая ценная деталь — **барьер видимости результата**. Remote HTTP response
может содержать `x-opencode-sync`: требуемые sequence по aggregate. Proxy
вызывает `Fence.wait`, пока локальная проекция не догонит их; при неуспехе
возвращается 503. Таким образом, успешная удалённая mutation не должна сразу
вести пользователя к устаревшему локальному чтению. Код ожидания различает
отмену и timeout.
[Proxy и fence][oc-routing], [waitForSync][oc-sync-wait].

**Что взять.** Сохранить для Uprava маршрут Web → Core → Node. Ответ Node на
команду должен содержать устойчивый результат и watermark событий. Core может
подтвердить «выполнено» после принятия результата, а «доступно в проекции» —
после соответствующего watermark. Для быстрых команд допустимо ограниченное
ожидание; для длинных — `accepted` и последующее событие. Timeout такого
ожидания не разрешает повторять side effect: повтор проверяет `command_id`.
Цена решения — учёт двух стадий завершения и понятный UI для задержки
синхронизации. Копировать весь универсальный HTTP proxy необязательно: наши
типизированные Core-to-Node команды дают более узкую границу полномочий.

## 2. Ввод, живое исполнение и доставка событий

В новом API `SessionV2.prompt()` сначала вызывает `SessionInput.admit`,
проверяет эквивалентность сохранённого ввода по `messageID`, prompt и delivery,
и только затем вызывает `execution.wake()`. Повтор с тем же ID и другим
содержимым становится конфликтом; HTTP handler переводит его в conflict error.
Хранение принятого ввода отделено от запуска runner.
[Приём prompt][oc-session], [HTTP mapping][oc-session-http].

`SessionRunCoordinator` хранит по session key одну активную execution:
повторный `run` присоединяется к ней, `wake` сворачивается в один pending wake,
а `interrupt` очищает прежний pending wake и ждёт остановки fiber. Новый wake,
пришедший во время cleanup, может создать последующее исполнение. Это
не распределённый lease: coordinator — process-local `Map`. В Uprava такой
автомат нужен внутри Node, поверх Core placement/attempt generation, чтобы
reconnect браузера не становился вторым запуском.
[Coordinator][oc-coordinator].

Event store полезен точностью границы commit: projector, operational commit,
sequence и запись durable event выполняются в одной SQLite immediate
transaction; уведомление durable subscribers идёт после неё. Подписка
`durable()` сначала регистрирует wake listener, затем читает историю после
sequence. Очередь wake имеет размер один: она сообщает «появились данные», а
сами данные повторно читаются из БД. Сворачивание уведомлений поэтому не
равнозначно потере истории.
[Транзакция][oc-event-commit], [durable follower][oc-event-follow].

Общий live stream устроен иначе: `EventHandler` использует bounded subscriber
на 256 событий, heartbeat раз в 15 секунд и SSE без `id`. При переполнении
`allBounded` завершает конкретного подписчика, не блокируя остальных. В UI
события группируются примерно на 16 ms, обработчик отдаёт управление после
8 ms работы; stream перезапускается с generation guard и задержкой 250 ms.
На `server.connected` активные directory поставлены на обновление. Этот
live/revalidation-путь нельзя объявлять durable replay браузера.
[Live endpoint][oc-live], [overflow][oc-overflow], [client loop][oc-client],
[revalidation][oc-refresh].

**Что взять.** У Uprava уже есть durable events и attempt-aware recovery;
нужна их простая первая пользовательская реализация. Ввести отдельные
инварианты: принятый input переживает разрыв связи; на attempt один владелец;
UI snapshot и cursor согласованы; медленный клиент отключается с явным
recovery, а не раздувает память. В React брать batching текстовых delta и
generation guards, сохраняя независимый быстрый путь для interrupt/deny.
Точные интервалы OpenCode не переносить как подтверждённые latency targets.

## 3. MCP: соединение, каталог, вызов и credentials

Здесь исследован **legacy MCP service**, а не предполагаемая готовая MCP
интеграция нового `ToolRegistry`.

`MCP.connectRemote()` пробует Streamable HTTP, затем SSE. После auth error
fallback прекращается: состояние становится `needs_auth` или
`needs_client_registration`, вместо маскировки ошибки сменой транспорта.
`connectTransport` закрывает transport при неуспешном подключении. Для stdio
передаются command, arguments, cwd и environment.
[Transport lifecycle][oc-mcp-connect].

После подключения `create()` загружает определения. `watch()` обрабатывает
`tools/list_changed`, проверяет, что notification принадлежит актуальному
client, и заменяет cached definitions. При `onclose` удаляются client,
definitions и instructions, статус становится failed. `tools()` читает cache
только подключённых клиентов. `McpCatalog.paginate` следует cursor, отклоняет
повторённый cursor и ограничивает число страниц.
[Catalog lifecycle][oc-mcp-watch], [выдача tools][oc-mcp-tools],
[pagination][oc-catalog].

В `SessionTools` MCP definition превращается в provider-compatible schema;
непосредственно перед execute вызывается `ctx.ask`. `McpCatalog.convertTool`
передаёт `abortSignal` и timeout в SDK, включает обновление timeout при
progress, переводит MCP `isError` в ошибку tool и обеспечивает текстовый
fallback для чистого `structuredContent`. Результаты адаптируются в текст и
вложения с ограничением некоторых бинарных ресурсов и truncation текста.
[Вызов и permission][oc-mcp-execute], [SDK call][oc-catalog].

**Что взять.** Node должен владеть соединением, process lifecycle и
transport-specific cancellation. Core — definition/version/schema hash,
visibility, policy и маршрутом вызова. Наличие definition, live connection и
разрешение execute — три разных факта. `list_changed` должен строить новый
проверенный snapshot каталога; старый callback не может заменить каталог
нового connection generation. После timeout нельзя автоматически повторять
mutation tool: abort signal обозначает запрос отмены, но не доказывает откат
удалённого эффекта. Помимо idle timeout нужен максимальный deadline вызова,
чтобы бесконечный progress не удерживал его постоянно.

OAuth даёт несколько удачных деталей. `McpAuth.getForUrl()` возвращает запись
только при точном совпадении server URL; JSON store защищён file lock и mode
`0600`. `McpOAuthPendingProvider` держит новые tokens в памяти до `commit`,
а callback требует одноразовый ожидаемый state и ограничивает ожидание пятью
минутами. Но callback слушает `127.0.0.1`: это локальный flow, не готовое
решение для браузера оператора и отдельной cloud Node.
[Credential storage][oc-auth], [pending credentials][oc-oauth],
[callback][oc-callback].

Для Uprava брать URL binding и замену credential generation после завершения
flow. Callback broker должен принадлежать Core и связывать попытку с actor,
integration, Node и exact redirect; secret material остаётся у выбранного
Node/ToolHive boundary. Различать `disconnect`, локальное удаление credentials
и upstream revocation: у OpenCode `disconnect` закрывает client, а
`removeAuth` удаляет local auth и pending flow; эти операции сами по себе не
содержат универсального отзыва upstream grant.
[Disconnect][oc-mcp-tools], [removeAuth][oc-mcp-remove].

## 4. Новый ToolRegistry: зафиксировать то, что было показано модели

`ToolRegistry.materialize()` создаёт определения для provider turn и
сохраняет identity каждой registration. При `settle` она сравнивается с
актуальной: удалённый или заменённый tool возвращает `Stale tool call`.
Scoped registration снимается finalizer. Это сильнее простого поиска по
имени: модель не сможет вызвать под прежним описанием новую реализацию.
[Materialization и settlement][oc-registry].

`Tool.make()` валидирует input и output codecs, отдельно формирует structured
output и model output. Общий settlement вызывает `ToolOutputStore.bound`.
Runner сначала публикует tool-call, затем запускает handler, публикует
tool-result и ждёт завершения tool fibers перед продолжением; interrupt
очищает fibers и закрывает незавершённые tool states.
[Tool codecs][oc-tool], [runner][oc-runner].

Здесь OpenCode сам владеет LLM loop: передаёт materialized definitions в
`LLM.request`, исполняет local tool calls и продолжает conversation. Uprava
подключает самостоятельные provider runtimes, поэтому переносить этот цикл
в Core не следует. У native tools провайдера и Core-managed tools разные
владельцы: первые адаптер наблюдает и ограничивает через подтверждённые
provider capabilities; вторые проходят наш authorization/execution route.
Общий timeline не означает общий механизм исполнения.
[Граница runner][oc-runner].

Для первой версии сохранить узкие `search_tools` / `inspect_tool` /
`execute_tool` из [контракта Uprava](../../../development/agent-tooling-contracts.md).
Snapshot registry связывать с конкретным execute, а dynamic mounting всего
каталога считать optional capability адаптера. OpenCode перечисляет
connected MCP tools в provider toolset; это полезный образец адаптации схем,
но само по себе не обосновывает стоимость большого каталога в контексте
модели или его горячую замену внутри чужого runtime.
[MCP-to-provider adaptation][oc-mcp-execute].

Это **не готовый централизованный security boundary**. Registry удаляет
полностью запрещённые tools из materialization, но его общий settlement не
выполняет универсальный `PermissionV2.assert`; конкретный `bash` делает это
сам. `PermissionV2` сохраняет remembered allow на project scope, не позволяет
ему перекрыть configured deny, а pending requests держит в памяти и завершает
при disposal.
[Registry][oc-registry], [bash permission][oc-bash],
[PermissionV2][oc-permission].

Для Uprava перенести materialization как immutable
`tool_id + definition_version + schema_hash + connection_generation`,
добавив fresh authorization на каждом execute и approval, связанную с
attempt/arguments/policy. Registration version не заменяет permission check.
Уже начавшийся вызов требует отдельной cancel/revoke policy: тест OpenCode
специально сохраняет captured execution после удаления регистрации.
[Тесты stale registration и running call][oc-registry-tests].

## Решения для второй итерации

| Решение | Взять или изменить | Владелец и цена |
| --- | --- | --- |
| Durable admission до wake | Взять; один input ID, конфликт при другом payload | Core хранит input, Node сериализует attempt; нужен reconciliation |
| Replay и барьер видимости | Адаптировать watermark к typed command/result | Core проецирует, Node сообщает sequence; UI различает выполнение и синхронизацию |
| Materialized tool catalog | Взять identity/version, добавить schema и policy checks | Core snapshot и authorization, Node generation; возможен явный stale отказ |
| MCP lifecycle | Взять явные auth/failure states и invalidation | Node/ToolHive supervise; Core хранит desired state и availability |
| OAuth reconnect | Взять URL binding и staged replacement | Core callback broker, Node secrets; требуется протокол generation |
| Общий server password | Не копировать как cloud identity model | В коде Basic auth опционален и без password отключён; Uprava нужен actor/session scope. [Код][oc-server-auth] |
| Наследование `process.env`, raw MCP logs | Не копировать | Local MCP получает весь environment; logging передаёт upstream `data`. Для Uprava нужны allowlist и redaction до persistence. [Код][oc-mcp-connect], [logging][oc-mcp-watch] |
| Лояльное исправление schemas | Адаптировать осознанно | Catalog имеет fallback без `outputSchema`, а имена нормализует с потерей символов. Сохранять raw identity, schema fidelity и диагностировать несовместимость. [Код][oc-catalog] |

**P0:** один Node-local provider adapter; durable input/command identity;
per-attempt execution owner; snapshot/replay; interrupt с подтверждённым
terminal outcome; один managed tool route с fresh policy, отзывом доступа
и materialized schema version. Это основной цикл «подключился → отправил → вмешался →
вернулся», а не отдельный поздний infrastructure epic.

**P1:** полноценный desired/actual MCP lifecycle, remote OAuth broker,
автоматическая rotation, catalog change handling, расширенные artifacts и несколько
provider capabilities. Remote session migration и универсальную proxy-сеть
отложить: прочитанный код показывает их отдельную стоимость, а первый
сценарий Uprava может иметь фиксированный placement.

Прочитаны сценарии тестов для [overflow и replay handoff][oc-event-tests],
[interrupt/wake cleanup][oc-coordinator-tests],
[catalog refresh и pagination][oc-mcp-tests],
[stale registration][oc-registry-tests]. Их следует перенести как требования
к контрактам Uprava. Тесты upstream в рамках исследования не выполнялись.

[oc-client]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/app/src/context/server-sdk.tsx#L187-L328
[oc-refresh]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/app/src/context/server-sync.tsx#L531-L571
[oc-local]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/session/execution/local.ts#L10-L35
[oc-session]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/session.ts#L346-L431
[oc-session-http]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/server/src/handlers/session.ts#L139-L170
[oc-routing]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/server/routes/instance/httpapi/middleware/workspace-routing.ts#L109-L184
[oc-paths]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/server/shared/workspace-routing.ts#L5-L44
[oc-sync]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/control-plane/workspace.ts#L307-L438
[oc-sync-wait]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/control-plane/workspace.ts#L827-L851
[oc-event-commit]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/event.ts#L236-L361
[oc-event-follow]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/event.ts#L541-L603
[oc-coordinator]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/session/run-coordinator.ts#L24-L103
[oc-live]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/server/src/handlers/event.ts#L9-L48
[oc-overflow]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/event.ts#L150-L164
[oc-mcp-connect]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/index.ts#L218-L412
[oc-mcp-watch]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/index.ts#L442-L492
[oc-mcp-tools]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/index.ts#L648-L688
[oc-mcp-execute]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/session/tools.ts#L390-L486
[oc-catalog]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/catalog.ts#L14-L167
[oc-auth]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/auth.ts#L37-L95
[oc-oauth]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/oauth-provider.ts#L187-L244
[oc-callback]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/oauth-callback.ts#L6-L146
[oc-mcp-remove]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/src/mcp/index.ts#L918-L948
[oc-registry]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/tool/registry.ts#L42-L134
[oc-tool]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/tool/tool.ts#L71-L130
[oc-runner]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/session/runner/llm.ts#L203-L323
[oc-bash]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/tool/bash.ts#L104-L154
[oc-permission]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/src/permission.ts#L117-L283
[oc-server-auth]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/server/src/middleware/authorization.ts#L29-L55
[oc-event-tests]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/test/event.test.ts#L323-L504
[oc-coordinator-tests]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/test/session-run-coordinator.test.ts#L218-L317
[oc-mcp-tests]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/opencode/test/mcp/lifecycle.test.ts#L219-L283
[oc-registry-tests]: https://github.com/anomalyco/opencode/blob/907b3bc518fa48e90e8ec24dd327d13eee71c36c/packages/core/test/session-runner-tool-registry.test.ts#L336-L449
