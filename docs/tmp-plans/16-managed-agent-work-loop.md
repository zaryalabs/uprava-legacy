# План реализации feature 16: Managed Agent Work Loop

Статус: `completed` — этапы 0–5 поставлены в `0.2.20`–`0.2.25`

Целевой delivery range: после implementation baseline `0.2.19`; фича может
занять несколько последовательных SemVer slices. Точные версии назначаются при
закрытии реально поставленных vertical slices, а не заранее всему плану.

## Канонические основания

- [Feature Queue, пункт 16](../product/feature-queue.md#16-managed-agent-work-loop);
- [Vision](../vision.md#managed-agent-work-loop);
- [A-002 Run Mode](../systems/areas/002-run-mode.md);
- [Architecture](../systems/architecture.md);
- [A-011 Background Jobs](../systems/areas/011-background-jobs.md);
- [A-013 Task-based Sandbox Runtime](../systems/areas/013-task-based-sandbox-runtime.md);
- [Versioning](../versioning.md).

Этот документ раскладывает принятую продуктовую позицию на этапы реализации.
При расхождении канонические документы выше имеют приоритет. Новые долговечные
решения, принятые во время protocol spike, нужно сначала перенести в `docs/`, а
не оставлять только в этом временном плане.

## Цель

Сделать поверхность Agent полноценной живой рабочей сессией Codex:

- provider-native runtime остаётся доступным между turns;
- Web видит streaming output и structured provider activity;
- approval или provider question останавливает именно текущее выполнение;
- решение человека продолжает то же provider execution;
- пользователь может interrupt, stop, detach, reattach and resume;
- safe managed profile становится default для новых Agent sessions;
- текущий `codex exec/resume` с
  `--dangerously-bypass-approvals-and-sandbox` остаётся явным надёжным Exec
  compatibility mode;
- Tasks и Jobs не поглощаются моделью Agent session.

Речь идёт о TUI-equivalent возможностях через semantic provider protocol. Сам
Codex TUI не встраивается и не эмулируется в Web Control Panel.

## Product acceptance

Пользователь создаёт новую Agent session и до запуска видит:

- execution mode `Managed`;
- effective sandbox and approval policy;
- target Node and workspace;
- доступность provider-native runtime capability.

После запуска пользователь отправляет несколько turns в одну живую Codex
session, наблюдает команды, tool activity, сообщения и изменения. Когда Codex
запрашивает approval или дополнительный input, Agent surface показывает
typed request. Approve/deny/input проходит через Core and Node и продолжает то
же provider execution без нового turn or reconstructed prompt.

Пользователь может закрыть Web, вернуться и подключиться к той же session.
Interrupt прекращает активный turn, Stop завершает provider process, а Resume
восстанавливает provider session в том же workspace или честно показывает
degraded recovery.

При создании Agent session пользователь может явно выбрать Exec compatibility.
UI постоянно показывает, что этот mode unrestricted и не предоставляет
настоящего approval continuation. Managed runtime никогда не переходит в него
автоматически после ошибки.

## Стратегия поставки

Фича делится на пять крупных этапов и финальный release gate:

```text
Этап 0. Provider protocol spike and architecture gate
  -> Этап 1. Shared contracts, persistence and policy foundation
    -> Этап 2. Node-managed Codex runtime
      -> Этап 3. Core orchestration and real interaction loop
        -> Этап 4. Agent Web work surface
          -> Этап 5. Recovery, rollout and release closure
```

После этапа 1 Core, Node and Web могут развиваться параллельно только в рамках
зафиксированных contracts. Managed mode не становится default, пока этап 5 не
закрыл real-provider acceptance and recovery matrix.

Рекомендуемая release slicing:

1. **Foundation slice** — этапы 0–1, без обещания пользовательского managed
   mode.
2. **Opt-in vertical slice** — этапы 2–4, managed mode доступен явно на
   проверенных Nodes, Exec compatibility остаётся default.
3. **Default-on closure** — этап 5, managed mode становится default для новых
   Agent sessions.

Это рабочая нарезка, а не заранее назначенные номера версий. Если foundation и
opt-in path безопасно помещаются в один release, искусственно делить их не
нужно. Нельзя объединять default-on с недоказанным provider protocol только
ради одного version bump.

## Scope

### Входит

- Codex-first provider-native managed transport;
- отдельные Agent profiles `managed` and `exec_compatibility`;
- capability probing and admission;
- effective runtime policy snapshot;
- managed process/session lifecycle on Node;
- live streaming and structured activity normalization;
- real approvals and provider questions;
- interrupt, stop, detach/reattach and resume/recovery;
- Core-owned ordered events, trace and audit;
- Web mode selection, policy preview, timeline and interaction cards;
- deterministic fake-provider coverage and real Codex acceptance;
- explicit compatibility and migration behavior for existing sessions;
- release docs, runbook and operator diagnostics.

### Не входит

- embedding or terminal emulation of Codex TUI;
- Agent-to-Task delegation;
- изменение Task Run execution contract;
- sessionless Jobs refactor;
- dangerous Job override;
- provider parity with OpenCode, Claude Code or every future adapter;
- team/cloud RBAC and hostile multi-tenant isolation;
- workflow engine, multi-agent orchestration or task pipelines;
- durable checkpoints of provider RAM/process memory;
- arbitrary remote provider connections outside the Node trust boundary.

## Зафиксированные решения

1. Managed runtime является развитием поверхности Agent, не Tasks or Jobs.
2. Node владеет provider process/connection; Core остаётся durable control
   plane and policy authority.
3. Managed runtime использует semantic provider protocol. Raw terminal screen
   не является protocol or source of truth.
4. Exec compatibility сохраняет текущий `codex exec/resume` path и dangerous
   flags. Это отдельный profile, не аварийная ветка managed adapter.
5. Silent fallback из `managed` в `exec_compatibility` запрещён.
6. Новые Agent sessions становятся managed by default только после real Codex
   acceptance and recovery gate.
7. Existing sessions после migration остаются `exec_compatibility`; release не
   меняет их trust posture задним числом.
8. Tasks продолжают unrestricted `codex exec` внутри OpenSandbox. Provider
   refactor не должен направить Task Run через managed session driver.
9. Пока Jobs всё ещё технически создают internal session/runtime, они обязаны
   явно выбирать exec path. Feature 16 не должна случайно перевести scheduled
   Jobs на interactive managed runtime.
10. Target Jobs contract остаётся отдельным follow-up: sessionless sandboxed
    exec with non-interactive approvals.
11. Один RuntimeSession принимает не больше одного active turn; параллельные
    provider requests внутри одной session не вводятся в первом slice.
12. Provider payloads проходят size limits, redaction and typed normalization;
    Core не хранит неограниченный raw stream.
13. Provider-specific types не протекают в public Core/Web contracts, кроме
    bounded diagnostics and capability metadata.
14. Shared command/event vocabulary расширяется typed variants, а не
    долговечным `Extension` payload.

## Рабочая архитектурная позиция

План исходит из разделения трёх уровней:

```text
SessionThread
  durable user-visible conversation/work context

RuntimeSession
  durable control-plane runtime lineage and selected execution profile

RuntimeAttempt
  one concrete managed provider process/connection incarnation
```

`RuntimeAttempt` нужен для process id/transport identity, policy snapshot,
start/stop reason, recovery and audit. Это рабочее решение плана; на этапе 0
его нужно подтвердить и перенести в A-002/architecture до schema implementation.
Если spike докажет, что provider protocol имеет другую устойчивую identity
model, термины можно скорректировать, сохранив distinction между durable thread
и конкретным process incarnation.

Первый managed adapter предпочтительно запускает отдельный provider process на
RuntimeAttempt. Shared multi-session daemon допустим только если spike докажет
изоляцию routing, cancellation, policy and crash domains не хуже process-per-
attempt baseline.

## Этап 0. Provider protocol spike and architecture gate

Статус: `completed` в implementation baseline `0.2.20`.

Выбран experimental Codex app-server v2 из `codex-cli 0.144.1` через локальный
WebSocket и topology process-per-`RuntimeAttempt`. Disposable Rust probe,
scrubbed fixtures, measured recovery/policy results и принятые решения находятся
в `tools/codex-app-server-probe` и каноническом
[`A-002 Run Mode`](../systems/areas/002-run-mode.md#provider-protocol-gate-0220).
Managed mode ещё не реализован и не становится default: следующий gate — этап
1, shared contracts and persistence foundation.

### Цель

До production contracts доказать, что установленный Codex предоставляет
управляемый двусторонний protocol с нужной семантикой. Не строить основной
runtime поверх предположения, что `app-server`, `remote-control` или
`exec-server` гарантируют одинаковый lifecycle.

### 0.1 Candidate transport inventory

Для pinned Codex CLI baseline проверить доступные provider-owned interfaces:

- `codex app-server`;
- remote app-server connection;
- `codex remote-control`;
- `codex exec-server`;
- другой официальный machine protocol, если он заменил перечисленные paths.

Для каждого candidate зафиксировать:

- transport: stdio, WebSocket, Unix socket or HTTP;
- handshake and version negotiation;
- session/thread identity;
- request/response correlation;
- notification ordering;
- approval and user-input semantics;
- interrupt/cancel semantics;
- reconnect and resume behavior;
- process lifetime and shutdown;
- auth and `CODEX_HOME` behavior;
- sandbox/approval configuration;
- MCP and workspace configuration;
- stability label and compatibility risk.

### 0.2 Required scenario spike

Disposable probe должен выполнить минимум:

1. запустить provider runtime в разрешённом workspace;
2. создать provider session and send first turn;
3. получить incremental output and structured command/tool activity;
4. отправить второй turn в ту же live session;
5. вызвать безопасный approval request;
6. approve and deny requests и доказать continuation того же execution;
7. обработать provider question/user input, если protocol различает его с
   approval;
8. interrupt active turn and verify process/turn state;
9. detach client transport and reconnect without duplicated events;
10. остановить provider process и проверить provider-native resume;
11. принудительно убить process и определить честный degraded recovery path;
12. проверить Uprava MCP access без вывода lease token в args/logs;
13. проверить safe sandbox profile and explicit unrestricted profile;
14. измерить idle process memory, active turn overhead and shutdown latency.

Spike не должен использовать production Core tables or public API. Маленький
throwaway Rust probe, fixtures and captured bounded protocol examples можно
оставить в repository test support, если они полезны для adapter tests.

### 0.3 Architecture decisions

По итогам spike принять и перенести в канонические docs:

- выбранный managed transport and pinned compatibility baseline;
- process-per-attempt or shared daemon topology;
- `RuntimeSession` versus `RuntimeAttempt` identity;
- provider resume reference shape and redaction;
- approval/question distinction;
- interrupt escalation contract;
- reconnect ownership between Core and Node;
- policy fields, которые provider реально может enforce;
- unsupported capability behavior.

### 0.4 Failure rule

Если ни один provider-native candidate не доказывает approval continuation,
interrupt and reconnect:

- этап 0 считается failed gate;
- Exec compatibility остаётся основным working mode;
- нельзя переименовать JSONL observation текущего `codex exec` в managed
  runtime;
- следующая итерация сравнивает официальный newer protocol or bounded adapter
  alternative и обновляет canonical risk note.

### Exit criteria этапа 0

- выбран один конкретный managed transport;
- два turns проходят через одну live provider session;
- approval round-trip продолжает то же execution;
- interrupt and stop подтверждены;
- reconnect/resume behavior измерен и описан;
- effective provider policy может быть задана и прочитана до start;
- protocol fixtures bounded and scrubbed of credentials;
- RuntimeAttempt/topology decision перенесено в canonical docs;
- unresolved provider limitation имеет typed product fallback, а не скрытый
  workaround.

## Этап 1. Shared contracts, persistence and policy foundation

Статус: `completed` в implementation baseline `0.2.21`.

Shared Rust/Web vocabulary, typed command/event extensions, migration 18,
immutable policy snapshot/hash, runtime attempts, provider interactions и
profile-aware capability admission реализованы. Existing sessions и отсутствие
profile сохраняют `exec_compatibility`; Node до этапа 2 объявляет managed
capabilities unavailable, поэтому managed start получает typed rejection без
fallback.

### Цель

Зафиксировать общий язык Core, Node and Web до параллельной реализации runtime
and UI.

### 1.1 Protocol vocabulary

Добавить или уточнить shared types:

```text
AgentExecutionProfile
  managed | exec_compatibility

RuntimeAttemptId
RuntimeAttemptState
ProviderRuntimeCapability
ProviderInteractionId
ProviderInteractionKind
EffectiveRuntimePolicy
RuntimePolicyHash
RuntimeRecoveryStatus
```

`EffectiveRuntimePolicy` должен как минимум отражать:

- provider sandbox mode;
- approval mode;
- writable workspace/additional paths;
- network posture, если provider protocol умеет её enforce;
- MCP/tool exposure summary;
- credential profile reference without secret value;
- unsafe override flag, actor, reason and expiry when applicable;
- provider capability/version used to calculate the policy.

### 1.2 Command and event contracts

Расширить typed commands:

- `StartRuntime` and `ResumeRuntime` получают execution profile and effective
  policy snapshot/reference;
- `ResolveApproval` адресует конкретный provider interaction;
- добавить typed submit-user-input command, если provider question отличается
  от approval;
- interrupt and stop получают attempt-aware validation;
- recovery/reconcile не маскируется обычным Start.

Нормализовать events:

```text
runtime.attempt.started
runtime.attempt.ready
runtime.attempt.disconnected
runtime.attempt.reconnecting
runtime.attempt.recovered
runtime.attempt.failed
provider.activity
provider.interaction.requested
provider.interaction.resolved
turn.interrupted
runtime.policy.effective
runtime.policy.override
```

Существующие stable event names можно переиспользовать, если их payload and
state semantics подходят. Нельзя создавать дубликаты только ради новой
терминологии.

### 1.3 Persistence migration

Новая numbered migration должна:

- добавить execution profile к runtime/session projection;
- сохранить immutable effective policy JSON and hash;
- хранить current attempt reference and provider managed resume reference;
- добавить `runtime_attempts`, если решение этапа 0 подтверждено;
- хранить pending provider interactions с request/resolve identity;
- сохранить start/stop/recovery reason and timestamps;
- мигрировать existing sessions в `exec_compatibility`;
- не менять checksums предыдущих migrations.

Provider process handles, sockets, bearer tokens and raw secrets в Core DB не
хранятся.

### 1.4 Capability and admission contract

Node сообщает capabilities раздельно:

```text
provider.codex.exec
provider.codex.managed
provider.codex.managed.approval
provider.codex.managed.interrupt
provider.codex.managed.resume
```

Core отклоняет managed start с typed unavailable/degraded reason, если Node не
доказал обязательные capabilities. Отказ не создаёт Exec session автоматически.

### 1.5 API and fixtures

- `CreateSessionRequest` принимает explicit execution profile;
- отсутствие profile до default-on gate сохраняет current compatibility
  behavior; после gate API default меняется согласованно с Web;
- session detail возвращает selected profile, current attempt, effective policy
  and recovery state;
- Agent projection показывает доступные commands and pending interactions;
- Rust fixtures and Web validators обновляются в одном contract change;
- old clients получают bounded compatible behavior or explicit protocol error.

### Exit criteria этапа 1

- shared contracts compile and round-trip through canonical Web fixtures;
- migration upgrade and clean-state paths протестированы;
- existing sessions остаются Exec compatibility;
- managed capability absence возвращает typed error without fallback;
- effective policy hash deterministic;
- commands/events имеют ownership, bounds and redaction rules;
- Core, Node and Web не используют собственные строковые mode names.

## Этап 2. Node-managed Codex runtime

Статус: `completed` в implementation baseline `0.2.22`.

Node теперь владеет process-per-attempt Codex app-server v2 supervisor,
provider socket/process lifetime, typed interaction continuation, native
interrupt/stop, bounded normalization и secret-free durable recovery
descriptor. Deterministic fake app-server покрывает two-turn live thread,
approval, user input and stop. Managed остаётся opt-in до этапов 3–5.

### Цель

Реализовать data-plane owner живого provider process, сохранив текущий exec
adapter как независимый compatibility driver.

### 2.1 Provider driver boundary

Разделить текущий `RuntimeManager` минимум на:

```text
AgentRuntimeDriver
  CodexManagedDriver
  CodexExecCompatibilityDriver
```

Task runtime не импортирует `AgentRuntimeDriver`: его `codex exec` остаётся
частью `TaskRuntimeBackend` execution path.

Driver contract покрывает:

- start/resume;
- send turn;
- resolve approval;
- submit user input;
- interrupt;
- stop;
- inspect/reconcile;
- normalized event stream;
- effective capability report.

### 2.2 Managed process supervisor

Node создаёт actor/supervisor per active RuntimeAttempt:

- запускает выбранный Codex managed process;
- выполняет handshake and version check;
- хранит live transport handle только в memory;
- сериализует commands для одной RuntimeSession;
- correlates provider requests and notifications;
- bounds queues and raw payload sizes;
- redacts auth, environment and MCP lease values;
- обновляет durable local attempt descriptor without storing secrets;
- завершает child process and joins reader/writer tasks on stop/shutdown.

### 2.3 Event normalization

Adapter преобразует provider events в существующие Uprava lifecycle, activity,
message, approval and trace events. Unknown provider event:

- не ломает stream;
- получает bounded diagnostic;
- не становится assistant message;
- не копирует arbitrary nested payload в Core;
- учитывается metric/counter для protocol drift.

### 2.4 Real interactions

- approval request создаёт stable interaction mapping;
- approve/deny отправляется в тот же provider transport;
- duplicate Core command не отправляет второй provider decision;
- provider question имеет отдельный typed path, если это подтверждено spike;
- late decision после interrupt/stop получает typed terminal conflict;
- active blocked request переживает Web detach;
- timeout/expiry оставляет explicit event and terminal reason.

### 2.5 Interrupt and stop

Interrupt сначала использует provider-native cancellation. Если process не
подтверждает cancellation в bounded timeout, Node применяет документированную
escalation policy к конкретному attempt, не ко всем provider sessions.

Stop обязан:

- прекратить active turn;
- закрыть transport;
- завершить process tree;
- revoke ephemeral MCP access;
- сохранить provider resume reference and terminal reason;
- не удалять SessionThread or workspace state.

### 2.6 Node restart and reconciliation

После Node restart live in-memory handles потеряны. Node:

- загружает durable attempt descriptors;
- проверяет, существует ли owned child/daemon process;
- reattaches только если transport and identity безопасно доказаны;
- иначе завершает orphan, помечает attempt lost and starts explicit recovery;
- не сообщает `ready` по одному наличию PID or stale socket;
- не запускает Exec compatibility автоматически.

### 2.7 Exec compatibility regression boundary

Сохранить текущие свойства:

- `codex exec` and `codex exec resume`;
- dangerous bypass flags;
- transcript fallback and provider resume ref;
- bounded JSONL activity;
- current error codes;
- existing smoke path.

Provider refactor не должен менять Tasks. Current internal Job sessions должны
передавать `exec_compatibility` явно.

### Exit criteria этапа 2

- deterministic fake managed provider доказывает two-turn live session;
- approve/deny and question input продолжают current execution;
- interrupt stops active turn without killing unrelated sessions;
- stop leaves resumable durable state;
- Node restart produces reattach, explicit recovery or degraded state;
- unknown/oversized provider events bounded;
- exec compatibility regression suite passes unchanged;
- Task Run still invokes unrestricted exec only inside OpenSandbox;
- no provider secret appears in command args, events, DB or debug logs.

## Этап 3. Core orchestration and real interaction loop

Статус: `completed` в implementation baseline `0.2.23`.

Core теперь рассчитывает и показывает policy preview до start, атомарно хранит
profile/policy/command/audit, различает accepted resolution intent и provider
confirmation, проецирует blocked/running turns, сверяет current attempt при
events/reconnect и публикует bounded recovery/interaction diagnostics.

### Цель

Сделать Core durable authority для profile selection, effective policy,
interaction lifecycle, ordered trace and recovery.

### 3.1 Session creation and profile admission

- validate requested profile against Node capabilities;
- calculate effective policy before creating Start command;
- return policy preview/read model to Web;
- persist profile and policy atomically with SessionThread/RuntimeSession;
- record explicit unsafe Exec compatibility selection in audit;
- never retry failed managed start as exec;
- ensure quota admission remains common without conflating profile policy.

### 3.2 Effective policy resolver

Первый resolver объединяет:

```text
Core deployment constraints
  -> Node provider capabilities
    -> placement/workspace scope
      -> requested Agent profile
        -> explicit user override
```

Resolver возвращает immutable snapshot and hash for an attempt. Policy drift on
resume не применяется молча: Core either reuses still-valid snapshot or asks
for a new start/confirmation and records the difference.

### 3.3 Approval and input state machine

Core должен различать:

- provider execution approval;
- provider question/user input;
- Tool Registry approval;
- Node enrollment approval.

Они могут переиспользовать UI primitives, но имеют разные aggregate ownership
and commands. Provider interaction lifecycle:

```text
requested -> resolving -> approved | denied | answered
          -> expired | cancelled | superseded
```

Resolve mutation атомарно записывает command and transition intent. Provider
resolution event завершает interaction; HTTP acceptance alone не считается
доказательством continuation.

### 3.4 Turn and runtime projection

- active managed turn stays running while provider emits activity;
- approval/input moves turn/runtime into blocked projection;
- resolved interaction returns runtime to running, not ready prematurely;
- assistant final message completes the turn;
- interrupt creates `TurnState::Interrupted` and consistent runtime state;
- process loss creates degraded/recovering state without fabricating completion;
- duplicate/out-of-order provider events preserve current sequence guarantees.

### 3.5 Reconnect and recovery orchestration

Core distinguishes:

- Web/SSE disconnect: no runtime state change;
- Core restart: Node reconnects and reports actual managed attempts;
- Node control disconnect: runtime becomes unobserved/stale after bounded grace;
- provider transport disconnect: attempt reconnect/recovery state;
- provider process exit: terminal attempt plus explicit resume opportunity.

Reconciliation uses Node actual-state report with generation/attempt identity.
Stale Node report cannot resurrect a superseded attempt.

### 3.6 Audit, metrics and diagnostics

Audit at minimum:

```text
runtime.profile.selected
runtime.policy.effective
runtime.policy.unsafe_override
provider.interaction.resolved
runtime.recovery.started
runtime.recovery.completed
runtime.recovery.failed
```

Metrics remain bounded by provider/profile/state, not session ids. Diagnostics
show selected driver, provider version, attempt age, queue pressure, last event
and recovery reason without prompt, secrets or raw workspace content.

### Exit criteria этапа 3

- managed start is capability- and policy-gated;
- no silent exec fallback exists in Core retry/recovery paths;
- approval HTTP -> Node -> provider -> event -> Core projection works end to end;
- provider questions use typed state and do not masquerade as approvals;
- Core restart and Node reconnect converge without duplicate turn/message;
- effective policy visible through API and trace;
- unsafe compatibility selection audited;
- projection tests cover duplicate, late, conflicting and expired interactions.

## Этап 4. Agent Web work surface

Статус: `completed` в implementation baseline `0.2.24`.

Web даёт явный opt-in Managed и acknowledgement-gated Exec compatibility,
показывает policy preview до старта и immutable runtime policy после него,
рендерит semantic provider activity и typed interaction cards, а lifecycle
controls следуют projected capabilities. Managed пока не становится default:
это остаётся rollout gate этапа 5.

### Цель

Сделать managed runtime понятным и управляемым человеком, не превращая Agent в
terminal mirror.

### 4.1 Session start UX

Start Agent control показывает:

- `Managed` as recommended mode when capability is available;
- `Exec compatibility` as unrestricted fallback;
- concise comparison of approvals, sandbox, interrupt and recovery;
- target Node/workspace;
- effective policy preview;
- typed unavailable/degraded reason for managed capability;
- explicit acknowledgement before unsafe compatibility start.

На opt-in slice текущий default остаётся compatibility. На default-on closure
Managed становится preselected for new sessions. Existing session profile не
переключается через UI toggle after creation.

### 4.2 Live timeline

Agent timeline отображает semantic blocks:

- assistant streaming/final message;
- command/tool activity with bounded status;
- file/diff references;
- approval request;
- provider question;
- interrupt/recovery state;
- raw diagnostic fallback for unsupported activity.

Нельзя рендерить ANSI terminal dump как основной Agent output. Visual Artifact
and renderer plugin paths продолжают работать поверх normalized content.

### 4.3 Interaction cards

Approval card показывает:

- requested action and bounded rationale;
- effective policy/scope relevant to the decision;
- approve and deny;
- optional user message if provider supports it;
- pending/resolving/terminal state;
- stale/expired conflict without повторной отправки.

Provider question card принимает typed response. Tool approval and provider
approval визуально родственны, но имеют clear source label and independent
actions.

### 4.4 Lifecycle controls

- Interrupt доступен только для running/blocked managed turn;
- Stop clearly ends provider process but preserves session history;
- Detach disconnects user surface, not provider runtime;
- Reattach reloads persisted events then live stream;
- Resume shows policy drift and recovery reason before action;
- exec compatibility hides unsupported controls or marks them honestly.

### 4.5 Policy and diagnostics UX

Agent surface постоянно показывает compact mode/policy badge. Inspector даёт:

- driver and provider version;
- sandbox/approval mode;
- unsafe override;
- current attempt and recovery state;
- last runtime activity;
- bounded diagnostics and trace links.

Warnings must be accessible, keyboard-operable and visible in narrow/mobile
layouts used for unblock decisions.

### Exit criteria этапа 4

- user can start managed and compatibility sessions intentionally;
- policy visible before and after start;
- streaming does not duplicate final messages after reconnect;
- approvals/questions keyboard-accessible and idempotent;
- interrupt/stop/detach/resume controls reflect real capability/state;
- compatibility mode has persistent unrestricted warning;
- UI never implies that Task or Job is an Agent live session;
- component, projection and Playwright coverage includes managed happy and
  blocked paths.

## Этап 5. Recovery, rollout and release closure

Статус: `completed` в implementation baseline `0.2.25`. Managed является
capability-aware default новых Agent sessions; missing profile не включает
Exec fallback, existing sessions сохраняют stored profile, Jobs и Tasks не
меняют contract. Recovery matrix, diagnostics и real-host acceptance описаны в
[`Managed Agent Runtime`](../runbooks/managed-agent-runtime.md).

### Цель

Доказать operational reliability, включить Managed by default and закрыть
feature without weakening compatibility paths.

### 5.1 Failure matrix

Автоматически or manually проверить:

- Web reload during streaming;
- SSE cursor reconnect;
- Core restart while provider process remains live;
- Node control reconnect;
- Node process restart;
- provider managed process crash;
- stale socket/PID and orphan cleanup;
- duplicate provider notification;
- sequence gap and bounded reload;
- approval resolve replay;
- late approval after interrupt;
- blocked request expiry;
- interrupt timeout and escalation;
- Stop during streaming and blocked states;
- 24h idle expiry using accelerated test clock;
- resume with and without provider resume reference;
- policy drift between stop and resume;
- Codex version/capability mismatch;
- saturated event/output queues;
- multiple managed sessions on one Node;
- concurrent Agent and current Job activity warning/guard behavior.

### 5.2 Security and resource review

- workspace canonicalization remains enforced on every start/resume;
- safe sandbox is actual provider configuration, not UI label;
- unsafe compatibility requires explicit actor action;
- MCP lease is per session/attempt, short-lived and revoked on stop;
- provider raw payloads cannot inject unbounded events or secrets;
- child process inherits only allowlisted environment;
- process tree cleanup and file permissions verified;
- idle and active resource measurements documented;
- current controlled-deployment limitations remain visible.

### 5.3 Real Codex acceptance

Добавить отдельный host-only manual/smoke path, который не входит в
deterministic CI without credentials. Он доказывает:

1. managed session start;
2. two sequential turns in one provider session;
3. streamed activity and final output;
4. real approval approve and deny;
5. provider question if supported;
6. interrupt;
7. Web detach/reattach;
8. stop and provider-native resume;
9. MCP access;
10. explicit Exec compatibility session remains functional.

Acceptance records pinned Codex version, OS, selected transport and known
limitations. Credential or prompt content не коммитится.

### 5.4 Rollout

1. Merge managed capability behind explicit opt-in.
2. Run dogfood on local and self-hosted Node.
3. Collect protocol drift, crash, recovery and resource evidence.
4. Fix blockers without weakening policy or enabling silent fallback.
5. Change new Agent session default to Managed only on capable Nodes.
6. Keep Exec compatibility user-selectable and documented.
7. Existing sessions retain their stored profile.

### 5.5 Documentation and release

- update A-002 from target to implemented behavior;
- update architecture and feature queue implementation note;
- add managed runtime operator/runbook section;
- document capability diagnostics and fallback selection;
- update self-hosting golden path;
- record version and slice in `docs/releases.md`;
- update package metadata according to `docs/versioning.md`;
- archive or mark completed this plan after all feature exit criteria pass.

### Exit criteria этапа 5

- real Codex acceptance passes on supported host profile;
- Managed is default for new Agent sessions on capable Nodes;
- Exec compatibility remains explicitly selectable and tested;
- failure matrix has no silent fallback, lost approval or fabricated success;
- Task Run regression proves unchanged external-sandbox execution;
- current Jobs remain explicitly exec-driven until their separate follow-up;
- Core/Node/Web diagnostics make managed failures actionable;
- `make c` passes from clean state;
- release metadata and canonical docs match actual behavior.

## Отдельный follow-up: sessionless Jobs

Этот follow-up нужен новому vision, но не входит в feature 16 delivery gate.
После стабилизации provider driver boundary отдельный plan должен:

- удалить создание `SessionThread/RuntimeSession` на каждый Job Run;
- дать `JobRun` собственный one-shot provider execution identity;
- запускать sandboxed `codex exec` с non-interactive approval policy;
- сохранять effective policy snapshot and run trace;
- добавить workspace concurrency guard между Agent and Job;
- сохранить current schedule, overlap, quota and stop-on-error semantics;
- рассмотреть dangerous Job mode только как explicit audited unsafe override.

До этого follow-up feature 16 обязана сохранить current Jobs on exec path and
не переключать их на provider-native interactive runtime.

## Сквозная test strategy

### Contract tests

- Rust serialization and Web fixture parity;
- policy hash stability;
- capability and profile compatibility;
- bounded provider payloads;
- migration of existing exec sessions;
- unknown enum/event compatibility.

### Node tests

- fake managed provider over selected transport;
- process supervisor lifecycle;
- approval/input correlation and replay;
- interrupt escalation;
- reconnect/reconcile;
- queue pressure and output truncation;
- secret redaction;
- exec and Task regressions.

### Core tests

- profile admission and no-fallback invariant;
- effective policy calculation;
- interaction state machine;
- command/event atomicity;
- restart/reconciliation;
- projection and SSE replay;
- audit and diagnostics bounds.

### Web tests

- mode selection and policy preview;
- managed unavailable/degraded states;
- streaming and reconnect deduplication;
- approval/question cards;
- lifecycle controls;
- persistent compatibility warning;
- narrow viewport and keyboard interaction.

### End-to-end tests

- deterministic fake-provider managed flow in CI;
- real host Codex smoke outside deterministic CI;
- Core/Node restart and reconnect scenarios;
- self-hosted controlled-deployment acceptance.

## Риски

### Provider protocol нестабилен

Mitigation: mandatory stage 0, pinned compatibility baseline, bounded protocol
adapter, unknown-event diagnostics and default-on only after real acceptance.

### Managed process survives but control connection is lost

Mitigation: attempt generation, explicit actual-state reconciliation, safe
reattach proof and orphan cleanup. Нельзя считать PID достаточным evidence.

### Approval UI принимает решение, которое provider не получил

Mitigation: `resolving` intermediate state; terminal resolution only after
provider event/ack; idempotent command and provider request mapping.

### Safe label does not match effective provider configuration

Mitigation: immutable effective policy passed to Node, provider handshake/
diagnostic evidence, policy hash in trace and no UI-only policy state.

### Runtime refactor ломает Tasks or Jobs

Mitigation: separate drivers, explicit profile on internal Job runtime,
TaskRuntimeBackend independence and mandatory regression gates.

### Feature grows into workflow engine

Mitigation: no Agent-to-Task delegation, pipelines or multi-agent scheduling in
scope. First acceptance ends at one human and one live provider session.

## Полное completion criteria feature 16

Feature 16 считается завершённой только если:

1. provider-native Managed mode является default для новых Agent sessions на
   capable Nodes;
2. two-turn live session, streaming, approvals, questions, interrupt,
   detach/reattach, stop and resume работают end to end;
3. effective policy видна и реально enforced;
4. Exec compatibility сохраняется как explicit unrestricted fallback;
5. managed failure никогда не включает compatibility silently;
6. existing sessions retain compatibility behavior after upgrade;
7. Tasks остаются externally sandboxed unrestricted exec;
8. Jobs не переводятся на interactive managed runtime;
9. deterministic and real-provider acceptance gates проходят;
10. canonical docs, runbooks, release metadata and implementation совпадают.
