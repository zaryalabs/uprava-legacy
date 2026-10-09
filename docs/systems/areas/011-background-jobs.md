# Background Jobs и scheduled agent runs

Статус: `implemented-0.2.5; workspace-ui-0.2.6`

## Vision

### Проблема и продуктовая модель

Пользователю нужна простая долговечная форма unattended agent work: запустить
Codex ночью или параллельно с работой над другим проектом, вернуться позже,
увидеть результат и понять, что произошло. Для этого не нужен visual workflow
builder, бессмертный agent process или заранее формализованный pipeline.

Основная сущность называется **Job**, а не Worker. Job описывает работу:
placement workspace, prompt/task description, provider launch parameters и
опциональное расписание. Agent/provider исполняет Job, но не является самим
Job. Каждый фактический запуск является отдельным наблюдаемым **Job Run**.

Первый срез остаётся prompt-first: ожидаемое поведение, проверки и результат в
основном задаются естественным языком. Новые формальные ограничения и policy
добавляются только по подтверждённой необходимости.

### Основные сценарии

- создать paused Job для конкретного placement workspace;
- выполнить manual test run, изучить summary и output;
- включить interval, daily или weekly schedule в IANA timezone;
- запустить Job вручную вне расписания;
- вернуться к history и открыть конкретный run;
- увидеть failure/skipped outcome и решить, продолжать ли расписание;
- force-start chat или Job Run, несмотря на предупреждение о provider quota.

### Граница доверия

