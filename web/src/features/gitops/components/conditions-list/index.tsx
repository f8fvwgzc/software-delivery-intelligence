import { TriangleAlertIcon } from 'lucide-react'

/**
 * Why the last comparison could not complete (render failed, cluster
 * unreachable, …), one sentence each. The statuses above them are the last
 * ones known, not current ones.
 */
export function ConditionsList({ conditions }: { conditions: readonly string[] }) {
  if (conditions.length === 0) return null
  return (
    <section aria-label="Conditions" className="rounded-lg bg-warning/5 px-3 py-2 ring-1 ring-warning/20">
      <ul className="flex flex-col gap-1">
        {conditions.map((condition, index) => (
          <li key={index} className="flex items-start gap-1.5 text-xs leading-5 text-warning">
            <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            <span className="min-w-0 break-words">{condition}</span>
          </li>
        ))}
      </ul>
      <p className="mt-1 pl-5 text-2xs text-muted-foreground">The statuses shown are the last ones known.</p>
    </section>
  )
}
