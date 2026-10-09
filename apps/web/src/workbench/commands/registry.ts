import type { LucideIcon } from "lucide-react";
import {
  Check,
  Clipboard,
  FolderPlus,
  KeyRound,
  LogIn,
  LogOut,
  Pause,
  Play,
  RotateCcw,
  Send,
  ShieldCheck,
  Square,
  Trash2,
} from "lucide-react";

import { coreApi } from "../../shared/api/http-client";
import type {
  ActionCapability,
  AgentExecutionProfile,
  UpravaRef,
  CreatePlacementRequest,
  ProjectPlacementSummary,
  RuntimeSummary,
  SessionSummary,
} from "../../shared/protocol/types";
import { copyReferenceText, routeForRef } from "../references/refs";
import { workspaceAgentSessionRoute } from "../../features/workspaces/routes";

export type WorkbenchCommandId =
  | "node.createEnrollment"
  | "node.approveEnrollment"
  | "node.revoke"
  | "node.rotateCredential"
  | "node.delete"
  | "placement.validate"
  | "placement.delete"
  | "session.start"
  | "session.attach"
  | "session.detach"
  | "session.sendTurn"
  | "runtime.interrupt"
  | "runtime.stop"
  | "runtime.resume"
  | "approval.resolve"
  | "providerInteraction.resolveApproval"
  | "providerInteraction.submitInput"
  | "warning.acknowledge"
  | "reference.openInInspector"
  | "reference.copy";

export type WorkbenchCommandContext = {
  nodeId?: string;
  nodeDisplayName?: string;
  enrollmentId?: string;
  placement?: ProjectPlacementSummary;
  placementRequest?: CreatePlacementRequest;
  provider?: string;
  executionProfile?: AgentExecutionProfile;
  force?: boolean;
  session?: SessionSummary;
  runtime?: RuntimeSummary;
  turnContent?: string;
  turnCollaborationMode?: "default" | "plan";
  approvalId?: string;
  approved?: boolean;
  providerInteractionId?: string;
  interactionMessage?: string;
  answers?: string[];
  warningKind?: string;
  reference?: UpravaRef;
  afterSuccess?: () => Promise<void> | void;
  navigate?: (path: string) => void;
  openReference?: (reference: UpravaRef) => Promise<void> | void;
  copyText?: (text: string) => Promise<void> | void;
  availableCommands?: ActionCapability[];
};

export type UiCommand = {
  id: WorkbenchCommandId;
  title: string;
  icon?: LucideIcon;
  when: (context: WorkbenchCommandContext) => boolean;
  run: (context: WorkbenchCommandContext) => Promise<unknown>;
};

