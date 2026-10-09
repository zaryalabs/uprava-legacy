# ToolHive: управление внешними инструментами для второй итерации

Дата чтения исходников: **2026-10-03**. Проверен `stacklok/toolhive`,
commit [`3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c`][snapshot]
из `main`. Это отдельный исследовательский snapshot, а не версия, уже
интегрированная в Uprava. Исторический baseline первой итерации —
ToolHive `0.40.0`, commit `505df835ed73790bb9be7db7944ec772dc136a0e`,
зафиксированный в [контрактах tooling](../../../development/agent-tooling-contracts.md).
Описанные ниже возможности нового snapshot нельзя автоматически приписывать
этому baseline.

Метод: чтение исходников и выбранных тестов без запуска ToolHive, контейнеров
или upstream tests. Ссылки закреплены на commit и строки. Выводы для Uprava —
наши проектные рекомендации; наличие ветки в коде не подтверждает её
эксплуатационную надёжность в нашей конфигурации.

## Главный вывод и границы ответственности

ToolHive полезен как исполнитель и защитный proxy для MCP. Он не заменяет
интеграцию с agent runtime: не владеет диалогом, очередью сообщений пользователя,
жизненным циклом provider turn, согласованиями Codex или адресацией агентских
процессов на наших нодах. Вторая итерация должна сохранить цепочку
`Core policy → Node tooling adapter → ToolHive → MCP server`, а
`Node provider adapter → agent process` оставить отдельной ответственностью.

В коде хорошо разделены каталог, конфигурация запуска, процесс-supervisor,
транспорт и middleware. Именно эти границы стоит перенести. Каталог сам по себе
не является enforcement: стандартный `PolicyGate` разрешает создание,
авторизация добавляется только при заданном `AuthzConfig`, а без OIDC proxy
назначает запросу синтетического local-user без проверки credentials.
Для облачного профиля нам нужен валидатор конфигурации, отказывающий запуску
при отсутствии обязательных identity/policy механизмов.
[Policy gate][gate], [сборка middleware][middleware],
[неаутентифицированный fallback][auth-fallback].

## Цепочка 1. От записи каталога до наблюдаемого инструмента

`WorkloadService.CreateWorkloadFromRequest` строит `RunConfig`, вызывает
ранний policy check, сохраняет конфигурацию, затем запускает detached worker.
В `BuildFullRunConfig` запись registry разрешается в metadata и defaults;
прямой URL имеет отдельный путь. Следовательно, «есть в registry»,
«разрешён к запуску» и «работает» — три разных факта.
[Create и сохранение][create], [разрешение registry][registry-resolution].

Дальше `Runner.Run` проверяет параметры, разрешает secrets, собирает
middleware и снова вызывает gate непосредственно перед созданием runtime.
Если `RemoteURL` отсутствует, он вызывает `runtime.Setup` с image,
аргументами, environment, permission profile и сетевыми настройками.
При наличии `RemoteURL` создаётся proxy к существующему MCP endpoint,
добавляются исходящие credentials и health/auth callbacks.
Это единая оболочка управления при разных владельцах исполнения.
[Runner: создание runtime и remote path][runtime].

`DefaultManager.RunWorkload` содержит ограниченный restart loop с backoff,
а статусное хранилище сопоставляет сохранённые записи с состоянием runtime;
запись status file атомарная. Но это управление workload конкретного host,
не распределённая транзакция между нашим Core и Node.
[Supervisor][supervisor], [status persistence][status].

Что взять: Core хранит `desiredGeneration`, pin образа/endpoint, разрешённые
tools и ссылку на policy; Node возвращает `observedGeneration`, реальное
состояние, timestamp и диагностический код. Доступность вычисляется по обоим
поколениям. Кнопка «Включить» означает принятое desired state, а не успешный
запуск. Повтор команды после обрыва должен сходиться к тому же instance,
не создавать второй контейнер. Эти generation/idempotency правила — наша
надстройка, не обещание прочитанного ToolHive manager.

Отдельный нюанс readiness: при OIDC `waitForInitializeSuccess` принимает
401/403 за готовность защищённого локального listener. Сам код оговаривает,
что такой probe не проверяет backend. В UI нужны отдельные сигналы
«proxy доступен», «авторизация готова», «backend отвечает» и «tools обнаружены».
[Readiness contract][readiness].

