# A-002 Run Mode

Статус: `working-position`

Этот документ фиксирует первую рабочую позицию по **Run Mode** как ключевой
механике Uprava. Run Mode описывает не одну конкретную фичу, а способ, которым
Uprava запускает агентскую работу в проекте, продолжает ее, наблюдает за ней и
останавливает.

Зафиксированная продуктовая позиция: основная поверхность Agent является
рабочей сессией с живым управляемым provider runtime на Node. Долговечной
является **рабочая сессия**:
`SessionThread`, workspace, files, trace, diff и resume context. Сам
provider process живет между turns, но может быть остановлен после суток без
meaningful runtime steps и позже возрожден в том же workspace.

Целевая форма начинается с **Persistent Runtime**: Node Daemon запускает Codex
provider runtime на ноде, runtime остается живым между turn-ами, а
пользователь может attach/detach к одной рабочей поверхности. Это ближе к
модели T3 Code. При этом Persistent Runtime не означает бессрочный OS process:
runtime должен иметь управляемый lifetime. V01 baseline: если у runtime
больше суток не было новых runtime steps, Node может остановить provider
process, но session thread, workspace state и resume context должны позволять
возродить процесс при возвращении пользователя. Позже Uprava может добавить
stateless/ephemeral strategies, более похожие на Codbash-like resume/launcher
подход или sandboxed task runtime.

Implementation baseline `0.2.26` закрывает Managed Agent Work Loop поверх
Node-owned managed runtime и Core-owned orchestration: Codex app-server остаётся
живым между turns, Managed является default новых Agent sessions на capable
Node, а существующий adapter `codex exec/resume` сохранён как отдельный явный
compatibility mode.

## Provider protocol gate 0.2.20

22 июля 2026 года disposable Rust probe проверил установленный
`codex-cli 0.144.1`. Gate пройден для **experimental Codex app-server v2** через
локальный WebSocket. Это compatibility baseline первого managed adapter, но не
обещание совместимости со всеми будущими Codex versions: Node обязан проверять
provider version and required methods до объявления managed capability.

Probe находится в `tools/codex-app-server-probe`, не импортирует production
Core/Node contracts и не использует Core tables or public API. Bounded scrubbed
wire examples сохранены рядом с tool. Реальный прогон подтвердил:

- `initialize` возвращает provider metadata и `CODEX_HOME`, после чего клиент
  отправляет `initialized`;
- `thread/start` возвращает effective model, approval policy и полный
  `SandboxPolicy` до первого turn;
- два последовательных `turn/start` используют один provider thread and
  process;
- incremental agent deltas и typed command items приходят отдельно;
- server-initiated `item/commandExecution/requestApproval` принимает
  `accept` и `decline`, после чего продолжается тот же turn;
- experimental `item/tool/requestUserInput` является отдельным request type и
  работает в provider Plan mode; Uprava выбирает его per turn через
  `SendTurnRequest.collaboration_mode=plan` и передаёт effective model,
  возвращённую `thread/start` или `thread/resume`;
- `turn/interrupt` завершает active turn со status `interrupted`;
- `thread/unsubscribe`, новый WebSocket client и `thread/resume` сохраняют
  identity без повторной доставки завершённых item notifications;
- после SIGTERM или потери app-server process новый process восстанавливает
  thread по opaque thread id; незавершённый turn после forced stop был
  сохранён как `interrupted`;
- изолированный Uprava-shaped Streamable HTTP MCP fixture обнаруживается через
  `mcpServerStatus/list`; bearer credential передаётся app-server только через
  environment indirection и не появляется в args, report or captured stderr;
- explicit `workspace-write + untrusted` и
  `danger-full-access + never` возвращаются как разные effective policies;
- на проверенной машине idle RSS составил примерно 103–137 MiB, наблюдавшийся
  active RSS 139–159 MiB, SIGTERM shutdown 20–35 ms, forced stop 2–16 ms.
  Числа являются sizing observation, не SLO или hard limit.

### Candidate inventory

| Candidate | Наблюдавшийся contract | Решение |
| --- | --- | --- |
| `codex app-server` | stdio, loopback WebSocket, Unix/remote endpoints; typed bidirectional thread/turn protocol | Выбран v2 over loopback WebSocket |
| remote app-server connection | Тот же protocol поверх reconnectable endpoint | Используется только локально внутри Node trust boundary |
| `codex remote-control` | Управление и pairing app-server daemon, не отдельный session protocol | Не используется первым adapter |
| `codex exec-server` | Experimental stdio/WebSocket execution service без доказанного interactive thread lifecycle | Не выбран |
| `codex exec --json` | One-shot JSONL observation, resume новым process | Остаётся Exec compatibility, не managed transport |

Remote-control/shared daemon не выбран: process-per-attempt лучше изолирует
policy, cancellation, crash and resource accounting. `app-server` запускается
с loopback-only endpoint; Web and Core не получают прямой provider socket.

### Identity and ownership

Протокольный spike подтвердил три разных уровня identity:

```text
SessionThread
  durable Core-owned user conversation and work context

RuntimeSession
  durable Core-owned runtime lineage and selected execution profile

RuntimeAttempt
  one Node-owned app-server process, local endpoint and policy snapshot
```

Provider thread id является opaque resume reference RuntimeSession. Core может
хранить его в bounded provider-specific resume envelope, но не показывает в
обычных logs, prompts or Web payloads. Process id, socket/port, pending JSON-RPC
request ids, bearer values and live transport handles принадлежат только Node
RuntimeAttempt и не сохраняются в Core.

Authentication and `CODEX_HOME` принадлежат credential profile process-а на
Node. Core хранит только non-secret profile reference; путь `CODEX_HOME`, auth
files and token values не входят в effective policy payload or provider args.

Первый adapter использует один app-server process на RuntimeAttempt. Один
RuntimeSession допускает не больше одного active turn. Node владеет provider
connection и reconnect; Web attach/detach происходит через Core и не касается
provider transport. После Node restart создаётся новый RuntimeAttempt и
выполняется explicit `thread/resume`, а не reattach по одному stale PID.

### Ordering, reconnect and recovery limits

WebSocket сохраняет порядок frames одного connection, request/response
correlation использует provider request id. Provider protocol не отдаёт
durable global event sequence или replay cursor. Persisted thread history
намеренно lossy для command/tool items. Поэтому Node присваивает собственную
монотонную attempt sequence каждому normalized event до отправки в Core.

