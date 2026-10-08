import { cn } from '@/lib/utils'

interface ScoreBarProps {
  /** 0 – 1 */
  value: number
  label?: string
  /** Show the percentage next to the bar. */
  showValue?: boolean
  className?: string
}

function toneFor(value: number) {
  if (value >= 0.8) return 'bg-success'
  if (value >= 0.55) return 'bg-brand'
  if (value >= 0.35) return 'bg-warning'
  return 'bg-destructive'
}

/** Thin horizontal bar for a 0–1 score. */
export function ScoreBar({ value, label, showValue = true, className }: ScoreBarProps) {
  const clamped = Math.max(0, Math.min(1, value))
  const percent = Math.round(clamped * 100)

  return (
    <div className={cn('flex items-center gap-2', className)}>
      {label && <span className="w-28 shrink-0 truncate text-xs text-muted-foreground">{label}</span>}
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
        <div
          className={cn('h-full rounded-full transition-[width] duration-500 ease-out', toneFor(clamped))}
          style={{ width: `${percent}%` }}
        />
      </div>
      {showValue && <span className="w-8 shrink-0 text-right text-xs tabular-nums text-muted-foreground">{percent}%</span>}
    </div>
  )
}
