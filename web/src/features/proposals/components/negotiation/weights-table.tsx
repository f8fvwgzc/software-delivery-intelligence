import { SCORE_SIGNALS, type ScoreWeights } from '../../lib/labels'

/** The ranking weights, which negotiation moves. Sum is shown so drift is visible. */
export function WeightsTable({ weights }: { weights: ScoreWeights }) {
  const total = SCORE_SIGNALS.reduce((sum, signal) => sum + weights[signal.key], 0)

  return (
    <section className="flex flex-col gap-2 px-4 py-3">
      <h3 className="text-xs font-medium text-muted-foreground">Weights</h3>
      <table className="w-full text-xs">
        <tbody>
          {SCORE_SIGNALS.map((signal) => (
            <tr key={signal.key} className="h-6">
              <td className="text-muted-foreground">{signal.label}</td>
              <td className="text-right font-mono tabular-nums">{weights[signal.key].toFixed(2)}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="h-6 border-t">
            <td className="text-muted-foreground">Total</td>
            <td className="text-right font-mono tabular-nums">{total.toFixed(2)}</td>
          </tr>
        </tfoot>
      </table>
    </section>
  )
}
