import { RelativeTime } from '@/components/shared/data/relative-time'
import { CrossFade } from '@/components/system/cross-fade'

import type { PipelineStage } from '../../api/types'
import { deployedAt } from '../../lib/labels'
import { CommitLink } from '../commit-link'
import { BLOCK_LABEL_CLASS } from './styles'

interface StageRunningProps {
  stage: PipelineStage
  repoUrl?: string
}

/**
 * What runs in the stage now. The block is keyed by the release, so when a
 * deployment lands the old version fades out under the new one (180 ms)
 * instead of the text snapping.
 */
export function StageRunning({ stage, repoUrl }: StageRunningProps) {
  const { current } = stage
  const since = deployedAt(stage)

  return (
    <div className="flex min-w-0 flex-col gap-1 px-3 py-2.5">
      <span className={BLOCK_LABEL_CLASS}>Deployed</span>
      <CrossFade swapKey={current?.id ?? 'none'} className="min-w-0">
        {current ? (
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="truncate font-mono text-sm leading-5 font-medium" title={current.version}>
              {current.version}
            </span>
            <span className="flex min-w-0 flex-wrap items-center gap-x-2 text-xs leading-4 text-muted-foreground">
              <CommitLink sha={current.commitSha} repoUrl={repoUrl} />
              {since ? (
                <span>
                  deployed <RelativeTime value={since} />
                </span>
              ) : null}
            </span>
          </div>
        ) : (
          <p className="text-xs leading-5 text-muted-foreground">Nothing deployed yet.</p>
        )}
      </CrossFade>
    </div>
  )
}
