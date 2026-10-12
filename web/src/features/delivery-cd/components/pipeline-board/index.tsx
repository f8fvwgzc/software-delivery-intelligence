import { useSuspenseQuery } from '@tanstack/react-query'

import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { cdQueries } from '../../api/queries'
import { BoardEmpty } from './board-empty'
import { StageRail } from './stage-rail'

export { PipelineBoardSkeleton } from './skeleton'

interface PipelineBoardProps {
  projectId: string
  onGenerateSetup: () => void
}

/**
 * The CD board: the project's environments in promotion order, each with
 * what runs there, what could go there next and its last run. Live through
 * the platform stream, so a stage moves as its run does. Suspends on the
 * pipeline; mount it inside a Panel.
 */
export function PipelineBoard({ projectId, onGenerateSetup }: PipelineBoardProps) {
  const { data: pipeline } = useSuspenseQuery(cdQueries.pipeline(projectId))
  const { currentWorkspace } = useAuth()

  if (pipeline.stages.length === 0) {
    return <BoardEmpty projectId={projectId} canAdmin={hasRole(currentWorkspace.myRole, 'admin')} onGenerateSetup={onGenerateSetup} />
  }

  return <StageRail projectId={projectId} stages={pipeline.stages} />
}
