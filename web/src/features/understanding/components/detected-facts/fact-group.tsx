import type { Project } from '@/features/projects/api/queries'

import type { FactGroup as FactGroupData } from '../../lib/labels'
import { FactRow } from './fact-row'

interface FactGroupProps {
  group: FactGroupData
  project: Pick<Project, 'repoUrl' | 'defaultBranch'>
}

/** A category heading over a two-column definition list. */
export function FactGroup({ group, project }: FactGroupProps) {
  return (
    <div className="flex flex-col gap-1">
      <h3 className="flex items-baseline gap-2 text-xs font-medium text-muted-foreground">
        {group.label}
        <span className="tabular-nums">{group.facts.length}</span>
      </h3>
      <dl className="grid grid-cols-[minmax(7rem,11rem)_minmax(0,1fr)] gap-x-3 [&>dd:last-of-type]:border-0 [&>dt:last-of-type]:border-0">
        {group.facts.map((fact) => (
          <FactRow key={fact.id} fact={fact} project={project} />
        ))}
      </dl>
    </div>
  )
}
