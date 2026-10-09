# DeepSeek Harness для следующей итерации Uprava

DeepSeek Harness стоит сделать основным референсом для **композиции
возможностей, расширяемого чата и developer tools**. T3 Code остаётся
референсом подключения provider runtime, OpenCode — отдельных контрактов
синхронизации и tools, ToolHive — исполнения внешних MCP. Их роли дополняют
друг друга.

Ценность Harness для нашего замысла — возможность менять устройство агента
и интерфейса через небольшие сервисные контракты, видеть получившуюся
композицию и прослеживать выполнение по событиям. Это более конкретное
основание для переосмысления модульности, чем внешний ориентир «как Vim».

Срез исходников — [`5badb15009ae1756c3afe0ae0cef1faafc290ccc`](https://github.com/deepseek-ai/deepseek-harness/commit/5badb15009ae1756c3afe0ae0cef1faafc290ccc),
root package `0.2.1-alpha.1`, commit от 3 октября; дата дополнения исследования
— **7 октября 2026 года**. Механизмы разобраны в
[модульности Harness](11-deepseek-harness-modularity.md) и
[developer tools и трассировке](12-deepseek-harness-developer-tools.md).
Это source review без практических запусков.

## Как меняется выбор референсов

| Направление Uprava | Главный референс | Что брать |
| --- | --- | --- |
| Композиция работающей системы | DeepSeek Harness и Cordis | Сервисы, зависимости, scopes, владение регистрациями и ресурсами |
| Расширение чата и инструментов | DeepSeek Harness | Семантические узлы диалога, tool contracts, per-agent presets и отдельные renderers |
| Наблюдаемость runtime | DeepSeek Harness | Общая описываемая модель для activation, inspector и диагностики |
| Причинность выполнения | DeepSeek Harness + durable-контракты Uprava | Turn/step/request/tool identities, ссылки на исходные события, сохранённые версии |
| Интеграция внешних агентов | T3 Code | Driver/instance/session/turn, provider-specific approvals и recovery |
| Синхронизация и tool bindings | OpenCode | Durable admission, watermark результата, защита от устаревшей регистрации |
| Внешние MCP workloads | ToolHive | Lifecycle, caller/upstream identity, admission конкретного вызова |

## Модуль должен описывать своё поведение

Из Harness полезно взять контракт, по которому модуль объявляет предоставляемые
сервисы, обязательные зависимости, регистрации tools/commands/views и
жизненный цикл. Тогда система может объяснить, почему модуль активен, ожидает
зависимость, завершился с ошибкой или отключается. Inspector получает сведения
из того же источника, который использует activation.

В `0.2.26` Uprava уже имеет declarative
[PluginManifest](../../../../crates/uprava-protocol/src/plugins.rs),
Core-owned contributions и
[Web Extension Host](../../../../apps/web/src/plugins/ExtensionHost.tsx).
Но реализация Web выбирается из явных bundled maps. Новая итерация должна
добавить к этой модели сервисные зависимости, принадлежность ресурсов и
объяснимый lifecycle. Само увеличение числа manifest entries этого не даст.

Предлагаемая переносимая единица — модуль с такими полями:

| Поле | Назначение |
| --- | --- |
| Identity и version | Однозначно определить поставляемую реализацию |
| Execution target | Core, Node, client или отдельный extension host |
| Provides и requires | Связать потребителей с реализациями сервисов |
| Contributions | Зарегистрировать команды, tools, представления и обработчики объектов |
| Scope и owner generation | Определить видимость и владельца ресурсов |
| Lifecycle state | Показывать activation, отсутствие зависимости, ошибку и завершение |
| Required grants | Проверять доступ независимо от наличия зависимости |
| Compatibility и fallback | Объяснять несовместимость и сохранять доступ к данным |

Это предложение для Rust/React, не перенос API Cordis буквально. Для первых
bundled modules достаточно типизированных интерфейсов и явного composition
root. Независимая загрузка стороннего кода и произвольная runtime replacement
могут появиться позже без отказа от этих контрактов.

## Владение ресурсами и замена модуля

Регистрация tool, command, обработчика события или renderer должна принадлежать
конкретному lifecycle owner. Отключение владельца прекращает новые обращения,
снимает подписки и ждёт определённого завершения cleanup. Повторный cleanup
не должен удалять регистрацию уже активированной замены.

Cordis связывает ресурсы с effects, а замену предоставляемого сервиса — с
пересозданием зависимых plugin instances. Agent presets Harness дополнительно
удерживают используемую revision до освобождения пользователей. Это полезные
механизмы для нашей проблемы «начали следующее, не довели предыдущее»: система
делает отсутствие зависимости и неполный teardown видимыми.

Для распределённой Uprava нужны два уровня. Локальный disposer завершает
ресурсы своего процесса. Core отдельно хранит desired revision; Node сообщает
observed revision, attempt и завершение cleanup. Потерю Node нельзя считать
успешным dispose. Старое поколение не должно подтверждать готовность новой
реализации или отзывать её доступ.

Замена renderer может выполняться независимо от работающего provider.
Замена provider adapter или credential во время turn требует собственного
протокола. Поэтому «hot reload» стоит разложить по типам ресурсов и допустимым
точкам остановки: новый turn, завершённый вызов, новый runtime attempt.

Scope композиции и security scope также различаются. Видимость сервиса в
дереве зависимостей помогает собрать правильную систему; право читать файлы,
вызвать tool или обратиться к другой Node проверяется на доверенной границе
исполнения. Нельзя превращать configurable hook или preset в возможность
отключить обязательную production policy.

## Чат как расширяемое представление событий

Из Harness стоит взять расширение через типы данных и семантические узлы
диалога. Tool output не обязан быть строкой внутри assistant bubble: у него
могут быть типизированный результат, представление для модели, ссылка на
артефакт и отдельный renderer. Полезность этого разделения видна и для будущих
computer/browser/perception возможностей.

Для Uprava предложение такое: Core хранит факт и устойчивые refs; клиентский
модуль строит представление разрешённых данных. Одна команда над ref доступна
из Chat node, palette и Inspector через общий application contract.
Отключение красивого представления оставляет базовый текст, metadata и ссылку
на исходный артефакт. UI-module не получает полномочий исполнять произвольный
код на Node из-за регистрации renderer.

Так основной чат остаётся маленьким, а новые типы результата добавляются без
специального условия в каждом transport и без копирования бизнес-логики
между кнопкой, agent tool и Inspector.

## Developer tools как часть первого продукта

Первой итерации недоставало ясности, почему широкий функционал не приносит
ожидаемой пользы. Из Harness стоит перенести возможность исследовать систему
на двух связанных уровнях:

- **Composition inspector:** какие модули и сервисы активны, где исполняются,
  почему зависимость отсутствует, какую revision действительно использует Node.
- **Session inspector:** какой input принят, какой attempt и turn работают,
  какие взаимодействия ожидают пользователя, к каким событиям относятся
  tool call, результат и артефакт.

Эти представления должны опираться на обычные registry и projection API.
Отдельный дублирующий «реестр для devtools» быстро разойдётся с runtime.
Первый интерфейс может быть read-only и небольшим; диагностируемость
является свойством контрактов с первого среза.

Полный browser графа runtime можно добавить позже. Для первого среза
Composition Inspector означает небольшой список реальных instances,
revisions и причин неактивности; он не требует живых object handles.

NodeJS Inspector Harness имеет более широкие полномочия: это живой debugger
с CDP и выполнением кода. Для Uprava такую функцию следует размещать в
отдельном привилегированном developer profile. Пользовательскому Inspector
достаточно управляемых команд и разрешённых проекций. Диагностическая очередь
наблюдений debugger не заменяет durable журнал Core.

## Что именно должен сохранять trace

Самый полезный перенос из Harness — trace с идентичностью и происхождением
данных. Session log различает turn, step, request, входное сообщение,
tool call и tool result; request связан с использованной конфигурацией и
набором tools. UI может перейти от видимого узла к исходным событиям.

Предлагаемая минимальная запись Uprava включает:

1. Actor, source input, command, session, Node и runtime attempt.
2. Parent/cause event refs и sequence, связывающие этапы выполнения.
3. Provider/version, tool binding/schema и policy decision.
4. Approval/input request, его scope и принятое решение.
5. Результат, ошибку или неизвестный исход; ссылки на сохранённые артефакты.
6. Доступную часть request/context с явно обозначенной полнотой.

Последний пункт особенно важен: Harness владеет собственным LLM loop и может
записать собранный запрос. Uprava часто управляет сторонним provider runtime;
adapter может не видеть полного system prompt или всех внутренних tools.
В этом случае trace показывает наблюдаемую часть и отсутствие конкретных
данных. Нормализованный timeline не становится полным prompt audit только
из-за наличия красивого Inspector.

Записанная конфигурация и source refs объясняют наблюдаемую цепочку действий.
Они не дают основания реконструировать скрытые рассуждения модели и выдавать
их за записанный факт. Payload с credentials или личными данными проходит
redaction и отдельные правила доступа до публикации в diagnostics.

## Удалённое исполнение и будущие возможности восприятия

У Harness есть seams для filesystem, subprocess, sandbox и subagent providers,
в том числе SSH-реализации. Это подтверждает ценность контрактов исполнения:
одна capability может иметь локальный или удалённый backend. Однако удалённый
filesystem/subprocess не равнозначен распределённому владению агентской
session, доставке команд и восстановлению Core/Node после разрыва связи.
[SSH filesystem](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/ssh/fs-ssh/src/index.ts#L19-L37),
[helper services](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/ssh/ssh/src/helper.ts#L27-L34).

Наличие native agents также подтверждено кодом: Codex subagent запускает
официальный `app-server --stdio` и создаёт ephemeral thread, Claude использует
Agent SDK. В этом пути Codex объявляет `NO_START_CAPABILITIES` и не наследует
полный parent context. Стоит взять правило публикации handle только после
provider readiness и rollback неуспешного startup. Постоянную native session
с reattach, steer и approvals нужно описывать отдельным adapter contract.
[Codex provider](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/subagent/subagent-codex/src/index.ts#L63-L108),
[ephemeral thread](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/subagent/subagent-codex/src/wire.ts#L283-L301),
[ready и rollback](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/subagent/subagent-codex/src/run.ts#L315-L353),
[Claude SDK](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/subagent/subagent-claude-code/src/run.ts#L1-L17).

SSH transport Harness намеренно не переподключается и не воспроизводит
неподтверждённые запросы. При потере связи исход mutation может остаться
неизвестным. Полезный перенос — такое состояние ошибки, а в Uprava его
дополняют durable receipts и reconciliation. У SSH protocol отдельно
зарезервирована capacity для heartbeat, close и process termination:
тяжёлый tool call не должен лишать оператора пути остановки и cleanup.
[Request semantics](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/ssh/ssh/src/protocol.ts#L57-L131),
[management classes](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/ssh/ssh/src/protocol.ts#L17-L30),
[contract test](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/ssh/ssh/tests/management-capacity.spec.ts#L103-L118).

Host/client admission Harness использует один `OperatorPeer` на lifetime
Host. В Uprava Core identity и session/Node scope сохраняются для каждого
действия; доступ к API сам по себе не определяет право на workspace.
Объединять local operator и права удалённого control plane в одну identity
нужно только при явно выбранном однопользовательском профиле.
[Operator admission](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/connection/src/rpc-host.ts#L63-L112).

Для Uprava такие seams реализуются через Node adapters. Core определяет
размещение и права, Node владеет workspace и фактическими ресурсами, provider
adapter — native protocol. Их надёжные команды и события сохраняются по
[контракту удалённых агентов](10-remote-agents-and-tools-for-v2.md).

Для будущего восприятия стоит предусмотреть расширяемые typed observations
и artifacts: источник, Node, время, media type и преобразования до передачи
модели. Renderer, perception backend и executor действия могут быть разными
модулями. Это оставляет место для новой функциональности, не включая её
реализацию в первый чат. Конкретный формат ObservationRef предстоит выбрать
под соответствующий пользовательский сценарий.

В browser runtime Harness ресурс принадлежит живому Agent, проверяется его
текущая registry identity, а операции одного владельца сериализуются.
В распределённой системе такой owner переводится в сериализуемую тройку
`node_id + runtime_attempt_id + resource_id`: resumed session не получает
автоматически старый browser/desktop handle. Для shared desktop дополнительно
нужен явный lease владения действиями.
[Resource ownership](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/experimental/browser-use-runtime/src/index.ts#L55-L95).

MCP adapter Harness отдельно хранит canonical result и строит model
projection; image admission проверяет поддерживаемую modality текущего model
route, сохраняет изображения в attachment store и отдаёт refs. Для Uprava
нужно сохранить это различие: исходное наблюдение, вид модели, представление
UI и trace могут иметь разные payload/access policies. В replay передаются
устойчивые artifact refs вместо многократно встроенного base64.
[Result projection](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/mcp/mcp-client/src/tools.ts#L225-L264),
[modality admission](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/mcp/mcp-client/src/tools.ts#L347-L372),
[durable images](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/mcp/mcp-client/src/tools.ts#L415-L438).

## Порядок переноса

| Приоритет | Решение | Условие и цена |
| --- | --- | --- |
| P0 | Описываемые модули и сервисные зависимости | Типизированные bundled implementations; observable activation без marketplace |
| P0 | Lifecycle owner, scopes и generation | Cleanup и замена проверяются локально; распределённое подтверждение отдельно |
| P0 | Общие refs и семантические узлы чата | Базовый renderer и один дополнительный тип результата |
| P0 | Read-only composition/session inspectors | Те же registry/projection APIs, которые использует runtime |
| P0 | Causal trace и версии tool bindings | Существующий Core log, явная полнота provider observations |
| P0 | Резерв управляющего transport budget | Heartbeat, interrupt и cleanup остаются доступны при тяжёлом tooling |
| P1 | Session presets и смена composition revision | Действующие сессии удерживают согласованную версию |
| P1 | Independently delivered extension host | Process/RPC boundary и capability grants для выбранной модели доверия |
| P1 | Live debugger и выполнение developer commands | Отдельный developer profile и явный scope |
| Позже | Perception backends и новые виды действий | Typed artifacts подготовлены, продуктовый сценарий выбирается отдельно |

Следующая спецификация модульности должна описать сервисы, lifecycle,
владельцев, permissions и introspection как один контракт. Подмена этого
контракта только plugin manifest или только набором React slots оставит
главный опыт Harness неиспользованным.

Один из вариантов дальнейшего развития — Node-supervised Harness как ещё
один agent provider. Его стоит оценивать отдельно от заимствования
архитектурных идей: понадобятся version-pinned adapter, capability mapping,
approval/input/recovery contracts и определённый владелец secrets. Текущее
исследование не меняет основной выбор Rust/React и первого provider.
