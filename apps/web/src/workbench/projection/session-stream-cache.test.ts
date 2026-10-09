import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { queryKeys } from "../../shared/api/query-keys";
import type {
  EventEnvelope,
  EventKind,
  EventPayload,
  InventorySnapshot,
  SessionDetail,
  SessionTraceProjection,
} from "../../shared/protocol/types";
import { eventPayloadTypeForKind } from "../../shared/protocol/validators";
import { applySessionStreamEventToCache } from "./session-stream-cache";

describe("applySessionStreamEventToCache", () => {
  it("applies a completed message and refreshes only canonical projections", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(1));

    const result = await applySessionStreamEventToCache(
      queryClient,
      "session-1",
      eventWithSeq(2, "provider.message.completed", {
        content: "Assistant reply",
      }),
    );

    const detail = queryClient.getQueryData<SessionDetail>(
      queryKeys.session("session-1"),
    );
    expect(result).toEqual({ kind: "applied" });
    expect(detail?.events.map((event) => event.seq)).toEqual([1, 2]);
    expect(detail?.messages.at(-1)).toMatchObject({
      role: "assistant",
      content: "Assistant reply",
    });
    expect(isInvalidated(queryClient, queryKeys.session("session-1"))).toBe(
      false,
    );
    expect(
      isInvalidated(
        queryClient,
        queryKeys.sessionEvidenceProjection("session-1"),
      ),
    ).toBe(true);
    expect(
      isInvalidated(queryClient, queryKeys.agentProjection("session-1")),
    ).toBe(false);
    expect(
      isInvalidated(queryClient, queryKeys.sessionTrace("session-1")),
    ).toBe(true);
    expect(
      isInvalidated(queryClient, queryKeys.eventLog("session-1", "")),
    ).toBe(false);
    expect(isInvalidated(queryClient, queryKeys.inventory)).toBe(false);
    expect(
      queryClient.getQueryData<SessionDetail>(queryKeys.session("session-1"))
        ?.session.message_count,
    ).toBe(1);
  });

  it("projects high-frequency provider activity without invalidating snapshots", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(1));

    const result = await applySessionStreamEventToCache(
      queryClient,
      "session-1",
      eventWithSeq(2, "provider.activity", { summary: "Reading files" }),
    );

    expect(result).toEqual({ kind: "applied" });
    expect(isInvalidated(queryClient, queryKeys.inventory)).toBe(false);
    expect(
      isInvalidated(queryClient, queryKeys.agentProjection("session-1")),
    ).toBe(false);
    expect(
      isInvalidated(
        queryClient,
        queryKeys.sessionEvidenceProjection("session-1"),
      ),
    ).toBe(false);
    expect(
      isInvalidated(queryClient, queryKeys.sessionTrace("session-1")),
    ).toBe(false);
    expect(
      queryClient.getQueryData<InventorySnapshot>(queryKeys.inventory)
        ?.sessions[0]?.runtime.last_runtime_step_at,
    ).toBe("2026-06-17T00:00:00Z");
    expect(
      queryClient.getQueryData<SessionTraceProjection>(
        queryKeys.sessionTrace("session-1"),
      )?.steps[0]?.summary,
    ).toBe("Reading files");
  });

  it("adds a pending provider question immediately and refreshes actions", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(1));

    const result = await applySessionStreamEventToCache(
      queryClient,
      "session-1",
      eventWithSeq(2, "provider.interaction.requested", {
        provider_interaction_id: "interaction-1",
        runtime_attempt_id: "attempt-1",
        interaction_kind: "user_input",
        prompt: "Which target?",
        expires_at: null,
      }),
    );

    expect(result).toEqual({ kind: "applied" });
    expect(
      queryClient.getQueryData<SessionDetail>(queryKeys.session("session-1"))
        ?.pending_interactions,
    ).toEqual([
      expect.objectContaining({ provider_interaction_id: "interaction-1" }),
    ]);
    expect(
      isInvalidated(queryClient, queryKeys.agentProjection("session-1")),
    ).toBe(true);
  });

  it("keeps one hundred live activity events on the push path", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(1));

    for (let seq = 2; seq <= 101; seq += 1) {
      const event = eventWithSeq(seq, "provider.activity", {
        summary: `Activity ${seq}`,
      });
      event.turn_id = "turn-1";
      await applySessionStreamEventToCache(queryClient, "session-1", event);
    }

    const trace = queryClient.getQueryData<SessionTraceProjection>(
      queryKeys.sessionTrace("session-1"),
    );
    expect(trace?.raw_event_count).toBe(101);
    expect(trace?.steps).toHaveLength(1);
    expect(isInvalidated(queryClient, queryKeys.inventory)).toBe(false);
    expect(
      isInvalidated(queryClient, queryKeys.agentProjection("session-1")),
    ).toBe(false);
  });

  it("keeps cached session data and invalidates all snapshots on sequence gap", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(1));

    const result = await applySessionStreamEventToCache(
      queryClient,
      "session-1",
      eventWithSeq(3),
    );

    const detail = queryClient.getQueryData<SessionDetail>(
      queryKeys.session("session-1"),
    );
    expect(result).toEqual({
      kind: "reloaded",
      reason: "sequence-gap",
      expectedSeq: 2,
      receivedSeq: 3,
    });
    expect(detail?.events.map((event) => event.seq)).toEqual([1]);
    expect(isInvalidated(queryClient, queryKeys.session("session-1"))).toBe(
      true,
    );
    expect(
      isInvalidated(
        queryClient,
        queryKeys.sessionEvidenceProjection("session-1"),
      ),
    ).toBe(true);
    expect(
      isInvalidated(queryClient, queryKeys.agentProjection("session-1")),
    ).toBe(true);
    expect(isInvalidated(queryClient, queryKeys.inventory)).toBe(true);
  });

  it("applies projected contiguous events when raw seq moves backward", async () => {
    const queryClient = queryClientWithSnapshots(detailWithSeq(5, 1));

    const result = await applySessionStreamEventToCache(
      queryClient,
      "session-1",
      eventWithSeq(1, "coordination.warning_acknowledged", {}, 2),
    );

    expect(result).toEqual({ kind: "applied" });
  });
});

