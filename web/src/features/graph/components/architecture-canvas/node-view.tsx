import { Handle, Position, type NodeProps } from '@xyflow/react'
import { memo } from 'react'

import { cn } from '@/lib/utils'

import { NODE_KIND_META } from '../../lib/labels'
import type { ArchNode, NodeEmphasis } from './types'

const EMPHASIS_CLASS: Record<NodeEmphasis, string> = {
  none: '',
  origin: 'ring-2 ring-brand',
  impacted: 'bg-brand/5 ring-brand/60',
  dimmed: 'opacity-35',
}

/** One node: kind icon, label, kind caption. Sized by the layout, not by content. */
function NodeView({ data, selected }: NodeProps<ArchNode>) {
  const meta = NODE_KIND_META[data.kind]
  const Icon = meta.icon

  return (
    <>
      <Handle type="target" position={Position.Left} isConnectable={false} />
      <div
        className={cn(
          'flex size-full items-center gap-2 rounded-lg bg-card px-2.5 ring-1 ring-border transition-[opacity,box-shadow,background-color] duration-150 ease-out hover:ring-muted-foreground/50 motion-reduce:transition-none',
          selected && 'ring-2 ring-brand',
          EMPHASIS_CLASS[data.emphasis],
        )}
      >
        <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <Icon className="size-3.5" />
        </span>
        <span className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-xs font-medium text-foreground">{data.label}</span>
          <span className="truncate text-2xs text-muted-foreground">{meta.label}</span>
        </span>
      </div>
      <Handle type="source" position={Position.Right} isConnectable={false} />
    </>
  )
}

export const ArchitectureNodeView = memo(NodeView)
