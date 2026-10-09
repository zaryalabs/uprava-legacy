import { describe, expect, it } from "vitest";

import { buildSessionTimelineBlocks, blockFromEvent } from "./timeline-blocks";
import type {
  EventEnvelope,
  EventKind,
  EventPayload,
  SessionDetail,
} from "../../shared/protocol/types";
import { eventPayloadTypeForKind } from "../../shared/protocol/validators";
import {
  getTimelineBlockRenderer,
  registeredTimelineBlockTypes,
} from "../../workbench/blocks/TimelineBlockRenderer";

describe("session timeline blocks", () => {
  it("maps messages and approval events to typed ui blocks", () => {
    const blocks = buildSessionTimelineBlocks(detailWithApproval());

    expect(blocks.map((item) => item.block.type)).toEqual([
      "core.assistant-message",
      "core.approval-request",
    ]);
    expect(blocks[0].block.source_refs[0]).toMatchObject({
      kind: "event",
      event_id: "event-message",
    });
    expect(blocks[1].approvalId).toBe("approval-1");
    expect(blocks[1].block.primary_ref).toEqual({
      kind: "approval",
      approval_id: "approval-1",
    });
    expect(blocks[1].block.actions).toEqual(["approval.resolve"]);
  });

  it("does not expose approval actions after a matching resolution event", () => {
    const detail = detailWithApproval();
    const blocks = buildSessionTimelineBlocks({
      ...detail,
      events: [
        ...detail.events,
        {
          ...eventWithPayload("approval.resolved", {
            approval_id: "approval-1",
            approved: true,
            message: "Approved",
          }),
          event_id: "event-approval-resolved",
          seq: 3,
          happened_at: "2026-06-17T00:00:01Z",
        },
      ],
    });

    const approvalBlock = blocks.find(
      (item) => item.block.type === "core.approval-request",
    );

    expect(approvalBlock?.approvalId).toBeUndefined();
    expect(approvalBlock?.block.actions).toEqual([]);
    expect(approvalBlock?.block.data).toMatchObject({ state: "resolved" });
  });

  it("renders newer dialogue blocks after older ones", () => {
    const detail = detailWithApproval();
    const userMessage = {
      message_id: "message-user",
      session_thread_id: "session-1",
      turn_id: "turn-1",
      role: "user" as const,
      content: "Run checks",
      created_at: "2026-06-17T00:00:00Z",
      completed_at: "2026-06-17T00:00:00Z",
      source_event_id: null,
    };
    const assistantMessage = {
      ...detail.messages[0],
      message_id: "message-assistant",
      turn_id: "turn-1",
      created_at: "2026-06-17T00:00:02Z",
      completed_at: "2026-06-17T00:00:02Z",
    };

    const blocks = buildSessionTimelineBlocks({
      ...detail,
      messages: [assistantMessage, userMessage],
      events: [
        {
          ...detail.events[0],
          turn_id: "turn-1",
          seq: 3,
          happened_at: "2026-06-17T00:00:02Z",
        },
        {
          ...detail.events[1],
          turn_id: "turn-1",
          seq: 2,
          happened_at: "2026-06-17T00:00:01Z",
        },
      ],
    });

    expect(blocks.map((item) => item.block.block_id)).toEqual([
      "message:message-user",
      "event:event-approval.requested",
      "message:message-assistant",
    ]);
  });

  it("maps known event families to registered renderer types", () => {
    const event = eventWithPayload("resource.snapshot.updated", {
      git_branch: "main",
    });

    const item = blockFromEvent(event);

    expect(item.block.type).toBe("core.resource-snapshot");
    expect(getTimelineBlockRenderer(item.block.type)?.allowedSurfaces).toEqual([
      "session.timeline",
    ]);
  });

  it("maps provider questions and resolutions to semantic interaction blocks", () => {
    const requested = blockFromEvent(
      eventWithPayload("provider.interaction.requested", {
        provider_interaction_id: "interaction-1",
        runtime_attempt_id: "attempt-1",
        interaction_kind: "user_input",
        prompt: "Which target should I use?",
        expires_at: null,
      }),
    );
    const resolved = blockFromEvent(
      eventWithPayload("provider.interaction.resolved", {
        provider_interaction_id: "interaction-1",
        runtime_attempt_id: "attempt-1",
        interaction_kind: "user_input",
        approved: null,
        answers: ["staging"],
      }),
    );

    expect(requested.block.type).toBe("core.provider-interaction");
    expect(requested.block.primary_ref).toEqual({
      kind: "provider_interaction",
      provider_interaction_id: "interaction-1",
    });
    expect(requested.block.data).toMatchObject({
      interactionKind: "user_input",
      state: "requested",
    });
    expect(resolved.block.data).toMatchObject({
      state: "resolved",
      answered: true,
    });
  });

  it("groups runtime bootstrap before the first user message", () => {
    const detail = detailWithApproval();
    const blocks = buildSessionTimelineBlocks({
      ...detail,
      messages: [
        {
          message_id: "message-user",
          session_thread_id: "session-1",
          turn_id: "turn-1",
          role: "user",
          content: "Hello",
          created_at: "2026-06-17T00:00:02Z",
          completed_at: "2026-06-17T00:00:02Z",
          source_event_id: null,
        },
      ],
      events: [
        {
          ...eventWithPayload("runtime.starting", {}),
          event_id: "event-runtime-starting",
          seq: 1,
          happened_at: "2026-06-17T00:00:00Z",
        },
        {
          ...eventWithPayload("runtime.ready", {}),
          event_id: "event-runtime-ready",
          seq: 2,
          happened_at: "2026-06-17T00:00:01Z",
        },
      ],
    });

    expect(blocks.map((item) => item.block.type)).toEqual([
      "core.session-activity",
      "core.user-message",
    ]);
    expect(blocks[0].block.data).toMatchObject({
      completed: true,
      eventCount: 2,
    });
  });

  it("groups provider activity events into one turn block before the assistant message", () => {
    const providerMessage = {
      ...eventWithPayload("provider.message.completed", {
        content: "Done",
      }),
      event_id: "event-assistant",
      turn_id: "turn-1",
      seq: 5,
      happened_at: "2026-06-17T00:00:04Z",
    };
    const detail = {
      ...detailWithApproval(),
      messages: [
        {
          message_id: "message-user",
          session_thread_id: "session-1",
          turn_id: "turn-1",
          role: "user" as const,
          content: "Run checks",
          created_at: "2026-06-17T00:00:00Z",
          completed_at: "2026-06-17T00:00:00Z",
          source_event_id: null,
        },
        {
          message_id: "message-assistant",
          session_thread_id: "session-1",
          turn_id: "turn-1",
          role: "assistant" as const,
          content: "Done",
          created_at: "2026-06-17T00:00:04Z",
          completed_at: "2026-06-17T00:00:04Z",
          source_event_id: "event-assistant",
        },
      ],
      events: [
        {
          ...eventWithPayload("turn.started", {}),
          event_id: "event-turn-started",
          turn_id: "turn-1",
          seq: 2,
          happened_at: "2026-06-17T00:00:01Z",
        },
        {
          ...eventWithPayload("provider.activity", {
            provider_event_type: "item.completed",
            provider_item_type: "command_execution",
            status: "completed",
            summary: "make c",
            raw_event: {
              type: "item.completed",
              unknown_future_field: true,
            },
          }),
          event_id: "event-activity-1",
          turn_id: "turn-1",
          seq: 3,
          happened_at: "2026-06-17T00:00:02Z",
        },
        {
          ...eventWithPayload("provider.activity", {
            provider_event_type: "stderr",
            status: "warning",
            summary: "warning",
          }),
          event_id: "event-activity-2",
          turn_id: "turn-1",
          seq: 4,
          happened_at: "2026-06-17T00:00:03Z",
        },
        providerMessage,
        {
          ...eventWithPayload("turn.completed", {}),
          event_id: "event-turn-completed",
          turn_id: "turn-1",
          seq: 6,
          happened_at: "2026-06-17T00:00:05Z",
        },
      ],
    };

    const blocks = buildSessionTimelineBlocks(detail);

    expect(blocks.map((item) => item.block.block_id)).toEqual([
      "message:message-user",
      "turn-activity:turn-1",
      "message:message-assistant",
    ]);
    expect(blocks[1].block.type).toBe("core.turn-activity");
    expect(blocks[1].block.data).toMatchObject({
      turnId: "turn-1",
      eventCount: 4,
      providerEventCount: 2,
      commandCount: 1,
      warningErrorCount: 1,
      completed: true,
    });
  });

  it("keeps the v01 renderer registry explicit", () => {
    expect(registeredTimelineBlockTypes()).toContain("core.session-activity");
    expect(registeredTimelineBlockTypes()).toContain("core.turn-activity");
    expect(registeredTimelineBlockTypes()).toContain("core.approval-request");
    expect(registeredTimelineBlockTypes()).toContain("core.unknown");
  });
});

