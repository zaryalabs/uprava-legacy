# T3 Code: удалённые агенты, управление исполнением и tooling

Дата исследования: **2026-10-03**. Зафиксирован исходный код
`pingdotgg/t3code@ce90eec1ffc2087395227a610843091f4b260cf7`. Ниже — статическое
чтение реализации и выбранных тестов; программы, сборки и тесты T3 не запускались.
Это дополнение к [первому обзору](01-reference-systems.md), которое заменяет
его незакреплённые наблюдения о T3 конкретными свидетельствами.

**Главное заимствование для Uprava — разделить долговечное намерение пользователя,
живое исполнение провайдера и восстановление клиентского экрана.** Tooling нужно
привязывать к владельцу исполнения и его полномочиям. T3 даёт подробные образцы
этих границ, но его environment совмещает часть обязанностей наших Core и Node.
Поэтому переносить топологию целиком нельзя.

## 1. Удалённое подключение: три отдельные авторизации

Сквозной путь подключения выглядит так:

```text
cloud identity + environment link
  → relay EnvironmentConnector
  → environment mintCredential
  → одноразовый bootstrap, привязанный к ключу клиента
  → environment /oauth/token
  → короткий WebSocket ticket
  → environment /ws → проверка scope RPC
```

`EnvironmentConnector` сначала получает разрешённый environment link и
managed endpoint, затем обращается к environment за credential. Он запрещает
redirect и проверяет подпись ответа, environment ID, nonce запроса и thumbprint
ключа клиента; клиенту возвращаются endpoint и bootstrap credential.
[Код брокера][relay]. На стороне environment `cloudMintCredentialHandler`
проверяет issuer/audience, связанного cloud user, срок proof, scope и replay
guards `jti`/nonce. Выдаваемый grant живёт две минуты и связан с proof key.
[Код выдачи][mint]. Это авторизация подключения к environment, а не вход в
аккаунт Codex/OpenCode.

Обмен bootstrap на access token позволяет только сузить scopes.
[Проверка grant][auth-exchange]. DPoP-bound
session требует DPoP proof; несоответствие не переключается на Bearer.
[Обмен и проверка][auth]. Для WebSocket клиент получает ticket через
аутентифицированный HTTP и помещает в URL ticket, а не долгоживущий access token.
[Клиентский путь][ticket]. Таблица `RPC_REQUIRED_SCOPES` типизирована по всему
набору RPC: новый метод без выбранного scope вызывает ошибку типов; диспетчер
оборачивает операции в проверку этого scope. [Таблица][rpc], [применение][rpc-wrap].

После bootstrap клиент обращается по endpoint окружения. Relay Worker не
становится прокси каждого сообщения: это прямо описано в internals и согласуется
с возвратом endpoint и построением клиентского `/ws` URL.
[Топология][connect-doc]. Для Uprava сохраняем **Web → Core → Node → provider**.
Из T3 берём отдельные идентичности пользователя, ноды, provider account и MCP
session; проверяемый bootstrap, отзыв credentials и исчерпывающую таблицу
полномочий. Cloudflare, Clerk и собственный DPoP flow пока не являются
обязательными технологиями: их цена оправдана только выбранной моделью доступа.

Ограничение: T3 считает filesystem boundary всем environment. Projects не
изолируют файлы; `orchestration:read` допускает доступ к читаемым серверным
аккаунтом абсолютным путям. Это явно заявленная модель, а не найденный обход.
[Граница доверия][auth-doc]. В production-профиле Uprava разрешённые roots и
изоляция исполнения должны принудительно обеспечиваться Node.

## 2. Сообщение: от RPC до native turn и обратно

Входящий RPC проходит `startup.enqueueCommand` → `ThreadMessageIntake` →
Orchestrator. [Точка входа][dispatch]. Orchestrator сериализует команды одного
thread через `threadDispatch.withLock`, проверяет `commandId` receipt и возвращает
ранее зафиксированный результат повтору. Использование ID для другого thread
отклоняется. [Receipt и lock][receipt].

`EventSink.commitCommand` в одной SQL-транзакции резервирует receipt, добавляет
domain events, обновляет проекции, ставит effects в outbox и сохраняет sequence
результата. После commit публикуются события и сигнал готовности outbox.
[Транзакция][commit]. Это важнее самого выбора WebSocket: принятая команда
оставляет долговечное намерение до обращения к провайдеру.

`EffectWorker` исполняет `provider-turn.start` через `ProviderTurnStartService`.
Worker использует durable claim и lease; после claim повторно проверяет отмену,
поскольку она могла произойти до регистрации локального ожидания.
[Маршрут effect][worker], [гонка отмены][cancel-race]. Затем service вызывает
`session.startTurn`; Codex adapter сохраняет `pendingRootTurns`, отправляет
native `turn/start`, сопоставляет native ID с run и убирает pending запись.
[Service][start-service], [Codex turn][codex-start].