Reconnect на idle thread подтверждён. Transport gap во время active turn не
обещает lossless replay: adapter должен выполнить resume/read reconciliation,
не повторять уже mapped item ids и пометить trace gap typed degraded recovery,
если точный stream восстановить нельзя. Silent reconstruction assistant text
или переход в Exec compatibility запрещены.

`thread/start` без первого turn ещё не создаёт persisted rollout; такой thread
нельзя считать provider-resumable. RuntimeAttempt переходит в durable
resumable state только после получения provider thread identity и успешного
начала первого turn. Это должно быть отражено отдельными `starting`, `ready`
and recovery states, а не одним optimistic start flag.

### Interaction, stop and policy contracts

Provider execution approval и provider user input имеют разные methods,
payloads and responses. Core может переиспользовать UI primitives, но хранит
разные interaction kinds. Provider `decline` продолжает turn, `cancel`
прерывает его; Uprava deny baseline использует `decline`, если человек отдельно
не выбрал cancel.

Interrupt сначала вызывает `turn/interrupt`. Stop закрывает transport и
посылает SIGTERM конкретной process group; после bounded timeout Node применяет
SIGKILL только к тому же RuntimeAttempt. Stop сохраняет opaque thread resume
reference, но не обещает сохранение in-memory state.

Минимальный enforceable policy первого adapter включает provider version,
model/provider, cwd and runtime workspace roots, sandbox policy, approval
policy/reviewer, MCP server summary and bearer-token env reference. Network
posture сохраняется только если присутствует в returned `SandboxPolicy` or
permission profile; неизвестное значение не выводится из requested config.

User input остаётся experimental capability в baseline `0.144.1`. Если version
probe, method inventory, policy echo, approval, interrupt, reconnect/resume or
MCP readiness не подтверждены, Node сообщает typed managed capability reason и
Core отклоняет Managed start. Exec compatibility не включается автоматически.

### Shared contract and persistence foundation 0.2.21

Implementation baseline `0.2.21` закрепляет provider-neutral foundation до
реализации живого Node driver:

- `AgentExecutionProfile` имеет только `managed` и `exec_compatibility`;
  отсутствие profile в API и все migrated sessions сохраняют compatibility
  posture;
- Core рассчитывает до dispatch immutable `EffectiveRuntimePolicy`, хранит
  canonical JSON и `sha256` hash и передаёт snapshot/hash в typed Start/Resume;
- migration 18 добавляет current attempt/recovery projection,
  `runtime_attempts` и `provider_interactions`; process handles, sockets,
  bearer tokens и secret values в Core DB не попадают;
- approval и user input имеют разные interaction kinds и команды, interrupt и
  stop адресуют текущий attempt, если он уже известен;
- Node capabilities разделены на exec, managed, approval, interrupt и resume.
  До поставки managed driver соответствующие managed facts имеют состояние
  unavailable с bounded reason;
- Core требует весь managed capability set и возвращает typed
  `runtime.profile_capability_unavailable`. Создание Exec session или retry с
  другим profile при этом не происходит;
- internal Job runtime явно использует `exec_compatibility`; Task Run contract
  не импортирует Agent execution profile и остаётся OpenSandbox-owned.

### Node-managed Codex runtime 0.2.22

Implementation baseline `0.2.22` материализует data-plane owner:

- `AgentRuntimeDriver` разделяет `CodexManagedDriver` и независимый
  `CodexExecCompatibilityDriver`; Task runtime не импортирует Agent driver;
- `ManagedRuntimeSupervisor` владеет отдельным app-server process и loopback
  WebSocket на RuntimeAttempt, сериализует provider operations и завершает
  process group на stop/shutdown;
- Start/Resume валидирует managed profile, immutable effective policy и hash до
  запуска; provider transport handle, JSON-RPC ids, socket and MCP token не
  попадают в durable state or logs;
- deltas, completed messages, structured activity, approval и user input
  преобразуются в provider-neutral typed events; unknown/oversized payloads
  ограничиваются и отмечаются как protocol drift;
- approval/input decision продолжает тот же active provider turn; duplicate or
  late interaction получает typed terminal conflict;
- cancellation вызывает `turn/interrupt`, а Stop сохраняет opaque thread id как
  resume reference и завершает только process group текущего attempt;
- local durable descriptor хранит attempt/thread identity, policy hash, active
  Uprava turn and terminal reason. После Node restart непроверяемый live handle
  становится `lost/stale`; автоматического перехода в Exec compatibility нет;
- deterministic fake app-server regression покрывает two-turn live thread,
  approval/input continuation and stop. Exec compatibility regression и
  OpenSandbox Task execution остаются неизменными.

Managed capabilities объявляются available только при доступном Codex binary с
распознанной совместимой версией.
Это ещё не default-on gate: Web interaction cards и реальная
acceptance/recovery matrix закрываются этапами 4–5.

### Core orchestration baseline 0.2.23

Implementation baseline `0.2.23` материализует control-plane authority:

- policy preview и session creation используют один resolver; profile,
  immutable policy/hash, Start command и audit фиксируются одной транзакцией;
- явный Exec compatibility selection записывается как unsafe policy audit;
  отсутствие profile выбирает Managed и проходит тот же capability gate;
- provider interaction проходит `requested -> resolving -> approved | denied |
  answered`, либо `expired | cancelled | superseded`; Core записывает decision
  intent вместе с dispatch command, а provider event остаётся единственным
  подтверждением continuation;
- interaction request блокирует active turn/runtime, provider confirmation
  возвращает его в `running`, assistant completion завершает turn, а stale или
  conflicting attempt/event не меняет current projection;
- control `Hello` несёт secret-free actual attempt report с runtime/attempt
  identity, state и policy hash. Core принимает только current attempt,
  восстанавливает пропущенный projection при совпавшей policy и не оживляет
  superseded attempt;
- recovery и interaction lifecycle пишут bounded audit и Prometheus series с
  labels только provider/profile/state, без session id, prompt или workspace
  content.

Это закрывает этап 3; Web work surface закрыт baseline `0.2.24`, rollout и
default-on closure — baseline `0.2.25`, deployment hardening — `0.2.26`.

