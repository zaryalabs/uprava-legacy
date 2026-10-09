# DeepSeek Harness: developer tools, история и причинность

Дата разбора: **2026-10-07**. Репозиторий `deepseek-ai/deepseek-harness`,
commit `5badb15009ae1756c3afe0ae0cef1faafc290ccc`, корневой package
`0.2.1-alpha.1`. Метод: чтение исходников и выбранных тестов; программы и
тесты upstream не запускались. Рассмотрены Session Inspector, NodeJS Inspector,
agent-loop, persistence, semantic checkpoint policy, telemetry и replay.

Основное заимствование для Uprava — инспекция тех же данных, на которых работает
чат, плюс явное происхождение контекста и результатов tools. Разработчик может
перейти от карточки в разговоре к её событию и от события обратно к отображению.
Это делает trace полезным при ежедневной работе. Отладчик JavaScript и просмотр
сырого сетевого обмена — следующий, существенно более привилегированный слой.

## 1. Запрос модели → журнал → Session Inspector → карточка чата

### Что именно фиксируется

`Agent.step()` получает подготовленную сборку, записывает system messages и
принятые user messages, строит запрос и создаёт `AssistantStreamAttempt`.
Во время генерации публикуются live frames. При прерывании уже полученный текст
становится `assistant/message` с `interrupted: true`; незавершённая попытка без
такого сообщения сохраняется как `assistant/attempt`. В обоих вариантах
сохраняется компактный исходный stream. Таким образом, у попытки остаётся
собственный результат даже при отсутствии успешного итогового ответа.
[Agent.step](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/agent-loop/src/agent.ts#L398-L475).

Перед отправкой фиксируется `request/header`: выбранный provider/model,
эффективные adapter defaults и собранные tool schemas. Новый header имеет
причину `initial`, `resume`, `change` или `series`. При изменении доступных tools
цикл добавляет `developer/message` с additions/removals и ссылкой на header,
определяющий новые инструменты. Это объясняет, какие tools были предложены модели
в конкретный момент, даже если текущий registry уже изменился.
[buildRequest: header и изменения tools](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/agent-loop/src/agent.ts#L610-L665).

System prompt находится в `system/message`, а динамический runtime context —
в user message с `source`, `form: snapshot` и именованными `sections`.
`RuntimeContextProjection.project()` создаёт новый снимок только при изменении;
исчезновение контекста тоже представлено явно. Источник сообщения позволяет
различить ввод человека, синтетическую инъекцию и продолжение goal.
[RuntimeContextProjection.project](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/agent-loop/src/runtime-context.ts#L146-L162),
[контракты сообщений и header](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/session/src/types.ts#L234-L327).

Для tools `appendToolCall()` возвращает sequence события; `appendToolResult()`
записывает его в `sourceEventSeqs`. Result сохраняет model-visible content,
структурированную ошибку и отдельный `meta`, например diff для карточки.
Прерванный до dispatch вызов тоже получает call/result с явной причиной.
Причинность и представление воспроизводятся из журнала без повторного tool call.
[appendSkippedToolCall / appendToolResult](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/agent-loop/src/tool-calls.ts#L249-L289).

### Как событие становится историей и интерфейсом

`Session.append()` делает lossless JSON snapshot входных данных, назначает
непрерывный `seq`, замораживает событие и проверяет surface contract до помещения
в журнал. Изменение исходного объекта после append не меняет историю.
Publication observers вызываются после добавления в память.
[Session.append](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/session/src/index.ts#L718-L762).

JSONL provider подписывается на `session/event`, буферизует запись и явно дренирует
writer на `session/flush`. `persistContiguous()` проверяет непрерывность, получает
write lease, ремонтирует оборванный хвост и продвигает cursor после успешной
записи. Semantic checkpoint plugin ждёт flush перед model dispatch и верхним
tool dispatch; ошибка checkpoint не допускает соответствующий side effect.
Наличие события в live UI само по себе не подтверждает завершение физической
записи. Политика checkpoint — отдельный подключаемый компонент, и её фактическое
включение входит в deployment contract.
[install / flush](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-persistence-jsonl/src/storage.ts#L534-L553),
[persistContiguous](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-persistence-jsonl/src/storage.ts#L318-L353),
[checkpoint policy](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-checkpoint-policy/src/index.ts#L63-L82).

`SessionHistoryController.follow()` сначала подписывается, затем читает opening
snapshot и его cursor. Буфер покрывает события между чтением и началом follow;
уже представленные записи пропускаются, разрыв ожидаемого sequence завершает
stream ошибкой. Assistant live frames идут отдельным вариантом с собственным
baseline. Этот stream использует семантически сохраняемые события сессии,
но не является самостоятельной квитанцией об их fsync.
[follow](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/api/session-controller/src/history.ts#L120-L239).

Session Inspector получает **общий SessionEventSource**, которым пользуются Chat
и Trajectory. `SessionLogModel` отдельно хранит порядок rows и observable каждой
записи, раскрывает embedded assistant stream и сохраняет идентичность live row
при превращении попытки в событие. Подписка существует только пока таблица
наблюдается. Выбранная строка открывает raw JSON; `callId`, anchor sequence,
turn/step связывают её с chat node. Picker работает в обратную сторону.
[SessionLogModel](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/session-inspector/src/client/views/session-log/model.ts#L22-L145),
[SessionInspectorView](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/session-inspector/src/client/views/View.tsx#L47-L97).

Таблица виртуализирована, имеет stable row keys, type filter, tail/manual scroll
и постраничную загрузку. Raw details показывают исходную запись, а Chat mode —
объекты уже построенной projection: можно выяснить, пропало ли событие или
ошибся renderer. Тест `session-log.client.spec.ts` проверяет сохранение старых
row objects при новых deltas и привязку tool/reasoning/step к чату.
[InspectorTable](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/session-inspector/src/client/views/InspectorTable.tsx#L112-L130),
[виртуализация](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/session-inspector/src/client/views/InspectorTable.tsx#L236-L243),
[прочитанные тесты](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/session-inspector/tests/session-log.client.spec.ts#L21-L87).

Для Uprava стоит сохранить схему «событие → projection → несколько views»,
добавив к причинным координатам `node_id`, runtime attempt, command receipt,
provider reference и revision tool catalog. При provider-native Codex нельзя
обещать восстановление полного prompt/tool header, если adapter не получает его
от провайдера. Полноту trace надо показывать явно: observed, reported by provider,
unavailable. Иначе красивый inspector создаст ложное ощущение полного контроля.

## 2. Cordis runtime → detached tree → Worker → DevTools

`HostPlugin.apply()` запускает Inspector Worker, публикует Cordis tree,
регистрирует `ctx.inspector`, bootstrap и frontend routes. Collector обходит
Context/Fiber, убирает технические shadow wrappers, присваивает object handles
и выдаёт versioned JSON snapshot с revision и флагом `truncated`. Число nodes
и размер ограничены. Worker получает данные через MessagePort для Host и
WebSocket для браузерного Client; живые Cordis objects остаются в своём realm.
[HostPlugin.apply](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/host/plugin.ts#L49-L69),
[CordisTreeCollector.snapshot](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/shared/cordis/collector.ts#L39-L114).

`CordisTreeStore` хранит latest-value snapshots и замороженную последнюю topology
отключившегося источника. После disconnect live object routing становится
недоступным; сохранённая картинка не выдается за живой runtime.
`ctx.inspector.cordis.getTree()` читает detached semantic tree без CDP ids.
Это полезная самостоятельная API-граница: иной UI может использовать ту же
топологию без Chrome DevTools.
[CordisTreeStore](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/inspection/cordis-store.ts#L57-L129),
[executeInspectorQuery](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/inspection/cordis-query.ts#L12-L16).

Observation transport ограничивает queue по records/bytes и отбрасывает старый
префикс при overflow, сохраняя sequence gap. `source/replace` передаёт актуальное
состояние, а append указывает `droppedBefore`. Тест протокола проверяет gaps и
resnapshot. Эта политика сохраняет отзывчивость приложения, но диагностический
stream не заменяет canonical journal.
[InspectorSourceBuffer](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/shared/bridge/buffer.ts#L78-L154),
[тест gap / resnapshot](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/tests/protocol.host.spec.ts#L30-L85).

DevTools отображает topology в Elements и маршрутизирует Console evaluation
в выбранный realm. Для Host открывается `node:inspector.Session` на соединение;
из Worker он подключается к main thread. Это позволяет управлять Host debugger,
когда главный JavaScript thread остановлен. Сама семантическая tree query
read-only; Console допускает выполнение произвольного кода. Config update и
hot reload имеют отдельный lifecycle loader/fiber, рассмотренный в
[разборе модульности](11-deepseek-harness-modularity.md).
[HostInspectorSession](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/realms/host/bridge.ts#L20-L91),
[Host capabilities](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/realms/host/index.ts#L12-L30).

Bottom panel зарегистрирован через slot, lazy iframe создаётся при первом
открытии и остаётся mounted при collapse. Смена session не уничтожает debug
соединение. Это конкретный пример модульного UI, сохраняющего собственный
lifecycle; подобный принцип стоит использовать для наших terminal/editor/
inspector surfaces.
[registerInspectorPage / iframe lifecycle](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/client/bottom/page.tsx#L28-L97).

Для Core/Node переносится **семантическая topology**: module instance,
предоставляемые capabilities, зависимости, состояние, scope и generation.
Rust Core не получает пользу от буквального копирования JS object handles.
Удалённый Node может публиковать ограниченный snapshot через свой
аутентифицированный control channel; чтение устаревшей topology должно явно
показывать disconnected/generation, а команды изменения проверять актуальную
версию независимо от UI.

## 3. Fetch → observation journal → Network panel

`installFetchObserver()` оборачивает `globalThis.fetch`, публикует URL, headers,
status/errors и захватывает body через clones. Original Response возвращается
после ответа upstream; чтение clone продолжается отдельно. Остановка восстанавливает
прежний fetch и отменяет readers. `NetworkStore` хранит нормализованные lifecycle
events и bytes в памяти; `NetworkDomain.enable()` воспроизводит retained events,
а `getResponseBody`, `getRequestPostData`, `streamResourceContent` обслуживают
Network/Response/EventStream views.
[installFetchObserver](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/host/inspection/network.ts#L58-L158),
[NetworkStore](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/inspection/network-store.ts#L92-L141),
[NetworkDomain](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/cdp/domains/network/session.ts#L74-L107).

Capture сохраняет headers и payload без redaction. Direct Worker CDP upgrade
не проверяет token; Client ingest требует случайный subprotocol token и origin.
App-relative CDP relay проверяет обычную web authorization. Выбор Client
ограничивает отображаемые realms и не создаёт границу полномочий. Такой debugger
следует оценивать как доступ к исполнению с властью Host, включая credentials.
[Worker upgrade / Client authorization](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/bridge/endpoint.ts#L126-L143),
[ingest token/origin](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/worker/bridge/endpoint.ts#L219-L229),
[authenticated relay](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/inspector/src/host/plugin.ts#L111-L147).

В Uprava P0 полезнее trace model/tool requests с заранее определённым
безопасным envelope и bounded payload references. Raw capture — отдельная
временная диагностика на конкретной Node с явным scope. Скрывать лишние
строки после загрузки в браузер недостаточно: redaction и ACL должны сработать
до передачи/сохранения. Принцип наблюдения вне основного loop стоит взять,
а глобальный перехват любого сетевого payload требует отдельного решения.

## 4. Canonical log → telemetry export и replay

`SessionTelemetryRecord` разделяет `ledger` с event envelope/sequence и `ops`
без log identity. Export body — копия payload. `session-telemetry/record`
waterfall является redaction seam: по умолчанию rules отсутствуют, а исключение
правила удерживает запись и не нарушает agent loop. Canonical log сохраняет
оригинал независимо от правил экспорта. OTel backend выбирает
`FEEDBACK_ONLY` или `DISABLED`; первое разрешает capture по feedback этой
сессии, а inherited fork feedback не служит новой авторизацией.
[SessionTelemetry contract](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-telemetry/src/index.ts#L25-L108),
[OTel sharing policy](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-telemetry-otel/src/index.ts#L32-L79),
[redaction tests](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/session/session-telemetry/tests/redact.spec.ts#L39-L76).

Запись истории и отправка telemetry имеют разные гарантии доставки. Для
Uprava это означает отдельные политики чтения локального trace, получения
trace через Core и выгрузки наружу; безопасность canonical log не возникает
автоматически из redaction export copy.

`llm-replay` — test-support adapter, который извлекает модельные streams из
session fixtures и подставляет их вместо реального провайдера. Throw/hang,
невосстановимые только по журналу, задаются override. Это воспроизводимость
protocol/UI сценария, а не обещание повторить внешний side effect или получить
от модели прежнее решение. Такой seam полезен позже для регрессий remote
reconnect, прерывания tool и повторной projection без платных вызовов.
[ReplayEntry / ReplayConfig](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/test-support/llm-replay/src/index.ts#L58-L70),
[fixtures и overrides](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/test-support/llm-replay/src/index.ts#L124-L163).

## Что переносить и в каком порядке

| Механизм | Решение для Uprava | Стоимость и адаптация |
| --- | --- | --- |
| Session Log рядом с Chat projection | Взять в P0 | Один event source, raw details, ссылки по call/event id; не дублировать subscriptions/history |
| Header/tool snapshot и context provenance | Взять в P0 | Фиксировать revision/hash при admission; показывать provider-visible ограничения |
| `sourceEventSeqs`, result presentation metadata | Взять в P0 | Сохранить причинность Core command → Node attempt → tool call → result/artifact |
| Semantic checkpoint до side effect | Адаптировать в P0 | Опереться на Core transaction/outbox и Node receipt; flush локального файла не распределённый commit |
| Detached module topology | Взять в P1 | Node snapshots + Core projection с scope, generation и disconnected status |
| Bounded diagnostic observation/gaps | Взять в P1 | Byte/count bounds и explicit truncation; durable domain events остаются отдельными |
| CDP Runtime evaluation и raw fetch capture | Отдельная developer capability | Не включать в основной production control API; доверие, credentials и runtime отличаются |
| Redaction seam + telemetry sink | Адаптировать в P1 | Deployment rules должны существовать до export; payload access/retention задаются отдельно |
| Keyless LLM replay | Взять для последующих регрессий | Fixtures детерминируют входные provider frames, не реальное выполнение tools |

**P0:** завершить один cloud-chat сценарий с прозрачной цепочкой ввода,
dispatch, provider response и tool outcome. Добавить маленькую Session Inspector
surface: одна таблица, lazy raw details, переход в чат и обратно, видимые
пропуски/неполнота. Проверить, что trace после reattach объясняет те же события,
которые пользователь видел live. Для новых domain events сразу определить
causality и безопасный payload; не достраивать их догадками renderer.

**P1:** topology модулей и инструментов, inspection отдельно от mutation,
bounded diagnostics, экспорт с redaction и fixtures для повторяемых ошибок.
Широкий debugger нужен после стабилизации основного пути. Стоимость Chrome
DevTools frontend, cross-realm object tables и JS-specific debugging нельзя
считать обязательной ценой качественного trace в Rust/React продукте.
