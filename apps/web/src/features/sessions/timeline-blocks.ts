import type {
  EventEnvelope,
  Message,
  SessionDetail,
  UpravaRef,
} from "../../shared/protocol/types";
import type { UiBlock } from "../../workbench/blocks/types";

export type TimelineBlockItem = {
  block: UiBlock;
  approvalId?: string;
};

type OrderedTimelineBlockItem = {
  item: TimelineBlockItem;
  timestamp: string;
  seq: number;
  sourceIndex: number;
};

export function buildSessionTimelineBlocks(
  detail: SessionDetail,
): TimelineBlockItem[] {
  const eventById = new Map(
    detail.events.map((event) => [event.event_id, event]),
  );
  const activityGroups = turnActivityGroups(detail.events);
  const groupedActivityEventIds = new Set(
    activityGroups.flatMap((group) =>
      group.events.map((event) => event.event_id),
    ),
  );
  const groupedActivityTurnIds = new Set(
    activityGroups.map((group) => group.turnId),
  );
  const sessionActivity = sessionActivityGroup(detail);
  const groupedSessionEventIds = new Set(
    sessionActivity?.events.map((event) => event.event_id) ?? [],
  );
  const pendingApprovals = pendingApprovalIds(detail.events);
  const messageSourceEventIds = new Set(
    detail.messages
      .map((message) => message.source_event_id)
      .filter((eventId): eventId is string => typeof eventId === "string"),
  );
  const messageBlocks = detail.messages.map((message, sourceIndex) => {
    const sourceEvent = message.source_event_id
      ? eventById.get(message.source_event_id)
      : undefined;
    return orderedTimelineBlockItem(
      { block: blockFromMessage(message, detail.events) },
      sourceEvent?.happened_at ?? message.created_at,
      sourceEvent?.seq ?? 0,
      sourceIndex,
    );
  });
  const activityBlocks = activityGroups.map((group, sourceIndex) =>
    orderedTimelineBlockItem(
      { block: blockFromTurnActivity(group, detail.events) },
      group.events[0]?.happened_at ?? group.startedAt ?? "",
      group.events[0]?.seq ?? 0,
      detail.messages.length + sourceIndex,
    ),
  );
  const sessionActivityBlocks = sessionActivity
    ? [
        orderedTimelineBlockItem(
          { block: blockFromSessionActivity(sessionActivity) },
          sessionActivity.events[0]?.happened_at ?? "",
          sessionActivity.events[0]?.seq ?? 0,
          detail.messages.length + activityBlocks.length,
        ),
      ]
    : [];
  const eventBlocks = detail.events
    .filter(
      (event) =>
        !messageSourceEventIds.has(event.event_id) &&
        !groupedActivityEventIds.has(event.event_id) &&
        !groupedSessionEventIds.has(event.event_id) &&
        !isGroupedTurnBoundaryEvent(event, groupedActivityTurnIds),
    )
    .map((event, sourceIndex) =>
      orderedTimelineBlockItem(
        blockFromEvent(event, { pendingApprovals }),
        event.happened_at,
        event.seq,
        detail.messages.length +
          activityBlocks.length +
          sessionActivityBlocks.length +
          sourceIndex,
      ),
    );

  return [
    ...messageBlocks,
    ...sessionActivityBlocks,
    ...activityBlocks,
    ...eventBlocks,
  ]
    .sort(compareTimelineBlockItems)
    .map(({ item }) => item);
}

export function blockFromMessage(
  message: Message,
  events: EventEnvelope[],
): UiBlock {
  const sourceEvent = message.source_event_id
    ? events.find((event) => event.event_id === message.source_event_id)
    : undefined;
  const sourceRefs = sourceEvent ? [eventRef(sourceEvent)] : [];
  const type =
    message.role === "assistant"
      ? "core.assistant-message"
      : message.role === "user"
        ? "core.user-message"
        : "core.runtime-event";

  return baseBlock({
    blockId: `message:${message.message_id}`,
    type,
    primaryRef: { kind: "message", message_id: message.message_id },
    sourceRefs,
    data: {
      role: message.role,
      content: message.content,
      createdAt: message.created_at,
      completedAt: message.completed_at,
    },
    fallbackText: message.content,
  });
}

