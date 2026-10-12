import { useSuspenseQuery } from '@tanstack/react-query'
import { GitPullRequestIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'
import { FrameFooter } from '@/components/system/frame'

import type { PullRequestFilter } from '../../api/keys'
import { pullRequestQueries } from '../../api/queries'
import { PullRequestRow } from './pull-request-row'
import { count } from '@/lib/intl'

interface PullRequestListProps {
  projectId: string
  state: PullRequestFilter
}

const EMPTY_COPY: Record<PullRequestFilter, string> = {
  open: 'No open pull requests on GitHub right now.',
  closed: 'No closed pull requests were read yet.',
  all: 'This repository has no pull requests yet.',
}

/** Rows per pull request, newest first, with the review state of each. */
export function PullRequestList({ projectId, state }: PullRequestListProps) {
  const { data } = useSuspenseQuery(pullRequestQueries.list(projectId, state))

  if (data.pullRequests.length === 0) {
    return <PanelEmpty icon={GitPullRequestIcon} title="Nothing to review" description={EMPTY_COPY[state]} />
  }

  return (
    <>
      <RowList stagger>
        {data.pullRequests.map((pullRequest) => (
          <li key={pullRequest.number}>
            <PullRequestRow pullRequest={pullRequest} />
          </li>
        ))}
      </RowList>
      <FrameFooter>
        <span className="tabular-nums">
          {count(data.pullRequests.length, 'pull request', 'pull requests')}
        </span>
        {data.syncedAt ? (
          <span className="ml-auto">
            Read from GitHub <RelativeTime value={data.syncedAt} className="font-mono" />
          </span>
        ) : null}
      </FrameFooter>
    </>
  )
}
