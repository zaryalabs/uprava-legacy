# A-013 Task-based Sandbox Runtime

Статус: `implemented-partial`

Implementation baseline: `0.2.19` (runtime mechanics; authorization deferred).

Этот документ фиксирует лаконичную архитектуру пункта `15` Feature Queue.
Первый implementation slice использует Docker через отдельный OpenSandbox
service. Решение намеренно рассчитано на local/self-hosted controlled
deployment и не пытается сразу построить multi-tenant cloud sandbox platform.

## Короткое решение

- Изоляция первого среза — обычные Docker containers. Kubernetes, microVM,
  Firecracker, gVisor, Kata and external sandbox providers не входят в baseline.
- Lifecycle и command orchestration не реализуются внутри Uprava с нуля.
  [OpenSandbox](https://github.com/opensandbox-group/OpenSandbox) запускается как
  отдельный pinned service рядом с Core/Web/ToolHive и управляет Docker через
  свой HTTP/OpenAPI contract.
- Node Daemon обращается к OpenSandbox напрямую по закрытому HTTP endpoint через
  собственную границу `TaskRuntimeBackend`. JavaScript/Python SDK или отдельный
  language runner между Rust Node и runtime не нужен.
- Uprava владеет bounded task contract, durable state, worktree/branch,
  context, evidence and review. OpenSandbox владеет container lifecycle, TTL,
  resource limits, volumes and command/file transport внутри container.
- Agent image является кастомным и versioned: Codex CLI плюс общие development
  utilities. Credentials в image не запекаются; они живут в отдельном
  persistent credential profile и монтируются в sandbox во время запуска.
- OpenSandbox является первым backend, а не новой продуктовой authority.
  Будущие VM/cloud providers должны подключаться за `TaskRuntimeBackend` без
  изменения `TaskRun` и review/evidence contracts.

## Реализованный baseline 0.2.19

Первый работающий vertical slice включает:

- migration 17 и Core API `GET/POST /task-runs`, detail и cancel;
- отдельные `RunTask`/`CancelTaskRun`, `TaskRun` scope/ref и typed lifecycle
  events без создания interactive session;
- capability-gated dispatch только на Node с
  `task_runtime.opensandbox.docker` и объявленным immutable runtime image;
- linked worktree `<project>/.uprava/runs/<task-run-id>`, отдельную branch и
  проверку repository/branch identity при retry;
- прямой Rust adapter к OpenSandbox lifecycle и `execd`: create, readiness
  polling, endpoint resolution, SSE, timeout, interrupt и delete;
- CPU/memory limits, sandbox TTL, UID/GID владельца host worktree и bounded
  stdout/stderr;
- result package с summary, Git status/diff, check results, SHA-256 explicit
  artifacts, unresolved risks и независимым cleanup outcome;
- persistent Node mapping для orphan cleanup после restart; worktree остаётся
  на host для review;
- Tasks surface в Web и digest-pinned task image в release manifest.

OpenSandbox API key, persistent `CODEX_HOME/auth.json`, profile admission и
login readiness намеренно не входят в `0.2.19`. До их реализации Compose
profile использует loopback-bound insecure mode, UI и evidence явно показывают
deferred auth, а направление не считается полностью закрытым. Операторский
порядок запуска и оставшаяся ручная проверка описаны в
[`task-sandbox-runtime.md`](../../runbooks/task-sandbox-runtime.md).

```text
Core Backend
  durable TaskRun, dispatch, events, evidence metadata, review
        |
        v
Node Daemon
  worktree, context, TaskRuntimeBackend, event translation
        |
        v  private HTTP/OpenAPI
OpenSandbox Server
  lifecycle, TTL, volumes, limits, endpoints
        |
        v  Docker API
uprava/codex-runtime:<version>
  execd + Codex CLI + tools + mounted workspace/auth
```

## Почему это соответствует текущей стадии

Самописный Docker wrapper быстро превращается в оркестратор: кроме
`docker run` нужны readiness, command streaming, cancellation, background
process status, cleanup после падения клиента, TTL, volumes, resource limits и
reconciliation. OpenSandbox уже отделяет lifecycle server от in-container
`execd`, предоставляет OpenAPI contracts, Docker backend, SSE command stream,
file operations and TTL lifecycle. При этом Docker deployment работает на
одной машине и по умолчанию может использовать SQLite, поэтому для первого
среза не нужны Kubernetes, Redis, PostgreSQL or object storage.

Это такая же интеграционная форма, как ToolHive: зрелость и совместимость
внешнего компонента проверяются отдельно, а Uprava сохраняет небольшой
versioned adapter и собственную domain model. Отличие в том, что OpenSandbox
уже является HTTP service, поэтому дополнительный bridge не нужен.

## Scope первого среза

### Входит

- один OpenSandbox service на Node host или рядом с локальным Compose stack;
- только OpenSandbox Docker runtime;
- один container на один bounded `TaskRun`;
- host-side git worktree/branch, bind-mounted как `/workspace`;
- custom Codex runtime image с pinned CLI and utilities;
- запуск `codex exec --json` с
  `--dangerously-bypass-approvals-and-sandbox` через execution API;
- streaming stdout/stderr/status в Node с преобразованием в Uprava events;
- interrupt/cancel, hard timeout, TTL cleanup and explicit delete;
- optional CPU/memory limits;
- checks в том же workspace/container;
- result package: summary, commit/diff, checks, artifacts and unresolved risks;
- persistent Codex credential profiles с однократной авторизацией — отложено
  после runtime mechanics baseline.

Dangerous bypass здесь является частью принятого Task contract, а не fallback
из Agent mode. Codex работает unrestricted внутри externally sandboxed
environment; реальную границу задают отдельный worktree, OpenSandbox container,
mounts, credentials, network policy, resource limits, timeout and TTL. Task
trace/evidence должен отдельно показывать provider policy и внешний sandbox,
чтобы unrestricted provider не путался с отсутствием общей изоляции run.

### Не входит

- Kubernetes deployment или scheduler;
- microVM/VM-grade hostile multi-tenant isolation;
- pools, snapshots, browser/VNC, Jupyter/code interpreter and RL workloads;
- OpenSandbox ingress gateway, egress sidecar or credential vault;
- distributed queue внутри OpenSandbox;
- external sandbox providers;
- перенос Core-owned workflow, review or evidence model в OpenSandbox;
- использование OpenSandbox SDK из JavaScript/Python только ради integration.

## Границы ответственности

### Core Backend

Core остаётся authority для:

- `TaskRun` identity, requested scope and lifecycle projection;
- placement/dispatch на Node;
- durable task, workflow and review state;
- expected evidence and result package metadata;
- ordered Uprava event log, causality and user-visible errors;
- admission policy and concurrency limits;
- link из bounded run обратно в session, job or future hybrid flow.

Core не управляет Docker и не обращается к OpenSandbox напрямую.

### Node Daemon

Node является владельцем local execution adapter:

- проверяет placement and local capability;
- создаёт отдельный git worktree/branch для run;
- собирает context package and effective runtime request;
- вызывает OpenSandbox lifecycle/exec APIs;
- хранит mapping `task_run_id -> sandbox_id -> command_id`;
- преобразует upstream status/SSE в typed Uprava events;
- выполняет cancel, reconciliation and final cleanup;
- собирает evidence из git, checks and produced artifacts.

Node не должен протаскивать OpenSandbox response types в Core protocol.
`TaskRuntimeBackend` нормализует минимум операций:

```text
create(run_spec) -> runtime_ref
wait_ready(runtime_ref)
exec(runtime_ref, command_spec) -> command_ref + event stream
cancel(command_ref)
inspect(runtime_ref)
delete(runtime_ref)
```

### OpenSandbox

OpenSandbox отвечает только за runtime mechanics:

- create/inspect/delete container;
- readiness and lifecycle states;
- TTL expiration and cleanup;
- bind mounts and named volumes;
- resource limits;
- endpoint resolution;
- command execution, streaming, status and interruption через `execd`;
- file/process diagnostics, нужные adapter-у.

OpenSandbox metadata получает `uprava_run_id`, `uprava_node_id` and image
version только для reconciliation. Она не становится source of truth для
Uprava task state.

## Workspace и git

Первый срез использует host-owned worktree:

```text
<project>/.uprava/runs/<run-id>/   ->   /workspace
```

Перед container creation Node:

1. выбирает immutable base revision;
2. создаёт worktree и отдельную branch;
3. записывает base revision, branch and host path в run state;
4. монтирует worktree read-write в `/workspace`;
5. задаёт container working directory `/workspace`.

После выполнения Node сам получает `git status`, diff/commit and check
evidence. Результат остаётся доступен на host даже после удаления container.
Container filesystem считается временным и не должен быть единственным местом
хранения результата.

Если OpenSandbox server сам запущен в Compose, разрешённые workspace and
credential roots монтируются в service по тем же абсолютным host paths. Это
нужно проверить отдельно на Linux and Docker Desktop: server-side path
validation и Docker daemon должны видеть один и тот же source path.

## Custom runtime image

Versioned image вида `uprava/codex-runtime:<version>` содержит:

- pinned Codex CLI;
- `git`, shell, certificates, `curl`, `jq`, `ripgrep` and common build tools;
- непривилегированного runtime user;
- стабильные `/workspace` and Codex config paths;
- image metadata/version для evidence.

Project-specific dependencies не нужно бесконечно добавлять в общий image.
Первый baseline покрывает общие инструменты; позже могут появиться несколько
явных image profiles или project-selected image. OpenSandbox добавляет свой
`execd` при запуске, поэтому custom image не обязан содержать Python SDK или
Uprava-specific orchestration code.

## Codex authentication

Цель — авторизовать credential profile один раз и переиспользовать его во всех
последующих containers.

Codex хранит локальное состояние под `CODEX_HOME`; при file-based storage
cached login находится в `auth.json`. Codex CLI повторно использует cached
login, а ChatGPT tokens обновляет во время использования. Поэтому первый срез:

1. создаёт host profile, например
   `~/.uprava/credentials/codex/default/auth.json`;
2. выполняет однократный operator-driven `codex login` для этого profile;
3. монтирует `auth.json` read-write в ожидаемый container path;
4. задаёт `cli_auth_credentials_store = "file"` в image config;
5. не копирует auth в image, task context, event log or result package.

Read-write mount обязателен, чтобы обновлённый token сохранился для следующего
run. В первом срезе один credential profile допускает не более одного
одновременного Codex run: это упрощает запись auth state and quota admission.
Несколько параллельных потоков требуют нескольких явно выбранных profiles.

`codex login status` является readiness check profile. Потеря или отзыв login
возвращается как typed `authentication_required`; Node не должен запускать
интерактивный browser login внутри каждого task container.

Официальный Codex contract для cached login и file storage:
[Authentication](https://learn.chatgpt.com/docs/auth) и
[Environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables).

## Lifecycle первого bounded run

```text
Core creates TaskRun
  -> Node claims dispatch
  -> Node creates branch/worktree and context package
  -> Node POST /v1/sandboxes with image, TTL, limits, mounts, metadata
  -> Node waits until sandbox is Running
  -> Node starts codex exec --json through execd
  -> SSE/status becomes ordered Uprava run events
  -> Node runs declared checks in the same sandbox
  -> Node collects summary, git diff/commit, checks and artifacts
  -> Core stores final result/evidence projection
  -> Node deletes sandbox and retires local transient mapping
```

Cancel сначала прерывает active command, затем удаляет sandbox. Если Node
падает, container TTL является последней линией cleanup. После restart Node
сопоставляет незавершённые local run mappings с OpenSandbox metadata, удаляет
orphans or продолжает только те операции, продолжение которых явно
поддерживается контрактом.

## Local deployment profile

Первый deployment profile должен быть небольшим:

- pinned OpenSandbox server image/service;
- Docker socket;
- persistent volume только для OpenSandbox SQLite/config;
- bind mounts разрешённых workspace and credential roots;
- private API key между Node и OpenSandbox;
- endpoint только на loopback/private Compose network;
- Docker backend and ordinary bridge networking;
- отключённые Kubernetes, pools, ingress, egress sidecar, snapshots,
  code-interpreter/Jupyter, browser and desktop subsystems.

OpenSandbox не требуется на каждой Node. Node сообщает capability
`task_runtime.opensandbox.docker`; Core dispatch-ит sandbox task только туда,
где service доступен and healthy. Interactive Persistent Runtime продолжает
работать без OpenSandbox.

## Оставшаяся ручная acceptance-проверка

OpenSandbox server закреплён как `v0.2.2`, `execd` — как `v1.0.21`, а Codex CLI
— как `0.144.1`. Автоматические contract/unit checks не заменяют проверку
реального Docker path и credential lifecycle. Полная acceptance считается
успешной, если вручную доказано следующее:

- service стартует в локальном Compose и управляет host Docker;
- idle OpenSandbox service укладывается в целевой бюджет `<= 200 MiB RSS`;
- worktree mount работает на Linux and Docker Desktop;
- persistent `auth.json` используется повторно и token refresh переживает
  удаление container;
- `codex exec --json` выдаёт stream без SDK/language runner;
- cancellation останавливает process tree;
- TTL удаляет sandbox после принудительного завершения Node;
- custom image запускается без Python runtime внутри него;
- branch/diff/check evidence остаётся на host после cleanup;
- Rust adapter работает по OpenAPI/HTTP без upstream SDK;
- измерены idle service, idle sandbox and active Codex run overhead.

Если оставшиеся условия не выполняются, решение возвращается к сравнению внешних
standalone runtime services. Это не разрешает автоматически заменить его
самописным Docker orchestrator или встроить JS/Python runner в Node.

## Будущее расширение

После подтверждения реальной потребности `TaskRuntimeBackend` может получить
другие реализации: remote OpenSandbox, VM/microVM runtime or managed provider.
Такие backend-ы могут усилить isolation или распределённое scheduling, но не
должны менять:

- `TaskRun` contract;
- Core/Node authority split;
- host/repository evidence model;
- expected evidence and review package;
- provider-neutral runtime events.

## Источники внешнего runtime contract

- [OpenSandbox architecture](https://github.com/opensandbox-group/OpenSandbox/blob/main/docs/architecture.md)
- [OpenSandbox server](https://github.com/opensandbox-group/OpenSandbox/blob/main/docs/components/server.md)
- [Sandbox lifecycle OpenAPI](https://github.com/opensandbox-group/OpenSandbox/blob/main/specs/sandbox-lifecycle.yml)
- [Execd OpenAPI](https://github.com/opensandbox-group/OpenSandbox/blob/main/specs/execd-api.yaml)
- [OpenSandbox examples](https://github.com/opensandbox-group/OpenSandbox/tree/main/examples)