### Agent Web work surface baseline 0.2.24

Web использует уже зафиксированные Core contracts без собственного durable
runtime state:

- Start Agent оставляет `exec_compatibility` opt-in-slice default, показывает
  доступный Managed как recommended и требует явного acknowledgement перед
  unrestricted compatibility start;
- policy preview показывает target Node/workspace, sandbox, approvals, network
  posture и recovery strategy до команды Start; выбранный profile передаётся в
  create-session request и после создания не меняется UI toggle;
- Agent timeline сохраняет assistant messages, bounded command/tool/file
  activity, typed provider interaction и raw diagnostic fallback как semantic
  blocks, не как ANSI terminal mirror;
- pending approval и user input имеют разные keyboard-accessible cards и
  endpoints; `resolving` блокирует повторную отправку до provider confirmation;
- mode/policy warning и diagnostics постоянно показывают driver/version,
  policy/hash, current attempt, recovery reason/status и last activity;
- lifecycle controls строятся из projected capabilities; managed interrupt не
  показывается для Exec compatibility, Detach не останавливает runtime, а
  Stop сохраняет durable session history.

В baseline `0.2.24` Managed ещё остаётся явным opt-in; смена default происходит
только в следующем stage 5 после real-provider, recovery and controlled-rollout
gate.

### Recovery и default-on closure 0.2.25

Stage 5 меняет только создание новых Agent sessions. Web выбирает Managed при
available `provider.codex.managed`; Create Session API также трактует missing
profile как Managed и возвращает typed capability error на incapable Node без
создания Exec runtime. Existing sessions сохраняют profile migration 18,
internal Jobs продолжают явно выбирать Exec compatibility, а Task Run не
создаёт interactive session.

Node очищает inherited environment provider child process и возвращает только
allowlisted OS/auth/proxy variables плюс отдельный ephemeral MCP token env.
Host-only `make codex-smoke` проверяет safe default policy, sequential Managed
turns, detach/reattach, stop/resume, reload и явный compatibility turn; полный
operator/recovery порядок зафиксирован в
[`managed-agent-runtime.md`](../../runbooks/managed-agent-runtime.md).

Дополнительное уточнение: V01 является **Codex-first**, но не должен
становиться **Codex-only** в продуктовой модели. Первый adapter может быть
практично оптимизирован под Codex, но Core, UI, trace и workflow state должны
говорить на языке `AgentProvider`, `RuntimeSession`, `SessionThread`, `Turn`,
events, approvals, files, diff и trace, а не на языке Codex-specific process
details. Future adapters для OpenCode, Claude Code и других agents должны
добавляться через тот же минимальный launch/resume/control boundary, а не через
новую параллельную модель.

Ключевое уточнение: Run Mode не должен смешивать product contract и runtime
strategy.

```text
Interactive session vs bounded task = product/work contract.
Persistent vs stateless/ephemeral = runtime continuity strategy.
```

В первой версии Uprava делает interactive developer workbench через
Persistent Runtime. Task-like bounded work и stateless/sandboxed execution
остаются архитектурно возможными, но не являются V01 реализацией.

Принятый post-V01 Docker/OpenSandbox baseline для bounded work вынесен в
[`A-013 Task-based Sandbox Runtime`](013-task-based-sandbox-runtime.md). Он
добавляет другую runtime strategy, но не меняет описанное здесь разделение
между product work contract и runtime strategy.

## Vision

### Какую проблему решает механика

Обычный агентский чат плохо подходит для долгой разработки. Пользователь видит
сообщения, но плохо видит реальное окружение: какие файлы открыты или
изменены, какие команды выполнялись, где агент ждет approval, какие проверки
прошли, что изменилось с последнего turn-а и можно ли безопасно продолжать
работу позже.

С другой стороны, "задачный" подход, где каждый запуск агента является
одноразовым process invocation, хорошо подходит для bounded work, но хуже
подходит для совместного проектирования, уточнений, live approvals и
ручного вмешательства.

Uprava должен иметь один Run Mode model, внутри которого можно явно выбрать
runtime strategy и work contract:

- пользователь работает в project/workspace на node;
- Uprava знает, какой runtime strategy используется;
- live work, files, output, diff, trace, approvals и review state видны в UI;
- долговечным является не только process, но и system state вокруг него;
- позже bounded tasks и stateless/sandboxed runs не требуют отдельной
  продуктовой модели.

V01 проверяет тезис: **Persistent Runtime + Node Daemon + Core UI дают
больше контроля и continuity, чем локальный agent chat или launcher поверх
логов**.

### Концептуально как реализуем

Run Mode нужно разложить на несколько слоев:

1. **Work Contract** - как пользователь понимает работу: interactive session,
   bounded task, review run, fix run, research run.
2. **Runtime Strategy** - как живет агентский runtime/process: persistent,
   stateless/ephemeral, sandboxed, external provider or sessionless one-shot.
3. **Provider Execution Driver** - двусторонний managed protocol или one-shot
   `exec/resume` compatibility path.
4. **Effective Execution Policy** - provider sandbox, approval behavior,
   writable scope, credentials, network and unsafe overrides.
5. **Session Thread** - долговечная история диалога, turns, activity, trace и
   review state.
6. **Project Placement** - Core-owned physical binding выбранной Node и
   canonical workspace path; checkout, branch/worktree, env и local
   capabilities являются facts этого Placement.
7. **Runtime Session** - live provider runtime/process, если он сейчас
   запущен и управляется Node Daemon.
8. **Turn** - один пользовательский input и связанный цикл работы агента до
   `idle`, `blocked`, `interrupted`, `completed` или `error`.

В V01 основной вариант:

```text
Run Mode: Persistent Runtime
Work Contract: interactive developer session

start session -> start provider runtime on Node
turn 1        -> send message into the same runtime
turn 2        -> send message into the same runtime
attach        -> subscribe UI to existing runtime/session state
detach        -> UI disconnects, runtime can keep living
24h no steps  -> stop runtime, keep thread/workspace/resume state
return later  -> start and resume runtime in the same workspace
stop          -> explicitly stop runtime
```

Новый turn не должен стартовать новый CLI process с нуля. Он отправляется в
уже открытый runtime через provider adapter/protocol. Session thread, его
`placement_id` и Core-owned `ProjectPlacement` при этом должны переживать
disconnect UI, transient node issues, runtime expiry и runtime recovery
attempts.

