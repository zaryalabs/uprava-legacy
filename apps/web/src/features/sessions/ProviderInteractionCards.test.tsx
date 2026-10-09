import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { coreApi } from "../../shared/api/http-client";
import type {
  ProviderInteractionSummary,
  SessionDetail,
} from "../../shared/protocol/types";
import { ProviderInteractionCards } from "./ProviderInteractionCards";

describe("ProviderInteractionCards", () => {
  afterEach(() => vi.restoreAllMocks());

  it("submits a typed provider answer once through the interaction endpoint", async () => {
    const submit = vi.spyOn(coreApi, "submitProviderInput").mockResolvedValue({
      command_id: "command-1",
      session: null,
    });
    renderCards(interaction("user_input", "requested"), [
      "providerInteraction.submitInput",
    ]);

    fireEvent.change(screen.getByRole("textbox", { name: "Your answer" }), {
      target: { value: "Use the staging target" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Submit answer" }));

    await waitFor(() =>
      expect(submit).toHaveBeenCalledWith("session-1", "interaction-1", {
        answers: ["Use the staging target"],
      }),
    );
  });

  it("resolves provider approvals through their own interaction identity", async () => {
    const resolve = vi
      .spyOn(coreApi, "resolveProviderApproval")
      .mockResolvedValue({ command_id: "command-2", session: null });
    renderCards(interaction("approval", "requested"), ["approval.resolve"]);

    fireEvent.change(
      screen.getByRole("textbox", { name: "Optional message to provider" }),
      { target: { value: "Checks are scoped to this workspace" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() =>
      expect(resolve).toHaveBeenCalledWith("session-1", "interaction-1", {
        approved: true,
        message: "Checks are scoped to this workspace",
      }),
    );
  });

  it("keeps resolving approvals terminal in the UI and prevents replay", () => {
    renderCards(interaction("approval", "resolving"), ["approval.resolve"]);

    expect(screen.getByText(/Decision sent/)).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Approve" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Deny" }),
    ).not.toBeInTheDocument();
  });
});

function renderCards(
  pendingInteraction: ProviderInteractionSummary,
  availableCommands: import("../../shared/protocol/types").ActionCapability[],
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ProviderInteractionCards
        detail={detail(pendingInteraction)}
        availableCommands={availableCommands}
      />
    </QueryClientProvider>,
  );
}

function interaction(
  kind: ProviderInteractionSummary["kind"],
  state: ProviderInteractionSummary["state"],
): ProviderInteractionSummary {
  return {
    provider_interaction_id: "interaction-1",
    runtime_attempt_id: "attempt-1",
    kind,
    state,
    prompt: kind === "approval" ? "Allow make c?" : "Which target?",
    requested_at: "2026-07-22T10:00:00Z",
    resolved_at: null,
  };
}

function detail(pendingInteraction: ProviderInteractionSummary): SessionDetail {
  return {
    session: {
      session_thread_id: "session-1",
      project_placement_id: "placement-1",
      runtime_session_id: "runtime-1",
      title: "Managed session",
      state: "active",
      runtime: {
        runtime_session_id: "runtime-1",
        provider: "codex",
        execution_profile: "managed",
        state: "blocked",
        resume_supported: true,
        degraded_reason: null,
        last_runtime_step_at: "2026-07-22T10:00:00Z",
      },
      message_count: 0,
      updated_at: "2026-07-22T10:00:00Z",
    },
    placement: {
      project_placement_id: "placement-1",
      project_id: "project-1",
      node_id: "node-1",
      display_name: "Workspace",
      workspace_path: "/workspace",
      state: "validated",
      resource_badges: [],
      last_validated_at: "2026-07-22T10:00:00Z",
    },
    messages: [],
    events: [],
    pending_interactions: [pendingInteraction],
  };
}
