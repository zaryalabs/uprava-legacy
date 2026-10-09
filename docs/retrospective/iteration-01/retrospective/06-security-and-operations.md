# Безопасность и эксплуатация

Снимок: `0.2.26`, commit `9d096834eadfacd9f04a551c5557b33194e7ca4b`,
2026-10-03. **Вывод:** baseline содержит существенные защитные механизмы и путь
self-hosted deployment, но поддерживаемый профиль остаётся `controlled_dev`.
Наличие deploy automation или слова production в имени smoke не является
доказательством безопасности для произвольного production-окружения.

**Опыт пользователя:** безопасное развёртывание должно было войти в первый
узкий сценарий облачных агентов. Ниже отдельно перечислены наблюдаемая
реализация, существующие проверки и требования, которые ещё нужно проверить.

## Границы доверия и существующие защиты

| Граница | Наблюдаемый механизм | Практический предел |
| --- | --- | --- |
| Браузер → Core | Локальный password setup/login, Argon2id, HttpOnly/SameSite cookie, CSRF, origin checks | Это не законченная team identity/authorization model |
| Node → Core | Enrollment, bearer credential hashes, rotation/revoke, проверка принадлежности command/event | Имя Node и heartbeat не доказывают безопасность самого host |
| Core → Node → provider | Capability admission, immutable effective policy/hash, current attempt, проверка workspace | Provider policy не изолирует недоверенный host от оператора Node |
| Provider → Uprava MCP | Session-scoped expiring lease, повторная проверка доступа при Execute, redacted trace | MCP не заменяет ограничения native CLI и файлового доступа |
| Generated UI → продукт | Opt-in, отдельный builder, iframe и permissioned actions | Нужны отдельные проверки browser isolation и supply chain |
| Task → sandbox service | Worktree, bounded execution/evidence и cleanup | OpenSandbox API key и persistent credential acceptance отложены |

Источники: [Core config](../../../../crates/uprava-server/src/config.rs),
[HTTP/auth boundary](../../../../crates/uprava-server/src/runtime/transport/http.rs),
[support](../../../../crates/uprava-server/src/runtime/support.rs),
[Tooling](../../../../crates/uprava-server/src/runtime/application/tooling.rs),
[Managed runbook](../../../runbooks/managed-agent-runtime.md).

`controlled_dev` — единственное принимаемое значение deployment profile;
отключение Web auth и общий `UPRAVA_AUTO_APPROVE_ENROLLMENTS=true` отвергаются.
По умолчанию Core слушает loopback; `Secure` cookie требует явной настройки
при HTTPS. Существуют rate limits, ограничения payload и безопасные ошибки с
correlation ID. Это конкретные механизмы, а не характеристика «вся система
безопасна».

При сверке обнаружен важный нюанс: [local-dev runbook](../../../runbooks/v01-local-dev.md)
говорит, что enrollment всегда требует ручного approval. Однако
[persistence/node.rs](../../../../crates/uprava-server/src/persistence/node.rs)
реализует opt-in `UPRAVA_AUTO_APPROVE_NODE_NAME`: первое подходящее enrollment
по точному имени может одобряться автоматически, дубликат — нет. Это
исключение необходимо учитывать при переносе конфигурации; имя не является
криптографической identity. Ретроспектива фиксирует расхождение, не меняя
исторические документы и конфигурацию.

## Managed и compatibility имеют разную политику

В [resolve_effective_runtime_policy](../../../../crates/uprava-server/src/runtime/application/session.rs)
Managed получает `workspace-write + untrusted`; network posture записан как
`unsupported`, поэтому нельзя обещать реализованное управление сетевым
доступом. Exec compatibility получает `danger-full-access + never`, требует
явного unsafe acknowledgement и audit. Отказ Managed capability не приводит
к скрытому ослаблению политики.

Node повторно канонизирует workspace, проверяет разрешённые roots и не
доверяет только прежнему Core snapshot. Provider child наследует allowlist
environment; MCP credential не передаётся через argv, обновляется через
native resume и отзывается при stop/expiry. Выключение shell snapshot и
одноразовое secret-env имя защищают от повторного использования cached
credentials. Реализация находится в [provider](../../../../crates/uprava-node/src/runtime/provider.rs)
и [managed supervisor](../../../../crates/uprava-node/src/runtime/managed_provider.rs).