### Runtime strategies

#### Persistent Runtime

Node Daemon запускает provider runtime и держит его живым между turns в рамках
простого active runtime window. Persistent здесь означает "живет достаточно
долго для интерактивной работы", а не "живет вечно".

Свойства:

- low-latency continuation;
- streaming output;
- interactive approvals и user-input requests;
- interrupt/stop;
- live runtime status;
- provider-native session state;
- attach/detach from multiple clients through Core;
- managed lifetime with idle expiry;
- better control-plane integration than plain terminal launcher.

Это целевой default для поверхности Agent. До пункта `16` первый Codex adapter
реализует stateless exec/resume compatibility path, а не provider-native live
runtime.

#### Runtime lifetime policy

Для V01 не нужен сложный scheduler с большим набором lease-политик.
Достаточно простого baseline: runtime process является live resource на Node,
а долговечным объектом является session thread вместе с workspace и resume
state.

Рабочая позиция для V01:

- session thread, ProjectPlacement association, files, trace/diff history и
  provider resume cursor/session id долговечны;
- provider process живет только пока active runtime window не истек;
- Node Daemon хранит `last_runtime_step_at`;
- runtime step - это meaningful activity: accepted user turn, provider output,
  tool/command step, approval/user-input request, explicit checkpoint/resume
  event; heartbeat сам по себе не должен продлевать жизнь процесса;
- если runtime не выполняет активный turn и `last_runtime_step_at` старше 24
  часов, Node может остановить provider process и отправить Core событие
  `runtime.expired`;
- attached UI client не делает process бессрочным: UI может показывать, что
  runtime давно idle, и при необходимости предложить resume;
- pending approval/user-input не должен исчезать молча: UI показывает blocked
  state и expiry time; если ответа нет больше суток, Node Daemon может
  cancel/expire request, остановить runtime и записать trace event;
- explicit Stop всегда завершает provider process, но не удаляет session
  thread/workspace state;
- resurrection процесса - нормальная часть lifecycle, а не только аварийный
  recovery path.

Пример:

```text
turn completed
  -> runtime ready
  -> no runtime steps for 24h
  -> Node stops provider process
  -> Core marks runtime expired
  -> user returns later
  -> Node starts provider runtime in the same workspace
  -> Node resumes provider session by resume cursor/session id
  -> Core marks runtime ready
```

"В том же состоянии" здесь означает тот же user-visible state: session thread,
workspace, files, branch/worktree, trace, diff baseline, provider session id
и resume cursor. Uprava не должен обещать сохранение RAM/in-memory state
убитого process. Если provider-native resume невозможен, UI должен честно
показать degraded resume: work surface остается readable, а новый runtime
получает явный resume context из сохраненного thread/workspace state.

Так Uprava сохраняет главный UX Persistent Runtime - несколько turn-ов попадают
в один live process, пока идет работа, - но не превращает Node в накопление
забытых CLI processes.

#### Stateless / Ephemeral Runtime

Позже Uprava может поддержать strategy, где каждый turn, resume action или task
step стартует новый CLI/provider process с тем же `resume_id`, `cwd`, project
context и `placement_id`. Долговечными остаются session thread,
ProjectPlacement, trace и provider history, но не OS process.

Это ближе к Codbash-like подходу: session identity и history существуют в
agent storage/logs, а продолжение может происходить через `codex resume`,
`claude --resume` или similar launch command. Такой подход сохраняется как
явный **Exec compatibility mode** для Agent и как естественный one-shot driver
для Jobs/Tasks. В Agent он слабее для reliable streaming, approvals, interrupts
и structured trace, если Uprava не владеет runtime protocol. Текущий Agent
compatibility profile использует `--dangerously-bypass-approvals-and-sandbox`;
UI не должен включать его скрыто или выдавать normalized approval events за
настоящий approval continuation.

#### Sandboxed Runtime

Sandboxed runtime - частный случай stateless или managed runtime strategy, где
workspace создается под bounded work: отдельная папка, branch/worktree,
container, microVM или external sandbox provider. Это естественная база для
future bounded tasks, но V01 не должен начинаться с нее.

#### Future hybrid composition

Managed Agent позже может получить tool для запуска bounded sandboxed runs, но
это не определяет основной Agent work contract и не входит в exit criteria
`Managed Agent Work Loop`. Task Run остаётся самостоятельной сущностью со своей
изоляцией, evidence и review contract.

### Work contracts

#### Interactive Session Contract

Пользователь работает с живой рабочей поверхностью. Цель может уточняться по
ходу. Важны attach/detach, visible environment, approvals, intervention и
return.

Это V01 contract.

#### Bounded Task Contract

Пользователь или agent задает ограниченную работу: goal, scope, context
package, stop condition, expected evidence и review-ready output. Runtime
strategy чаще будет stateless/sandboxed, но это не обязательно.

Это later contract.

### Пользовательские сценарии

#### 1. Start live work in project

Пользователь выбирает Project и существующий Placement, нажимает start. Core
проверяет их association и создаёт session thread со ссылкой на `placement_id`.
Node Daemon запускает Codex runtime по canonical path этого Placement,
сообщает Core status `starting -> ready`, UI показывает chat, terminal/output,
files, diff и trace panels.

#### 2. Continue the same agent

Пользователь отправляет второе сообщение. Uprava не создает новую задачу и не
запускает новый агентский процесс. Он отправляет turn в тот же runtime
session. Agent сохраняет conversational/runtime context, а Uprava привязывает
новые events, output и file changes к новому turn.

#### 3. Detach and return later

Пользователь закрывает браузер или открывает work surface с телефона. Runtime
может продолжать жить на Node, пока `last_runtime_step_at` не ушел за 24-hour
expiry window. Новый клиент подключается к Core, получает snapshot of session
thread, latest status, diff, trace и active runtime state, затем подписывается
на live events через Core. Если runtime уже auto-stopped по 24h no-steps
policy, UI показывает stopped/expired runtime state и предлагает resume.

#### 4. Runtime expired or died but work remains