export function blockFromEvent(
  event: EventEnvelope,
  options: { pendingApprovals?: Set<string> } = {},
): TimelineBlockItem {
  if (
    event.payload.type === "provider_interaction_requested" ||
    event.payload.type === "provider_interaction_resolved"
  ) {
    const payload = event.payload;
    const requested = payload.type === "provider_interaction_requested";
    return {
      block: baseBlock({
        blockId: `event:${event.event_id}`,
        type: "core.provider-interaction",
        primaryRef: {
          kind: "provider_interaction",
          provider_interaction_id: payload.provider_interaction_id,
        },
        sourceRefs: [eventRef(event)],
        data: {
          providerInteractionId: payload.provider_interaction_id,
          interactionKind: payload.interaction_kind,
          prompt: requested ? payload.prompt : null,
          state: requested ? "requested" : "resolved",
          approved: requested ? null : payload.approved,
          answered: requested ? false : payload.answers.length > 0,
          seq: event.seq,
          happenedAt: event.happened_at,
        },
        fallbackText: requested
          ? payload.prompt
          : `Provider ${payload.interaction_kind} resolved`,
      }),
    };
  }
  const approvalId = approvalIdFromEvent(event);
  if (approvalId) {
    const pending = options.pendingApprovals?.has(approvalId) ?? true;
    return {
      block: baseBlock({
        blockId: `event:${event.event_id}`,
        type: "core.approval-request",
        primaryRef: { kind: "approval", approval_id: approvalId },
        sourceRefs: [eventRef(event)],
        data: {
          approvalId,
          prompt: approvalPromptFromEvent(event),
          eventKind: event.kind,
          seq: event.seq,
          happenedAt: event.happened_at,
          state: pending ? "pending" : "resolved",
        },
        actions: pending ? ["approval.resolve"] : [],
        fallbackText: approvalPromptFromEvent(event),
      }),
      approvalId: pending ? approvalId : undefined,
    };
  }

  const type = blockTypeForEvent(event);
  const summary = payloadSummary(event.payload);

  return {
    block: baseBlock({
      blockId: `event:${event.event_id}`,
      type,
      primaryRef: primaryRefForEvent(event),
      sourceRefs: [eventRef(event), ...event.source_refs],
      evidenceRefs: event.evidence_refs,
      causeRefs: event.cause_refs,
      data: {
        eventKind: event.kind,
        seq: event.seq,
        happenedAt: event.happened_at,
        commandId: event.command_id,
        summary,
      },
      fallbackText: `${event.kind} seq ${event.seq}`,
    }),
  };
}

type TurnActivityGroup = {
  turnId: string;
  commandId?: string;
  startedAt?: string;
  completedAt?: string;
  terminalKind?: string;
  events: EventEnvelope[];
};

function turnActivityGroups(events: EventEnvelope[]): TurnActivityGroup[] {
  const turnCommandIds = new Map<string, string>();
  for (const event of events) {
    if (event.turn_id && event.command_id) {
      turnCommandIds.set(event.turn_id, event.command_id);
    }
  }
  const turnIds = new Set(
    events
      .filter(
        (event) =>
          Boolean(event.turn_id) &&
          (event.kind === "turn.started" || event.kind === "provider.activity"),
      )
      .map((event) => event.turn_id as string),
  );

  return Array.from(turnIds)
    .map((turnId) => {
      const commandId = turnCommandIds.get(turnId);
      const groupEvents = events.filter(
        (event) =>
          isTurnActivityEvent(event) &&
          (event.turn_id === turnId ||
            Boolean(commandId && event.command_id === commandId)),
      );
      const sortedEvents = [...groupEvents].sort(compareEvents);
      const terminalEvent = [...sortedEvents]
        .reverse()
        .find((event) => isTurnTerminalEvent(event));
      return {
        turnId,
        commandId,
        startedAt: eventTime(events, turnId, "turn.started"),
        completedAt: terminalEvent?.happened_at,
        terminalKind: terminalEvent?.kind,
        events: sortedEvents,
      };
    })
    .filter((group) => group.events.length > 0)
    .sort((left, right) => compareEvents(left.events[0]!, right.events[0]!));
}

