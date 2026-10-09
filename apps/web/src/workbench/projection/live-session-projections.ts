import type { InfiniteData, QueryClient } from "@tanstack/react-query";

import { queryKeys } from "../../shared/api/query-keys";
import type {
  AgentProjection,
  EventEnvelope,
  EventLogPage,
  InventorySnapshot,
  SessionDetail,
  SessionEvidenceProjection,
  SessionTraceProjection,
  TraceStep,
  UpravaRef,
} from "../../shared/protocol/types";

const AGENT_REFRESH_EVENT_KINDS = new Set<EventEnvelope["kind"]>([
  "runtime.ready",
  "runtime.blocked",
  "runtime.expired",
  "runtime.stopped",
  "runtime.error",
  "turn.interrupted",
  "approval.requested",
  "approval.resolved",
  "provider.interaction.requested",
  "provider.interaction.resolved",
  "coordination.warning_acknowledged",
  "workspace.validated",
  "resource.snapshot.updated",
]);

const CANONICAL_PROJECTION_EVENT_KINDS = new Set<EventEnvelope["kind"]>([
  "provider.message.completed",
  "runtime.error",
  "approval.requested",
  "approval.resolved",
  "provider.interaction.requested",
  "provider.interaction.resolved",
]);

export function applyLiveSessionProjections(
  queryClient: QueryClient,
  sessionThreadId: string,
  detail: SessionDetail,
  event: EventEnvelope,
) {
  queryClient.setQueryData<InventorySnapshot>(queryKeys.inventory, (current) =>
    patchInventory(current, detail, event),
  );
  queryClient.setQueryData<AgentProjection>(
    queryKeys.agentProjection(sessionThreadId),
    (current) => patchAgentProjection(current, detail, event),
  );
  queryClient.setQueryData<SessionEvidenceProjection>(
    queryKeys.sessionEvidenceProjection(sessionThreadId),
    (current) => patchEvidenceProjection(current, event),
  );
  queryClient.setQueryData<SessionTraceProjection>(
    queryKeys.sessionTrace(sessionThreadId),
    (current) => patchTraceProjection(current, event),
  );
  patchEventLogQueries(queryClient, sessionThreadId, event);
}

export function shouldRefreshAgentProjection(event: EventEnvelope) {
  return AGENT_REFRESH_EVENT_KINDS.has(event.kind);
}

export function shouldRefreshCanonicalProjections(event: EventEnvelope) {
  return CANONICAL_PROJECTION_EVENT_KINDS.has(event.kind);
}

function patchInventory(
  current: InventorySnapshot | undefined,
  detail: SessionDetail,
  event: EventEnvelope,
) {
  if (!current) return current;
  return {
    ...current,
    sessions: current.sessions.map((session) =>
      session.session_thread_id === detail.session.session_thread_id
        ? detail.session
        : session,
    ),
    placements: current.placements.map((placement) =>
      placement.project_placement_id === detail.placement.project_placement_id
        ? detail.placement
        : placement,
    ),
    generated_at: event.happened_at,
  };
}

function patchAgentProjection(
  current: AgentProjection | undefined,
  detail: SessionDetail,
  event: EventEnvelope,
) {
  if (!current) return current;
  let currentTurn = current.current_turn;
  if (event.kind === "turn.started") currentTurn = event.turn_id;
  if (
    event.kind === "turn.completed" ||
    event.kind === "turn.interrupted" ||
    event.kind === "runtime.error"
  ) {
    currentTurn = currentTurn === event.turn_id ? null : currentTurn;
  }
  return {
    ...current,
    project_placement: detail.placement,
    runtime_summary: detail.session.runtime,
    current_turn: currentTurn,
    generated_at: event.happened_at,
  };
}

function patchEvidenceProjection(
  current: SessionEvidenceProjection | undefined,
  event: EventEnvelope,
) {
  if (!current) return current;
  const evidenceId = `event:${event.event_id}`;
  if (current.root.children.some((child) => child.evidence_id === evidenceId)) {
    return current;
  }
  return {
    ...current,
    generated_at: event.happened_at,
    root: {
      ...current.root,
      children: [
        ...current.root.children,
        {
          evidence_id: evidenceId,
          label: evidenceLabel(event),
          primary_ref: primaryEventReference(event),
          source_refs: event.source_refs,
          evidence_refs: event.evidence_refs,
          cause_refs: event.cause_refs,
          children: [],
        },
      ],
    },
  };
}

function patchTraceProjection(
  current: SessionTraceProjection | undefined,
  event: EventEnvelope,
) {
  if (!current) return current;
  const rawEventRef = eventReference(event);
  if (current.steps.some((step) => hasReference(step.raw_refs, rawEventRef))) {
    return current;
  }
  if (event.kind === "provider.output.delta") {
    return {
      ...current,
      raw_event_count: current.raw_event_count + 1,
      generated_at: event.happened_at,
    };
  }
  if (event.kind === "provider.activity") {
    return patchActivityTrace(current, event, rawEventRef);
  }
  return {
    ...current,
    steps: [...current.steps, traceStepForEvent(event, rawEventRef)].sort(
      compareTraceSteps,
    ),
    raw_event_count: current.raw_event_count + 1,
    generated_at: event.happened_at,
  };
}