Если runtime остановился после 24 часов без runtime steps, Node Daemon
перезапустился, provider runtime упал или нода ушла в sleep, Core не теряет
session thread. UI показывает, что live runtime отсутствует, expired или stale.
Если у provider есть resume cursor/session id, Node Daemon может восстановить
runtime в том же workspace. Во время восстановления UI показывает `resuming`:
есть задержка на старт process, handshake с provider и подхват session state.
Если восстановление невозможно, work surface остается readable: chat, trace,
files и diff доступны как historical state.

#### 5. Agent asks for a decision

Когда runtime запрашивает approval, file-change approval или structured
user-input, Node Daemon нормализует request и стримит его в Core. UI
показывает blocking state и action. Ответ пользователя возвращается в тот же
live runtime.

#### 6. Future optional composition: task-like launch

Позже пользователь сможет сказать "сделай bounded run из этой session":
зафиксировать goal/scope, создать isolated workspace, запустить agent step и
вернуть review-ready output обратно в общую рабочую поверхность. Это не часть
V01, но Run Mode должен не закрывать такой путь.

### Agent-facing сценарии

Для агента Run Mode должен выглядеть как стабильная рабочая среда, а не как
невидимый локальный shell:

- агент знает selected project/workspace и granted scope;
- tool access проходит через provider runtime, Node Daemon или registered tools;
- agent-visible context может включать session summary, recent turns, files,
  constraints, pending approvals и current workspace state;
- runtime не получает long-term node credentials;
- важные actions становятся events, artifacts или trace entries, а не только
  текстом в assistant response;
- если runtime восстановлен после перезапуска, агент получает достаточно
  контекста, чтобы продолжить без притворства, что process никогда не падал;
- если runtime был остановлен по 24h no-steps policy, агент должен получить
  явный resume context вместо неявной иллюзии непрерывного процесса;
- future task contract должен быть agent-readable: goal, scope, stop condition,
  expected evidence и review gate.

### First release vs later

#### V01 design contract

Для Distributed Agent Control Panel нужно:

- Core session/thread registry;
- Node Daemon controlled Codex runtime;
- Persistent Runtime as default runtime strategy;
- runtime lifetime policy на базе `last_runtime_step_at` и 24h no-steps
  expiry;
- interactive session as default work contract;
- one existing `ProjectPlacement` reference per session;
- thread with turns и messages;
- live event stream through Core;
- status: `starting`, `ready`, `running`, `blocked`, `expired`, `resuming`,
  `stale`, `stopped`, `error`;
- user input path for chat turns;
- approval/user-input request path;
- interrupt и stop;
- chat/session view;
- node/project/session placement visible in UI;
- basic message and runtime event history;
- basic offline/stale/resource warning states;
- basic trace и event log;
- runtime started/stopped/expired events;
- attach/detach semantics;
- durable provider resume cursor/session id;
- runtime resurrection attempt when provider supports resume cursor.

V01 does not need:

- bounded task execution contract;
- general stateless sandbox runs;
- durable workflow engine;
- CI webhook wakeups;
- PR/MR automation;
- multi-agent orchestration;
- provider-neutral feature parity across every CLI agent.

V01 **does** need a minimal Provider Adapter boundary. Иначе Codex-specific
launch, resume, approval и event semantics начнут протекать в Core API, UI и
trace model, а будущие OpenCode/Claude Code adapters придется добавлять через
ломку доменной модели.

Фактический V01 adapter поставил долговечный `SessionThread`, Core/Node
lifecycle, events и provider resume reference, но реализовал turn через новый
`codex exec/resume` process. Это compatibility approximation исходного
контракта, а не доказательство живого provider-native runtime. Разрыв закрывает
пункт `16 Managed Agent Work Loop`.

#### Later

Post-V01 versions добавляют или уже добавили:

- file browser and read-only workspace inspector;
- terminal/output view;
- basic diff per turn or since baseline;
- provider-native managed runtime как основной Agent driver;
- явный stateless/ephemeral exec compatibility mode;
- richer lease policies, configurable TTLs, quotas и per-project runtime
  budgets;
- sandboxed runtime for bounded work;
- task contract with context package и review contract;
- опциональный Agent tool для bounded task delegation;
- richer checkpoints и rollback;
- multiple concurrent runtimes per project;
- session handoff between providers;
- mobile-first unblock/review flows;
- stronger causality graph from result to prompt/tool/command/file change.

## Architecture

### Target Agent implementation: Provider-native Managed Interactive Session

Целевой основной режим Uprava:

- runtime strategy: **Persistent Runtime**;
- work contract: **Interactive Session**;
- provider: **Codex**;
- execution location: **Node Daemon**;
- durable control plane: **Core**;
- first client: **Web Control Panel**.

Рабочее название внутри архитектуры: **Managed Interactive Session**.
Это живая рабочая сессия в проекте, где пользовательский thread долговечен, а
provider process живет на Node между turns, пока active runtime window не
истек.

Инварианты первого режима:

- один `SessionThread` открывается пользователем как рабочая поверхность;
- один существующий `ProjectPlacement` задаёт для session физическую Node и
  canonical workspace path; `Workspace` остаётся user-facing surface над этим
  Placement и не создаёт вторую persisted binding identity;
- один active `RuntimeSession` держит live Codex process для этой session;
- новые turns идут в тот же process, пока он жив;
- отсутствие runtime steps больше 24 часов позволяет Node остановить process;
- возврат пользователя запускает resurrection через provider resume
  cursor/session id в том же workspace;
- если provider-native resume невозможен, UI показывает degraded resume, а не
  притворяется, что live state полностью восстановлен.

Provider-native здесь означает семантические возможности живого Codex runtime:
двусторонний event/control protocol, approvals, questions, tool/command
activity, streaming, interrupt and reconnect. Это не означает встраивание
Codex TUI или эмуляцию terminal UI в Web Control Panel.

Текущий compatibility baseline сохраняет тот же `SessionThread` и
`RuntimeSession` control plane, но под каждым turn запускает
`codex exec/resume`. Он остаётся поддерживаемым явным fallback после появления
managed driver.

### Execution profiles by product surface

