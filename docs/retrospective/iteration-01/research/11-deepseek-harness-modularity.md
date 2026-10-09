# DeepSeek Harness: модульность, владение расширениями и композиция

**Для второй итерации наиболее полезны три механизма DeepSeek Harness: каждое
расширение владеет своими ресурсами; сессия удерживает определённую ревизию
композиции; UI собирается из типизированных contributions с управляемым
отключением.** Это гораздо ближе к желаемой программируемости Uprava, чем
registry, который лишь перечисляет доступные плагины. Переносить следует эти
контракты в Rust/React и распределённую модель Core/Node.

Исследованный snapshot: `deepseek-ai/deepseek-harness`, ветка `master`, commit
[`5badb15009ae1756c3afe0ae0cef1faafc290ccc`](https://github.com/deepseek-ai/deepseek-harness/tree/5badb15009ae1756c3afe0ae0cef1faafc290ccc),
package version `0.2.1-alpha.1`; исходники и выбранные тесты прочитаны
7 октября 2026 года. Ниже описана реализация этого snapshot. Тестовые assertions
использованы как свидетельство контрактов; upstream не запускался.

## 1. Контекст означает владельца, зависимости и область видимости

Основная единица исполнения — `Fiber`, создаваемый `ctx.plugin()`. Он хранит
контекст, конфигурацию, injected services, состояние и выполняющийся переход
`inertia`. Дочерний fiber сразу становится effect родителя: освобождение
родителя включает освобождение ребёнка. Публикация `internal/plugin` происходит
после назначения disposer, чтобы синхронный observer не создал orphan при
отключении во время активации. См.
[конструктор Fiber](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/cordis/src/fiber.ts#L222-L319).

Зависимости продолжают влиять на работающий плагин. `_refresh()` вычисляет
epoch из идентичностей fibers, предоставивших required services. Исчезновение
сервиса переводит consumer в unload/pending; новая реализация меняет epoch и
вызывает повторную активацию. `_reload()` проверяет, что epoch всё ещё актуален
после асинхронной границы, и не исполняет устаревшую активацию. Это сильнее
однократной проверки зависимостей при startup. См.
[dependency transitions](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/cordis/src/fiber.ts#L597-L695).

`ctx.effect()` связывает mutation и cleanup. Внутри составного effect
disposers выполняются в обратном порядке, последовательно с ожиданием async
cleanup; повторный вызов присоединяется к уже начатому освобождению.
Операция setup также остаётся видна владельцу при reentrant teardown. При этом
`Fiber._unload()` запускает верхнеуровневые effects через `Promise.all`:
зависимый порядок освобождения нужно задавать составным effect явно. См.
[effect implementation](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/cordis/src/fiber.ts#L418-L560)
и [fiber unload](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/cordis/src/fiber.ts#L675-L695).

Для Uprava это означает единый `ExtensionInstance`/`RegistrationOwner`,
которому принадлежат subscriptions, commands, tool registrations, UI slots,
таймеры и дочерние процессы. Отключение должно иметь ожидаемый результат:
снять допуск новых вызовов, отменить или дождаться работы и вернуть отчёт о
cleanup. Rust `Drop` помогает с синхронными handles, но не заменяет явный
async `deactivate()` для PTY, процессов, RPC и долговечных операций.

Здесь есть **две разные области видимости**. Loader `isolate` выбирает symbol,
под которым зарегистрирована реализация сервиса: `true` создаёт entry-local
realm, строка — общий именованный realm для provider и consumers. В одном
процессе могут существовать несколько реализаций одинакового API. См.
[realms и перенос service resolution](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/loader/src/config/isolate.ts#L25-L145).

DSH `ScopeKey` отдельно определяет наследование contributions и доставку
событий. `createScope()` создаёт дочерний fiber; `ScopedLayers.merge()` собирает
global→ancestors→own, ближайшее одноимённое contribution побеждает.
`scopeTarget()` допускает observers из текущего scope и его ancestors, сохраняя
filter исходного сервиса. Поэтому один preset может наблюдать дочерних agents,
а их registrations не смешиваются между siblings. См.
[scope lifecycle и event carrier](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/scope/src/index.ts#L105-L186)
и [layers и owned undo](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/scope/src/store.ts#L185-L265).

Uprava нужны отдельные идентификаторы instance, session, project, node и
composition revision. Локальные object identity/WeakMap удобны внутри runtime;
между Core и Node нужны сериализуемые references и версии. Service DI scope,
маршрутизация событий и security authorization должны иметь разные контракты.

## 2. Цепочка «конфигурация → preset → agent → cleanup»

При boot создаётся `Context`, монтируется Loader, затем корневой Include;
host дожидается дерева и проверяет startup failures. `mountRootInclude()`
добавляет builtins `cordis:include`/`cordis:group` и применяет подготовленные
profile patches до импорта. Patch имеет стабильный entry id; последовательные
layers могут настроить ранее добавленную строку. См.
[boot](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/boot/app-boot/src/index.ts#L973-L1019),
[root composition](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/boot/app-boot/src/index.ts#L539-L585)
и [patch semantics](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/vendor/include/src/index.ts#L43-L118).

`AgentPresetRegistry.register()` создаёт definition и eagerly активирует
его собственный scope. `mountPreset()` строит вложенное Loader tree,
проверяет import/activation failures и запрещает services, утёкшие из preset
в root realm. Failed composition сохраняется в roster с диагностикой;
здоровые presets остаются доступны. Это даёт оператору объяснение, почему
выбранный агент не может стартовать. См.
[definition activation](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/src/index.ts#L77-L151)
и [mount audit](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/src/mount.ts#L191-L215).

Agent через `mount()` удерживает `Generation`; `join()` связывает его scope
с ключом preset и увеличивает `users`. Удалённая/заменённая definition
помечает старую generation `retired`, но `collect()` освобождает её только
при `users === 0`. Дочерний agent через `composeFrom()` наследует **точную
ревизию родителя**, даже если уже опубликована новая definition.
См. [retain, bind и inheritance](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/src/index.ts#L232-L310).

Смена preset разрешена через `select()` до первого turn; выбор сериализован
на agent id и записывается в session event. Удержание revision применяется
также к чтению старого transcript через `acquireScope()`. Тесты явно проверяют
замену с живым ребёнком, отказ менять начатую сессию и in-flight cold read.
См. [selection и cold lease](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/src/index.ts#L329-L375)
и [revision assertions](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/tests/registry.spec.ts#L38-L113),
[cold read assertion](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/tests/registry.spec.ts#L149-L157).

Для Uprava стоит сохранять immutable composition manifest с digest, adapter
version, tools/schema revisions и разрешённым placement. Core выбирает
revision, Node подтверждает фактически активированный состав; session и child
runtime ссылаются на него. Обычное обновление preset не должно незаметно
менять активный агент. **Отзыв полномочий должен действовать независимо от
удержания revision:** DSH удаляет definition без немедленного отзыва живых
bindings, и эту семантику нельзя переносить на security revocation.

## 3. Цепочка «package → browser module → Chat contribution → replacement»

Host `ClientModuleRegistry` наблюдает Loader fibers, разрешает metadata
относительно конкретного entry и ищет package `dsh.client` плюс export
`./client`. Он собирает versioned boot graph и доставляет advertised bundles.
Пакетная dependency graph и runtime service `inject` описывают разные этапы:
сначала доступность module factory, затем активацию plugin с сервисами. См.
[host scan](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/src/index.ts#L596-L655)
и [metadata resolution](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/src/index.ts#L823-L857).

Chat сам является client plugin. Его `apply()` предоставляет session hooks,
регистрирует renderers и в `conversation.view` добавляет contribution `chat`,
которое объявляет собственные child slots для nodes и images. К компонентам
передаются данные/actions и observable hooks. Slot contract явно различает
`single`, `list`, `keyed`, `chain` и `root`/session scope. Это позволяет
расширять чат без центрального switch по plugin ids. См.
[Chat registrations](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/ui-chat/src/client/apply.ts#L111-L188)
и [slot contract](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/ui-slots/src/index.ts#L100-L178).

Browser `ClientEntries` сериализует sync/reload в одной queue. Changed desired
graph увеличивает generation; после await проверяется актуальность операции.
Replacement делает prefetch до teardown, дожидается старого fiber, удаляет
owned CSS, импортирует новую ревизию и ждёт активации. Ошибка отдельного пакета
публикуется отдельно и не теряет ownership entry для retry. См.
[queued reconciliation](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/src/client/entries.ts#L150-L245)
и [teardown/CSS](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/src/client/entry-lifecycle.ts#L4-L27).
Тесты проверяют re-enable после async cleanup и запрещают завершить
устаревшую замену после download/teardown:
[assertions](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/tests/entries.client.spec.ts#L84-L129),
[stale replacement](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/modules/tests/entries.client.spec.ts#L265-L285).

Для первого Uprava slice достаточно bundled slot API для chat item, action,
inspector panel и artifact renderer, с owner id, deterministic ordering,
fallback и disposer. Server/Node/UI части одного расширения следует описать
раздельно в manifest. Code replacement должен различать desired revision,
downloaded artifact и active instance. Возможность rollback после неудачной
активации нужно проектировать явно: прочитанный порядок teardown→import сам
по себе её не гарантирует.

## 4. Tools и динамический код: где проходит граница

`ToolRuntime.register()` сохраняет definition в принадлежащем context layer.
`restrict()` фильтрует inherited tools, а собственные registrations остаются
видимы — например, child's reporting tool. Следовательно, фильтр доступных
schemas нельзя считать полной политикой полномочий. После расширяемого
`tools/pre-execute` runtime вызывает monotonic guards: любой применимый guard
может запретить вызов, последующий plugin не может принудительно разрешить его.
См. [registration/restriction/guards](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/tools/src/index.ts#L1057-L1153),
[effective view](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/tools/src/index.ts#L1178-L1218)
и [pipeline](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/tools/src/index.ts#L1493-L1537).
Это проверяется assertion, где поздний prepended listener возвращает allow,
но guard блокирует body:
[guard assertion](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/core/tools/tests/scoped.spec.ts#L284-L333).

Agent-defined extension имеет stable `pluginId`, immutable `packageId` и
отдельный `pluginRunId`. `define()` не исполняет код. `run()` проверяет
владельца и план активации, для client-bearing package создаёт approval;
host-only path активируется напрямую. Update сначала отзывает прежний run.
Host half получает ограниченный context facade; tool lookup выдаёт schemas,
не callable `execute`, чтобы не обходить tool pipeline. Remote invocation
проверяет точный `pluginRunId` и отвергает stale run; retract снимает handlers
и дожидается fiber disposal. См.
[define/run](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/index.ts#L154-L299),
[facade](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/guard.ts#L639-L779),
[activation](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/index.ts#L832-L924),
[invoke fencing](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/index.ts#L749-L773)
и [retract](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/index.ts#L1225-L1245).

Эта версия хранит dynamic registry в process-local Maps; `node:vm` timeout
ограничивает только синхронную evaluation. Facade разрешает optional `ctx.get()`
и вызовы exposed services; это требует доверия к данным API. Эти свойства
не доказывают tenant isolation или восстановление после crash. См.
[registry storage](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/registry.ts#L140-L147)
и [VM evaluation bound](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/extensions/cordis-host-runner/src/sandbox.ts#L215-L237).

## 5. Что переносить и в каком порядке

| Решение | Действие для Uprava | Цена или граница |
| --- | --- | --- |
| Resource ownership и ожидаемый teardown | **Взять, P0:** owner для каждой регистрации и async lifecycle | Процессы/сеть требуют drain, deadlines и observable failure |
| Immutable composition generation | **Адаптировать, P0:** durable manifest/digest, session pin, Node activation receipt | Local refcount недостаточен для offline ноды; revocation действует отдельно |
| DI realm и contribution scope | **Адаптировать, P0:** typed services плюс явные project/session/instance refs | Не давать extension произвольный доступ к Core state |
| Small typed UI slots | **Взять, P0:** несколько chat/trace/tool inspector contributions | Каждый slot требует conflict/fallback contract; богатый slot DSL рано |
| Monotonic execution guards | **Взять, P0:** Core policy и Node boundary независимо от schemas/UI | Guard не должен быть снимаемым недоверенным plugin effect |
| Revision-aware replacement | **Адаптировать, P1:** serialized transitions, stale fencing и cleanup report | Rollback и partial Core/Node/UI activation требуют собственного протокола |
| Agent-defined host/client code | **Отложить:** сохранить identities и inspection pattern | Нужны изолированное исполнение, durable approval и ограниченные RPC capabilities |
| YAML `!!js`, произвольный in-process service доступ | **Не копировать автоматически** | Для удалённых manifests предпочтительны validated data и ограниченные conditions |

P0 следует завершить одним доказуемым вертикальным сценарием: выбранный
composition запускается на одной удалённой Node, добавляет инструмент и его
chat card, отражается в inspector, затем отключается с понятным результатом.
Контракт должен описывать missing dependency, отказ policy и cleanup.
Это уточнение порядка будущей реализации; данный ресерч не добавляет
практических экспериментов в текущую работу.

P1 — удержание старых revisions для живых сессий, child inheritance,
управляемое обновление и совместимость independent packages. После этого
можно решать, нужен ли отдельный extension host и какой scripting API
полезен пользователю. DeepSeek Harness показывает рабочие швы композиции;
распределённые leases, placement, credentials и отзыв доступа остаются
ответственностью архитектуры Uprava.