Эти меры не превращают host process в hostile multi-tenant sandbox. Для
фоновых Jobs сохраняется explicit Exec contract. У
[Task sandbox](../../../runbooks/task-sandbox-runtime.md) API key и persistent
Codex credential profile прямо отложены; OpenSandbox работает через loopback
с `OPENSANDBOX_INSECURE_SERVER=YES`. Это отдельная незавершённая граница,
которую нельзя переносить как production security baseline.

## Эксплуатационные решения

[Self-hosting golden path](../../../development/self-hosting-golden-path.md)
разделяет workspace агента и каталог активного deployment. Агент работает в
разрешённых workspace, готовит изменения и evidence; изменение установленного
приложения, systemd/proxy, volumes и секретов не входит в его обычный work
loop. Доставка проходит через Git/CI/CD.

История, политика и диагностика переживают закрытие браузера. Stop/idle expiry
требуют teardown процесса, отзыва lease и закрытия interactions. Recovery
опирается на attempt и provider resume reference, а не на сохранённый PID.
Rollback требует согласованной версии binary/config/state; baseline `0.2.0`
не переинтерпретирует старое состояние `0.1.x` молча.

Сильная сторона — формализованные boundaries и runbooks. **Не проверено этим
обзором:** восстановление backup на чистом host, фактическая настройка proxy,
права daemon user, ротация всех внешних credentials и поведение при
компрометации Node. Для этих выводов нужны отдельные эксплуатационные прогоны.

## Что подтверждают существующие тесты

| Evidence в репозитории | Проверяемые случаи |
| --- | --- |
| [Core HTTP](../../../../crates/uprava-server/src/runtime/tests/http.rs) | Argon2id, setup/login, отказ mutation без CSRF, origin и payload limits |
| [Core Node](../../../../crates/uprava-server/src/runtime/tests/node.rs) | Revoke/rotation, rate limit, pending/expired enrollment, ограниченный auto-approval |
| [Core control](../../../../crates/uprava-server/src/runtime/tests/control.rs) | Cross-node ACK/event/terminal rejection, oversized/deep payload |
| [Core tooling](../../../../crates/uprava-server/src/runtime/tests/tooling.rs) | Lease expiry/rotation/revoke/foreign scope, Execute schema recheck, ephemeral credential |
| [Node workspace](../../../../crates/uprava-node/src/runtime/tests/workspace.rs) | Parent/symlink escape, revalidation, stale write, disallowed executable |
| [Node provider](../../../../crates/uprava-node/src/runtime/tests/provider.rs) | Environment allowlist, отсутствие MCP bearer в argv/debug |

Прочитанные тесты показывают защищаемые инварианты; они не заменяют penetration
test или проверку реального deployment. Новый credential-backed live smoke
этим документом не заявляется. Общие результаты проверок находятся в
[паспорте пакета](../README.md).

## Что требуется следующему первому deployment

Новая проверка во время подготовки ретроспективы обнаружила дополнительную
границу актуальности: `make c` остановился на dependency audit для закреплённого
`rustls 0.23.40`. [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285)
опубликован после основных implementation slices. Детали и warning
`event-listener` записаны в [экспериментах](../research/experiments.md).
Это наблюдаемый результат проверки 2026-10-03, не перенос findings аудита
`0.1.8`. Перед новым deployment требуется обновление и повторная проверка
зависимостей; ранее пройденный gate не гарантирует их актуальность сегодня.

**Рекомендация:** заранее описать ограниченный production-профиль: кто
пользователи, какие Node доверены, какие данные/секреты доступны и какие
действия агент вправе выполнять. Для него проверить:

1. Identity, session expiry/revoke и разрешения на project, workspace,
   terminal, tools и approvals; отсутствие межпользовательского доступа.
2. Enrollment с проверяемой identity и отзывом доступа, поведение при
   недоверенной или потерянной Node.
3. Реальные ограничения filesystem/network/process и credentials, включая
   отказ, который нельзя обойти через другой execution profile.
4. Ротацию secrets, scrub журналов, backup/restore, update/rollback и cleanup
   после crash/interrupt.
5. Измеримую отрицательную acceptance: запрещённое действие отклоняется,
   оставляет безопасное evidence и понятное состояние в UI.

Это требования к исследованию и приёмке, а не обещание готовой защиты.
Подробная программа — [production security research](../research/03-production-security.md).
Урок итерации: безопасность должна ограничивать первый поддерживаемый
сценарий с начала, а готовность подтверждаться его полной проверкой.