| Surface | Driver | Provider policy | Human interaction |
| --- | --- | --- | --- |
| Agent / Managed | Provider-native managed protocol | Safe sandbox; effective policy видна | Real approvals, questions, interrupt |
| Agent / Exec compatibility | `codex exec/resume` | `--dangerously-bypass-approvals-and-sandbox` | Нет настоящего approval continuation |
| Tasks | One-shot `codex exec` inside OpenSandbox | Provider unrestricted; внешний sandbox является boundary | Нет интерактивной сессии |
| Jobs target | Sessionless one-shot `codex exec` | Sandbox enabled; non-interactive approval policy | Запрещённое действие возвращается модели как failure |

Профиль является частью work contract и effective policy snapshot. Node не
должен незаметно переключать Agent из managed safe profile в unrestricted exec
fallback. Tasks и Jobs не создают пользовательскую Agent session только ради
переиспользования provider adapter.

### Responsibility boundaries

#### Core

Core является durable control plane.

Core отвечает за:

- Project, Node, `ProjectPlacement`, session thread, turn и runtime identities;
- хранение thread messages, turns, status projection и event log;
- routing команд от клиентов к нужной Node;
- подписки клиентов на session events;
- отображение последнего известного runtime state;
- хранение provider resume reference, если это безопасно и достаточно для
  восстановления;
- review-facing state: approvals, trace, diff metadata, artifacts и status.

Core не должен:

- запускать provider process напрямую;
- иметь прямой доступ к workspace files на Node мимо Node Daemon;
- считать provider process долговечным источником истины;
- полагаться на то, что UI подключен во время работы агента.

#### Node Daemon

Node Daemon является data plane и runtime owner.

Node Daemon отвечает за:

- проверку, что workspace доступен на этой Node;
- запуск, мониторинг, interrupt и stop provider process;
- provider adapter lifecycle;
- filesystem observation, local diff и command/output capture;
- нормализацию provider events в Uprava events;
- хранение local runtime handle/process metadata;
- `last_runtime_step_at` и 24h no-steps expiry;
- resurrection runtime в том же workspace через provider resume cursor/session
  id;
- graceful degradation, если provider-native resume невозможен.

Node Daemon не должен:

- владеть продуктовой историей session thread;
- принимать решения review/approval без Core/user path;
- превращать local terminal logs в единственный source of truth;
- держать забытые provider processes бесконечно.

#### Provider Adapter

Provider Adapter изолирует конкретный способ общения с provider runtime.

Первый production adapter - Codex. Future adapters могут быть OpenCode, Claude
Code или другие provider runtimes. Они могут отличаться launch command,
session identity, resume mechanism, output format, approval semantics и tool
permissions, но наружу должны отдавать нормализованные Uprava events и runtime
lifecycle.

Adapter отвечает за:

- старт runtime process в нужном `cwd`;
- создание или resume provider session;
- отправку user turns в live runtime;
- стрим provider output/events;
- interrupt/stop;
- extraction provider session id/resume cursor;
- mapping provider-specific requests на Uprava approval/user-input requests.

V01 может иметь один production adapter для Codex. Provider-neutral API
нужно держать минимальным и практичным, а не пытаться сразу покрыть все CLI
agents.

#### Web Control Panel

UI является клиентом Core, а не прямым клиентом Node.

UI отвечает за:

- session work surface: chat, output, files, diff, trace и approvals;
- attach/detach к существующей session thread;
- отображение `expired`, `resuming`, `stale`, `blocked` и `running` states;
- отправку turns, approvals, interrupts и stop через Core;
- явное объяснение пользователю, когда live runtime отсутствует и нужен resume.

### Domain objects

V01 должен различать долговечные и live объекты.

#### Project

Product-level контейнер. В V01 это может быть тонкая запись: name, repo или
folder reference, default node/workspace preferences.

#### Project Placement

Core-owned aggregate и единственная physical binding identity для конкретной
Node и canonical workspace path. Один Project может владеть Placements на
нескольких Nodes; session ссылается на уже существующий Placement.

Минимальные поля:

```text
placement_id
project_id
node_id
canonical_workspace_path
git_repository_url optional
git_branch optional
baseline_ref optional
created_at
```

ProjectPlacement долговечен. Если runtime process умер, Placement остаётся.
Node не создаёт его identity и хранит только operational placement/path context
и local facts.

#### Session Thread

Долговечный пользовательский thread работы.

Минимальные поля:

```text
session_thread_id
project_id
placement_id
run_mode = process_backed_interactive_session
runtime_strategy = persistent_runtime
work_contract = interactive_session
status
created_at
updated_at
last_turn_id optional
active_runtime_session_id optional
provider_resume_ref optional
```

Session Thread является главным объектом, который пользователь открывает в UI.
Он не равен OS process.

#### Runtime Session

Live-or-recoverable runtime на Node.

Минимальные поля:

```text
runtime_session_id
session_thread_id
node_id
provider = codex
provider_session_id optional
provider_resume_cursor optional
process_state
started_at
last_runtime_step_at
expired_at optional
stopped_at optional
exit_reason optional
```

Core хранит projection и identifiers. Node хранит process-local handle: pid,
transport connection, temporary sockets, local adapter state. Эти process-local
details не должны становиться Core contract.

#### Turn

Один пользовательский input и связанный цикл agent work.

Минимальные поля:

```text
turn_id
session_thread_id
seq
user_message_id
status
started_at
completed_at optional
blocked_request_id optional
```

Turn может завершиться `completed`, `blocked`, `interrupted`, `error` или
`expired`. Runtime Session может пережить много turns.

#### Runtime Step

Meaningful activity внутри runtime.

Runtime step обновляет `last_runtime_step_at`. Heartbeat, ping и passive UI
attach не являются runtime step.

Runtime step kinds для V01:

- accepted user turn;
- provider output chunk или assistant message;
- tool/command started;
- tool/command output;
- tool/command completed;
- file change observed;
- approval/user-input requested;
- approval/user-input resolved;
- checkpoint/resume event;
- runtime started/resumed/stopped/expired.

### State ownership

Core persisted state:

- projects;
- nodes и node health projection;
- ProjectPlacement aggregates and identities;
- session threads;
- messages и turns;
- runtime session projection;
- provider resume reference, when available;
- event log;
- approvals и user-input requests;
- artifact metadata;
- diff/checkpoint metadata enough for review UI.

Node local state:

- workspace files;
- provider process handle;
- provider local storage/logs;
- runtime transport details;
- command output buffers before they are streamed to Core;
- local filesystem watcher state;
- local diff computation cache.

Provider state:

