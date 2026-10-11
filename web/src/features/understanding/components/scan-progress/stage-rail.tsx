import { CheckIcon, MinusIcon, XIcon } from 'lucide-react'

import type { ScanStage, StageStatus } from '@/features/projects/api/queries'
import { cn } from '@/lib/utils'
import { testIds } from '@/test-ids'

const MARKER_CLASS: Record<StageStatus, string> = {
  unspecified: 'bg-background ring-1 ring-border',
  pending: 'bg-background ring-1 ring-border',
  running: 'bg-brand text-brand',
  done: 'bg-success/15 text-success',
  failed: 'bg-destructive/15 text-destructive',
  skipped: 'bg-muted text-muted-foreground',
}

/** Every transition is a 150ms fade; nothing moves, nothing bounces. */
const FADE = 'transition-[opacity,color,background-color] duration-150 ease-out motion-reduce:transition-none'

function StageMarker({ status }: { status: StageStatus }) {
  return (
    <span
      className={cn('relative z-10 flex size-4 shrink-0 items-center justify-center rounded-full', FADE, MARKER_CLASS[status])}
      aria-hidden
    >
      {status === 'running' ? (
        <span className="absolute inline-flex size-full rounded-full bg-brand opacity-60 motion-safe:animate-ping" />
      ) : null}
      {status === 'done' ? <CheckIcon className="size-2.5" strokeWidth={2.5} /> : null}
      {status === 'failed' ? <XIcon className="size-2.5" strokeWidth={2.5} /> : null}
      {status === 'skipped' ? <MinusIcon className="size-2.5" strokeWidth={2.5} /> : null}
    </span>
  )
}

/**
 * Vertical step rail, the same shape the execution pages use. One hairline
 * runs behind the markers; the running stage is the one that pulses. Stage
 * changes arrive live, so each row fades between its states.
 */
export function StageRail({ stages }: { stages: ScanStage[] }) {
  return (
    <ol
      data-testid={testIds.understanding.scanRail}
      className="relative flex flex-col gap-4 before:absolute before:top-2 before:bottom-2 before:left-2 before:w-px before:bg-border"
    >
      {stages.map((stage) => {
        const isRunning = stage.status === 'running'
        const isPending = stage.status === 'pending' || stage.status === 'unspecified'

        return (
          <li key={stage.key} className="flex items-start gap-3">
            <StageMarker status={stage.status} />
            <div className="flex min-w-0 flex-col gap-0.5 leading-4">
              <span
                className={cn(
                  'text-sm',
                  FADE,
                  isRunning && 'font-medium',
                  isPending ? 'opacity-55' : 'opacity-100',
                  stage.status === 'failed' && 'text-destructive',
                )}
              >
                {stage.label}
                {isRunning ? <span className="sr-only"> (running)</span> : null}
              </span>
              {stage.detail ? (
                <span
                  className={cn('truncate font-mono text-2xs text-muted-foreground', FADE)}
                  title={stage.detail}
                >
                  {stage.detail}
                </span>
              ) : null}
            </div>
          </li>
        )
      })}
    </ol>
  )
}