function isTurnActivityEvent(event: EventEnvelope) {
  return (
    event.kind === "runtime.running" ||
    event.kind === "runtime.ready" ||
    event.kind === "runtime.blocked" ||
    event.kind === "runtime.error" ||
    event.kind === "runtime.stopped" ||
    event.kind === "turn.started" ||
    event.kind === "turn.completed" ||
    event.kind === "turn.interrupted" ||
    event.kind === "provider.activity" ||
    event.kind.startsWith("provider.output.")
  );
}

function isTurnTerminalEvent(event: EventEnvelope) {
  return (
    event.kind === "turn.completed" ||
    event.kind === "turn.interrupted" ||
    event.kind === "runtime.blocked" ||
    event.kind === "runtime.error" ||
    event.kind === "runtime.stopped"
  );
}

type SessionActivityGroup = {
  events: EventEnvelope[];
  completed: boolean;
};

function sessionActivityGroup(
  detail: SessionDetail,
): SessionActivityGroup | null {
  const firstUserMessageAt = detail.messages
    .filter((message) => message.role === "user")
    .map((message) => message.created_at)
    .sort()[0];
  const events = detail.events
    .filter(
      (event) =>
        !event.turn_id &&
        (event.kind === "runtime.starting" || event.kind === "runtime.ready") &&
        (!firstUserMessageAt || event.happened_at <= firstUserMessageAt),
    )
    .sort(compareEvents);
  if (events.length === 0) return null;
  return {
    events,
    completed: events.some((event) => event.kind === "runtime.ready"),
  };
}

function blockFromSessionActivity(group: SessionActivityGroup): UiBlock {
  const first = group.events[0];
  if (!first) {
    throw new Error("session activity group must contain at least one event");
  }
  const last = group.events.at(-1);
  return baseBlock({
    blockId: `session-activity:${first.event_id}`,
    type: "core.session-activity",
    primaryRef: eventRef(first),
    sourceRefs: group.events.map(eventRef),
    data: {
      completed: group.completed,
      startedAt: first.happened_at,
      completedAt: group.completed ? (last?.happened_at ?? null) : null,
      eventCount: group.events.length,
      rows: group.events.map(turnActivityRow),
    },
    fallbackText: group.completed
      ? "Session initialized"
      : "Session initialization in progress",
  });
}

function blockFromTurnActivity(
  group: TurnActivityGroup,
  allEvents: EventEnvelope[],
): UiBlock {
  const rows = group.events.map(turnActivityRow);
  const counts = turnActivityCounts(rows);
  const durationMs =
    group.startedAt && group.completedAt
      ? new Date(group.completedAt).getTime() -
        new Date(group.startedAt).getTime()
      : null;

  return baseBlock({
    blockId: `turn-activity:${group.turnId}`,
    type: "core.turn-activity",
    primaryRef: { kind: "turn", turn_id: group.turnId },
    sourceRefs: group.events.map(eventRef),
    causeRefs: allEvents
      .filter(
        (event) =>
          event.turn_id === group.turnId &&
          (event.kind === "turn.started" || event.kind === "turn.completed"),
      )
      .map(eventRef),
    data: {
      turnId: group.turnId,
      startedAt: group.startedAt ?? null,
      completedAt: group.completedAt ?? null,
      completed: Boolean(group.completedAt),
      terminalKind: group.terminalKind ?? null,
      lastObservedAt:
        group.events.at(-1)?.happened_at ?? group.startedAt ?? null,
      durationMs: durationMs !== null && durationMs >= 0 ? durationMs : null,
      eventCount: rows.length,
      providerEventCount: group.events.filter(
        (event) => event.kind === "provider.activity",
      ).length,
      commandCount: counts.commandCount,
      fileChangeCount: counts.fileChangeCount,
      reasoningCount: counts.reasoningCount,
      warningErrorCount: counts.warningErrorCount,
      rows,
    },
    fallbackText: `${rows.length} provider activity events`,
  });
}

