SHELL := /bin/sh

RUST_MANIFEST := Cargo.toml
CARGO ?= cargo
PYTHON ?= python3
RUSTUP ?= rustup
RUST_TOOLCHAIN ?=
WEB_DIR := apps/web
WEB_PACKAGE := $(WEB_DIR)/package.json
WEB_NODE_MODULES := $(WEB_DIR)/node_modules
COMPOSE ?= docker compose
DEV_COMPOSE_FILE ?= compose.dev.yaml
COMPOSE_PARALLEL_LIMIT ?= 1
RUST_TOOL_TOML_FILES := $(wildcard Cargo.toml crates/*/Cargo.toml deny.toml taplo.toml)
# rsa is retained in Cargo.lock as an inactive optional dependency and
# RUSTSEC-2023-0071 has no fixed release.
CARGO_AUDIT_IGNORE := --ignore RUSTSEC-2023-0071
CLAWPATCH ?= npx --yes clawpatch@0.3.0
DEV_COMPOSE_CMD = $(COMPOSE) -f $(DEV_COMPOSE_FILE)
GIT_SHA ?= $(shell git rev-parse HEAD)
SHORT_SHA ?= $(shell printf '%.12s' "$(GIT_SHA)")
IMAGE_REGISTRY ?= ghcr.io
IMAGE_OWNER ?= zaryalabs
IMAGE_NAMESPACE ?= $(IMAGE_REGISTRY)/$(IMAGE_OWNER)
IMAGE_TAG ?= sha-$(GIT_SHA)
UPRAVA_CORE_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-core:$(IMAGE_TAG)
UPRAVA_WEB_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-web:$(IMAGE_TAG)
UPRAVA_GENERATED_UI_BUILDER_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-generated-ui-builder:$(IMAGE_TAG)
UPRAVA_NODE_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-node:$(IMAGE_TAG)
UPRAVA_TOOLHIVE_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-toolhive:$(IMAGE_TAG)
UPRAVA_TOOLHIVE_VERSION ?= 0.40.0
UPRAVA_TASK_RUNTIME_IMAGE ?= $(IMAGE_NAMESPACE)/uprava-codex-runtime:$(IMAGE_TAG)
UPRAVA_OPENSANDBOX_IMAGE ?= opensandbox/server:v0.2.2
UPRAVA_TASK_RUNTIME_DEV_IMAGE ?= uprava/codex-runtime:0.2.26
UPRAVA_NODE_PACKAGE_VERSION := $(shell awk -F'"' '/^version = / { print $$2; exit }' crates/uprava-node/Cargo.toml)
UPRAVA_NODE_VERSION ?= $(UPRAVA_NODE_PACKAGE_VERSION)+$(GIT_SHA)
RELEASE_ID ?= $(SHORT_SHA)
RELEASE_DIR ?= builds/releases/$(RELEASE_ID)
RELEASE_MANIFEST ?= $(RELEASE_DIR).env.release
NODE_ARTIFACT_PATH ?= $(RELEASE_DIR)/uprava-node
BUILD_TIMESTAMP ?= $(shell date -u "+%Y-%m-%dT%H:%M:%SZ")
ALLOW_UNRESOLVED_DIGESTS ?= 0
UPRAVA_RELEASE_FAMILY ?= 0.2.0
UPRAVA_CORE_STATE_DIR ?= state/core
UPRAVA_TOOLHIVE_STATE_DIR ?= state/toolhive
UPRAVA_CORE_CONFIG ?= /etc/uprava/core.env
UPRAVA_NODE_CONFIG ?= /etc/uprava/node.env
UPRAVA_TOOLHIVE_CONFIG ?= /etc/uprava/toolhive.env
UPRAVA_NODE_STATE_PATH ?= /var/lib/uprava-node/node.sqlite
UPRAVA_AUTO_APPROVE_NODE_NAME ?= Zarya Server
UPRAVA_TOOLHIVE_PROFILE ?= toolhive
INSTALL_DIR ?= /opt/apps/uprava
SYSTEMD_UNIT_PATH ?= /etc/systemd/system/uprava-node.service
SUDO ?=

ifneq (,$(wildcard pnpm-lock.yaml))
WEB_PM := pnpm
WEB_RUN := pnpm --dir $(WEB_DIR)
else ifneq (,$(wildcard bun.lockb))
WEB_PM := bun
WEB_RUN := cd $(WEB_DIR) && bun
else ifneq (,$(wildcard package-lock.json))
WEB_PM := npm
WEB_RUN := npm --prefix $(WEB_DIR)
else
WEB_PM := npm
WEB_RUN := npm --prefix $(WEB_DIR)
endif
PLAYWRIGHT_RUN ?= $(WEB_RUN)

.DEFAULT_GOAL := help

help: ## Show available make targets
	@awk 'BEGIN {FS = ":.*## "}; /^[a-zA-Z0-9_.-]+:.*## / {printf "  %-14s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

prepare: rust-toolchain protocol-check rust-l rust-t web-l web-t generated-ui-t web-dl web-e2e ops-config systemd-check scripts-check ## Run all pre-release checks

ci-prepare: ## Run the prepare phase in the current execution boundary
	ci/prepare.sh

ci-build: ## Build, verify and publish one immutable release
	ci/build.sh

ci-deploy: ## Bootstrap and activate one published release on the host
	ci/deploy.sh

ci-finalize: ## Validate the active release and apply bounded retention
	ci/finalize.sh

build: ## Build releasable Core/Web/Generated UI images and Node artifact
	docker build --build-arg UPRAVA_GIT_SHA="$(GIT_SHA)" -t "$(UPRAVA_CORE_IMAGE)" -f Dockerfile.core .
	docker build \
		--build-arg VITE_UPRAVA_API_BASE=/api/v1 \
		--build-arg VITE_UPRAVA_RELEASE_ID="$(GIT_SHA)" \
		-t "$(UPRAVA_WEB_IMAGE)" \
		-f apps/web/Dockerfile \
		apps/web
	docker build -t "$(UPRAVA_GENERATED_UI_BUILDER_IMAGE)" -f Dockerfile.generated-ui-builder .
	docker build --build-arg UPRAVA_GIT_SHA="$(GIT_SHA)" -t "$(UPRAVA_NODE_IMAGE)" -f Dockerfile.node .
	docker build -t "$(UPRAVA_TOOLHIVE_IMAGE)" -f Dockerfile.toolhive .
	docker build -t "$(UPRAVA_TASK_RUNTIME_IMAGE)" -f Dockerfile.codex-runtime .
	docker pull "$(UPRAVA_OPENSANDBOX_IMAGE)"
	scripts/extract-node-artifact.sh "$(UPRAVA_NODE_IMAGE)" "$(NODE_ARTIFACT_PATH)" >/dev/null

image-runtime: ## Run production images non-root with read-only filesystems
	UPRAVA_CORE_IMAGE="$(UPRAVA_CORE_IMAGE)" \
	UPRAVA_WEB_IMAGE="$(UPRAVA_WEB_IMAGE)" \
	UPRAVA_GENERATED_UI_BUILDER_IMAGE="$(UPRAVA_GENERATED_UI_BUILDER_IMAGE)" \
	UPRAVA_NODE_IMAGE="$(UPRAVA_NODE_IMAGE)" \
	UPRAVA_TOOLHIVE_IMAGE="$(UPRAVA_TOOLHIVE_IMAGE)" \
	UPRAVA_TOOLHIVE_VERSION="$(UPRAVA_TOOLHIVE_VERSION)" \
	UPRAVA_RELEASE_SHA="$(GIT_SHA)" \
	scripts/check-image-runtime.sh

clean-state-restore: ## Rehearse isolated 0.2.0 Core/Node state and online restore
	$(CARGO) build --locked -p uprava-server --bin uprava-server -p uprava-node --bin uprava-node
	scripts/check-clean-state-restore.sh

push: ## Push releasable artifacts and write release manifest
	docker push "$(UPRAVA_CORE_IMAGE)"
	docker push "$(UPRAVA_WEB_IMAGE)"
	docker push "$(UPRAVA_GENERATED_UI_BUILDER_IMAGE)"
	docker push "$(UPRAVA_NODE_IMAGE)"
	docker push "$(UPRAVA_TOOLHIVE_IMAGE)"
	docker push "$(UPRAVA_TASK_RUNTIME_IMAGE)"
	$(MAKE) --no-print-directory release-manifest

release-manifest: ## Write builds/releases/<release-id>.env.release
	RELEASE_MANIFEST="$(RELEASE_MANIFEST)" \
	RELEASE_ID="$(RELEASE_ID)" \
	GIT_SHA="$(GIT_SHA)" \
	BUILD_TIMESTAMP="$(BUILD_TIMESTAMP)" \
	UPRAVA_CORE_IMAGE="$(UPRAVA_CORE_IMAGE)" \
	UPRAVA_WEB_IMAGE="$(UPRAVA_WEB_IMAGE)" \
	UPRAVA_GENERATED_UI_BUILDER_IMAGE="$(UPRAVA_GENERATED_UI_BUILDER_IMAGE)" \
	UPRAVA_NODE_IMAGE="$(UPRAVA_NODE_IMAGE)" \
	UPRAVA_TOOLHIVE_IMAGE="$(UPRAVA_TOOLHIVE_IMAGE)" \
	UPRAVA_TOOLHIVE_VERSION="$(UPRAVA_TOOLHIVE_VERSION)" \
	UPRAVA_TASK_RUNTIME_IMAGE="$(UPRAVA_TASK_RUNTIME_IMAGE)" \
	UPRAVA_OPENSANDBOX_IMAGE="$(UPRAVA_OPENSANDBOX_IMAGE)" \
	UPRAVA_NODE_VERSION="$(UPRAVA_NODE_VERSION)" \
	UPRAVA_RELEASE_FAMILY="$(UPRAVA_RELEASE_FAMILY)" \
	UPRAVA_CORE_STATE_DIR="$(UPRAVA_CORE_STATE_DIR)" \
	UPRAVA_TOOLHIVE_STATE_DIR="$(UPRAVA_TOOLHIVE_STATE_DIR)" \
	UPRAVA_CORE_CONFIG="$(UPRAVA_CORE_CONFIG)" \
	UPRAVA_NODE_CONFIG="$(UPRAVA_NODE_CONFIG)" \
	UPRAVA_TOOLHIVE_CONFIG="$(UPRAVA_TOOLHIVE_CONFIG)" \
	UPRAVA_NODE_STATE_PATH="$(UPRAVA_NODE_STATE_PATH)" \
	UPRAVA_AUTO_APPROVE_NODE_NAME="$(UPRAVA_AUTO_APPROVE_NODE_NAME)" \
	UPRAVA_TOOLHIVE_PROFILE="$(UPRAVA_TOOLHIVE_PROFILE)" \
	NODE_ARTIFACT_PATH="$(NODE_ARTIFACT_PATH)" \
	ALLOW_UNRESOLVED_DIGESTS="$(ALLOW_UNRESOLVED_DIGESTS)" \
	scripts/write_release_manifest.sh

install-release-manifest: ## Install active release manifest into INSTALL_DIR
	@test -f "$(RELEASE_MANIFEST)" || { echo "Missing $(RELEASE_MANIFEST); run make release-manifest first"; exit 1; }
	$(SUDO) install -d "$(INSTALL_DIR)/builds/releases"
	$(SUDO) install -m 644 "$(RELEASE_MANIFEST)" "$(INSTALL_DIR)/builds/releases/$(RELEASE_ID).env.release"

install-ops: ## Bootstrap product-owned directories and ops files
	$(SUDO) install -d -o root -g root -m 755 "$(INSTALL_DIR)"
	$(SUDO) install -d -o root -g root -m 755 "$(INSTALL_DIR)/builds/releases"
	$(SUDO) install -d -o 10001 -g 10001 -m 750 "$(INSTALL_DIR)/state/core"
	$(SUDO) install -d -o 10002 -g 10002 -m 700 "$(INSTALL_DIR)/state/toolhive"
	$(SUDO) install -d -o root -g root -m 750 "$(INSTALL_DIR)/state/opensandbox"
	$(SUDO) install -d -o root -g root -m 755 "$(INSTALL_DIR)/scripts" "$(INSTALL_DIR)/systemd"
	$(SUDO) install -d -o uprava -g uprava -m 700 /var/lib/uprava-node
	$(SUDO) install -m 644 ops/Makefile "$(INSTALL_DIR)/Makefile"
	$(SUDO) install -m 644 ops/compose.yaml "$(INSTALL_DIR)/compose.yaml"
	$(SUDO) install -m 644 ops/opensandbox.toml "$(INSTALL_DIR)/opensandbox.toml"
	$(SUDO) install -m 644 ops/README.md "$(INSTALL_DIR)/README.md"
	$(SUDO) install -m 644 ops/systemd/uprava-node.service.example "$(INSTALL_DIR)/systemd/uprava-node.service"
	$(SUDO) install -m 644 ops/systemd/uprava-node.service.example "$(SYSTEMD_UNIT_PATH)"
	$(SUDO) install -m 755 scripts/prune-uprava-images.sh "$(INSTALL_DIR)/scripts/prune-uprava-images.sh"
	$(SUDO) install -m 755 scripts/prune-uprava-releases.sh "$(INSTALL_DIR)/scripts/prune-uprava-releases.sh"
	$(SUDO) install -m 755 scripts/backup-sqlite.sh "$(INSTALL_DIR)/scripts/backup-sqlite.sh"
	$(SUDO) install -m 755 scripts/verify-sqlite-backup.sh "$(INSTALL_DIR)/scripts/verify-sqlite-backup.sh"

init: ## Install local hooks and project dependencies when manifests exist
	@set -e; \
	if command -v pre-commit >/dev/null 2>&1; then \
		pre-commit install --hook-type pre-commit --hook-type pre-push; \
	else \
		echo "pre-commit is not installed; install it to enable commit hooks"; \
	fi
	@set -e; \
	if [ -f "$(RUST_MANIFEST)" ]; then \
		$(RUSTUP) toolchain install 1.88.0 --profile minimal --component rustfmt,clippy; \
		$(CARGO) fetch; \
		$(MAKE) --no-print-directory rust-tools-install; \
	else \
		echo "No Cargo.toml found; skipping Rust dependency fetch"; \
	fi
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		$(WEB_RUN) install; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web dependency install"; \
	fi

fmt: docs-fmt rust-fmt web-fmt ## Format all supported project files

l: docs-l rust-l web-l ## Run light checks

dl: l protocol-check rust-dl web-dl ## Run deep checks

t: rust-t web-t generated-ui-t ## Run tests

c: fmt dl t ## Run full local quality gate

push-check: docs-l protocol-check rust-l rust-t web-l web-t generated-ui-t web-dl scripts-check push-check-msrv rust-dl web-e2e ## Run the same source checks as a main CI prepare

push-check-msrv: ## Check the Rust workspace against the minimum supported toolchain
	$(RUSTUP) run 1.88.0 cargo check --workspace --all-targets --locked

pc: ## Run pre-commit hooks on all files
	@if command -v pre-commit >/dev/null 2>&1; then \
		pre-commit run --all-files; \
	else \
		echo "pre-commit is not installed; cannot run hooks"; \
		exit 1; \
	fi

claw-doctor: ## Check Clawpatch and local Codex setup
	$(CLAWPATCH) doctor

claw-init: ## Initialize Clawpatch project state
	$(CLAWPATCH) init

claw-map: ## Build Clawpatch semantic feature map
	$(CLAWPATCH) map

claw-review: ## Run Clawpatch review. Usage: make claw-review [LIMIT=10] [JOBS=3]
	$(CLAWPATCH) review --limit $(or $(LIMIT),10) --jobs $(or $(JOBS),3)

claw-report: ## Generate Clawpatch findings report
	$(CLAWPATCH) report

claw-ci: ## Run Clawpatch CI-style review report. Usage: make claw-ci [SINCE=origin/main]
	$(CLAWPATCH) ci --since $(or $(SINCE),origin/main) --output clawpatch-report.md

claw-show: ## Show one Clawpatch finding. Usage: make claw-show FINDING=id
	$(CLAWPATCH) show --finding $(FINDING)

claw-fix: ## Apply one explicit Clawpatch fix. Usage: make claw-fix FINDING=id
	$(CLAWPATCH) fix --finding $(FINDING)

docs-fmt: ## Format/check docs when a formatter is available
	@echo "No docs formatter configured yet; skipping docs format"

docs-l: ## Run lightweight docs checks
	@$(PYTHON) scripts/check_markdown.py --self-test
	@$(PYTHON) scripts/check_markdown.py

web-install: ## Install web dependencies when web app exists
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -f "$(WEB_DIR)/package-lock.json" ]; then \
			$(WEB_RUN) ci; \
		else \
			$(WEB_RUN) install; \
		fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web dependency install"; \
	fi

ops-config: ## Validate production ops Compose config
	@cd ops && UPRAVA_TOOLHIVE_CONFIG=toolhive.env.example \
		$(COMPOSE) --env-file toolhive.env.example -f compose.yaml config >/dev/null
	@cd ops && UPRAVA_TOOLHIVE_CONFIG=toolhive.env.example UPRAVA_TOOLHIVE_IMAGE=uprava-toolhive:test \
		$(COMPOSE) --env-file toolhive.env.example --profile toolhive -f compose.yaml config >/dev/null

systemd-check: ## Validate product-owned systemd unit template is present
	@test -s ops/systemd/uprava-node.service.example
	@grep -q '^ExecStart=/opt/apps/uprava/current/uprava-node$$' ops/systemd/uprava-node.service.example

scripts-check: ## Run shell syntax checks for product scripts
	@set -e; \
	for script in scripts/*.sh; do \
		sh -n "$$script"; \
	done; \
	for script in ci/*.sh; do \
		bash -n "$$script"; \
	done; \
	sh scripts/check-container-runtime-users.sh; \
	$(PYTHON) scripts/check_logging_policy.py; \
	$(PYTHON) scripts/check_runtime_boundaries.py; \
	sh scripts/check-ci-policy.sh; \
	sh scripts/check-release-manifest.sh; \
	sh scripts/check-ci-phases.sh; \
	sh scripts/check-clean-bootstrap-deploy.sh; \
	sh scripts/check-deployment-rollback.sh; \
	sh scripts/check-release-retention.sh; \
	sh scripts/check-backup-restore.sh

protocol-check: ## Check Rust/Web protocol literal drift
	$(PYTHON) scripts/protocol_check.py

rust-fmt: ## Format Rust code when Cargo workspace exists
	@if [ -f "$(RUST_MANIFEST)" ]; then \
		$(CARGO) fmt --all; \
	else \
		echo "No Cargo.toml found; skipping Rust format"; \
	fi

rust-toolchain: ## Ensure Rust toolchain components used by local checks are present
	@if [ -f "$(RUST_MANIFEST)" ]; then \
		if [ -n "$(RUST_TOOLCHAIN)" ]; then \
			$(RUSTUP) default "$(RUST_TOOLCHAIN)"; \
			$(RUSTUP) component add --toolchain "$(RUST_TOOLCHAIN)" rustfmt clippy; \
		else \
			$(RUSTUP) component add rustfmt clippy; \
		fi; \
	else \
		echo "No Cargo.toml found; skipping Rust toolchain prep"; \
	fi

rust-l: ## Run Rust format check and clippy when Cargo workspace exists
	@set -e; \
	if [ -f "$(RUST_MANIFEST)" ]; then \
		$(CARGO) fmt --all -- --check; \
		$(CARGO) clippy --workspace --all-targets -- -D warnings; \
	else \
		echo "No Cargo.toml found; skipping Rust lint"; \
	fi

rust-dl: ## Run deeper Rust dependency/config checks when tools are available
	@set -e; \
	if [ -f "$(RUST_MANIFEST)" ]; then \
		require_tool() { \
			if ! command -v "$$1" >/dev/null 2>&1; then \
				echo "$$1 is not installed; run make rust-tools-install"; \
				exit 1; \
			fi; \
		}; \
		require_tool cargo-audit; \
		require_tool cargo-deny; \
		require_tool cargo-machete; \
		require_tool taplo; \
		$(CARGO) audit $(CARGO_AUDIT_IGNORE); \
		$(CARGO) deny check; \
		$(CARGO) machete; \
		$(CARGO) doc --workspace --no-deps; \
		taplo fmt --check $(RUST_TOOL_TOML_FILES); \
	else \
		echo "No Cargo.toml found; skipping deep Rust checks"; \
	fi

rust-tools-install: ## Install Rust quality tools required by rust-dl
	@set -e; \
	if [ -f "$(RUST_MANIFEST)" ]; then \
		install_tool() { \
			bin="$$1"; \
			package="$$2"; \
			if command -v "$$bin" >/dev/null 2>&1; then \
				echo "$$bin already installed"; \
			else \
				echo "Installing $$package"; \
				$(CARGO) install --locked "$$package"; \
			fi; \
		}; \
		install_tool cargo-audit cargo-audit; \
		install_tool cargo-deny cargo-deny; \
		install_tool cargo-machete cargo-machete; \
		if command -v taplo >/dev/null 2>&1; then \
			echo "taplo already installed"; \
		else \
			echo "Installing taplo-cli"; \
			$(CARGO) install --locked taplo-cli --no-default-features; \
		fi; \
	else \
		echo "No Cargo.toml found; skipping Rust quality tool install"; \
	fi

rust-t: ## Run Rust tests when Cargo workspace exists
	@if [ -f "$(RUST_MANIFEST)" ]; then \
		if $(CARGO) nextest --version >/dev/null 2>&1; then \
			$(CARGO) nextest run --workspace; \
		else \
			$(CARGO) test --workspace; \
		fi; \
	else \
		echo "No Cargo.toml found; skipping Rust tests"; \
	fi

generated-ui-t: ## Run Generated UI builder tests
	node --test services/generated-ui-builder/server.test.mjs

web-r: ## Run web development server when web app exists
	@if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(WEB_RUN) run dev; else echo "Web dependencies are not installed; run make init"; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web dev server"; \
	fi

core-r: ## Run Core Backend locally when Cargo workspace exists
	@if [ -f "$(RUST_MANIFEST)" ]; then \
		$(CARGO) run -p uprava-server; \
	else \
		echo "No Cargo.toml found; skipping Core run"; \
	fi

node-r: ## Run Node Daemon locally when Cargo workspace exists
	@if [ -f "$(RUST_MANIFEST)" ]; then \
		UPRAVA_NODE_WORKSPACES="$${UPRAVA_NODE_WORKSPACES:-$(CURDIR)}" $(CARGO) run -p uprava-node; \
	else \
		echo "No Cargo.toml found; skipping Node run"; \
	fi

task-runtime-build: ## Build the pinned Codex task runtime image
	docker build -t "$(UPRAVA_TASK_RUNTIME_DEV_IMAGE)" -f Dockerfile.codex-runtime .

task-runtime-up: task-runtime-build ## Start local OpenSandbox for Task Runs
	$(DEV_COMPOSE_CMD) --profile task-runtime up -d opensandbox

task-runtime-down: ## Stop local OpenSandbox for Task Runs
	$(DEV_COMPOSE_CMD) --profile task-runtime stop opensandbox

dev-up: ## Start local Core/Web/ToolHive/Generated UI development profile
	@set -e; \
	if [ -f "$(DEV_COMPOSE_FILE)" ]; then \
		COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build core; \
		COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build web; \
		COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build generated-ui-builder; \
		COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build toolhive; \
		COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) up --no-build; \
	else \
		echo "No $(DEV_COMPOSE_FILE) found; skipping dev up"; \
	fi

dev-down: ## Stop local development profile
	@if [ -f "$(DEV_COMPOSE_FILE)" ]; then \
		$(DEV_COMPOSE_CMD) down; \
	else \
		echo "No $(DEV_COMPOSE_FILE) found; skipping dev down"; \
	fi

dev-logs: ## Show local development logs
	@if [ -f "$(DEV_COMPOSE_FILE)" ]; then \
		$(DEV_COMPOSE_CMD) logs -f; \
	else \
		echo "No $(DEV_COMPOSE_FILE) found; skipping dev logs"; \
	fi

dev-reset: ## Remove local development state intentionally
	@if [ -f "$(DEV_COMPOSE_FILE)" ]; then \
		$(DEV_COMPOSE_CMD) down -v; \
	else \
		echo "No $(DEV_COMPOSE_FILE) found; skipping dev reset"; \
	fi

dev-smoke: ## Smoke-check local development profile
	@set -e; \
	if [ "$${SMOKE_SKIP_COMPOSE_UP:-0}" != "1" ]; then \
		if [ -f "$(DEV_COMPOSE_FILE)" ]; then \
			COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build core; \
			COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build web; \
			COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build generated-ui-builder; \
			COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) build toolhive; \
			COMPOSE_PARALLEL_LIMIT=$(COMPOSE_PARALLEL_LIMIT) $(DEV_COMPOSE_CMD) up -d --no-build; \
		else \
			echo "No $(DEV_COMPOSE_FILE) found; skipping dev startup"; \
		fi; \
	fi
	@if [ -f scripts/dev-smoke.sh ]; then \
		sh scripts/dev-smoke.sh; \
	else \
		echo "No scripts/dev-smoke.sh found; skipping dev smoke"; \
	fi

compose-up: ## Deprecated alias for dev-up
	@echo "compose-up is deprecated; use dev-up"
	@$(MAKE) --no-print-directory dev-up

compose-down: ## Deprecated alias for dev-down
	@echo "compose-down is deprecated; use dev-down"
	@$(MAKE) --no-print-directory dev-down

compose-logs: ## Deprecated alias for dev-logs
	@echo "compose-logs is deprecated; use dev-logs"
	@$(MAKE) --no-print-directory dev-logs

compose-reset: ## Deprecated alias for dev-reset
	@echo "compose-reset is deprecated; use dev-reset"
	@$(MAKE) --no-print-directory dev-reset

compose-smoke: ## Deprecated alias for dev-smoke
	@echo "compose-smoke is deprecated; use dev-smoke"
	@$(MAKE) --no-print-directory dev-smoke

codex-smoke: ## Smoke-check real Codex provider with host Core/Web/Node
	@if [ -f scripts/codex-provider-smoke.sh ]; then \
		sh scripts/codex-provider-smoke.sh; \
	else \
		echo "No scripts/codex-provider-smoke.sh found; skipping Codex provider smoke"; \
	fi

web-fmt: ## Format web app when package script exists
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(WEB_RUN) run format; else echo "Web dependencies are not installed; run make init"; exit 1; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web format"; \
	fi

web-l: ## Run web lint/typecheck when package scripts exist
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(WEB_RUN) run lint; $(WEB_RUN) run typecheck; else echo "Web dependencies are not installed; run make init"; exit 1; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web lint"; \
	fi

web-dl: ## Run web production build when package scripts exist
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(WEB_RUN) run audit:prod; $(WEB_RUN) run build; else echo "Web dependencies are not installed; run make init"; exit 1; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web build"; \
	fi

web-t: ## Run web tests when package scripts exist
	@set -e; \
	if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(WEB_RUN) run test; else echo "Web dependencies are not installed; run make init"; exit 1; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web tests"; \
	fi

web-e2e: ## Run Playwright web E2E tests when browser dependencies exist
	@if [ -f "$(WEB_PACKAGE)" ]; then \
		if [ -d "$(WEB_NODE_MODULES)" ]; then $(PLAYWRIGHT_RUN) run e2e; else echo "Web dependencies are not installed; skipping web E2E tests"; fi; \
	else \
		echo "No $(WEB_PACKAGE) found; skipping web E2E tests"; \
	fi

clean: ## Remove common local build and cache artifacts
	rm -rf target htmlcov coverage .pytest_cache .ruff_cache .mypy_cache .ty
	rm -rf $(WEB_DIR)/dist $(WEB_DIR)/coverage

.PHONY: help prepare ci-prepare ci-build ci-deploy ci-finalize build image-runtime task-runtime-build task-runtime-up task-runtime-down clean-state-restore push release-manifest install-release-manifest install-ops init fmt l dl t c push-check push-check-msrv pc claw-doctor claw-init claw-map claw-review claw-report claw-ci claw-show claw-fix docs-fmt docs-l web-install ops-config systemd-check scripts-check rust-fmt rust-l rust-dl rust-tools-install rust-t generated-ui-t web-r web-fmt web-l web-dl web-t web-e2e core-r node-r dev-up dev-down dev-logs dev-reset dev-smoke compose-up compose-down compose-logs compose-reset compose-smoke codex-smoke clean