const commands: UiCommand[] = [
  {
    id: "node.createEnrollment",
    title: "Create enrollment",
    icon: ShieldCheck,
    when: (context) => Boolean(context.nodeDisplayName?.trim()),
    run: async (context) => {
      const displayName = requireValue(
        context.nodeDisplayName?.trim(),
        "node.createEnrollment requires nodeDisplayName",
      );
      return finishCommand(
        context,
        coreApi.createNodeEnrollment({ display_name: displayName }),
      );
    },
  },
  {
    id: "node.revoke",
    title: "Revoke node",
    icon: ShieldCheck,
    when: (context) => Boolean(context.nodeId),
    run: async (context) => {
      const nodeId = requireValue(
        context.nodeId,
        "node.revoke requires nodeId",
      );
      return finishCommand(context, coreApi.revokeNode(nodeId));
    },
  },
  {
    id: "node.rotateCredential",
    title: "Rotate node credential",
    icon: KeyRound,
    when: (context) => Boolean(context.nodeId),
    run: async (context) => {
      const nodeId = requireValue(
        context.nodeId,
        "node.rotateCredential requires nodeId",
      );
      return finishCommand(context, coreApi.rotateNodeCredential(nodeId));
    },
  },
  {
    id: "node.delete",
    title: "Delete node",
    icon: Trash2,
    when: (context) => Boolean(context.nodeId),
    run: async (context) => {
      const nodeId = requireValue(
        context.nodeId,
        "node.delete requires nodeId",
      );
      return finishCommand(context, coreApi.deleteNode(nodeId));
    },
  },
  {
    id: "node.approveEnrollment",
    title: "Approve enrollment",
    icon: ShieldCheck,
    when: (context) => Boolean(context.enrollmentId),
    run: async (context) => {
      const enrollmentId = requireValue(
        context.enrollmentId,
        "node.approveEnrollment requires enrollmentId",
      );
      return finishCommand(
        context,
        coreApi.approveNodeEnrollment(enrollmentId),
      );
    },
  },
  {
    id: "placement.validate",
    title: "Validate workspace",
    icon: FolderPlus,
    when: (context) =>
      Boolean(
        context.placementRequest?.node_id &&
        context.placementRequest.display_name &&
        context.placementRequest.workspace_path,
      ),
    run: async (context) => {
      const request = requireValue(
        context.placementRequest,
        "placement.validate requires placementRequest",
      );
      return finishCommand(context, coreApi.validatePlacement(request));
    },
  },
  {
    id: "placement.delete",
    title: "Delete workspace",
    icon: Trash2,
    when: (context) => Boolean(context.placement),
    run: async (context) => {
      const placement = requireValue(
        context.placement,
        "placement.delete requires placement",
      );
      const response = await coreApi.deletePlacement(
        placement.project_placement_id,
      );
      await context.afterSuccess?.();
      context.navigate?.(
        routeForRef({ kind: "node", node_id: placement.node_id }) ??
          `/nodes/${placement.node_id}`,
      );
      return response;
    },
  },
  {
    id: "session.start",
    title: "Start session",
    icon: Play,
    when: (context) =>
      Boolean(context.placement) &&
      context.placement?.state === "validated" &&
      !context.placement.resource_badges.some(
        (badge) => badge.severity === "hard_block",
      ),
    run: async (context) => {
      const placement = requireValue(
        context.placement,
        "session.start requires placement",
      );
      const session = await coreApi.createSession({
        project_placement_id: placement.project_placement_id,
        provider: requireValue(
          context.provider,
          "session.start requires provider",
        ),
        execution_profile: requireValue(
          context.executionProfile,
          "session.start requires executionProfile",
        ),
        force: context.force ?? false,
      });
      await context.afterSuccess?.();
      context.navigate?.(
        workspaceAgentSessionRoute(
          placement.project_placement_id,
          session.session.session_thread_id,
        ),
      );
      return session;
    },
  },
  {
    id: "session.attach",
    title: "Attach session",
    icon: LogIn,
    when: (context) => hasAction(context, "session.attach"),
    run: async (context) => {
      const session = requireValue(
        context.session,
        "session.attach requires session",
      );
      return finishCommand(
        context,
        coreApi.attachSession(session.session_thread_id),
      );
    },
  },
  {
    id: "session.detach",
    title: "Detach session",
    icon: LogOut,
    when: (context) => hasAction(context, "session.detach"),
    run: async (context) => {
      const session = requireValue(
        context.session,
        "session.detach requires session",
      );
      return finishCommand(
        context,
        coreApi.detachSession(session.session_thread_id),
      );
    },
  },
  {
    id: "session.sendTurn",
    title: "Send turn",
    icon: Send,
    when: (context) =>
      hasAction(context, "session.sendTurn") &&
      Boolean(context.turnContent?.trim()),
    run: async (context) => {
      const session = requireValue(
        context.session,
        "session.sendTurn requires session",
      );
      const content = requireValue(
        context.turnContent?.trim(),
        "session.sendTurn requires turnContent",
      );
      return finishCommand(
        context,
        coreApi.sendTurn(session.session_thread_id, {
          content,
          collaboration_mode: context.turnCollaborationMode,
        }),
      );
    },
  },
  {
    id: "runtime.interrupt",
    title: "Interrupt runtime",
    icon: Pause,
    when: (context) => hasAction(context, "runtime.interrupt"),
    run: async (context) => {
      const runtime = requireValue(
        context.runtime,
        "runtime.interrupt requires runtime",
      );
      return finishCommand(
        context,
        coreApi.interruptRuntime(runtime.runtime_session_id),
      );
    },
  },
  {
    id: "runtime.stop",
    title: "Stop runtime",
    icon: Square,
    when: (context) => hasAction(context, "runtime.stop"),
    run: async (context) => {
      const runtime = requireValue(
        context.runtime,
        "runtime.stop requires runtime",
      );
      return finishCommand(
        context,
        coreApi.stopRuntime(runtime.runtime_session_id),
      );
    },
  },
  {
    id: "runtime.resume",
    title: "Resume runtime",
    icon: RotateCcw,
    when: (context) => hasAction(context, "runtime.resume"),
    run: async (context) => {
      const runtime = requireValue(
        context.runtime,
        "runtime.resume requires runtime",
      );
      return finishCommand(
        context,
        coreApi.resumeRuntime(runtime.runtime_session_id),
      );
    },
  },
  {
    id: "approval.resolve",
    title: "Resolve approval",
    icon: Check,
    when: (context) =>
      hasAction(context, "approval.resolve") &&
      Boolean(context.approvalId) &&
      typeof context.approved === "boolean",
    run: async (context) => {
      const session = requireValue(
        context.session,
        "approval.resolve requires session",
      );
      const approvalId = requireValue(
        context.approvalId,
        "approval.resolve requires approvalId",
      );
      const approved = requireValue(
        context.approved,
        "approval.resolve requires approved",
      );
      return finishCommand(
        context,
        coreApi.resolveApproval(session.session_thread_id, approvalId, {
          approved,
          message: approved ? "Approved" : "Denied",
        }),
      );
    },
  },
  {
    id: "providerInteraction.resolveApproval",
    title: "Resolve provider approval",
    icon: Check,
    when: (context) =>
      hasAction(context, "approval.resolve") &&
      Boolean(context.providerInteractionId) &&
      typeof context.approved === "boolean",
    run: async (context) => {
      const session = requireValue(
        context.session,
        "providerInteraction.resolveApproval requires session",
      );
      const interactionId = requireValue(
        context.providerInteractionId,
        "providerInteraction.resolveApproval requires providerInteractionId",
      );
      const approved = requireValue(
        context.approved,
        "providerInteraction.resolveApproval requires approved",
      );
      return finishCommand(
        context,
        coreApi.resolveProviderApproval(
          session.session_thread_id,
          interactionId,
          {
            approved,
            message: context.interactionMessage?.trim() || null,
          },
        ),
      );
    },
  },
  {
    id: "providerInteraction.submitInput",
    title: "Submit provider input",
    icon: Send,
    when: (context) =>
      hasAction(context, "providerInteraction.submitInput") &&
      Boolean(context.providerInteractionId) &&
      Boolean(context.answers?.some((answer) => answer.trim())),
    run: async (context) => {
      const session = requireValue(
        context.session,
        "providerInteraction.submitInput requires session",
      );
      const interactionId = requireValue(
        context.providerInteractionId,
        "providerInteraction.submitInput requires providerInteractionId",
      );
      const answers = requireValue(
        context.answers,
        "providerInteraction.submitInput requires answers",
      ).map((answer) => answer.trim());
      return finishCommand(
        context,
        coreApi.submitProviderInput(session.session_thread_id, interactionId, {
          answers,
        }),
      );
    },
  },
  {
    id: "warning.acknowledge",
    title: "Acknowledge warning",
    icon: ShieldCheck,
    when: (context) =>
      hasAction(context, "warning.acknowledge") && Boolean(context.warningKind),
    run: async (context) => {
      const session = requireValue(
        context.session,
        "warning.acknowledge requires session",
      );
      const warningKind = requireValue(
        context.warningKind,
        "warning.acknowledge requires warningKind",
      );
      return finishCommand(
        context,
        coreApi.acknowledgeWarning(session.session_thread_id, warningKind, {}),
      );
    },
  },
  {
    id: "reference.openInInspector",
    title: "Open in inspector",
    icon: Clipboard,
    when: (context) => Boolean(context.reference && context.openReference),
    run: async (context) => {
      const reference = requireValue(
        context.reference,
        "reference.openInInspector requires reference",
      );
      const openReference = requireValue(
        context.openReference,
        "reference.openInInspector requires openReference",
      );
      await openReference(reference);
      await context.afterSuccess?.();
      return reference;
    },
  },
  {
    id: "reference.copy",
    title: "Copy reference",
    icon: Clipboard,
    when: (context) => Boolean(context.reference),
    run: async (context) => {
      const reference = requireValue(
        context.reference,
        "reference.copy requires reference",
      );
      const text = copyReferenceText(reference);
      if (context.copyText) {
        await context.copyText(text);
      } else if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        throw new Error("Clipboard API is not available");
      }
      await context.afterSuccess?.();
      return text;
    },
  },
];

