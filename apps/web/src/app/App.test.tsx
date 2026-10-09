import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import protocolFixtures from "../shared/protocol/fixtures.json";
import { encodeUpravaRef } from "../workbench/references/refs";

vi.mock("../features/workspace-inspector/MonacoViews", () => ({
  MonacoFileEditor: ({ path }: { path: string }) => (
    <div role="region" aria-label={`File editor ${path}`} />
  ),
  MonacoDiffTextViewer: () => <div role="region" aria-label="Diff viewer" />,
  MonacoWorkspaceDiffViewer: ({ path }: { path: string }) => (
    <div role="region" aria-label={`Workspace diff ${path}`} />
  ),
}));

describe("App routes", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    window.localStorage.clear();
  });

  it("renders nodes, placement, and session routes from mocked Core snapshots", async () => {
    renderApp("/");

    expect(
      await screen.findByRole("heading", { name: "Dashboard" }),
    ).toBeVisible();
    expect(await screen.findByText("Core API")).toBeVisible();
    expect(screen.getByRole("link", { name: "Add Node" })).toHaveAttribute(
      "href",
      "/nodes/pair",
    );

    renderApp("/nodes");

    expect(
      await screen.findByRole("heading", { name: "Local Node" }),
    ).toBeVisible();
    const nodeMetrics = screen.getByRole("region", { name: "Node metrics" });
    expect(within(nodeMetrics).getByText("Last Heartbeat")).toBeVisible();
    expect(within(nodeMetrics).getByText("Workspaces")).toBeVisible();
    expect(within(nodeMetrics).getByText("Active Runtimes")).toBeVisible();
    expect(within(nodeMetrics).getByText("Running Jobs")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Diagnostics" })).toBeVisible();
    expect(screen.queryByText("Daemon version")).not.toBeInTheDocument();
    expect(screen.queryByText("Platform")).not.toBeInTheDocument();

    renderApp("/nodes/pair");

    expect(
      await screen.findByRole("heading", { name: "Pair Node", level: 1 }),
    ).toBeVisible();
    expect(screen.getByText("not production-secure")).toBeVisible();

    renderApp("/workspaces/placement-1");

    expect(
      await screen.findByRole(
        "heading",
        { name: "Uprava" },
        { timeout: 5_000 },
      ),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Agent" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/agent",
    );
    expect(screen.getByRole("link", { name: "Workbench" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/workbench",
    );
    expect(await screen.findByRole("heading", { name: "Agent" })).toBeVisible();
    const startCompatibility = await screen.findByRole("button", {
      name: "Start Exec compatibility",
    });
    expect(startCompatibility).toBeDisabled();
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: /I understand this mode is unrestricted/,
      }),
    );
    expect(startCompatibility).toBeEnabled();
    const selectedSession = screen.getByRole("link", { name: /Fix issue/ });
    expect(selectedSession).toHaveAttribute("aria-current", "page");
    expect(
      within(selectedSession).getByText("Lifecycle: Active"),
    ).toBeVisible();
    expect(
      within(selectedSession).getByText("Attention: Blocked"),
    ).toBeVisible();
    expect(
      screen.getByRole("img", { name: "Attention: Dirty workspace" }),
    ).toBeVisible();

    renderApp("/workspaces/placement-1/workbench");

    expect(
      await screen.findByRole("heading", { name: "Uprava" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Start Codex" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { name: "Workbench", level: 2 }),
    ).toBeVisible();
    expect(workspaceReviewRequests).toBe(0);
    fireEvent.click(screen.getByRole("tab", { name: "Review" }));
    expect(await screen.findByText("main")).toBeVisible();
    expect(workspaceReviewRequests).toBe(1);
    fireEvent.click(await screen.findByRole("button", { name: /README\.md/ }));
    expect(
      await screen.findByRole("region", {
        name: "Workspace diff README.md",
      }),
    ).toBeVisible();
    expect(workspaceReviewRequests).toBe(2);
    expect(screen.queryByText("No commands recorded")).not.toBeInTheDocument();
    expect((await screen.findAllByText("README.md")).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("treeitem", { name: "README.md" }));
    expect(
      await screen.findByRole(
        "region",
        { name: "File editor README.md" },
        { timeout: 15_000 },
      ),
    ).toBeVisible();

    renderApp("/workspaces/placement-1");
    expect(
      await screen.findByRole("heading", { name: "Workbench", level: 2 }),
    ).toBeVisible();

    renderApp("/projects/project-1");

    expect(
      await screen.findByRole("heading", { name: "Uprava" }),
    ).toBeVisible();
    expect(screen.getByText("/workspace/uprava")).toBeVisible();

    renderApp("/sessions/session-1");

    expect(
      await screen.findByRole("heading", { name: "Fix issue" }),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        screen
          .getAllByText("Assistant reply")
          .some((element) => element.closest(".uprava-markdown") !== null),
      ).toBe(true),
    );
    expect(screen.getByRole("link", { name: "Workspace" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/agent",
    );
    fireEvent.click(screen.getByText("Session details"));
    expect(
      (await screen.findAllByText("Session evidence projection"))[0],
    ).toBeVisible();
    expect(await screen.findByText("session.sendTurn")).toBeVisible();
    expect(MockEventSource.created).toBe(1);
    MockEventSource.latest?.emit("uprava.event", {
      ...messageEvent,
      event_id: "event-streamed",
      seq: 2,
      session_projection_seq: 2,
      payload: {
        type: "provider_message_completed",
        content: "Streamed reply",
      },
    });
    expect(await screen.findByText("Streamed reply")).toBeVisible();
    await waitFor(() => expect(MockEventSource.created).toBe(1));

    renderApp("/settings/runtime");

    expect(
      await screen.findByRole("heading", { name: "Runtime Settings" }),
    ).toBeVisible();
    expect(await screen.findByText("uprava-core 0.1.8")).toBeVisible();
    expect(screen.getByText("v2")).toBeVisible();
    expect(screen.getByText("1")).toBeVisible();

    renderApp("/settings/plugins");

    expect(
      await screen.findByRole("heading", { name: "Plugins & Appearance" }),
    ).toBeVisible();
    expect(await screen.findByText("Dark Theme")).toBeVisible();
    expect(await screen.findByText("Markdown Renderer")).toBeVisible();
    expect(await screen.findByText("Plain Text Renderer")).toBeVisible();
    expect(await screen.findByText("Conflict")).toBeVisible();
    expect(screen.getByText("Winner")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Move uprava.plain-text up" }),
    );
    await waitFor(() =>
      expect(lastContributionPreferenceRequest).toMatchObject({
        expected_revision: 0,
        ordered_contributions: [
          { plugin_id: "uprava.plain-text" },
          { plugin_id: "uprava.markdown" },
        ],
      }),
    );
    expect(screen.getByRole("radio", { name: /Light/ })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /Dark/ }));
    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("uprava.dark"),
    );
    const darkThemeCard = screen.getByText("Dark Theme").closest("article");
    expect(darkThemeCard).not.toBeNull();
    fireEvent.click(
      within(darkThemeCard as HTMLElement).getByRole("button", {
        name: "Disable",
      }),
    );
    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("core.light"),
    );
    expect(
      screen.queryByRole("radio", { name: /Dark/ }),
    ).not.toBeInTheDocument();
  }, 45_000);

  it("opens canonical Jobs routes and resolves legacy deep links", async () => {
    renderApp("/workspaces/placement-1/jobs");
    expect(
      await screen.findByRole("heading", { name: "Background Jobs" }),
    ).toBeVisible();
    expect(
      await screen.findByRole("link", { name: /Nightly check/ }),
    ).toBeVisible();
    expect(screen.queryByText("Other workspace Job")).not.toBeInTheDocument();
    expect(screen.getByText("Select a Job")).toBeVisible();
    expect(jobsRequests).toBe(1);

    renderApp("/workspaces/placement-3/jobs");
    expect(await screen.findByText("No Jobs yet")).toBeVisible();
    expect(
      screen.getAllByRole("link", { name: "Create Job" })[0],
    ).toHaveAttribute("href", "/workspaces/placement-3/jobs/new");

    renderApp("/workspaces/placement-1/jobs/new");
    expect(await screen.findByText("New paused Job")).toBeVisible();
    expect(
      screen.queryByRole("combobox", { name: "Workspace" }),
    ).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), {
      target: { value: "Created check" },
    });
    fireEvent.change(
      screen.getByRole("textbox", { name: "Prompt / task contract" }),
      { target: { value: "Inspect workspace" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Create paused Job" }));
    expect(
      await screen.findByRole("heading", { name: "Created check" }),
    ).toBeVisible();
    expect(lastCreateJobRequest).toEqual(
      expect.objectContaining({
        name: "Created check",
        project_placement_id: "placement-1",
        prompt: "Inspect workspace",
        provider: "codex",
        schedule: { kind: "interval", minutes: 60 },
        continue_after_error: false,
      }),
    );

    renderApp("/jobs/job-1?inspect=bad-ref&tab=runs");
    expect(
      await screen.findByRole("heading", { name: "Nightly check" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Workbench" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/workbench?inspect=bad-ref&tab=runs",
    );

    renderApp("/job-runs/run-1?inspect=bad-ref");
    expect(
      await screen.findByRole("heading", { name: "Run run-1" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Open Job" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/jobs/job-1",
    );
  });

  it("preserves query parameters and corrects mismatched workspace context", async () => {
    renderApp(
      "/workspaces/placement-2/agent/session-1?inspect=bad-ref&tab=timeline",
    );

    expect(
      await screen.findByRole("heading", { name: "Fix issue" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Workbench" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/workbench?inspect=bad-ref&tab=timeline",
    );

    renderApp("/workspaces/placement-2/jobs/job-1?inspect=bad-ref");
    expect(
      await screen.findByRole("heading", { name: "Nightly check" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Agent" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/agent?inspect=bad-ref",
    );

    renderApp(
      "/workspaces/placement-2/jobs/job-2/runs/run-1?inspect=bad-ref&tab=summary",
    );
    expect(
      await screen.findByRole("heading", { name: "Run run-1" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Jobs" })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/jobs?inspect=bad-ref&tab=summary",
    );
  });

  it("starts and selects the first session from an empty workspace Agent surface", async () => {
    renderApp("/workspaces/placement-2/agent?inspect=bad-ref");

    expect(
      await screen.findByText("Start a session", { selector: ".font-medium" }),
    ).toBeVisible();
    const forceStart = screen.getByRole("checkbox", {
      name: "Force start at 5% or less provider quota",
    });
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: /I understand this mode is unrestricted/,
      }),
    );
    fireEvent.click(forceStart);
    fireEvent.click(
      screen.getByRole("button", { name: "Start Exec compatibility" }),
    );

    expect(
      await screen.findByRole("heading", { name: "Started session" }),
    ).toBeVisible();
    expect(
      screen.getByRole("link", { name: /Started session/ }),
    ).toHaveAttribute("aria-current", "page");
    expect(
      screen.getByRole("link", { name: /Started session/ }),
    ).toHaveAttribute(
      "href",
      "/workspaces/placement-2/agent/session-created?inspect=bad-ref",
    );
    expect(lastCreateSessionRequest).toEqual({
      project_placement_id: "placement-2",
      provider: "codex",
      execution_profile: "exec_compatibility",
      force: true,
    });
  });

  it("keeps one stream through shell rerenders and closes it on another surface", async () => {
    renderApp("/workspaces/placement-1/agent/session-1");

    expect(
      await screen.findByRole("heading", { name: "Fix issue" }),
    ).toBeVisible();
    await waitFor(() => expect(MockEventSource.created).toBe(1));
    fireEvent.click(screen.getByRole("button", { name: "Hide navigation" }));
    expect(MockEventSource.created).toBe(1);

    fireEvent.click(screen.getByRole("link", { name: "Jobs" }));
    expect(
      await screen.findByRole("heading", { name: "Background Jobs" }),
    ).toBeVisible();
    expect(MockEventSource.created).toBe(1);
    expect(MockEventSource.closed).toBe(1);
  });

  it("uses Dashboard when the legacy Jobs route has no workspace preference", async () => {
    renderApp("/jobs");
    expect(
      await screen.findByRole("heading", { name: "Dashboard" }),
    ).toBeVisible();
  });

  it("keeps inventory metrics and session activity when Jobs are unavailable", async () => {
    renderApp("/dashboard", { jobsFail: true });

    expect(
      await screen.findByRole("heading", { name: "Dashboard" }),
    ).toBeVisible();
    const metrics = await screen.findByRole("region", {
      name: "System metrics",
    });
    expect(within(metrics).getByText("Reachable Nodes")).toBeVisible();
    expect(within(metrics).getByText("1/1")).toBeVisible();
    expect(within(metrics).getByText("Active Runtimes")).toBeVisible();
    expect(within(metrics).getByText("Jobs unavailable")).toBeVisible();
    expect(await screen.findByText("Job activity unavailable")).toBeVisible();
    expect(screen.getByRole("link", { name: /Fix issue/ })).toHaveAttribute(
      "href",
      "/workspaces/placement-1/agent/session-1",
    );
    expect(screen.queryByText("System Overview")).not.toBeInTheDocument();
    expect(screen.queryByText("Runtime Topology")).not.toBeInTheDocument();
  });

  it("keeps shell navigation, sidebar preference, and Inspector state independent", async () => {
    renderApp("/dashboard");
    expect(
      await screen.findByRole("heading", { name: "Dashboard" }),
    ).toBeVisible();

    const primaryNavigation = screen.getByRole("navigation", {
      name: "Primary Navigation",
    });
    expect(within(primaryNavigation).getAllByRole("link")).toHaveLength(1);
    expect(
      within(primaryNavigation).getByRole("link", { name: "Dashboard" }),
    ).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Runtime Settings" }),
    ).toHaveAttribute("href", "/settings/runtime");

    const hideNavigation = screen.getByRole("button", {
      name: "Hide navigation",
    });
    expect(hideNavigation).toHaveAttribute(
      "aria-controls",
      "workspace-navigation",
    );
    expect(hideNavigation).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(hideNavigation);
    expect(
      screen.getByRole("button", { name: "Show navigation" }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByRole("complementary", {
        name: "Node and workspace navigation",
      }),
    ).not.toBeInTheDocument();

    const inspectorReference = encodeURIComponent(
      encodeUpravaRef({ kind: "node", node_id: "node-1" }),
    );
    renderApp(`/dashboard?inspect=${inspectorReference}`);
    expect(
      await screen.findByRole("complementary", { name: "Context Inspector" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Show navigation" }),
    ).toHaveAttribute("aria-expanded", "false");

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(
        screen.queryByRole("complementary", { name: "Context Inspector" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "Show navigation" }),
    ).toHaveAttribute("aria-expanded", "false");

    renderApp(`/dashboard?inspect=${inspectorReference}`);
    fireEvent.click(
      await screen.findByRole("button", { name: "Close Inspector" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("complementary", { name: "Context Inspector" }),
      ).not.toBeInTheDocument(),
    );
  });
});

function renderApp(path: string, options: { jobsFail?: boolean } = {}) {
  cleanup();
  vi.unstubAllGlobals();
  createdSession = null;
  lastCreateSessionRequest = null;
  lastCreateJobRequest = null;
  jobsRequests = 0;
  jobsShouldFail = options.jobsFail ?? false;
  workspaceReviewRequests = 0;
  pluginEnabled = true;
  lastContributionPreferenceRequest = null;
  vi.stubGlobal("fetch", vi.fn(mockFetch));
  vi.stubGlobal("EventSource", MockEventSource);
  MockEventSource.reset();

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

class MockEventSource {
  static created = 0;
  static closed = 0;
  static latest: MockEventSource | null = null;
  onerror: (() => void) | null = null;
  private listeners = new Map<string, (event: MessageEvent) => void>();

  constructor(readonly url: string) {
    MockEventSource.created += 1;
    MockEventSource.latest = this;
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners.set(type, listener);
  }

  emit(type: string, payload: unknown) {
    this.listeners.get(type)?.({
      data: JSON.stringify(payload),
    } as MessageEvent);
  }

  close() {
    MockEventSource.closed += 1;
  }

  static reset() {
    MockEventSource.created = 0;
    MockEventSource.closed = 0;
    MockEventSource.latest = null;
  }
}

let createdSession: typeof session | null = null;
let lastCreateSessionRequest: unknown = null;
let lastCreateJobRequest: unknown = null;
let jobsRequests = 0;
let jobsShouldFail = false;
let workspaceReviewRequests = 0;
let pluginEnabled = true;
let lastContributionPreferenceRequest: unknown = null;

async function mockFetch(input: RequestInfo | URL, init?: RequestInit) {
  const url = new URL(input.toString());
  if (
    url.pathname ===
      "/api/v1/plugin-contribution-targets/renderer-target-fixture" &&
    init?.method === "PUT"
  ) {
    lastContributionPreferenceRequest = JSON.parse(String(init.body));
    return jsonResponse(
      protocolFixtures.plugin_contract.effective_snapshot.resolutions[0],
    );
  }
  if (
    url.pathname === "/api/v1/sessions/policy-preview" &&
    init?.method === "POST"
  ) {
    const request = JSON.parse(String(init.body)) as {
      project_placement_id: string;
      execution_profile: "managed" | "exec_compatibility";
    };
    return jsonResponse({
      project_placement_id: request.project_placement_id,
      node_id: "node-1",
      effective_policy: {
        contract_version: 1,
        execution_profile: request.execution_profile,
        provider: "codex",
        provider_version: "0.144.1",
        provider_capabilities: ["provider.codex.exec"],
        sandbox_mode: "danger-full-access",
        approval_mode: "never",
        workspace_root: "/workspace",
        additional_writable_paths: [],
        network_posture: "provider_default",
        tool_exposure: { server_count: 0, tool_count: 0, server_names: [] },
        credential_profile_ref: null,
        unsafe_override: {
          actor: { kind: "local_user" },
          reason: "exec_compatibility",
          expires_at: "2026-07-23T00:00:00Z",
        },
        capability_metadata: {},
      },
      effective_policy_hash: "exec-policy-hash",
    });
  }
  if (url.pathname === "/api/v1/sessions" && init?.method === "POST") {
    lastCreateSessionRequest = JSON.parse(String(init.body));
    createdSession = {
      ...session,
      session_thread_id: "session-created",
      project_placement_id: "placement-2",
      runtime_session_id: "runtime-created",
      title: "Started session",
      updated_at: "2026-06-18T00:00:00Z",
      runtime: {
        ...runtime,
        runtime_session_id: "runtime-created",
        state: "ready",
      },
    };
    return jsonResponse(sessionDetailFor(createdSession, placementTwo));
  }
  if (url.pathname === "/api/v1/jobs" && init?.method === "POST") {
    lastCreateJobRequest = JSON.parse(String(init.body));
    return jsonResponse(createdJobDetail);
  }
  if (
    url.pathname === "/api/v1/plugins/uprava.theme-dark/disable" &&
    init?.method === "POST"
  ) {
    pluginEnabled = false;
    return jsonResponse(pluginInstallation());
  }
  if (url.pathname === "/api/v1/jobs") {
    jobsRequests += 1;
    if (jobsShouldFail) {
      return new Response(
        JSON.stringify({
          error_code: "jobs.unavailable",
          message: "Jobs are temporarily unavailable",
          retryable: true,
          correlation_id: "corr-jobs",
        }),
        { status: 503, headers: { "content-type": "application/json" } },
      );
    }
  }
  if (url.pathname === "/api/v1/placements/placement-1/workspace/review") {
    workspaceReviewRequests += 1;
    return jsonResponse(workspaceReviewForPath(url.searchParams.get("path")));
  }
  const payload = responseForPath(url.pathname);
  return jsonResponse(payload);
}

function responseForPath(pathname: string) {
  switch (pathname) {
    case "/api/v1/auth/status":
      return {
        auth_required: false,
        setup_required: false,
        authenticated: true,
        profile: "controlled_dev",
        security: {
          mode: "controlled_dev",
          web_auth_required: false,
          web_auth_configured: false,
          cookie_secure: false,
        },
      };
    case "/api/v1/health":
      return { status: "ok", profile: "controlled_dev" };
    case "/api/v1/version":
      return {
        name: "uprava-core",
        version: "0.1.8",
        api_version: "v2",
        schema_version: 1,
        profile: "controlled_dev",
      };
    case "/api/v1/inventory":
      return createdSession
        ? { ...inventory, sessions: [...inventory.sessions, createdSession] }
        : inventory;
    case "/api/v1/plugins":
      return {
        items: protocolFixtures.plugin_contract.plugins.items.map((plugin) =>
          plugin.package.plugin_id === "uprava.theme-dark"
            ? pluginInstallation()
            : plugin,
        ),
      };
    case "/api/v1/plugin-contributions":
      return pluginEnabled
        ? protocolFixtures.plugin_contract.effective_snapshot
        : {
            contributions:
              protocolFixtures.plugin_contract.effective_snapshot.contributions.filter(
                (contribution) => contribution.contribution.kind !== "ui_theme",
              ),
            resolutions:
              protocolFixtures.plugin_contract.effective_snapshot.resolutions,
            generated_at: "2026-07-19T12:00:00Z",
          };
    case "/api/v1/node-enrollments":
      return [];
    case "/api/v1/placements/placement-1":
      return placement;
    case "/api/v1/placements/placement-2":
      return placementTwo;
    case "/api/v1/placements/placement-3":
      return placementThree;
    case "/api/v1/placements/placement-1/workspace/tree":
      return workspaceTree;
    case "/api/v1/placements/placement-1/workspace/file":
      return workspaceFile;
    case "/api/v1/placements/placement-1/workspace/diff":
      return workspaceDiff;
    case "/api/v1/placements/placement-1/workspace/terminals":
      return workspaceTerminals;
    case "/api/v1/sessions/session-1":
      return sessionDetail;
    case "/api/v1/sessions/session-1/evidence-projection":
      return evidenceProjection;
    case "/api/v1/sessions/session-1/agent-projection":
      return agentProjection;
    case "/api/v1/sessions/session-created":
      return createdSession
        ? sessionDetailFor(createdSession, placementTwo)
        : sessionDetail;
    case "/api/v1/sessions/session-created/evidence-projection":
      return {
        ...evidenceProjection,
        session_thread_id: "session-created",
      };
    case "/api/v1/sessions/session-created/agent-projection":
      return {
        ...agentProjection,
        session_thread_id: "session-created",
        project_placement: placementTwo,
        runtime_summary: createdSession?.runtime ?? runtime,
      };
    case "/api/v1/jobs":
      return [jobSummary, otherWorkspaceJob];
    case "/api/v1/jobs/job-1":
      return jobDetail;
    case "/api/v1/jobs/job-created":
      return createdJobDetail;
    case "/api/v1/job-runs/run-1":
      return jobRun;
    case "/api/v1/provider-quota/codex":
      return {
        provider: "codex",
        state: "available",
        remaining_percent: 80,
        observed_at: "2026-06-17T00:00:00Z",
        unavailable_reason: null,
      };
    default:
      throw new Error(`Unhandled mocked Core path: ${pathname}`);
  }
}

function pluginInstallation() {
  const plugin = protocolFixtures.plugin_contract.plugins.items[0];
  return pluginEnabled
    ? plugin
    : {
        ...plugin,
        desired_state: "disabled",
        effective_state: "disabled",
      };
}

function jsonResponse(payload: unknown) {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

const runtime = {
  runtime_session_id: "runtime-1",
  provider: "codex",
  state: "blocked",
  resume_supported: true,
  degraded_reason: null,
  last_runtime_step_at: "2026-06-17T00:00:00Z",
};

const placement = {
  project_placement_id: "placement-1",
  project_id: "project-1",
  node_id: "node-1",
  display_name: "Uprava",
  workspace_path: "/workspace/uprava",
  state: "validated",
  resource_badges: [
    { kind: "dirty_workspace", severity: "warning", label: "Dirty workspace" },
  ],
  last_validated_at: "2026-06-17T00:00:00Z",
};

const placementTwo = {
  ...placement,
  project_placement_id: "placement-2",
  project_id: "project-2",
  display_name: "Other workspace",
  workspace_path: "/workspace/other",
  resource_badges: [],
};

const placementThree = {
  ...placementTwo,
  project_placement_id: "placement-3",
  project_id: "project-3",
  display_name: "Empty workspace",
  workspace_path: "/workspace/empty",
};

const session = {
  session_thread_id: "session-1",
  project_placement_id: "placement-1",
  runtime_session_id: "runtime-1",
  title: "Fix issue",
  state: "active",
  runtime,
  message_count: 2,
  updated_at: "2026-06-17T00:00:00Z",
};

const inventory = {
  nodes: [
    {
      node_id: "node-1",
      display_name: "Local Node",
      presence: "reachable",
      sleep_hint: "unknown",
      heartbeat_age_seconds: 4,
      active_runtime_count: 1,
      capabilities: [
        {
          key: "provider.codex",
          value: {
            kind: "provider",
            available: true,
            configured: true,
            mode: "exec",
            timeout_seconds: 120,
            unavailable_reason: null,
          },
        },
      ],
      diagnostics: "ok",
    },
  ],
  placements: [placement, placementTwo, placementThree],
  sessions: [session],
  generated_at: "2026-06-17T00:00:00Z",
};

const workspaceTree = {
  placement_id: "placement-1",
  truncated: false,
  total_entries: 1,
  generated_at: "2026-06-17T00:00:00Z",
  root: {
    name: ".",
    path: ".",
    kind: "directory",
    status: "directory",
    classification: "normal",
    expandable: true,
    byte_len: null,
    modified_at: null,
    children: [
      {
        name: "README.md",
        path: "README.md",
        kind: "file",
        status: "readable",
        classification: "normal",
        expandable: false,
        byte_len: 12,
        modified_at: "2026-06-17T00:00:00Z",
        children: [],
      },
    ],
  },
};

const workspaceFile = {
  placement_id: "placement-1",
  path: "README.md",
  metadata: {
    name: "README.md",
    path: "README.md",
    kind: "file",
    status: "readable",
    classification: "normal",
    expandable: false,
    byte_len: 12,
    modified_at: "2026-06-17T00:00:00Z",
    children: [],
  },
  content: "# Uprava",
  truncated: false,
  generated_at: "2026-06-17T00:00:00Z",
};

const workspaceDiff = {
  placement_id: "placement-1",
  diff_id: "diff-1",
  git_snapshot: {
    state: "ready",
    repo_id: "sha256:fixture",
    head_state: "branch",
    branch: "main",
    commit: "0123456789abcdef",
    upstream: "origin/main",
    ahead: 0,
    behind: 0,
    worktree_kind: "primary",
    operation: null,
    changed_files: [
      {
        path: "README.md",
        previous_path: null,
        index_status: null,
        worktree_status: "modified",
        conflicted: false,
        binary: false,
      },
    ],
    staged_count: 0,
    unstaged_count: 1,
    untracked_count: 0,
    conflicted_count: 0,
    truncated: false,
    generated_at: "2026-06-17T00:00:00Z",
  },
  summary: "README.md | 1 +",
  diff: "diff --git a/README.md b/README.md\n+# Uprava",
  scope: "all",
  path: null,
  changed_files: [
    {
      path: "README.md",
      previous_path: null,
      index_status: null,
      worktree_status: "modified",
      conflicted: false,
      binary: false,
    },
  ],
  hunks: [],
  original: null,
  modified: null,
  binary: false,
  summary_truncated: false,
  diff_truncated: false,
  generated_at: "2026-06-17T00:00:00Z",
};

function workspaceReviewForPath(path: string | null) {
  return {
    placement_id: "placement-1",
    git_snapshot: workspaceDiff.git_snapshot,
    diff: {
      ...workspaceDiff,
      path,
      original: path ? "# Before" : null,
      modified: path ? "# Uprava" : null,
    },
    checks: [],
    generated_at: "2026-06-17T00:00:00Z",
  };
}

const workspaceTerminals = {
  placement_id: "placement-1",
  terminals: [],
  generated_at: "2026-06-17T00:00:00Z",
};

const messageEvent = {
  event_id: "event-message",
  command_id: "command-1",
  actor_ref: { kind: "provider" },
  scope_ref: { kind: "runtime", runtime_session_id: "runtime-1" },
  node_id: "node-1",
  runtime_session_id: "runtime-1",
  session_thread_id: "session-1",
  turn_id: "turn-1",
  seq: 1,
  kind: "provider.message.completed",
  happened_at: "2026-06-17T00:00:01Z",
  source_refs: [],
  evidence_refs: [],
  cause_refs: [],
  result_refs: [],
  payload: {
    type: "provider_message_completed",
    content: "**Assistant reply**",
  },
};

const sessionDetail = {
  session,
  placement,
  messages: [
    {
      message_id: "message-assistant",
      session_thread_id: "session-1",
      turn_id: "turn-1",
      role: "assistant",
      content: "**Assistant reply**",
      created_at: "2026-06-17T00:00:01Z",
      completed_at: "2026-06-17T00:00:01Z",
      source_event_id: "event-message",
    },
  ],
  events: [messageEvent],
};

function sessionDetailFor(
  nextSession: typeof session,
  nextPlacement: typeof placement,
) {
  return {
    ...sessionDetail,
    session: nextSession,
    placement: nextPlacement,
    messages: [],
    events: [],
  };
}

const evidenceProjection = {
  session_thread_id: "session-1",
  generated_at: "2026-06-17T00:00:00Z",
  root: {
    evidence_id: "session:session-1",
    label: "Session evidence projection",
    primary_ref: { kind: "session", session_thread_id: "session-1" },
    source_refs: [],
    evidence_refs: [],
    cause_refs: [],
    children: [],
  },
};

const agentProjection = {
  session_thread_id: "session-1",
  project_placement: placement,
  runtime_summary: runtime,
  current_turn: "turn-1",
  pending_approvals: [],
  active_warnings: [],
  recent_turn_summaries: ["turn-1 running"],
  recent_message_refs: [{ kind: "message", message_id: "message-assistant" }],
  evidence_projection_summary: "Session evidence projection",
  available_block_types: ["core.assistant-message"],
  available_commands: ["session.sendTurn"],
  visible_refs: [{ kind: "session", session_thread_id: "session-1" }],
  source_cause_summary: "Known event source refs are preserved",
  resume_context: "Runtime blocked on approval",
  generated_at: "2026-06-17T00:00:00Z",
};

const jobRun = {
  job_run_id: "run-1",
  job_id: "job-1",
  trigger: "manual",
  state: "succeeded",
  scheduled_for: null,
  queued_at: "2026-06-17T00:00:00Z",
  started_at: "2026-06-17T00:00:01Z",
  finished_at: "2026-06-17T00:00:02Z",
  session_thread_id: "session-1",
  runtime_session_id: "runtime-1",
  summary: "Completed",
  terminal_reason: null,
  config_snapshot: { provider: "codex" },
  force: false,
};

const jobSummary = {
  job_id: "job-1",
  name: "Nightly check",
  project_placement_id: "placement-1",
  placement_name: "Uprava",
  provider: "codex",
  enabled: true,
  schedule: { kind: "daily", hour: 2, minute: 0 },
  timezone: "UTC",
  overlap_policy: "skip",
  continue_after_error: false,
  next_run_at: "2026-06-18T02:00:00Z",
  paused_reason: null,
  latest_run: jobRun,
  created_at: "2026-06-16T00:00:00Z",
  updated_at: "2026-06-17T00:00:02Z",
};

const jobDetail = {
  job: jobSummary,
  prompt: "Run checks",
  runs: [jobRun],
};

const otherWorkspaceJob = {
  ...jobSummary,
  job_id: "job-other",
  name: "Other workspace Job",
  project_placement_id: "placement-2",
  placement_name: "Other workspace",
  latest_run: null,
};

const createdJobDetail = {
  job: {
    ...jobSummary,
    job_id: "job-created",
    name: "Created check",
    enabled: false,
    latest_run: null,
    next_run_at: null,
  },
  prompt: "Inspect workspace",
  runs: [],
};