## Цепочка 2. От discovery до реального tools/call

`authz.Middleware` получает уже разобранный MCP request, классифицирует
метод и запрещает неизвестные методы. Для защищённых list-операций он требует
response filter: backend возвращает список, после чего descriptor каждого
tool проверяется через право `call`. Отсутствие фильтра для защищённого списка
заканчивается отказом до dispatch.
[Классификация методов][authz], [обязательный response filter][list-auth],
[проверка каждого descriptor][tool-filter].

При настоящем `tools/call` право проверяется повторно, теперь с аргументами.
Это принципиальное различие: список — помощь агенту выбрать инструмент,
а вызов — новая попытка действия. Более того, discovery вызывает policy
с `arguments=nil`; правило, разрешающее только определённые аргументы, может
скрыть инструмент из списка. В Uprava следует явно разделить «может обнаружить»
и «может выполнить с этими аргументами», сохраняя обязательную проверку на
execution boundary.
[Повторная авторизация][call-auth], [nil arguments при list][tool-filter].

Для progressive discovery особенно полезен `handleToolsCall`: meta-tool
`call_tool` раскрывается до внутреннего имени и аргументов тем же decoder,
который использует dispatch. Это предотвращает ситуацию, когда policy
разрешила оболочку, а исполнился другой tool. Регрессионный тест также описывает
отказ для duplicate keys, разных регистров `name/Name`, `method/Method` и
вложенных аргументов. Тест прочитан, не запускался.
[Meta-tool admission][meta-tools], [case-smuggling test][case-test].

Uprava уже имеет progressive interface. Во второй итерации его разрешённый
результат должен быть конкретным: `definitionId + schemaHash +
backendInstanceId + policyRevision + normalizedArguments`. Approval
привязывается к этому результату; после смены schema, backend или policy
старое подтверждение не переносится автоматически. Один нормализованный
объект проходит validation, approval, audit и dispatch.

Cedar получает identity из request context, отдельно нормализует claims и
arguments, затем строит решение для конкретной операции. Tool annotations
берутся из server-side discovery, не из аргументов клиента.
[Построение policy input][cedar], [источник annotations][annotations].
Однако upstream `readOnlyHint` остаётся утверждением MCP server. Для
недоверенной интеграции оно не заменяет классификацию риска, утверждённую
оператором. Policy engine и registry не изолируют произвольный shell или file
tool, встроенный в сам агент: для них остаются provider policy и sandbox Node.

## Цепочка 3. Remote MCP, credentials и владение сессией

Входящий caller и credential удалённого сервиса разделены. Remote handler
выбирает bearer token либо восстанавливает OAuth и выполняет новый flow;
runner передаёт `TokenSource` транспорту. В конфигурациях с embedded
auth server предусмотрен явный режим удаления `Authorization`, `Cookie`
и `Proxy-Authorization`; несовместимые способы повторной инъекции credentials
проверяются при сборке. Это полезный образец явного security contract,
но не универсальное утверждение, что любой режим ToolHive всегда удаляет
входящий токен.
[Remote auth][remote-auth], [credential stripping][strip].

`sessionbinding` связывает protocol session с проверенными
`issuer + subject`, а не со строкой bearer token: refresh не меняет владельца.
Отсутствующий, чужой и некорректный owner скрываются одинаково; проверка
предшествует восстановлению backend session. TestSessionOwnershipHTTP
описывает GET/POST/DELETE от другого subject/issuer и проверяет, что backend
не вызван. Это полезный шаблон контрактного теста для Node transport.
[Binding][binding], [ownership test][owner-test].

Наша граница уже строже простого subject: Core lease связан также с
session/project/placement/Node и revocation state.
[Текущая реализация lease](../../../../crates/uprava-server/src/runtime/application/tooling.rs).
Её стоит сохранить, добавив явную revision подключения. Общий технический
upstream token нельзя трактовать как пользователя Uprava. Для shared gateway
identity пользователя должна доходить до enforcement по доверенному каналу;
альтернатива — отдельный instance/credential на выбранную область изоляции.

