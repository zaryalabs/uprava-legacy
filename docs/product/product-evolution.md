# Эволюция продукта Uprava

Статус: `active`

Этот документ заменяет старую модель стадий. Каноническое описание первой
версии теперь находится ниже, в разделе [V01](#v01), а порядок следующих работ - в
[`feature-queue.md`](feature-queue.md).

## Рабочая модель

- `V01` - первая осязаемая версия продукта: что пользователь сможет запустить,
  открыть, увидеть, потрогать и проверить.
- `Feature Queue` - очередь реализационных срезов, отсортированная по
  зависимости, сложности, риску и продуктовой ценности.
- `Feature Inventory` - полный инвентарь известных идей и направлений без
  обещания порядка реализации.
- Design docs - целевая модель ключевых механик без жесткой временной нарезки.

## V01

Первая версия продукта - **Distributed Agent Control Panel**:

- Core Backend и Web Control Panel;
- одна или несколько нод с Node Daemon;
- persistent Codex-backed session через Agent Provider Adapter;
- navigation tree формата `Nodes -> Projects/Workspaces -> Sessions`;
- project/workspace binding как placement context;
- chat/session view как первая primary work surface;
- session lifecycle controls: start, attach, detach, interrupt, stop, resume and
  return later, если provider это поддерживает;
- basic node, project, runtime, session, message and event persistence;
- UI shell, entity model and command/event envelopes, подготовленные для будущих
  workspace, editor, terminal, tools, plugins, trace and artifact surfaces.
- trusted local/single-user or controlled development deployment, with
  production security hardening deferred to the first post-V01 slice.

V01 проверяет тезис:

```text
Persistent Runtime + Node Daemon + Core UI
превращают agent chat в distributed control surface.
```

Поставленный V01 adapter приблизил этот contract через долговечный
`SessionThread` и отдельные `codex exec/resume` processes. Целевая живая форма
с provider-native streaming, approvals, interruption and reconnect вынесена в
`16 Managed Agent Work Loop`; exec/resume сохраняется как compatibility mode.

## Дальше

Дальнейшее развитие не фиксируется как линейная цепочка стадий. Вместо этого
следующие срезы должны выбираться из очереди фич:

- security baseline;
- runtime/session hardening;
- workspace shell and references;
- Project Workspace Inspector;
- отложенные сообщения в сессии;
- Background Jobs и scheduled agent runs;
- causality and trace UX;
- git and review basics;
- Agent Tooling and Tool Registry v1;
- Plugin Registry v1;
- visual artifact system;
- dynamic UI from agents;
- task-based sandbox runtime;
- Managed Agent Work Loop;
- team/cloud model;
- expansion beyond software development.

Точный порядок и размер каждого среза должны уточняться в
[`feature-queue.md`](feature-queue.md).
