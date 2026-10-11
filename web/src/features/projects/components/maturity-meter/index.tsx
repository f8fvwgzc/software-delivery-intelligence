import { cn } from '@/lib/utils'

import { filledSegments, MATURITY_SEGMENTS, segmentOpacity, stageLabel } from '../../lib/maturity'

interface MaturityMeterProps {
  /** 0–100. */
  score: number
  /** The server's stage word when known; derived from the score otherwise. */
  stage?: string
  /** No scan has scored the project yet. */
  unscored?: boolean
  className?: string
}

/**
 * Score, a 36px eight-segment bar and the stage word, for table rows.
 * The bar fills in brand, fainter for low scores, so a column of them
 * reads like a sparkline of the whole portfolio.
 */
export function MaturityMeter({ score, stage = '', unscored = false, className }: MaturityMeterProps) {
  if (unscored) {
    return <span className={cn('text-xs text-muted-foreground', className)}>Not scored</span>
  }

  const filled = filledSegments(score)
  const label = stageLabel(stage, score)

  return (
    <span className={cn('inline-flex items-center gap-2', className)}>
      <span className="w-6 text-right text-sm tabular-nums">{score}</span>
      <span
        role="img"
        aria-label={`${score} of 100, ${label.label.toLowerCase()}`}
        className="flex h-2 w-9 shrink-0 items-center gap-px"
      >
        {Array.from({ length: MATURITY_SEGMENTS }, (_, index) => (
          <span
            key={index}
            className={cn('h-full flex-1 rounded-[1px]', index < filled ? 'bg-brand' : 'bg-muted')}
            style={index < filled ? { opacity: segmentOpacity(score) } : undefined}
          />
        ))}
      </span>
      <span className="text-xs text-muted-foreground">{label.label}</span>
    </span>
  )
}