- provider-native conversation/runtime state;
- provider-native resume id/session id;
- provider-specific logs or storage.

Core должен считать, что состояние Node и provider может исчезнуть. Product
остается readable из Core state, а work можно возродить, когда Node/provider
может дать достаточно resume context.

### Runtime state machine

Runtime Session status - это projection, а не вся правда о provider process.

```text
absent
  -> starting
  -> ready
  -> running
  -> blocked
  -> running
  -> ready
  -> expired
  -> resuming
  -> ready
  -> stopped
```

Failure branches:

```text
starting -> error
running  -> error
running  -> stale
blocked  -> expired
resuming -> error
stale    -> resuming
```

Status meanings:

| Status | Meaning |
| --- | --- |
| `starting` | Node запускает provider process. |
| `ready` | Runtime жив и может принять новый turn. |
| `running` | Runtime выполняет turn или tool/command step. |
| `blocked` | Runtime ждет approval или structured user input. |
| `expired` | Process остановлен no-steps policy, durable state сохранен. |
| `resuming` | Node запускает process и восстанавливает provider session. |
| `stale` | Core потерял свежий контакт с Node/runtime и показывает last known state. |
| `stopped` | User или system явно остановил runtime. |
| `error` | Runtime упал и требует user-visible recovery или restart. |

`blocked` не равен `running`: agent не может продолжить, пока human или policy
не ответит. `stale` не равен `expired`: `stale` означает, что Core не уверен в
состоянии, потому что contact с Node/runtime не свежий; `expired` означает, что
Node намеренно остановила process и сообщила об этом.

### Lifecycle

#### 1. Create session

1. UI отправляет `CreateSessionThread(project_id, placement_id)` для
   существующего `ProjectPlacement`.
2. Core проверяет access и что Placement принадлежит выбранному Project, затем
   создаёт `SessionThread` со ссылкой на `placement_id`.
3. Core не создаёт вторую binding entity: physical Node и canonical workspace
   path уже authoritative в `ProjectPlacement`.
4. Core отправляет `StartRuntime(session_thread_id, placement_id)` на Node,
   владеющую Placement.
5. UI подписывается на session event stream через Core.

#### 2. Start runtime

1. Node проверяет local workspace path.
2. Node запускает Codex provider runtime в `workspace_path`.
3. Adapter создает или получает provider session id/resume cursor.
4. Node отправляет `runtime.starting`, затем `runtime.ready`.
5. Core сохраняет runtime projection и provider resume reference.

Если provider startup падает, Node отправляет `runtime.error` с user-safe
reason. Core сохраняет session thread readable.

#### 3. Send turn

1. UI отправляет message в Core.
2. Core создает `Message` и `Turn`.
3. Если runtime находится в `ready`, Core отправляет `SendTurn` на Node.
4. Если runtime находится в `expired`, Core сначала отправляет `ResumeRuntime`,
   затем `SendTurn`.
5. Node отправляет user input в provider adapter.
6. Node нормализует provider output в events и стримит их в Core.
7. Core обновляет thread, turn, runtime projection и review-facing views.

Каждый accepted user turn обновляет `last_runtime_step_at`.

#### 4. Stream work

Во время running turn Node отправляет event stream:

```text
turn.started
provider.output.delta
command.started
command.output.delta
command.completed
files.changed
diff.updated
approval.requested
turn.completed
```

V01 не требует идеальной trace semantics. Нужно достаточно events, чтобы
ответить на вопросы:

- что попросил user;
- что сделал agent;
- какие files изменились;
- какие commands запускались;
- где agent ждал human;
- какой result готов к review.

#### 5. Block for approval/user input

Если provider запрашивает approval или structured user input:

1. Node отправляет `approval.requested` или `user_input.requested`.
2. Core сохраняет request и помечает runtime как `blocked`.
3. UI показывает blocking control.
4. User отвечает через Core.
5. Core отправляет answer на Node.
6. Node передает answer в provider adapter.
7. Runtime возвращается в `running` или `ready`.

Blocked requests тоже обновляют `last_runtime_step_at`. Если ответа нет 24
часа, Node может expire request и остановить runtime. Core сохраняет trace
event, чтобы user видел, почему work остановилась.

#### 6. Detach and attach

Detach не является runtime operation. Это только означает, что UI client
отписался.

Attach flow:

1. UI открывает существующий `session_thread_id`.
2. Core возвращает snapshot: messages, turns, runtime status, latest diff, files
   summary, pending approvals и artifacts.
3. UI подписывается на live events.
4. Если runtime находится в `expired`, UI может предложить Resume.
5. Если runtime находится в `stale`, UI может показать reconnect/resume action
   после проверки Node health.

Attached clients не держат runtime живым навсегда. No-steps policy основана на
meaningful runtime activity, а не на присутствии браузера.

#### 7. Expire after no steps

Node периодически проверяет live runtimes:

```text
if runtime.status not in [running, starting, resuming]
and now - last_runtime_step_at > 24h:
  stop provider process
  emit runtime.expired
```

Это намеренно простой baseline для V01. Later versions могут добавить
per-node TTLs, budgets, pinning, quotas и resource-pressure eviction.

#### 8. Resurrect runtime

Resurrection - нормальный return path после expiry.

1. User открывает expired session или отправляет новый turn.
2. Core отправляет `ResumeRuntime(session_thread_id, placement_id)` на Node.
3. Node resolve-ит `canonical_workspace_path` из operational Placement context
   и запускает там provider runtime.
4. Adapter восстанавливает provider session через `provider_resume_cursor` или
   `provider_session_id`.
5. Node отправляет `runtime.resuming`, затем `runtime.ready`.
6. Core обновляет projection, и UI может send/continue turn.

Если provider-native resume не сработал, Node отправляет
`runtime.resume_failed`. Core может предложить degraded resume: запустить новый
runtime в том же workspace и передать resume context package из session
summary, recent turns, open files, diff и constraints. UI должен явно показать,
что provider-native continuity не восстановлена.

#### 9. Stop runtime

Explicit Stop:

- останавливает provider process;
- помечает runtime как `stopped`;
- не удаляет session thread;
- не удаляет workspace files;
- записывает `runtime.stopped` with actor и reason.

После explicit Stop можно все еще предлагать Resume, если provider resume state
и workspace доступны. UI должен отличать "stopped by user" от "expired after no
steps".

