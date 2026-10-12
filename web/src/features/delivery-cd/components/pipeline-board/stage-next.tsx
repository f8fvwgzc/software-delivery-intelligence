import type { PipelineStage } from '../../api/types'
import { CommitLink } from '../commit-link'
import { PromoteButton } from '../promote-button'
import { BLOCK_LABEL_CLASS } from './styles'

interface StageNextProps {
  projectId: string
  stage: PipelineStage
  canPromote: boolean
  repoUrl?: string
}

/** The release that could go to the stage next, with the button that sends it; otherwise that there is none. */
export function StageNext({ projectId, stage, canPromote, repoUrl }: StageNextProps) {
  const { candidate, current } = stage

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1 border-t px-3 py-2.5">
      <span className={BLOCK_LABEL_CLASS}>Next</span>
      {candidate ? (
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="truncate font-mono text-sm leading-5" title={candidate.version}>
              {candidate.version}
            </span>
            <CommitLink sha={candidate.commitSha} repoUrl={repoUrl} className="text-xs leading-4 text-muted-foreground" />
          </div>
          <PromoteButton projectId={projectId} stage={stage} candidate={candidate} canPromote={canPromote} />
        </div>
      ) : (
        <p className="text-xs leading-5 text-muted-foreground">{current ? 'Up to date.' : 'No release to promote yet.'}</p>
      )}
    </div>
  )
}
