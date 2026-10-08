import { LightbulbIcon } from 'lucide-react'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { EmptyState } from '@/components/shared/feedback'
import { cn } from '@/lib/utils'

import type { Proposal } from '../../api/types'
import { DOT_CLASS, PROPOSAL_KIND_LABEL, PROPOSAL_STATUS } from '../../lib/labels'
import { CompactList, CompactListRow } from '../compact-list'

interface RecentProposalsProps {
  proposals: Proposal[]
  /** Proposals carry only a project id; names come from the projects list. */
  projectNames: ReadonlyMap<string, string>
}

export function RecentProposals({ proposals, projectNames }: RecentProposalsProps) {
  return (
    <CompactList
      title="Recent proposals"
      viewAll={{ to: routes.proposals(), label: 'All proposals' }}
      empty={
        proposals.length === 0 ? (
          <EmptyState
            icon={LightbulbIcon}
            title="No proposals yet"
            description="Open a project and ask for a proposal once its first scan is done."
            className="py-6"
          />
        ) : undefined
      }
    >
      {proposals.map((proposal) => {
        const status = PROPOSAL_STATUS[proposal.status]
        return (
          <CompactListRow
            key={proposal.id}
            to={routes.proposal(proposal.id)}
            leading={<span aria-hidden className={cn('size-1.5 shrink-0 rounded-full', DOT_CLASS[status.tone])} />}
            title={proposal.title}
            meta={
              <span className="truncate">
                {projectNames.get(proposal.projectId) ?? 'Unknown project'} · {PROPOSAL_KIND_LABEL[proposal.kind]} ·{' '}
                {status.label}
              </span>
            }
            trailing={<RelativeTime value={proposal.updatedAt} />}
          />
        )
      })}
    </CompactList>
  )
}
