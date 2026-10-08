import { ScoreBar } from '@/components/shared/data'

import { SCORE_SIGNALS, type ScoreBreakdown } from '../../lib/labels'

/** Seven thin bars, one per ranking signal, in the formula's weight order. */
export function ScoreBreakdownBars({ score }: { score: ScoreBreakdown }) {
  return (
    <dl className="flex flex-col gap-2">
      {SCORE_SIGNALS.map((signal) => (
        <div key={signal.key} className="flex items-center">
          <dt className="sr-only">{signal.label}</dt>
          <dd className="flex-1">
            <ScoreBar value={score[signal.key]} label={signal.label} />
          </dd>
        </div>
      ))}
    </dl>
  )
}
