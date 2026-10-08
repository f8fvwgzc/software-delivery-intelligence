import { CheckIcon, ClockIcon, LockIcon, MinusIcon, XIcon } from 'lucide-react'

import { Spinner } from '@/components/ui/spinner'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { STEP_STATUS } from '../../lib/labels'

interface RailNodeProps {
  status: components['schemas']['StepStatus']
}

const NODE_CLASS: Record<components['schemas']['StepStatus'], string> = {
  unspecified: 'bg-background ring-border text-muted-foreground',
  pending: 'bg-background ring-border text-muted-foreground',
  ready: 'bg-background ring-info/40 text-info',
  waiting_approval: 'bg-warning/10 ring-warning/40 text-warning',
  running: 'bg-brand/10 ring-brand/40 text-brand',
  verifying: 'bg-brand/10 ring-brand/40 text-brand',
  succeeded: 'bg-success/10 ring-success/40 text-success',
  failed: 'bg-destructive/10 ring-destructive/40 text-destructive',
  blocked: 'bg-muted ring-border text-muted-foreground',
  skipped: 'bg-muted ring-border text-muted-foreground',
  cancelled: 'bg-muted ring-border text-muted-foreground',
}

function NodeIcon({ status }: RailNodeProps) {
  switch (status) {
    case 'succeeded':
      return <CheckIcon className="size-3.5" />
    case 'running':
    case 'verifying':
      return <Spinner className="size-3.5" />
    case 'failed':
      return <XIcon className="size-3.5" />
    case 'blocked':
      return <LockIcon className="size-3" />
    case 'waiting_approval':
      return <ClockIcon className="size-3.5" />
    case 'skipped':
    case 'cancelled':
      return <MinusIcon className="size-3" />
    default:
      return <span className="size-1.5 rounded-full bg-current" />
  }
}

/** One node on the rail: a 24px ring with the step's state icon. */
export function RailNode({ status }: RailNodeProps) {
  return (
    <span
      role="img"
      aria-label={STEP_STATUS[status].label}
      className={cn('relative z-10 flex size-6 shrink-0 items-center justify-center rounded-full ring-1', NODE_CLASS[status])}
    >
      <NodeIcon status={status} />
    </span>
  )
}