function detailWithApproval(): SessionDetail {
  const messageEvent = eventWithPayload("provider.message.completed", {
    content: "Done",
  });
  const approvalEvent = eventWithPayload("approval.requested", {
    approval_id: "approval-1",
    prompt: "Allow command?",
  });

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
        state: "blocked",
        resume_supported: true,
        degraded_reason: null,
        last_runtime_step_at: null,
      },
      message_count: 1,
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
    messages: [
      {
        message_id: "message-1",
        session_thread_id: "session-1",
        turn_id: null,
        role: "assistant",
        content: "Done",
        created_at: "2026-06-17T00:00:00Z",
        completed_at: "2026-06-17T00:00:01Z",
        source_event_id: messageEvent.event_id,
      },
    ],
    events: [messageEvent, approvalEvent],
  };
}

function eventWithPayload(kind: EventKind, payload: unknown): EventEnvelope {
  return {
    event_id:
      kind === "provider.message.completed" ? "event-message" : `event-${kind}`,
    command_id: null,
    actor_ref: { kind: "system" },
    scope_ref: { kind: "runtime", runtime_session_id: "runtime-1" },
    node_id: "node-1",
    runtime_session_id: "runtime-1",
    session_thread_id: "session-1",
    turn_id: null,
    seq: kind === "provider.message.completed" ? 1 : 2,
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