function patchActivityTrace(
  current: SessionTraceProjection,
  event: EventEnvelope,
  rawEventRef: UpravaRef,
) {
  const blockId = `trace:activity:${event.turn_id ?? event.event_id}`;
  const index = current.steps.findIndex((step) => step.block_id === blockId);
  if (index < 0) {
    return {
      ...current,
      steps: [...current.steps, activityTraceStep(event, rawEventRef)].sort(
        compareTraceSteps,
      ),
      raw_event_count: current.raw_event_count + 1,
      generated_at: event.happened_at,
    };
  }
  const previous = current.steps[index];
  const rawCount = previous.raw_refs.length;
  const summary =
    rawCount < 4
      ? [previous.summary, eventSummary(event)].filter(Boolean).join("; ")
      : previous.summary;
  const updated: TraceStep = {
    ...previous,
    summary,
    completed_at: event.happened_at,
    source_refs: [...previous.source_refs, ...event.source_refs],
    evidence_refs: [...previous.evidence_refs, ...event.evidence_refs],
    cause_refs: [...previous.cause_refs, ...event.cause_refs],
    result_refs: [...previous.result_refs, ...event.result_refs],
    raw_refs: [...previous.raw_refs, rawEventRef],
  };
  return {
    ...current,
    steps: current.steps.map((step, stepIndex) =>
      stepIndex === index ? updated : step,
    ),
    raw_event_count: current.raw_event_count + 1,
    generated_at: event.happened_at,
  };
}

function traceStepForEvent(
  event: EventEnvelope,
  rawEventRef: UpravaRef,
): TraceStep {
  return {
    block_id: `trace:event:${event.event_id}`,
    title: event.kind,
    summary: eventSummary(event),
    actor_ref: event.actor_ref,
    started_at: event.happened_at,
    completed_at: event.happened_at,
    precision:
      event.source_refs.length > 0 ||
      event.evidence_refs.length > 0 ||
      event.cause_refs.length > 0
        ? "exact"
        : "unknown",
    primary_ref: rawEventRef,
    source_refs: event.source_refs,
    evidence_refs: event.evidence_refs,
    cause_refs: event.cause_refs,
    result_refs: event.result_refs,
    raw_refs: [rawEventRef],
  };
}

function activityTraceStep(
  event: EventEnvelope,
  rawEventRef: UpravaRef,
): TraceStep {
  return {
    ...traceStepForEvent(event, rawEventRef),
    block_id: `trace:activity:${event.turn_id ?? event.event_id}`,
    title: "Provider activity",
    precision: "coarse",
  };
}

function patchEventLogQueries(
  queryClient: QueryClient,
  sessionThreadId: string,
  event: EventEnvelope,
) {
  const queries = queryClient.getQueryCache().findAll({
    queryKey: queryKeys.eventLogRoot(sessionThreadId),
  });
  for (const query of queries) {
    const kind = typeof query.queryKey[2] === "string" ? query.queryKey[2] : "";
    if (kind && kind !== event.kind) continue;
    queryClient.setQueryData<InfiniteData<EventLogPage>>(
      query.queryKey,
      (current) => {
        if (!current || current.pages.length === 0) return current;
        if (
          current.pages.some((page) =>
            page.events.some(
              (candidate) => candidate.event_id === event.event_id,
            ),
          )
        ) {
          return current;
        }
        const [first, ...rest] = current.pages;
        return {
          ...current,
          pages: [{ ...first, events: [event, ...first.events] }, ...rest],
        };
      },
    );
  }
}

function eventReference(event: EventEnvelope): UpravaRef {
  return {
    kind: "event",
    event_id: event.event_id,
    scope_ref: event.scope_ref,
    seq: event.seq,
  };
}

function primaryEventReference(event: EventEnvelope): UpravaRef {
  if (
    (event.kind === "approval.requested" ||
      event.kind === "approval.resolved") &&
    "approval_id" in event.payload &&
    typeof event.payload.approval_id === "string"
  ) {
    return { kind: "approval", approval_id: event.payload.approval_id };
  }
  return eventReference(event);
}

function evidenceLabel(event: EventEnvelope) {
  if (event.kind === "approval.requested" && "prompt" in event.payload) {
    return `Approval requested: ${truncate(event.payload.prompt, 80)}`;
  }
  if (event.kind === "approval.resolved") return "Approval resolved";
  if (
    event.kind === "runtime.error" &&
    "message" in event.payload &&
    typeof event.payload.message === "string"
  ) {
    return `Runtime error: ${truncate(event.payload.message, 80)}`;
  }
  return `${event.kind} #${event.seq}`;
}

function eventSummary(event: EventEnvelope) {
  const payload = event.payload as unknown as Record<string, unknown>;
  for (const key of ["summary", "message", "content", "prompt", "question"]) {
    const value = payload[key];
    if (typeof value === "string") return truncate(value, 500);
  }
  return event.kind;
}

function truncate(value: string, maxChars: number) {
  return Array.from(value).slice(0, maxChars).join("");
}

function hasReference(references: UpravaRef[], expected: UpravaRef) {
  return references.some(
    (reference) =>
      reference.kind === "event" &&
      expected.kind === "event" &&
      reference.event_id === expected.event_id,
  );
}

function compareTraceSteps(left: TraceStep, right: TraceStep) {
  return (
    left.started_at.localeCompare(right.started_at) ||
    left.block_id.localeCompare(right.block_id)
  );
}
