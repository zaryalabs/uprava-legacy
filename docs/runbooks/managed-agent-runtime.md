# Managed Agent Runtime

Статус: `active`

Runbook относится к implementation baseline `0.2.26` и описывает rollout,
диагностику, recovery и host-only acceptance для provider-native Managed Agent
runtime. Поддерживаемый первый профиль — Codex app-server v2 через loopback
WebSocket на Node; проверенный compatibility baseline — `codex-cli 0.144.1`.

## Default и совместимость

- Web выбирает Managed для новой Agent session, только если Node объявил
  `provider.codex.managed` available.
- Create Session API трактует отсутствующий `execution_profile` как `managed`.
  Incapable Node получает `runtime.profile_capability_unavailable`; Core не
  создаёт Exec runtime вместо него.
- Exec compatibility остаётся отдельным explicit unsafe выбором с
  acknowledgement в Web и `runtime.policy.unsafe_override` в audit.
- Stored profile существующей session не меняется после upgrade или нового
  heartbeat. Migration 18 сохраняет старые sessions как `exec_compatibility`.
- Composer Managed session позволяет выбрать `Default` или `Plan`. Public
  `SendTurnRequest.collaboration_mode` принимает `default | plan`; Core
  отклоняет Plan для Exec compatibility. Node передаёт Plan как typed
  explicit `default | plan` `collaborationMode` с effective model из
  `thread/start`/`thread/resume`, что не позволяет Plan протечь в следующий
  turn и делает provider user-input requests доступными без скрытой
  конфигурации.
- Internal Job runtime по-прежнему задаёт `exec_compatibility` явно. Task Run
  остаётся независимым OpenSandbox contract без interactive session.

## Capability gate

Перед start проверьте inventory Node. Для Managed Codex должны быть доступны:

```text
provider.codex.managed
provider.codex.managed.approval
provider.codex.managed.interrupt
provider.codex.managed.resume
```

Частые `unavailable_reason`:

| Reason | Действие оператора |
| --- | --- |
| `binary_not_found` | Установить Codex для daemon user или задать абсолютный `UPRAVA_CODEX_BINARY`, затем перезапустить Node. |
| `version_unrecognized` | Проверить `UPRAVA_CODEX_BINARY --version` и wrapper; Node должен увидеть semver Codex CLI. |
| `version_unsupported` | Обновить Codex до поддерживаемой версии и повторить heartbeat. |
| `capability_not_reported` | Проверить версию Node, control connection и diagnostics heartbeat. |

Managed start также требует совпадения immutable policy hash. Mismatch не
исправляется fallback: runtime переходит в typed error/recovery state.

## Recovery matrix

Deterministic `make c` покрывает следующие границы:

| Failure | Ожидаемое поведение |
| --- | --- |
| duplicate notification или reconnect replay | Event/message projection остаётся idempotent. |
| sequence gap или lagged SSE bus | Runtime получает degraded reason; Web выполняет bounded reload. |
| Core/Node control reconnect | Принимается только newest control generation и current attempt с тем же policy hash. |
| Node restart со stale process descriptor | Attempt становится lost/provider-resumable; stale PID не считается live evidence. |
| provider crash или transport loss | Публикуется typed failure/recovery; Exec compatibility не запускается. |
| expired или replayed interaction | Late decision отклоняется, новый provider command не создаётся. |
| stop/interrupt/shutdown | Управление ограничено current attempt; interrupt не блокируется socket mutex, имеет bounded confirmation и TERM/KILL escalation, stop закрывает attempt, отменяет pending interactions и отзывает session MCP leases. Node держит SIGINT/SIGTERM handler активным на всём supervisor loop и завершает managed children при service shutdown. |
| Managed start/resume MCP bootstrap | Process lease переиспользуется до refresh window; при rotation Node поднимает replacement app-server с новым credential и делает provider-native resume до остановки старого. Встроенный `uprava` server имеет provider approval mode `approve`, потому что Search/Inspect/Execute уже проходят Core-owned permission и approval boundary. Доступ остановленной session ограничен живой `ResumeRuntime` command. |
| Codex повторно использует cached shell snapshot | Node отключает provider `shell_snapshot`, задаёт одноразовое имя MCP secret-env и наследует его только из предварительно очищенного allowlist environment. Lease value не попадает в argv/config/log. Rejected lease логируется только с несекретным lease id. |
| неизвестный app-server callback | Node отвечает protocol error, завершает provider process и публикует `provider.managed_callback_unsupported`, не оставляя turn зависшим. |
| параллельные command/event/MCP writes в SQLite | Lease rotation и event projection резервируют writer до read snapshot; Node outbox не теряет `RuntimeReady` из-за `SQLITE_BUSY_SNAPSHOT`. |
| queue pressure и oversized payload | Bounded queue/payload diagnostics без fabricated success. |
| 24h idle expiry | Core записывает system StopRuntime, Node завершает provider process, Core отзывает MCP lease и помечает interactions expired; resume разрешён только после terminal stop confirmation. |
| existing Agent, Job и Task contracts | Existing profile immutable; Job остаётся Exec; Task не создаёт Agent session. |

