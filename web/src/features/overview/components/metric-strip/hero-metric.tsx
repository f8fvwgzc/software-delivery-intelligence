import { describeWaiting, type WaitingSummary } from '../../lib/metrics'

/**
 * The one accent-tinted number on the page: how many things are waiting on
 * the human. The line under it says which kinds.
 */
export function HeroMetric({ waiting }: { waiting: WaitingSummary }) {
  return (
    <div className="flex flex-col gap-1.5 rounded-lg bg-brand/5 px-4 py-3 ring-1 ring-brand/15">
      <span className="text-xs text-muted-foreground">Waiting on you</span>
      <span className="text-3xl font-semibold leading-none tracking-tight tabular-nums text-brand">
        {waiting.total.toLocaleString()}
      </span>
      <span className="truncate text-xs text-muted-foreground" title={describeWaiting(waiting)}>
        {describeWaiting(waiting)}
      </span>
    </div>
  )
}
