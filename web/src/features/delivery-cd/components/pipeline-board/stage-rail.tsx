import { useQuery } from '@tanstack/react-query'

import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { cdQueries } from '../../api/queries'
import type { PipelineStage } from '../../api/types'
import { StageConnector } from './stage-connector'
import { StageFrame } from './stage-frame'

interface StageRailProps {
  projectId: string
  /** In promotion order. */
  stages: PipelineStage[]
}

/**
 * The stages left to right in promotion order, one column each, joined by
 * a hairline; below `lg` they stack and the hairline runs down. More
 * stages than fit scroll sideways rather than squeeze.
 */
export function StageRail({ projectId, stages }: StageRailProps) {
  const { currentWorkspace } = useAuth()
  const canPromote = hasRole(currentWorkspace.myRole, 'approver')
  const canAdmin = hasRole(currentWorkspace.myRole, 'admin')
  // Already loaded by the project layout: only the repository URL for the commit links.
  const { data: project } = useQuery(cdQueries.project(projectId))

  return (
    // `p-px`: the frames draw their hairline as a ring, which a scroll container would clip.
    // The column gap is the connector's width (`lg:w-16`): it sits in the gap before its stage.
    <ol aria-label="Environments in promotion order" className="flex flex-col p-px lg:flex-row lg:items-stretch lg:gap-x-16 lg:overflow-x-auto">
      {stages.map((stage, index) => (
        <li key={stage.environment.id} className="relative flex min-w-0 flex-col lg:min-w-60 lg:flex-1">
          {index > 0 ? <StageConnector auto={stage.environment.autoPromote} /> : null}
          <StageFrame
            projectId={projectId}
            stage={stage}
            first={index === 0}
            canPromote={canPromote}
            canAdmin={canAdmin}
            repoUrl={project?.repoUrl}
          />
        </li>
      ))}
    </ol>
  )
}