Ответный путь проходит через нормализованные provider events и
`ProviderEventIngestor` в `EventSink`, затем в thread stream и клиентскую проекцию.
Ingestor имеет условные записи `writeIfRunCurrent` и
`writeIfProviderThreadOwner`, позволяющие отвергать события прежнего владельца.
[Запись событий][ingest]. Для Uprava такой guard нужно выражать через
`session + attempt + generation + policy`, а не только provider thread ID.

Receipt **не доказывает exactly-once выполнение внешнего tool**. Прочитанная
ветка проверяет принадлежность receipt thread, но сама не сравнивает fingerprint
полного payload. Наша рекомендация — неизменяемый payload для `commandId`,
проверка его hash и отдельная reconciliation для неизвестного исхода внешнего
запуска. Повтор принятой UI-команды и повтор provider side effect — разные задачи.

## 3. Живой runtime и восстановление

T3 разделяет driver, provider instance, provider session, provider thread и turn.
`ProviderSessionManager` получает adapter по **instance ID**, готовит MCP
credential перед `openSession`, хранит live entry и управляет scope процесса.
Credential резервируется на время открытия, чтобы concurrent release не отозвал
его до публикации entry. [Открытие session][session-open]. Codex factory запускает
app-server child process и строит двусторонний протокол поверх child transport;
adapter реализует `initialize`, `turn/start` и `turn/steer`.
[Factory][codex-factory], [инициализация][codex-init].

У OpenCode 2 другой контракт: один server на provider instance. При наличии
`serverUrl` T3 подключается к внешнему серверу; иначе владеет запускаемым
процессом с генерируемым паролем и idle release. [Владелец сервера][opencode-server].
Нельзя навязывать всем адаптерам схему «один процесс на чат». Для первого
среза Uprava достаточно Codex, но instance/account identity и ownership
процесса должны быть явными с начала.

У session есть собственный event pump. Он сохраняет runless запросы login,
project trust или hooks ещё до появления подписчика активного run. Иначе
`openSession` может ожидать ответа, который UI никогда не покажет.
[Event pump][pump]. Процесс живёт по правилам Node supervisor, а не React-компонента;
закрытие вкладки должно закрывать подписку пользователя, не сессию агента.

Восстановление имеет два уровня:

- **Web ↔ server.** Сервер подписывается на live events до чтения persisted
  tail, фиксирует high-water sequence и склеивает replay с live. Публикации
  после разных SQL commit дополнительно упорядочиваются `publishLane`, иначе
  seq 2 мог бы прийти раньше seq 1 и клиент отбросил бы первое событие.
  [Replay][replay], [порядок публикации][publish-order]. Клиент отбрасывает
  sequence ≤ уже применённого и сохраняет согласованную пару projection/cursor;
  snapshot/replay поддерживают явный completion marker и bounded history.
  [Клиентская проекция][client-state], [deduplication][client-dedup],
  [server stream][thread-stream].
- **Adapter ↔ provider.** После обрыва OpenCode adapter сверяет active sessions,
  восстанавливает пропущенную историю и отдельно читает pending permissions/forms.
  Запрос, возникший во время разрыва, снова появляется; исчезнувший у провайдера
  перестаёт висеть в UI. [Reconciliation][opencode-reconcile]. Это не заменяется
  повторной подпиской браузера. В коде теста есть сценарий разрешения, возникшего
  при отключённом stream; тест здесь только прочитан. [Тест][opencode-test].

Для Uprava нужны оба уровня. Core хранит пользовательскую историю и replay,
Node восстанавливает правду о native execution. Web может показывать сохранённую
историю, пока состояние исполнения неизвестно. Цена — курсоры, snapshots,
проекции и миграции; это часть первого надёжного чата, а не поздняя функция.

## 4. Approvals, вопросы и остановка

Codex callback `item/commandExecution/requestApproval` создаёт execution node,
runtime request и turn item, сохраняет `Deferred` решения и публикует события.
`respondToRuntimeRequest` находит именно pending request и завершает Deferred;
callback переводит `acceptAlways` в `acceptForSession`.
[Ожидание][approval], [ответ][approval-response]. В Orchestrator пользовательский
ответ сначала проверяется по сохранённому pending request; затем resolution и
effect доставки фиксируются через общий command commit. [Команда ответа][request-command].

Особенно полезно различие **live request** и **вопроса, ответ на который является
новым сообщением**. Codex async question получает `responseMode: message`.
Orchestrator сохраняет решение и пользовательское сообщение в одной транзакции,
после чего может поставить его в очередь либо steer активный run.
[Async question][async-question], [ответ сообщением][async-answer]. После смерти
процесса нельзя изображать старый RPC callback живым; такую семантику нужно
передавать UI через capability.

