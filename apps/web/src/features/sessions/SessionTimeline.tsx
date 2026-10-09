import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";

import { queryKeys } from "../../shared/api/query-keys";
import type {
  ActionCapability,
  SessionDetail,
} from "../../shared/protocol/types";
import { Button } from "../../shared/ui/button";
import { TimelineBlockRenderer } from "../../workbench/blocks/TimelineBlockRenderer";
import {
  canRunCommand,
  runWorkbenchCommand,
} from "../../workbench/commands/registry";
import { ReferenceActions } from "../../workbench/references/ReferenceActions";
import { ProviderInteractionCards } from "./ProviderInteractionCards";
import { buildSessionTimelineBlocks } from "./timeline-blocks";

export function SessionTimeline({
  detail,
  availableCommands,
}: {
  detail: SessionDetail;
  availableCommands: ActionCapability[];
}) {
  const queryClient = useQueryClient();
  const invalidateSession = async () => {
    await queryClient.invalidateQueries({
      queryKey: queryKeys.session(detail.session.session_thread_id),
    });
    await queryClient.invalidateQueries({ queryKey: queryKeys.inventory });
  };
  const resolveApproval = useMutation({
    mutationFn: ({
      approvalId,
      approved,
    }: {
      approvalId: string;
      approved: boolean;
    }) =>
      runWorkbenchCommand("approval.resolve", {
        session: detail.session,
        runtime: detail.session.runtime,
        approvalId,
        approved,
        availableCommands,
        afterSuccess: invalidateSession,
      }),
  });
  const blocks = useMemo(() => buildSessionTimelineBlocks(detail), [detail]);
  const canResolveApproval = (approvalId: string, approved: boolean) =>
    canRunCommand("approval.resolve", {
      session: detail.session,
      runtime: detail.session.runtime,
      approvalId,
      approved,
      availableCommands,
    });

  return (
    <div className="space-y-3">
      <ProviderInteractionCards
        detail={detail}
        availableCommands={availableCommands}
      />
      {blocks.map(({ block, approvalId }) => (
        <TimelineBlockRenderer
          key={block.block_id}
          block={block}
          actions={
            <>
              <ReferenceActions reference={block.primary_ref} />
              {approvalId ? (
                <>
                  <Button
                    variant="primary"
                    disabled={
                      resolveApproval.isPending ||
                      !canResolveApproval(approvalId, true)
                    }
                    onClick={() =>
                      resolveApproval.mutate({ approvalId, approved: true })
                    }
                  >
                    Approve
                  </Button>
                  <Button
                    variant="danger"
                    disabled={
                      resolveApproval.isPending ||
                      !canResolveApproval(approvalId, false)
                    }
                    onClick={() =>
                      resolveApproval.mutate({ approvalId, approved: false })
                    }
                  >
                    Deny
                  </Button>
                </>
              ) : null}
            </>
          }
        />
      ))}
    </div>
  );
}