Provider-native managed Agent runtime из пункта очереди
[`16`](../../product/feature-queue.md#16-managed-agent-work-loop) не является
prerequisite: Job — unattended one-shot work contract, а не долговечная
интерактивная session.

Текущий `0.2.5` controlled-deployment baseline переиспользует unrestricted
Agent exec path, полагается на отдельного OS user и/или VM и принимает этот
риск. Целевой Job profile должен запускать sessionless `codex exec` с provider
sandbox enabled и non-interactive approval policy. UI и docs не должны выдавать
ни текущую, ни целевую модель за hostile-workload isolation.

### Scope первого среза

Входит: один placement на Job; только существующий placement workspace;
paused-by-default creation и manual test-before-enable; manual, interval, daily
и weekly starts; explicit IANA timezone; run history, summary и доступный
provider output/logs; default overlap `skip`; stop-on-error по умолчанию с
opt-out; общая quota admission для chats и Jobs с force override.

Не входит: Git worktree или isolated workspace/runtime; arbitrary cron,
event/webhook triggers и backfill; multi-step pipelines, workflow canvas,
PR/review loops, сложные budgets и автоматическая оценка качества результата.

## Architecture

### Минимальные сущности

`Job` хранит identity, display name, placement, prompt/task description,
provider parameters, enabled state, schedule, overlap policy и
continue-after-error flag. Отдельная система immutable revisions не обязательна
в первом срезе; каждый Job Run сохраняет snapshot эффективной конфигурации,
чтобы история оставалась объяснимой после edit Job.

`JobRun` хранит Job reference, configuration snapshot, trigger kind, timestamps,
state, provider execution reference, effective policy snapshot, summary,
failure и refs на events/evidence.
Отдельная persisted TriggerOccurrence не обязательна: skipped occurrence может
быть компактным terminal Job Run без provider start.

### Lifecycle и scheduling

Минимальные состояния Job Run:

```text
queued -> starting -> running -> succeeded
                     |       -> failed
                     |       -> cancelled
                     |       -> timed_out
                     -> skipped
```

`skipped` используется для штатного non-start, например overlap. Ошибка provider
start или execution даёт `failed`. Terminal reason всегда typed и видим.

Core владеет durable расписанием, atomic claim и recovery после рестарта.
Calendar schedule вычисляется из local rule плюс IANA timezone, сохранённый due
time является UTC instant. Default `continue_after_error = false`: failed run
или start error приостанавливает следующие automatic starts. Пользователь может
resume schedule, сделать manual run или заранее включить
`continue_after_error = true`. Overlap `skipped` не приостанавливает schedule.

### Provider quota admission

Quota awareness является общей provider capability. Перед новым interactive
chat/session start и Job Run Core получает последний достоверный Codex usage
snapshot, если адаптер умеет это сделать.

- `remaining <= 5%` пятичасового или недельного окна: typed rejection;
- explicit `force = true`: запуск разрешён, override фиксируется в audit/event;
- свежие данные недоступны: quota state `unknown`, запуск не блокируется;
- usage нельзя угадывать из косвенных логов без надёжного provider contract.

Реализация сначала исследует, предоставляет ли установленный Codex CLI
стабильный machine-readable источник обоих limits. Capability и unavailable
reason должны быть наблюдаемыми.

### Run observation и UX

Job list показывает enabled/schedule state, next start, last outcome и attention
marker. Job detail показывает configuration и run history. Run detail начинается
с outcome и summary, затем показывает timestamps, trigger, effective config,
provider execution/policy, failure reason и доступный output/log stream. Raw output
остаётся fallback; summary не заменяет evidence.

Summary берётся из финального provider result. Если структурированного summary
нет, UI показывает terminal assistant output или отсутствие summary, не запуская
скрытый второй agent call.

В Web baseline `0.2.6` Jobs являются placement/workspace-scoped surface рядом с
Agent и Workbench. List, create, detail и nested run history всегда используют
placement из canonical workspace route; create form не предлагает второй выбор
workspace. При этом Core `/jobs` остаётся глобальным read endpoint, а Web
фильтрует его по `project_placement_id`. Это UI/navigation boundary, не изменение
authority, scheduler или persisted Job model.

### Provider execution contract

Целевая модель не создаёт `SessionThread` или интерактивный `RuntimeSession` для
Job Run. Core создаёт долговечный `JobRun`, Node запускает отдельный one-shot
provider execution на target placement, а correlation связывает run с events,
output и workspace evidence.

Default Codex profile:

```text
driver: codex exec
sandbox: workspace-write
approval: never / non-interactive
```

Job не должен зависать в ожидании пользователя. Запрещённое sandbox policy
действие возвращается provider-у как execution failure; если агент не может
завершить работу в доступном scope, `JobRun` получает typed failure. Более
строгий read-only profile допустим. Dangerous bypass не является default и
может появиться только как явный audited unsafe override.

Поскольку Job и Agent могут писать в один canonical placement workspace,
sessionless execution требует отдельного workspace concurrency guard: lease,
conflict rejection или эквивалентной наблюдаемой политики. Managed Agent
session не должна использоваться как неявный locking primitive.

### Минимальная проверка реализации

- рестарт Core не дублирует claimed start;
- overlap создаёт видимый `skipped` outcome;
- failed run при default policy ставит schedule на pause;
- continue-after-error оставляет schedule активным;
- manual run работает при paused schedule;
- history сохраняет effective configuration snapshot;
- run сохраняет effective provider policy snapshot;
- unattended run не остаётся навсегда blocked на approval;
- quota threshold одинаково блокирует chat и Job;
- force override оставляет audit evidence;
- quota `unknown` не блокирует запуск;
- UI показывает summary/output и typed terminal reason.

## Реализовано в 0.2.5

Core теперь хранит paused-by-default Jobs и отдельные Job Runs для каждого
occurrence, продвигает interval/daily/weekly schedules из явного IANA timezone,
атомарно claim-ит due occurrences, сохраняет overlap skips и создаёт отдельную
обычную managed session для каждого provider launch. Конфигурация run
сохраняется snapshot до запуска; driver связывает runtime/turn completion с
summary и typed terminal state. Failed start и turn ставят automatic schedule
на паузу, если не включён `continue_after_error`.

Web Control Panel показывает создание Job, enable/pause и manual run, run
history, quota override, редактирование конфигурации, terminal reasons,
snapshots и ссылки на обычную session evidence surface. Codex CLI `0.144.1` не
предоставляет стабильного machine-readable источника пятичасового и недельного
quota, поэтому реализованная provider capability честно сообщает `unknown` и
не блокирует starts. Core всё равно применяет общий порог 5%, когда доступен
свежий надёжный adapter snapshot, а force overrides пишет в security audit log.

Создание обычной session на каждый Job Run и unrestricted provider flags
являются сохранённым implementation baseline, а не целевым Job contract.
Переход на sessionless sandboxed exec выполняется отдельным follow-up и не
блокирует пункт `16`, сфокусированный на поверхности Agent.