Остановка тоже не равна успешной отправке команды: Codex adapter ожидает native
start для ещё queued turn и только затем вызывает `turn/interrupt`; ожидание
ограничено десятью секундами с явной ошибкой недоставленной остановки.
[Interrupt][interrupt]. Для Uprava сохраняем отдельные `requested`,
`delivered`, `confirmed/failed/unknown` и приоритет управляющих команд.

## 5. Tooling: полезный образец и граница его применимости

Сквозной путь собственного инструмента T3:

```text
ProviderSessionManager → thread-scoped MCP credential
  → adapter добавляет t3-code MCP в native provider config
  → provider выполняет MCP HTTP call
  → McpHttpServer auth → McpInvocationContext
  → capability + caller/project checks → application service → command/events
```

`McpSessionRegistry` создаёт случайный token, хранит его hash и scope
`environmentId/threadId/providerSessionId/providerInstanceId/capabilities`.
Resolve и `touch` поддерживают liveness; штатная остановка отзывает credential.
По умолчанию окно liveness — 24 часа, registry находится в памяти.
[Credential registry][mcp-registry]. `McpHttpServer` преобразует Bearer в
`McpInvocationContext`; toolkit handlers получают доверенный контекст, а не
thread ID, произвольно переданный моделью. [HTTP boundary][mcp-http].
Проверки caller привязывают чтение к project, а mutation — к активному run и
provider instance. [Проверки ресурса][mcp-access].

Остановка прежней session учитывает ID credential и наличие live replacement
или незавершённого открытия. Поэтому stale cleanup не уничтожает доступ новой
session. [Отзыв владельцем][mcp-revoke]. Тесты фиксируют expiry, продление
активностью собственного thread и отсутствие продления от чужого thread;
это свидетельства выбранного контракта, не результаты нашего запуска.
[Тесты lease][mcp-tests].

Codex получает endpoint/header в native `mcp_servers` config.
[Инъекция][mcp-config]. OpenCode 2 требует дополнительного решения: MCP entries
видны на уровне directory, поэтому каждый thread получает имя `t3-code-<thread>`;
session rules сначала запрещают все T3 entries, затем разрешают собственный,
включая full-access mode. [Namespace и deny/allow][mcp-rules]. Ответ «для сессии»
превращается в T3 session rule и native `once`, поскольку native `always`
сохраняет grant шире, на project. [Перевод permission][opencode-permission].
Для Uprava это обязательное правило адаптации: продуктовый scope нельзя молча
расширять из-за более грубого permission API провайдера.

T3 tool timeline нормализует native `mcpToolCall`/`dynamicToolCall`: имя, input,
status, structured result или content и error. Это наблюдение уже выполняемого
tool, а не перехват исполнения. [Нормализация][tool-projection]. Собственный
`/mcp` регистрирует конкретные application toolkits: orchestration, threads,
worktrees, preview, projects и другие. [Регистрация][mcp-toolkit]. В прочитанном
пути нет аналога общего Core-owned каталога внешних инструментов с
`Search → Inspect → Execute`; заимствовать у T3 подтверждение готовности нашей
ToolHive-интеграции нельзя.

Наш вывод: сохраняем **observed native tools** отдельно от **managed tools**.
Core владеет каталогом, desired state, policy, approvals и audit; Node —
фактической доступностью, provider-specific конфигурацией и локальным bridge.
Первый срез содержит маленький собственный MCP toolkit и один полный lifecycle
инструмента. Progressive discovery и ToolHive расширяются после того, как
подключение, scope, отзыв и ошибка одного инструмента стали понятны пользователю.

## 6. Что переносить и в каком порядке

| Решение | Действие для Uprava | Цена и приоритет |
| --- | --- | --- |
| Driver / instance / live session / native thread / turn | Взять различие идентичностей; Node владеет runtime, Core хранит refs | Больше сущностей, зато account и process не смешиваются; **P0** |
| Durable command → event/projection/outbox | Адаптировать к Core dispatch и Node acknowledgements, добавить payload hash | Транзакции, reconciliation неизвестного исхода; **P0** |
| Snapshot + cursor + ordered replay | Взять контракт; Core sequence не заменяет provider reconciliation | Storage и версия проекций; **P0** |
| Typed interaction + response capability | Взять live/message/not-resumable, pending-state checks и native option IDs | Таблицы перевода на каждый provider; **P0** |
| MCP credential принадлежит исполнению | Адаптировать к Core lease и Node attempt; очистка проверяет generation/credential ID | Ротация и recover после process loss; **P0** |
| Namespaced MCP и узкие grants | Взять инвариант даже для shared server и full-access | Provider-specific permissions; **P0** для первого adapter, **P1** для остальных |
| Relay bootstrap и per-RPC scopes | Взять разделение доверия и полный scope map; сохранить нашу топологию | Identity/revocation; **P0**, relay/DPoP продуктового масштаба — **P1** |
| Весь environment как filesystem boundary | Не копировать для изолированных production workspace | Node path enforcement и isolation входят в **P0** |
| Широкий каталог toolkit и multi-provider workflows | Сохранить как идеи; сначала минимальная полезная поверхность | Не повторять расширение scope раньше качества; **P1** |

