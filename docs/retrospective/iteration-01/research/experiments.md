# Проверки и воспроизводимые эксперименты

Этот журнал отличает выполненные проверки от заданий, необходимых для
окончательного выбора новой реализации. Дата — 3 октября 2026 года. Исследован
commit `9d096834eadfacd9f04a551c5557b33194e7ca4b`, baseline `0.2.26`; изменения
этой работы относятся к документации.

## Выполненные проверки

### Общий quality gate

Выполнен `make c` с разрешённым доступом к локальным caches. Результат —
**exit 2**, 70.13 секунды полного запуска. До остановки прошли format,
Markdown links, Rust clippy, Web lint/typecheck и protocol drift check
(40 Web-facing enums и canonical fixtures). Rust dev/check stage занял
36.22 секунды; это смешанный прогон с существующим cache, а не чистый
build benchmark.

Gate остановился в `rust-dl` на `cargo audit`. В `Cargo.lock` найден
`rustls 0.23.40`, затронутый
[RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285),
опубликованным 14 сентября 2026 года; исправленная ветка начинается с
`0.23.45`. Также выдан разрешённый warning для `event-listener 5.4.1`
([RUSTSEC-2026-0221](https://rustsec.org/advisories/RUSTSEC-2026-0221),
patched `5.4.2`). Практическая эксплуатируемость конкретного deployment этим
прогоном не оценивалась.

Часть запросов проверки yanked packages завершилась registry timeout;
полнота сетевой проверки не подтверждена. Это отдельное ограничение, оно не
отменяет найденный advisory. После `rust-dl` полный gate не дошёл до Web
production build и тестовой цели. Зависимости, audit policy и исходный код
в рамках документационной работы не менялись; успешный `make c` не заявляется.

### Тесты после остановки общего gate

Отдельно выполнен `make t`, чтобы audit failure не скрывал состояние тестов.
Результат: **exit 0**, 50.44 секунды полного запуска.

| Набор | Результат |
| --- | --- |
| Rust workspace | 384 passed, 0 failed, 0 ignored; Cargo test |
| Web unit/component | 43 test files, 175 tests passed; Vitest `4.1.9` |
| Generated UI builder | 4 tests passed |

В Rust прогоне прошёл, в частности,
`deterministic_fake_provider_keeps_one_live_thread_across_interactions`.
Это даёт новое evidence для двух turn и interactions с fake provider.
Оно не закрывает production deployment, реальный Codex/OAuth или performance
acceptance. Успешные тесты не отменяют неуспешный dependency audit.

### Браузерные сценарии

Выполнена существующая выборка E2E:

```sh
npm --prefix apps/web run e2e -- --grep 'enables, persists|loads Workbench|starts Managed'
```

Результат: **3 passed**, суммарно 11.8 секунды, Chromium, один worker.
Длительности ниже — время выполнения теста, не пользовательская latency.

| Сценарий | Результат | Длительность теста |
| --- | --- | --- |
| Bundled Dark Theme | Включение, сохранение после reload и безопасное отключение проходят | 1.9 с |
| Workbench | Отложенная загрузка Monaco/xterm и refit после изменения shell проходят | 4.1 с |
| Managed interaction | Явный start и восстановление blocked question после reload проходят | 3.4 с |

Источник — [golden.spec.ts](../../../../apps/web/e2e/golden.spec.ts) и
[Playwright config](../../../../apps/web/playwright.config.ts). Использован
`mockCoreApi`: реальные Core/Node, provider и OAuth не проверялись. Запуск
проводился на Vite dev server, не на production build. Snapshots не обновлялись,
пакеты и браузеры не устанавливались. Первая попытка остановилась на sandbox
`EPERM` при bind локального порта; разрешённый повтор с доступом к локальному
listener завершился успешно.

Вывод: базовые UI contracts имеют исполняемое evidence. Это не подтверждает
удобство повседневной работы, новые возможности extension host или бюджеты
отзывчивости.

### Первая попытка измерения Cargo

```sh
cargo check --locked --offline -p uprava-protocol --timings
```

Среда: cargo/rustc `1.93.1` Homebrew, rustc commit `01f6ddf75` от 2026-02-11.
Команда завершилась с exit `101` до compilation: sandbox запретил распаковку
cached `autocfg v1.5.1` в Cargo registry. Время 0.16 секунды описывает отказ
старта, **не время сборки**. Второго warm результата в этом прогоне нет.
`cargo clean` не запускался, существующие caches не удалялись.

Вывод: первоначальная попытка не дала численного ответа на жалобу о сборке.
Параметры полноценного замера заданы ниже; неисправность окружения не
приписывается Rust или архитектуре проекта.

### Повторный замер Cargo с разрешённым cache

После завершения `make t` та же команда успешно выполнена дважды с доступом к
cache. Среда: macOS `aarch64-apple-darwin`, dev profile, cargo/rustc `1.93.1`,
`jobs=12`, `ncpu=12`. Время запуска — 18:03 MSK 2026-10-03. Исходники между
прогонами не менялись. Наши тесты к этому моменту завершились, посторонняя
системная нагрузка не контролировалась.

| Прогон | Wall time | Cargo time | Fresh / dirty units | Результат |
| --- | --- | --- | --- | --- |
| После clippy/test cache | 5.75 с | 5.72 с | 48 / 1 | exit 0 |
| Повтор без изменений | 0.12 с | 0.11 с | 49 / 0 | exit 0 |

Единственная compilation unit первого прогона — `uprava-protocol`, 5.52 с.
Timings IDs: `20261003T150320.429794Z`, `20261003T150332.470725Z`.
Это подтверждение работы cache в узкой проверке, **не cold build**, не
сравнение оптимизаций и не профиль полного edit-build-test цикла.
Детали интерпретации — в [исследовании сборки](06-tooling-and-build-performance.md).

### Проверка документации

`make docs-l` подтвердил корректность локальных ссылок во всех 71 Markdown
файлах репозитория. `git diff --check` не выявил whitespace errors в tracked
изменениях. Три Mermaid-схемы нового пакета дополнительно разобраны parser
установленного `mermaid` с DOM окружением jsdom: синтаксис корректен.
Эта проверка синтаксиса не является визуальной проверкой каждого renderer.

## Как воспроизвести проверки контрактов

Канонический общий quality gate — `make c`. В Makefile нет фильтра отдельного
Rust-теста; для диагностического повторения можно использовать следующие
существующие тесты. Наличие команды здесь не означает её отдельный успешный
запуск в этой работе.

```sh
cargo test -p uprava-node --lib runtime::managed_provider::tests::deterministic_fake_provider_keeps_one_live_thread_across_interactions -- --exact
cargo test -p uprava-server --lib runtime::tests::session::create_session_without_profile_rejects_incapable_node_without_exec_fallback -- --exact
cargo test -p uprava-server --lib runtime::tests::runtime::expired_provider_interaction_rejects_late_input_without_command -- --exact
cargo test -p uprava-server --lib runtime::tests::http::hardened_web_auth_sets_session_and_enforces_csrf_for_mutations -- --exact
cargo test -p uprava-server --lib runtime::tests::tooling::leases_reject_rotation_expiry_revocation_and_foreign_scope -- --exact
cargo test -p uprava-node --lib runtime::tests::workspace::validate_workspace_command_rejects_symlink_escape_from_allowed_root -- --exact
cargo test -p uprava-node --lib runtime::tests::provider::provider_environment_keeps_only_allowlisted_inherited_values -- --exact
```

Fake-provider suite проверяет protocol/lifecycle границы. Реальная host-only
приёмка описана в [Managed runbook](../../../runbooks/managed-agent-runtime.md)
и требует авторизованного provider, доступной модели и отдельного окружения.
Её результат не подменяется fake-тестом.

## Оставшиеся эксперименты для проектирования

Дополнение от **2026-10-07**: source review DeepSeek Harness выполнен на
`5badb15009ae1756c3afe0ae0cef1faafc290ccc`, без запуска upstream runtime или
тестов. Проверены файлы и диапазоны 84 pinned ссылок в 61 исходном/тестовом
файле. `make docs-l` прошёл для 78 Markdown-файлов; `git diff --check` и
проверка newline, trailing whitespace и fenced blocks прошли для 25 документов
пакета. Повторный code gate для этого дополнения не запускался: изменена
только документация. Это проверка ссылок и структуры, не новое свидетельство
runtime/performance acceptance.

После первичного обзора владелец уточнил задачу: углубиться в исходники
существующих систем без практических запусков. Поэтому source review
[T3 Code](07-t3-remote-agent-implementation.md),
[OpenCode](08-opencode-agent-tooling-implementation.md) и
[ToolHive](09-toolhive-control-implementation.md) выполнен отдельно:
GitHub refs закреплены на SHA, исходники и тесты прочитаны, ссылки на файлы
и диапазоны строк сверены с source archives. Upstream runtime, контейнеры,
авторизация и тестовые suites не запускались. Это дополнение доказательной
базы, а не новый положительный результат экспериментов из таблицы ниже.

Проверка дополненного пакета: `make docs-l` прошёл, 75 Markdown-файлов.
В 22 документах ретроспективы проверены завершающие newline, trailing
whitespace и парность fenced blocks. Для трёх source snapshots проверены
122 pinned file links, включая 110 диапазонов строк; отсутствующих файлов
и выходящих за пределы диапазонов нет. `git diff --check` прошёл.
Повторный полный code gate для этого дополнения не запускался: изменения
касаются только документации, прежний результат audit сохранён выше.

Эти работы не объявлены выполненными. Они нужны там, где source review не
даёт ответа: пригодность реального deployment, UX latency и цена будущей
модульности. Задания ограничены одним решением каждое и выполняются до
соответствующего архитектурного выбора.

| Эксперимент | Вопрос и условия | Измерение и критерий завершения |
| --- | --- | --- |
| Живой cloud chat | Один provider version, Core/Node, два turn; approve/deny/input, interrupt, detach, network break и provider kill | Раздельно command receipt, UI update, provider response и recovery; нет дублированных side effects и потери принятого ввода |
| Production boundary | Выбранный профиль из модели угроз; чужая/отозванная identity, wrong scope, symlink escape, replay approval, oversized payload | Все обязательные негативные случаи дают отказ и audit без credentials; backup восстанавливается |
| Editor baseline и одна вариация | Production build; одинаковые файлы и browser; RTT 0/50/150 мс; stream параллельно вводу | Input-to-paint, warm switch, open/save и сохранность draft; сначала baseline, затем одна переменная; бюджеты из editor research согласованы до сравнения |
| Два расширения | Trace viewer и read-only capability inspector; enable/disable, ошибочный handler, конфликт contribution, несовместимая версия | Нет прав вне scope; отказ не блокирует чат; нет feature-specific ветвей ядра; измерены startup/memory costs |
| Один внешний tool | Version-pinned MCP + ToolHive; connect, read-only call, deny, revoke и reconnect на fixture service, затем реальном сервисе | Подтверждена совместимость; наблюдаемая доступность совпадает с полномочиями; локальный disconnect не выдаётся за upstream revoke |
| Rust edit loop | Разрешённая cache directory; зафиксированные machine/toolchain/lock; без параллельного Cargo | Отдельно cold build, no-op warm, изменение leaf crate, check/test/link; timings показывают bottleneck, а выбранная оптимизация сравнивается при одинаковых условиях |

Для build experiments cold cache должен быть отдельным disposable target,
чтобы не удалять рабочий cache. Для UI сохраняются trace, число повторов,
распределение p50/p95, размеры файлов и версия стенда. Малое число запусков
не интерпретируется как устойчивый p95. Сравнение редакторов допускается
только с эквивалентными функциями и нагрузкой.

## Формат записи результата

Каждый следующий прогон добавляет дату, вопрос, commit/tag, команды,
параметры среды, ожидаемый и наблюдаемый результат, статус pass/fail/blocked,
артефакт доказательства и предел вывода. Секреты и рабочее содержимое
пользовательских репозиториев в журнал не включаются.

Результат, не изменяющий решения, не требует новых экспериментов ради
количества. Результат, оставивший неизвестность, ограничивает рекомендацию;
он не превращается в утверждение о готовности системы.