function queryClientWithSnapshots(detail: SessionDetail) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(queryKeys.session("session-1"), detail);
  queryClient.setQueryData(queryKeys.sessionEvidenceProjection("session-1"), {
    session_thread_id: "session-1",
    root: {
      evidence_id: "session:session-1",
      label: "Session",
      primary_ref: { kind: "session", session_thread_id: "session-1" },
      source_refs: [],
      evidence_refs: [],
      cause_refs: [],
      children: [],
    },
    generated_at: "2026-06-17T00:00:01Z",
  });
  queryClient.setQueryData(queryKeys.agentProjection("session-1"), {
    session_thread_id: "session-1",
    project_placement: detail.placement,
    runtime_summary: detail.session.runtime,
    current_turn: null,
    pending_approvals: [],
    active_warnings: [],
    recent_turn_summaries: [],
    recent_message_refs: [],
    evidence_projection_summary: "",
    available_block_types: [],
    available_commands: [],
    visible_refs: [],
    source_cause_summary: "",
    resume_context: "",
    generated_at: "2026-06-17T00:00:01Z",
  });
  queryClient.setQueryData(queryKeys.sessionTrace("session-1"), {
    session_thread_id: "session-1",
    precision: "coarse",
    steps: [],
    raw_event_count: 1,
    generated_at: "2026-06-17T00:00:01Z",
  });
  queryClient.setQueryData(queryKeys.eventLog("session-1", ""), {
    pages: [{ events: [], next_cursor: null }],
    pageParams: [undefined],
  });
  queryClient.setQueryData(queryKeys.inventory, {
    nodes: [],
    placements: [detail.placement],
    sessions: [detail.session],
    generated_at: "2026-06-17T00:00:01Z",
  });
  return queryClient;
}

function isInvalidated(queryClient: QueryClient, queryKey: readonly unknown[]) {
  return queryClient.getQueryState(queryKey)?.isInvalidated ?? false;
}

function detailWithSeq(
  seq: number,
  sessionProjectionSeq?: number,
): SessionDetail {
  return {
    session: {
      session_thread_id: "session-1",
      project_placement_id: "placement-1",
      runtime_session_id: "runtime-1",
      title: "Session",
      state: "active",
      runtime: {
        runtime_session_id: "runtime-1",
        provider: "codex",
        state: "ready",
        resume_supported: true,
        degraded_reason: null,
        last_runtime_step_at: null,
      },
      message_count: 0,
      updated_at: "2026-06-17T00:00:00Z",
    },
    placement: {
      project_placement_id: "placement-1",
      project_id: null,
      node_id: "node-1",
      display_name: "uprava",
      workspace_path: "/workspace",
      state: "validated",
      resource_badges: [],
      last_validated_at: null,
    },
    messages: [],
    events: [eventWithSeq(seq, "runtime.ready", {}, sessionProjectionSeq)],
  };
}

function eventWithSeq(
  seq: number,
  kind: EventKind = "runtime.ready",
  payload: unknown = {},
  sessionProjectionSeq?: number,
): EventEnvelope {
  return {
    event_id: `event-${seq}`,
    command_id: null,
    actor_ref: { kind: "system" },
    scope_ref: { kind: "runtime", runtime_session_id: "runtime-1" },
    node_id: "node-1",
    runtime_session_id: "runtime-1",
    session_thread_id: "session-1",
    turn_id: null,
    seq,
    session_projection_seq: sessionProjectionSeq,
    kind,
    happened_at: "2026-06-17T00:00:00Z",
    source_refs: [],
    evidence_refs: [],
    cause_refs: [],
    result_refs: [],
    payload: typedPayload(kind, payload),
  };
}

function typedPayload(kind: EventKind, payload: unknown): EventPayload {
  const fields =
    typeof payload === "object" && payload !== null && !Array.isArray(payload)
      ? payload
      : {};
  return { type: eventPayloadTypeForKind(kind), ...fields } as EventPayload;
}