### Core to Node command contract

V01 command set:

```text
StartRuntime {
  session_thread_id
  runtime_session_id
  placement_id
  provider = codex
}

ResumeRuntime {
  session_thread_id
  runtime_session_id
  placement_id
  provider_resume_ref
  resume_context optional
}

SendTurn {
  session_thread_id
  runtime_session_id
  turn_id
  message
}

SubmitApproval {
  request_id
  decision
  payload optional
}

InterruptRuntime {
  runtime_session_id
  reason
}

StopRuntime {
  runtime_session_id
  reason
}
```

Commands должны быть idempotent там, где это практично. Core должен передавать
ids, созданные на control-plane side, чтобы retry не создавал duplicate turns
или runtimes.

### Node to Core event contract

V01 events должны быть append-only и ordered per runtime session.

Minimum envelope:

```text
event_id
session_thread_id
runtime_session_id optional
turn_id optional
seq
kind
happened_at
node_id
payload
```

Minimum event kinds:

```text
runtime.starting
runtime.ready
runtime.running
runtime.blocked
runtime.resuming
runtime.expired
runtime.stopped
runtime.error
turn.started
turn.completed
turn.interrupted
turn.error
provider.output.delta
provider.message.completed
command.started
command.output.delta
command.completed
files.changed
diff.updated
approval.requested
approval.resolved
approval.expired
user_input.requested
user_input.resolved
```

Event log не является UI layout. UI строит projections из events: chat
messages, output panel, diff panel, trace panel, approval controls и status.

### Provider adapter contract

Codex adapter должен открыть небольшой internal interface:

```text
start(request) -> RuntimeHandle
resume(request) -> RuntimeHandle
send_turn(handle, turn_input) -> event stream
submit_approval(handle, request_id, decision)
interrupt(handle)
stop(handle)
snapshot(handle) optional
```

Adapter output является provider-specific на edge и нормализуется Node до того,
как Core его увидит. Если Codex дает structured app-server protocol, Node
должна использовать его. Если provider поддерживает только CLI resume, это
относится к later stateless/ephemeral strategy или degraded fallback.

### Diff and workspace observation

В V01 diff не требует full checkpoint/rollback.

Минимум:

- record baseline when session starts;
- compute changed files since baseline;
- optionally compute changed files per turn using filesystem timestamps и git
  diff snapshots;
- show current working tree diff in UI;
- attach file-change events to the active turn when possible.

Если exact attribution неясен, UI должен выбирать честную формулировку:
"changed during this session", а не делать вид, что каждая line связана с
точным agent action.

### Storage implications

Core storage должен поддерживать:

- session thread list and detail;
- latest runtime status projection;
- append-only event log;
- turns and messages;
- approval requests;
- provider resume reference;
- ProjectPlacement reference and association;
- artifact/diff metadata.

Node storage должен поддерживать:

- mapping `runtime_session_id -> local process handle`;
- local runtime metadata for restart while Node process is alive;
- workspace path validation;
- provider local state location;
- local logs/output buffering until events reach Core.

Если Node перезапускается, она должна выполнить reconcile:

1. report live provider processes, которые она еще может identify;
2. mark unknown previous runtimes as `stale` или `expired`;
3. allow Core to request resurrection для selected session.

### Permissions and safety

Controlled-development V01 safety posture может быть простой and explicit.
Формальная per-tool permission/security model остается post-V01 work.

- runtime scoped to one `ProjectPlacement` through `placement_id`;
- provider process runs with Node-local permissions, not Core database access;
- Core routes approvals и user-input decisions;
- dangerous tool permissions become visible approval requests when provider
  supports it;
- provider credentials should not be copied into browser state.

Agent не должен получать broad Uprava admin credentials только потому, что он
запущен внутри project session.

### Failure modes

| Failure | V01 behavior |
| --- | --- |
| Provider process exits | Node отправляет `runtime.error` или `runtime.expired` с reason; Core сохраняет thread readable. |
| Node disconnects | Core помечает runtime как `stale`; UI показывает last known state и reconnect/resume path. |
| Core restarts | Core загружает durable state; Node переподключается и отправляет runtime snapshot. |
| Resume cursor missing | UI предлагает degraded resume из saved thread/workspace context. |
| Workspace path missing | Runtime не может resume; UI показывает workspace error, historical thread остается readable. |
| Approval expires | Request становится expired, runtime может остановиться, trace фиксирует причину. |
| Event delivery duplicates | Core дедуплицирует по `event_id` и per-runtime `seq`. |

### V01 implementation checklist

- Core models: `ProjectPlacement`, `SessionThread`, `RuntimeSession`, `Turn`,
  `RuntimeEvent` и approval requests; `Workspace` является только user-facing
  surface над Placement.
- Core API: create/open session по существующему `placement_id` с validation
  Project/Placement association, send turn, approve, interrupt, stop и resume.
- `StartRuntime` и `ResumeRuntime` получают `placement_id`; Node resolve-ит
  authoritative canonical workspace path из Placement context, не из второй
  binding entity.
- Core event subscription endpoint для session thread updates.
- Node command handler для start/resume/send/approve/interrupt/stop.
- Codex provider adapter находится за Provider Adapter boundary. Managed
  profile владеет живым app-server process/session на Node; CLI `exec/resume`
  сохранён только как явный compatibility profile без silent fallback.
- Persist provider resume cursor/session id when available.
- Track `last_runtime_step_at` from meaningful runtime events.
- 24h no-steps expiry scheduler в Core создаёт system `StopRuntime`; Node
  подтверждает terminal teardown, после чего Core отзывает MCP lease и
  закрывает pending interactions.
- Runtime resurrection path для expired sessions.
- UI states: `ready`, `running`, `blocked`, `expired`, `resuming`, `stale`,
  `stopped` и `error`.
- Basic files/diff/output/trace panels from normalized events.

### Remaining architecture questions

- How much provider raw event data should Core persist for debugging, and how
  much should be normalized only?
- Do we need per-turn diff attribution in V01, or is session-level diff
  enough for the first implementation?
- Should `provider_resume_ref` live fully in Core, or should Core store only an
  opaque Node-owned reference?
- What is the exact degraded resume context package when provider-native resume
  is unavailable?