const commandById = new Map(commands.map((command) => [command.id, command]));

export const commandRegistry = commands;

export function getWorkbenchCommand(id: WorkbenchCommandId) {
  const command = commandById.get(id);
  if (!command) throw new Error(`Unknown workbench command: ${id}`);
  return command;
}

export function canRunCommand(
  id: WorkbenchCommandId,
  context: WorkbenchCommandContext,
) {
  return getWorkbenchCommand(id).when(context);
}

export async function runWorkbenchCommand(
  id: WorkbenchCommandId,
  context: WorkbenchCommandContext,
) {
  const command = getWorkbenchCommand(id);
  if (!command.when(context)) {
    throw new Error(`Workbench command is not available: ${id}`);
  }
  return command.run(context);
}

async function finishCommand(
  context: WorkbenchCommandContext,
  operation: Promise<unknown>,
) {
  const result = await operation;
  await context.afterSuccess?.();
  return result;
}

function requireValue<T>(value: T | null | undefined, message: string): T {
  if (value === null || value === undefined || value === "") {
    throw new Error(message);
  }
  return value;
}

function hasAction(
  context: WorkbenchCommandContext,
  action: ActionCapability,
): boolean {
  return (
    Boolean(context.session) &&
    context.availableCommands?.includes(action) === true
  );
}
