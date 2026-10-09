# Codex app-server protocol probe

Этот disposable tool проверяет provider-owned protocol до реализации managed
adapter в Node. Он не импортирует production contracts, не читает Core tables
и выводит только bounded summary без provider thread/turn/request identifiers.

Probe рассчитан на pinned baseline из
[`docs/systems/areas/002-run-mode.md`](../../docs/systems/areas/002-run-mode.md).
Запуск создаёт и архивирует provider thread в текущем `CODEX_HOME` и использует
отдельный `codex app-server` process на loopback WebSocket.

```sh
cargo run -p codex-app-server-probe -- \
  --workspace /absolute/path/to/disposable/workspace
```

Необязательные параметры:

- `--codex /path/to/codex` — другой Codex binary;
- `--timeout-seconds 120` — timeout одного protocol step;
- `--skip-live-turns` — только handshake, policy echo и MCP readiness; команда
  намеренно возвращает failed gate, потому что semantic scenarios пропущены;
- `--help` — полная краткая справка.

Команда печатает один scrubbed JSON report в stdout. Raw provider messages,
prompts, environment, `CODEX_HOME`, thread ids и approval ids не выводятся.
