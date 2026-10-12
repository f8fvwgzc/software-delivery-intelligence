import { LightbulbIcon } from 'lucide-react'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { PanelEmpty } from '@/components/system/panel'
import { TagDot } from '@/components/system/tag'

import type { Proposal } from '../../api/types'
import { PROPOSAL_KIND_LABEL, PROPOSAL_STATUS } from '../../lib/labels'
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
          <PanelEmpty
            icon={LightbulbIcon}
            title="No proposals yet"
            description="Open a project and ask for a proposal once its first scan is done."
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
            leading={<TagDot tone={status.tone} />}
            title={proposal.title}
            meta={
              <span className="truncate">
                {projectNames.get(proposal.projectId) ?? 'Unknown project'} · {PROPOSAL_KIND_LABEL[proposal.kind]} ·{' '}
                {status.label}
              </span>
            }
            trailing={<RelativeTime value={proposal.updatedAt} className="font-mono tabular-nums" />}
          />
        )
      })}
    </CompactList>
  )
}