function isGroupedTurnBoundaryEvent(
  event: EventEnvelope,
  groupedActivityTurnIds: Set<string>,
) {
  return Boolean(
    event.turn_id &&
    groupedActivityTurnIds.has(event.turn_id) &&
    (event.kind === "turn.started" || event.kind === "turn.completed"),
  );
}

function turnActivityRow(event: EventEnvelope) {
  const payload =
    event.payload.type === "provider_activity" ? event.payload : null;
  const providerEventType = stringValue(
    payload?.provider_event_type,
    event.kind,
  );
  const providerItemType = optionalString(payload?.provider_item_type);
  const providerItemId = optionalString(payload?.provider_item_id);
  const phase = optionalString(payload?.phase);
  const status = optionalString(payload?.status);
  const summary = stringValue(
    payload?.summary,
    event.kind === "turn.started"
      ? "Agent turn started"
      : event.kind === "turn.completed"
        ? "Agent turn completed"
        : event.kind,
  );

  return {
    eventId: event.event_id,
    seq: event.seq,
    happenedAt: event.happened_at,
    providerEventType,
    providerItemType,
    providerItemId,
    phase,
    status,
    summary,
    rawEvent: payload?.raw_event,
    rawEventPreview: optionalString(payload?.raw_event_preview),
    rawEventTruncated: payload?.raw_event_truncated === true,
  };
}

