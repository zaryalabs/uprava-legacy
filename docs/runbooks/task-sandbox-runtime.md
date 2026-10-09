# Task-based Sandbox Runtime

Статус: `active`

Runbook относится к implementation baseline `0.2.26`. Он проверяет
Docker/OpenSandbox, Task Run, worktree, cancellation, cleanup и evidence.
OpenSandbox API key и persistent Codex credential profile пока намеренно
отложены; профиль нельзя считать production-ready до отдельной auth acceptance.

## Что запускается

```text
Web -> Core TaskRun -> host Node -> OpenSandbox -> Docker sandbox
                                      |
                                      +-> <project>/.uprava/runs/<task-id>
```

Core хранит Task Run и result projection. Node владеет worktree, runtime
mapping, `execd` stream и evidence. OpenSandbox владеет container lifecycle,
TTL, limits и endpoint transport. После terminal state container удаляется, а
worktree остаётся для review.

## Предварительные условия

- Docker Engine и Docker Compose доступны текущему оператору;
- репозиторий является Git workspace с обычным immutable `HEAD`;
- Docker Desktop разрешает file sharing для абсолютного пути репозитория;
- Core, Web и host Node могут быть запущены обычным local-dev способом;
- порт `127.0.0.1:18083` свободен.

Локальный OpenSandbox profile монтирует `${PWD}` по тому же абсолютному пути.
Поэтому команды ниже нужно выполнять из корня workspace. Development config
временно задаёт `allowed_host_paths = ["/"]`; endpoint остаётся на loopback.

## Локальный запуск

Соберите `uprava/codex-runtime:0.2.26` и поднимите pinned OpenSandbox:

```sh
make task-runtime-up
curl --fail http://127.0.0.1:18083/health
```

Запустите Node с тем же workspace boundary и runtime image:

```sh
export UPRAVA_NODE_WORKSPACES="$PWD"
export UPRAVA_OPENSANDBOX_URL=http://127.0.0.1:18083
export UPRAVA_TASK_RUNTIME_IMAGE=uprava/codex-runtime:0.2.26
make node-r
```

После heartbeat Node должен объявить capability
`task_runtime.opensandbox.docker` с `available=true` и тем же
`runtime_image` и `provider=codex`. Host-бинарник Codex для Task Run не нужен:
он находится в runtime image. Core отвергает произвольную image override, не
совпадающую с capability Node.

Остановить только runtime service:

```sh
make task-runtime-down
```

## Создание и наблюдение

1. Откройте workspace и вкладку `Tasks`.
2. Проверьте immutable base commit, prompt, checks, explicit evidence paths,
   timeout, TTL и limits. Core принимает только CPU `0.5/1/2/4/8` и память
   `512Mi/1Gi/2Gi/4Gi/8Gi/16Gi`.
3. Создайте Task Run.
4. Наблюдайте состояния `queued -> preparing_workspace -> starting_runtime ->
   running -> checking -> collecting_evidence -> terminal`.
5. Для cancel нажмите `Cancel`; Core оставляет состояние `cancelling`, пока Node
   не подтвердит `cancelled`.

Worktree создаётся в `<project>/.uprava/runs/<task-run-id>` на branch
`uprava/task/<task-run-id>`. Повторный delivery того же run переиспользует
только worktree с совпадающими repository identity и branch.

## Ожидаемое evidence

Terminal detail должен содержать:

- summary и terminal reason;
- base/final revision, branch, runtime image и host worktree path;
- bounded `git status`/binary diff с явным признаком truncation;
- exit code, stdout/stderr, duration и truncation каждого declared check;
- размер и SHA-256 каждого существующего regular non-symlink artifact;
- unresolved risks, включая отложенную credential integration;
- task outcome и cleanup state как независимые поля.

Отсутствующий, слишком большой или выходящий через symlink artifact не
копируется и становится unresolved risk.

## Cleanup и recovery

Node всегда пытается удалить sandbox после evidence collection. Mapping
`task_run_id -> sandbox_id` хранится в Node SQLite до cleanup. После restart
Node удаляет sandbox из оставшихся mappings, очищает mapping и сохраняет
worktree для review. Task state events используют стабильные разреженные
lifecycle sequence ranks: повторная доставка состояния идемпотентна, а иной
путь после crash не занимает чужой `seq`. TTL остаётся последней линией защиты,
если Node и cleanup недоступны одновременно.

После review worktree удаляется осознанно из исходного repository:

```sh
git worktree remove .uprava/runs/<task-run-id>
git branch -d uprava/task/<task-run-id>
```

Не удаляйте `.uprava/runs` рекурсивно: Git должен сначала снять linked worktree
metadata.

## Production path boundary

Production Compose разрешает и монтирует только
`/srv/uprava-workspaces:/srv/uprava-workspaces`. Значение
`UPRAVA_NODE_WORKSPACES` должно находиться под тем же root. Task runtime image
публикуется как отдельный digest-pinned release artifact; активный release
manifest также фиксирует digest OpenSandbox server image и подключается к
systemd Node через optional `EnvironmentFile`.

Пока auth work отложена, OpenSandbox слушает только loopback и требует
`OPENSANDBOX_INSECURE_SERVER=YES`. Не публикуйте порт `18083` наружу и не
считайте этот профиль production security boundary. Node дополнительно
отвергает не-loopback `UPRAVA_OPENSANDBOX_URL` до появления API-key support.

## Отложенная ручная auth acceptance

Отдельная завершающая проверка должна добавить и подтвердить:

- private OpenSandbox API key и header из Node;
- host-owned `CODEX_HOME` profile с file credential store;
- operator-driven login без попадания token в image, event, result или log;
- read-write token refresh между двумя последовательными containers;
- один concurrent run на credential profile и typed
  `authentication_required` readiness failure.

До этой проверки unauthenticated Codex execution ожидаемо завершится typed
`task_run.provider_failed`; worktree, checks, cleanup и evidence mechanics при
этом остаются наблюдаемыми.

## Диагностика

- `node.task_runtime_unavailable`: Node не объявил URL/image capability;
- `task_run.runtime_image_not_advertised`: request image не совпала с Node;
- `task_run.worktree_*`: workspace не Git repository, commit/branch identity не
  совпали или путь вышел за allow-list;
- `task_run.sandbox_*`: проверьте `docker compose --profile task-runtime logs
  opensandbox`, Docker socket, image pull и совпадение абсолютного host path;
- `task_run.endpoint_*`: sandbox не достиг `Running` или `execd` endpoint
  недоступен;
- cleanup `failed`: не удаляйте worktree; проверьте orphan mapping и повторно
  запустите Node reconciliation.

Каноническая архитектура и полный acceptance checklist находятся в
[`A-013 Task-based Sandbox Runtime`](../systems/areas/013-task-based-sandbox-runtime.md).
