# Managed Agent Runtime

Статус: `active`

Runbook относится к implementation baseline `0.2.25` и описывает rollout,
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
| stop/interrupt | Управление ограничено current attempt; stop закрывает attempt, отменяет pending interactions и отзывает session MCP leases. |
| Managed start/resume MCP bootstrap | Lease выдаётся только привязанной Node и eligible command; доступ остановленной session ограничен живой `ResumeRuntime` command и закрывается при terminal result. |
| параллельные command/event/MCP writes в SQLite | Lease rotation и event projection резервируют writer до read snapshot; Node outbox не теряет `RuntimeReady` из-за `SQLITE_BUSY_SNAPSHOT`. |
| queue pressure и oversized payload | Bounded queue/payload diagnostics без fabricated success. |
| 24h idle expiry | Runtime становится expired и допускает только explicit resume. |
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
6. Explicit Exec compatibility session стартует и выполняет отдельный turn.

На чистом checkout enrollment имеет отдельное startup-окно: компиляция Node не
расходует короткий HTTP readiness budget.

`$CODEX_SMOKE_STATE_DIR/acceptance.env` сохраняет только timestamp, Codex
version, OS, transport и пройденные profile gates. Prompt, output и credentials
в acceptance record не записываются. Core/Node/Web logs остаются рядом для
локальной диагностики; перед публикацией evidence их нужно scrub.

Provider interactions и MCP дополнительно проверяются вручную на той же
Managed session:

1. Попросить выполнить безопасную команду, approve typed request и убедиться,
   что продолжается тот же turn; повторить с deny.
2. В provider mode, поддерживающем user input, запросить уточнение и ответить
   через question card.
3. Запустить длинный turn, нажать Interrupt и убедиться, что late interaction
   больше нельзя разрешить.
4. Попросить вызвать MCP `search_tools`; проверить tool activity и отсутствие
   lease token в args, trace и logs.

Если конкретная Codex version не предоставляет user-input request в выбранном
mode, это фиксируется как provider limitation, а не заменяется approval или
reconstructed prompt.

## Диагностика и rollback

Сначала зафиксируйте execution profile, runtime/attempt state, recovery reason,
policy hash, Node capability reason и последние bounded provider events. Не
копируйте raw environment, auth files или MCP bearer values.

Rollback выполняется только для новых sessions: пользователь явно выбирает
Exec compatibility и подтверждает unrestricted posture. Existing Managed
session не меняет profile; её нужно Stop, сохранить recovery evidence и создать
отдельную compatibility session. Silent fallback запрещён во всех случаях.

Текущая поддержка рассчитана на controlled deployment. Persistent credential
profiles, hostile multi-tenant isolation и sessionless sandboxed Jobs остаются
отдельными follow-up.