function turnActivityCounts(rows: ReturnType<typeof turnActivityRow>[]) {
  return rows.reduce(
    (counts, row) => {
      const searchable = [
        row.providerEventType,
        row.providerItemType,
        row.phase,
        row.status,
        row.summary,
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      if (
        searchable.includes("command") ||
        searchable.includes("tool") ||
        searchable.includes("exec")
      ) {
        counts.commandCount += 1;
      }
      if (
        searchable.includes("file") ||
        searchable.includes("patch") ||
        searchable.includes("diff")
      ) {
        counts.fileChangeCount += 1;
      }
      if (searchable.includes("reason")) counts.reasoningCount += 1;
      if (
        searchable.includes("warn") ||
        searchable.includes("error") ||
        searchable.includes("failed") ||
        searchable.includes("parse_error") ||
        searchable.includes("stderr")
      ) {
        counts.warningErrorCount += 1;
      }
      return counts;
    },
    {
      commandCount: 0,
      fileChangeCount: 0,
      reasoningCount: 0,
      warningErrorCount: 0,
    },
  );
}

export function approvalIdFromEvent(event: EventEnvelope) {
  if (event.kind !== "approval.requested") return null;
  return approvalIdFromPayload(event.payload);
}

function approvalIdFromResolutionEvent(event: EventEnvelope) {
  if (event.kind !== "approval.resolved") return null;
  return approvalIdFromPayload(event.payload);
}

function approvalIdFromPayload(payload: unknown) {
  if (!isRecord(payload)) return null;
  const approvalId = payload.approval_id;
  return typeof approvalId === "string" ? approvalId : null;
}

function pendingApprovalIds(events: EventEnvelope[]) {
  const pending = new Set<string>();
  for (const event of [...events].sort(compareEvents)) {
    const requestedId = approvalIdFromEvent(event);
    if (requestedId) {
      pending.add(requestedId);
      continue;
    }
    const resolvedId = approvalIdFromResolutionEvent(event);
    if (resolvedId) {
      pending.delete(resolvedId);
    }
  }
  return pending;
}

function approvalPromptFromEvent(event: EventEnvelope) {
  return event.payload.type === "approval_requested" && event.payload.prompt
    ? event.payload.prompt
    : "Approval requested";
}

function blockTypeForEvent(event: EventEnvelope) {
  if (event.kind === "coordination.warning_acknowledged") {
    return "core.warning";
  }
  if (event.kind === "workspace.validated") {
    return "core.workspace-validation";
  }
  if (event.kind === "resource.snapshot.updated") {
    return "core.resource-snapshot";
  }
  if (event.kind.startsWith("provider.output.")) {
    return "core.provider-output-stream";
  }
  if (event.kind === "runtime.error") {
    return "core.error";
  }
  return "core.runtime-event";
}

function primaryRefForEvent(event: EventEnvelope): UpravaRef {
  return eventRef(event);
}

function eventRef(event: EventEnvelope): UpravaRef {
  return {
    kind: "event",
    event_id: event.event_id,
    scope_ref: event.scope_ref,
    seq: event.seq,
  };
}

function baseBlock({
  blockId,
  type,
  primaryRef,
  data,
  parentRef = null,
  sourceRefs = [],
  evidenceRefs = [],
  causeRefs = [],
  relatedRefs = [],
  traceRefs = [],
  actions = [],
  fallbackText = null,
}: {
  blockId: string;
  type: string;
  primaryRef: UpravaRef;
  data: unknown;
  parentRef?: UpravaRef | null;
  sourceRefs?: UpravaRef[];
  evidenceRefs?: UpravaRef[];
  causeRefs?: UpravaRef[];
  relatedRefs?: UpravaRef[];
  traceRefs?: UpravaRef[];
  actions?: string[];
  fallbackText?: string | null;
}): UiBlock {
  return {
    block_id: blockId,
    type,
    schema_version: 1,
    surface_id: "session.timeline",
    primary_ref: primaryRef,
    parent_ref: parentRef,
    children: [],
    source_refs: sourceRefs,
    evidence_refs: evidenceRefs,
    cause_refs: causeRefs,
    related_refs: relatedRefs,
    trace_refs: traceRefs,
    data,
    actions,
    fallback_text: fallbackText,
  };
}

function payloadSummary(payload: unknown) {
  if (!isRecord(payload)) return "No structured payload";
  const message = payload.message;
  if (typeof message === "string" && message.length > 0) return message;
  const content = payload.content;
  if (typeof content === "string" && content.length > 0) return content;
  const code = payload.code;
  if (typeof code === "string" && code.length > 0) return code;
  const keys = Object.keys(payload);
  return keys.length > 0 ? keys.slice(0, 4).join(", ") : "No payload fields";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function optionalString(value: unknown) {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function stringValue(value: unknown, fallback: string) {
  return typeof value === "string" && value.length > 0 ? value : fallback;
}

function eventTime(events: EventEnvelope[], turnId: string, kind: string) {
  return events.find((event) => event.turn_id === turnId && event.kind === kind)
    ?.happened_at;
}

function compareEvents(left: EventEnvelope, right: EventEnvelope) {
  const timestampComparison = left.happened_at.localeCompare(right.happened_at);
  if (timestampComparison !== 0) return timestampComparison;
  return left.seq - right.seq;
}

function orderedTimelineBlockItem(
  item: TimelineBlockItem,
  timestamp: string,
  seq: number,
  sourceIndex: number,
): OrderedTimelineBlockItem {
  return { item, timestamp, seq, sourceIndex };
}

function compareTimelineBlockItems(
  left: OrderedTimelineBlockItem,
  right: OrderedTimelineBlockItem,
) {
  const timestampComparison = left.timestamp.localeCompare(right.timestamp);
  if (timestampComparison !== 0) return timestampComparison;
  const seqComparison = left.seq - right.seq;
  if (seqComparison !== 0) return seqComparison;
  return left.sourceIndex - right.sourceIndex;
}
