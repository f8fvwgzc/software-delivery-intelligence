import type { components } from '@/lib/api/schema'
import { testIds } from '@/test-ids'

type Constraint = components['schemas']['Constraint']

interface ConstraintsListProps {
  constraints: Constraint[]
  /** Message id → position in the thread. */
  positions: Map<string, number>
}

/** What has been agreed so far, each with the message that introduced it. */
export function ConstraintsList({ constraints, positions }: ConstraintsListProps) {
  return (
    <section className="flex flex-col gap-2 border-t px-4 py-3">
      <h3 className="text-xs font-medium text-muted-foreground">
        Constraints <span className="tabular-nums">({constraints.length})</span>
      </h3>
      <ul className="flex flex-col gap-1.5">
        {constraints.map((constraint) => {
          const position = positions.get(constraint.sourceMessageId)
          return (
            <li key={constraint.key} data-testid={testIds.proposal.constraint} className="flex flex-col gap-0.5 text-xs">
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-2xs text-muted-foreground">{constraint.key}</span>
                <span className="text-2xs text-muted-foreground">
                  {position !== undefined ? `from message ${position}` : 'from the request'}
                </span>
              </div>
              <span>{constraint.description}</span>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
