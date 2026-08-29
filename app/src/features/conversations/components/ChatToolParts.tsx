import type { ToolCallMessagePartComponent } from '@assistant-ui/react';
import { CheckIcon, ChevronDownIcon, Loader2Icon, WorkflowIcon } from 'lucide-react';
import type { FC, PropsWithChildren } from 'react';

import { cn } from '../../../components/assistant-ui/lib/utils';
import type { ThreadGroupPart } from '../../../components/assistant-ui/thread';
import { ToolFallback } from '../../../components/assistant-ui/tool-fallback';
import {
  ToolGroupContent,
  ToolGroupRoot,
  ToolGroupTrigger,
} from '../../../components/assistant-ui/tool-group';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '../../../components/assistant-ui/ui/collapsible';
import type { SubagentActivity } from '../../../store/chatRuntimeSlice';
import { SubagentActivityBlock } from './SubagentActivityBlock';

function asSubagentActivity(value: unknown): SubagentActivity | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const candidate = value as Partial<SubagentActivity>;
  if (
    typeof candidate.taskId !== 'string' ||
    typeof candidate.agentId !== 'string' ||
    !Array.isArray(candidate.toolCalls)
  ) {
    return undefined;
  }
  return candidate as SubagentActivity;
}

function readSubagentState(
  args: unknown,
  result: unknown
): { activity: SubagentActivity | undefined; running: boolean } {
  const completed = asSubagentActivity(result);
  if (completed) return { activity: completed, running: false };
  const progress =
    args && typeof args === 'object'
      ? asSubagentActivity((args as { progress?: unknown }).progress)
      : undefined;
  return { activity: progress, running: result === undefined };
}

/** Render a real OpenHuman `task` delegation using the existing activity view. */
export const SubagentCall: ToolCallMessagePartComponent = ({ args, result }) => {
  const { activity, running } = readSubagentState(args, result);
  const description = (args as { description?: string } | undefined)?.description;
  const name =
    activity?.displayName ??
    activity?.agentId ??
    (args as { subagent_type?: string } | undefined)?.subagent_type ??
    'subagent';

  return (
    <Collapsible
      data-slot="aui_subagent-call"
      defaultOpen
      className={cn(
        'aui-subagent-call border-border/60 dark:border-muted-foreground/15 rounded-xl border',
        running && 'border-dashed'
      )}>
      <CollapsibleTrigger className="group/subagent text-muted-foreground hover:text-foreground flex w-full items-center gap-2 px-3 py-2 text-sm transition-colors">
        <WorkflowIcon className="size-4 shrink-0" />
        <span className="text-start leading-none">
          Delegated to <b className="text-foreground">{name}</b>
        </span>
        {running ? (
          <span className="bg-muted text-muted-foreground flex shrink-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] leading-none">
            <Loader2Icon className="size-3 animate-spin [animation-duration:0.6s]" />
            running
          </span>
        ) : (
          <span className="text-muted-foreground flex shrink-0 items-center gap-1.5 text-[11px] leading-none">
            <CheckIcon className="size-3.5" />
            {activity?.elapsedMs != null && (
              <span className="tabular-nums">{(activity.elapsedMs / 1000).toFixed(1)}s</span>
            )}
          </span>
        )}
        <ChevronDownIcon className="ml-auto size-4 shrink-0 -rotate-90 transition-transform group-data-[state=open]/subagent:rotate-0" />
      </CollapsibleTrigger>
      <CollapsibleContent className="px-3 pb-3">
        {description && <p className="text-muted-foreground text-xs">{description}</p>}
        {activity && <SubagentActivityBlock subagent={activity} />}
      </CollapsibleContent>
    </Collapsible>
  );
};

/** Route delegations to the rich renderer and ordinary tools to assistant-ui. */
export const ChatToolFallback: ToolCallMessagePartComponent = props =>
  props.toolName === 'task' ? <SubagentCall {...props} /> : <ToolFallback {...props} />;

/** Keep a tool group open while any contained call is still running. */
export const ChatToolGroup: FC<PropsWithChildren<{ group: ThreadGroupPart }>> = ({
  group,
  children,
}) => {
  const running = group.status.type === 'running';
  return (
    <ToolGroupRoot variant="ghost" defaultOpen={running}>
      <ToolGroupTrigger count={group.indices.length} active={running} />
      <ToolGroupContent>{children}</ToolGroupContent>
    </ToolGroupRoot>
  );
};