P0 завершается архитектурным контрактом одного удалённого Codex-чата: повтор
команды не создаёт второй run, возврат клиента восстанавливает историю и запросы,
старая session не управляет новым attempt, инструмент получает только действующий
scope. P1 добавляет shared-server провайдеры, управление расширенным каталогом
внешних MCP и более сложное подключение окружений без изменения этих инвариантов.

## Ограничения и проверяемость выводов

Чтение целевое: entry points, auth, orchestration, provider session lifecycle,
Codex/OpenCode adapters, MCP и клиентский replay. Полного security-аудита,
анализа всех драйверов, лицензирования заимствований или оценки производительности
нет. Наличие механизма и теста не доказывает отсутствие других путей обхода.
Internals использованы как карта; утверждения о реализации подкреплены pinned
code links. Модели угроз и рекомендации P0/P1 — выводы для Uprava, не обещания T3.
Проверки программ не проводились по запросу пользователя; новый практический
прогон не является условием завершения этого исследования.

[relay]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/infra/relay/src/environments/EnvironmentConnector.ts#L560-L670
[mint]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/cloud/http.ts#L1511-L1589
[auth]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/auth/EnvironmentAuth.ts#L638-L688
[auth-exchange]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/auth/EnvironmentAuth.ts#L806-L837
[ticket]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/packages/client-runtime/src/authorization/remote.ts#L161-L249
[rpc]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/auth/RpcAuthorization.ts#L17-L212
[rpc-wrap]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/ws.ts#L1561-L1595
[connect-doc]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/t3-connect.md#L1-L36
[auth-doc]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/docs/internals/environment-auth.md#L58-L64
[dispatch]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/ws.ts#L1753-L1779
[receipt]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Orchestrator.ts#L9442-L9630
[commit]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/EventSink.ts#L518-L576
[worker]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/EffectWorker.ts#L147-L167
[cancel-race]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/EffectWorker.ts#L637-L670
[start-service]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/ProviderTurnStartService.ts#L1166-L1187
[codex-start]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L5531-L5614
[ingest]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/ProviderEventIngestor.ts#L550-L592
[session-open]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/ProviderSessionManager.ts#L1704-L1805
[codex-factory]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L1379-L1429
[codex-init]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L1550-L1622
[opencode-server]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/provider/opencode2/OpenCode2Server.ts#L138-L196
[pump]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/ProviderSessionManager.ts#L1541-L1592
[replay]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/EventSink.ts#L712-L748
[publish-order]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/EventSink.ts#L228-L261
[client-state]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/packages/client-runtime/src/state/threads.ts#L229-L251
[client-dedup]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/packages/client-runtime/src/state/threads.ts#L443-L469
[thread-stream]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/ws.ts#L648-L765
[opencode-reconcile]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/OpenCode2AdapterV2.ts#L2740-L2808
[opencode-test]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/OpenCode2AdapterV2.test.ts#L2603-L2629
[approval]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L4473-L4533
[approval-response]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L6023-L6059
[request-command]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Orchestrator.ts#L6774-L6847
[async-question]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L4384-L4412
[async-answer]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Orchestrator.ts#L6935-L7000
[interrupt]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L5870-L5891
[mcp-registry]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/mcp/McpSessionRegistry.ts#L67-L212
[mcp-http]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/mcp/McpHttpServer.ts#L102-L128
[mcp-access]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/mcp/threadAccess.ts#L21-L101
[mcp-revoke]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/ProviderSessionManager.ts#L928-L965
[mcp-tests]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/mcp/McpSessionRegistry.test.ts#L127-L181
[mcp-config]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L1201-L1228
[mcp-rules]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/OpenCode2AdapterV2.ts#L439-L485
[opencode-permission]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/OpenCode2AdapterV2.ts#L3995-L4074
[tool-projection]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts#L456-L501
[mcp-toolkit]: https://github.com/pingdotgg/t3code/blob/ce90eec1ffc2087395227a610843091f4b260cf7/apps/server/src/mcp/McpHttpServer.ts#L663-L729