Секреты вынесены в provider, runtime state хранит references.
`persistRefreshToken` сначала сохраняет secret и новую конфигурацию, затем
меняет живую конфигурацию. Тест проверяет отсутствие plaintext client secret и
bearer token в сохранённом state, а также сохранение прежнего live config при
ошибке записи. Scope-prefix в secret provider разделяет внутренние категории,
но не образует tenant authorization; присутствует и migration fallback к
старому имени.
[Сохранение refresh token][token-persistence],
[тест инвариантов][token-test], [secret namespaces][secret-scopes].

`MonitoredTokenSource` объединяет конкурентные refresh через singleflight,
различает временный сетевой сбой и постоянный отказ, а после исчерпания коротких
retry переводит credential в `AuthRetrying` с быстрым отказом новых вызовов.
Для нашей панели это лучше единого «инструмент недоступен»: пользователь должен
видеть «восстанавливаем связь» либо «нужна повторная авторизация».
Численные таймауты ToolHive не переносим без своего UX-бюджета.
[Refresh state machine][token-monitor].

## Где гарантии заканчиваются

Владение protocol session не равно восстановлению агентского диалога. Даже
Redis-backed metadata в коде прямо отделены от доставки live stream между
репликами. Наш replay журнала, cursor и состояние approval остаются в Core.
[Owner storage][owner-storage].

Transparent proxy умеет восстанавливать backend session после dial error
или 404, повторяя сохранённый initialize и исходный запрос.
Это транспортное восстановление, не гарантия exactly-once side effect.
Вторая итерация должна иметь классификацию retry: безопасный повтор,
проверка результата по operation id либо `outcome_unknown`. Потеря ответа
после mutation не должна автоматически превращаться в новый вызов.
[Условия recovery][recovery-trigger], [reinitializeAndReplay][recovery].

Stop remote workload останавливает локальный proxy и очищает клиентскую
конфигурацию; это не доказательство отмены уже принятой операции на удалённом
сервисе. Update реализован как stop/delete/save/start, поэтому его нельзя
представлять как бесшовную смену policy для текущего вызова.
[Remote stop][stop], [Update][update].
Core должен отдельно фиксировать revoke доступа, запрос cancellation,
подтверждённое завершение и неизвестный исход. Policy revocation для новых
вызовов должна срабатывать до ожидания cleanup на Node.

Поддержка протокольных ревизий также неоднородна: в таблице авторизации
есть `server/discover`, но `tasks/*`, sampling и elicitation на этом пути
намеренно запрещены до появления соответствующих правил. Из этого нельзя
делать вывод о равной поддержке всех методов и всех transport/vMCP путей.
Нужна узкая compatibility matrix для выбранного provider, MCP revision,
transport и разрешённых методов. [Таблица методов][authz].

## Трассировка без утечки содержимого

Порядок middleware полезен сам по себе: audit оборачивает authentication и
authorization, поэтому может учитывать отказ до исполнения; mutating webhooks
расположены перед окончательным authz, а telemetry получает окончательно
разобранный request. Такой порядок следует закрепить тестами, а не надеждой
на порядок регистрации плагинов. [Сборка цепочки][middleware].

Не стоит копировать `sanitizeArguments` как достаточную защиту: он проверяет
имя верхнеуровневого ключа, а остальные значения форматирует в строку и обрезает.
Вложенный secret под нейтральным ключом или credential внутри shell command
этим не гарантированно скрыт; аргументы затем добавляются в span.
[Запись аргументов][trace-arguments], [Sanitizer][sanitizer].
Наш default — структурные поля: actor/session/call id, tool/version,
policy decision, duration, outcome. Payload включается только по явному
правилу с рекурсивной/schema-aware обработкой и отдельным доступом.
Это сохраняет полезный «трейсинг решений», не превращая trace в копию секретов.

## Что переносить и в каком порядке

Оценка стоимости относительная: низкая — локальная граница/контракт,
средняя — согласованные Core/Node/provider изменения, высокая —
эксплуатация дополнительного распределённого сервиса.