Основные regression suites находятся в Core `runtime/tests/{control,event,runtime,session,scheduling,task}.rs`
и Node `runtime/tests/{provider,reliability}.rs`, включая deterministic fake
app-server two-turn/interaction coverage.

## Host-only acceptance

На host с установленным и авторизованным Codex выполните:

```sh
make codex-smoke
```

Smoke создаёт disposable Core/Web/Node и Git workspace, затем проверяет:

1. Managed выбран API default и имеет `workspace-write + untrusted` policy.
2. Первый turn стримится и завершается.
3. Web detach/attach не останавливает provider.
4. Stop и provider-native Resume создают новый attempt той же session.
5. Второй turn после Resume завершается и остаётся после Web reload.
6. Approve и deny продолжают тот же provider turn через typed interactions.
7. Turn с `collaboration_mode=plan` создаёт provider user-input question и
   получает typed answer.
8. Длинный turn прерывается, а живой runtime возвращается в `ready`.
9. Uprava MCP `search_tools` вызывается реальным provider process.
10. Explicit Exec compatibility session стартует и выполняет отдельный turn.

Host target запускает только real-profile Playwright scenario; полный
детерминированный Web suite остаётся частью `make c`. После cleanup не должно
оставаться app-server процессов с cwd внутри disposable smoke workspace.

На чистом checkout enrollment имеет отдельное startup-окно: компиляция Node не
расходует короткий HTTP readiness budget.

`$CODEX_SMOKE_STATE_DIR/acceptance.env` сохраняет только timestamp, Codex
version, OS, transport и пройденные profile gates. Prompt, output и credentials
в acceptance record не записываются. Core/Node/Web logs остаются рядом для
локальной диагностики; перед публикацией evidence их нужно scrub.

Acceptance завершается ошибкой, если конкретная Codex version не предоставляет
user-input request, approval continuation, interrupt или MCP в выбранном mode;
такая версия не получает deployment evidence.

## Диагностика и rollback

Сначала зафиксируйте execution profile, runtime/attempt state, recovery reason,
policy hash, Node capability reason и последние bounded provider events. Не
копируйте raw environment, auth files или MCP bearer values.

При `mcp_lease.invalid` сравнивайте только `presented_lease_id` в Core warning с
`lease_id` audit-события `provider.mcp_access.issued`. Значения bearer token и
полный environment в диагностические материалы не включайте.

Rollback выполняется только для новых sessions: пользователь явно выбирает
Exec compatibility и подтверждает unrestricted posture. Existing Managed
session не меняет profile; её нужно Stop, сохранить recovery evidence и создать
отдельную compatibility session. Silent fallback запрещён во всех случаях.

Текущая поддержка рассчитана на controlled deployment. Persistent credential
profiles, hostile multi-tenant isolation и sessionless sandboxed Jobs остаются
отдельными follow-up.
