import { FileSearchIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import type { Project } from '@/features/projects/api/queries'

import type { FactGroup as FactGroupData } from '../../lib/labels'
import { FactGroup } from './fact-group'

interface DetectedFactsProps {
  groups: FactGroupData[]
  project: Pick<Project, 'repoUrl' | 'defaultBranch'>
}

/** Facts read straight from files, grouped by category. Never mixed with inference. */
export function DetectedFacts({ groups, project }: DetectedFactsProps) {
  if (groups.length === 0) {
    return (
      <EmptyState
        icon={FileSearchIcon}
        title="Nothing detected"
        description="The scan did not find any manifests, configs or workflows it recognises."
        className="py-8"
      />
    )
  }

  return (
    <div className="flex flex-col gap-5">
      {groups.map((group) => (
        <FactGroup key={group.category} group={group} project={project} />
      ))}
    </div>
  )
}

export { DetectedFactsSkeleton, InferredFactsSkeleton } from './facts-skeleton'