| Решение | Выбор для Uprava | Очередь и стоимость |
|---|---|---|
| Каталог отдельно от desired/observed runtime | Взять; добавить generation и timestamps | P0, средняя |
| List-фильтрация и повторный admission call | Взять; discovery не выдаёт полномочие | P0, средняя |
| Общий decoder для meta-tool и dispatch | Взять для progressive tooling | P0, низкая |
| Identity отдельно от outgoing credential | Взять, сохранить session/placement lease | P0, средняя |
| Обязательный cloud profile | Адаптировать; отказ при local-user fallback и отсутствии policy | P0, средняя |
| AuthRetrying и отдельный unauthenticated | Взять состояния, подобрать свои deadline | P1, средняя |
| Автоматический replay всех исходных запросов | Не копировать без контракта side effects | P0, средняя |
| Весь vMCP/operator/Redis стек | Отложить до доказанной потребности в нескольких proxy replicas | P1+, высокая |

Конкретный порядок следующего среза:

1. **P0:** описать один production trust profile: кто запускает агент,
   где находится MCP proxy, кто держит secrets, кто авторизует native и MCP tools.
2. **P0:** реализовать один backend adapter с pin конфигурации,
   desired/observed generation и явными ошибками readiness/auth/schema.
3. **P0:** объединить normalisation, scope check, approval и dispatch
   конкретного вызова; запретить неоднозначные JSON-представления.
4. **P0:** сохранить leases и немедленную ревокацию в Core; связать их с
   поколением подключения. Cleanup downstream отслеживать отдельно.
5. **P0:** определить cancellation/retry/outcome-unknown для mutation tools
   и не обещать отмену по одному закрытию socket.
6. **P0:** сделать metadata-only trace стандартом и отдельно проверить
   отсутствие credentials в событиях, state и диагностике.
7. **P1:** добавить управляемый OAuth refresh и понятные пользователю
   промежуточные auth-состояния.
8. **P1:** расширять transport/protocol matrix и число интеграций после
   завершения одного удобного сквозного сценария.

В первой итерации уже появились многие нужные контракты: session leases,
desired/actual dependency state, bounded bridge и запрет direct fallback.
ToolHive source review подтверждает ценность этого разделения. Основное
изменение второй итерации — сузить интеграцию и довести эти контракты до
прозрачного поведения для пользователя, а не переносить весь текущий
upstream toolkit как новую обязательную платформу.

[snapshot]: https://github.com/stacklok/toolhive/commit/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c
[gate]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/policy_gate.go#L12-L38
[middleware]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/middleware.go#L67-L285
[auth-fallback]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/auth/utils.go#L62-L94
[create]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/api/v1/workload_service.go#L79-L107
[registry-resolution]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/api/v1/workload_service.go#L193-L205
[runtime]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/runner.go#L460-L610
[supervisor]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/workloads/manager.go#L534-L638
[status]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/workloads/statuses/file_status.go#L793-L820
[readiness]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/runner.go#L1082-L1108
[authz]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/middleware.go#L34-L114
[tool-filter]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/tool_filter.go#L35-L95
[list-auth]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/middleware.go#L279-L319
[call-auth]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/middleware.go#L434-L455
[meta-tools]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/middleware.go#L468-L525
[case-test]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/case_smuggling_test.go#L30-L97
[cedar]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/authorizers/cedar/core.go#L1272-L1360
[annotations]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/authz/authorizers/annotations.go#L8-L34
[remote-auth]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/auth/remote/handler.go#L62-L119
[strip]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/middleware.go#L481-L553
[binding]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/auth/sessionbinding/binding.go#L33-L75
[owner-test]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/transport/proxy/transparent/ownership_test.go#L24-L150
[token-persistence]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/runner.go#L914-L948
[token-test]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/runner/persist_refresh_token_test.go#L24-L116
[secret-scopes]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/secrets/scoped.go#L33-L108
[token-monitor]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/auth/monitored_token_source.go#L365-L440
[owner-storage]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/transport/session/ownership.go#L8-L21
[recovery-trigger]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/transport/proxy/transparent/transparent_proxy.go#L682-L746
[recovery]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/transport/proxy/transparent/transparent_proxy.go#L956-L1077
[stop]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/workloads/manager.go#L454-L502
[update]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/workloads/manager.go#L1246-L1295
[trace-arguments]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/telemetry/middleware.go#L449-L474
[sanitizer]: https://github.com/stacklok/toolhive/blob/3ca3c8f71152ad6e109dbb881be13ceb17a2cf3c/pkg/telemetry/middleware.go#L570-L617
