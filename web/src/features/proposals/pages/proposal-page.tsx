import { useSuspenseQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Link, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime, StatusBadge } from '@/components/shared/data'
import { Page, PageHeader, Section } from '@/components/shared/page'

import { proposalQuery } from '../api/queries'
import { AcceptDialog } from '../components/accept-dialog'
import { NegotiationPanel } from '../components/negotiation'
import { OptionsTable } from '../components/options-table'
import { PlanRail } from '../components/plan-rail'
import { ProposalStatusBanner } from '../components/proposal-status-banner'
import { RecommendedOption } from '../components/recommended-option'
import { RejectDialog } from '../components/reject-dialog'
import { isProposalClosed, PROPOSAL_KIND_LABEL, PROPOSAL_STATUS } from '../lib/labels'

/** /proposals/:proposalId — the core screen: options on the left, negotiation on the right. */
export default function ProposalPage() {
  const { proposalId = '' } = useParams()
  const { data: proposal } = useSuspenseQuery(proposalQuery(proposalId))

  // The option in focus. Null means "follow the recommendation", so a
  // re-rank from negotiation moves the focus without an effect.
  const [chosenId, setChosenId] = useState<string | null>(null)
  const closed = isProposalClosed(proposal.status)
  const focusId =
    (closed ? proposal.acceptedOptionId : null) ??
    (chosenId && proposal.options.some((option) => option.id === chosenId) ? chosenId : null) ??
    proposal.recommendedOptionId
  const focus = proposal.options.find((option) => option.id === focusId) ?? proposal.options[0]
  const status = PROPOSAL_STATUS[proposal.status]
  const others = proposal.options.length - 1

  return (
    <Page wide>
      <PageHeader
        eyebrow={
          <>
            <StatusBadge tone={status.tone} live={status.live}>
              {status.label}
            </StatusBadge>
            <span className="text-xs text-muted-foreground">{PROPOSAL_KIND_LABEL[proposal.kind]}</span>
            <span className="text-xs text-muted-foreground" aria-hidden="true">
              ·
            </span>
            <Link to={routes.project(proposal.projectId)} className="text-xs text-muted-foreground hover:underline">
              Project
            </Link>
          </>
        }
        title={proposal.title}
        description={
          <>
            {proposal.summary}
            {proposal.prompt ? <span className="block text-xs">Asked for: “{proposal.prompt}”</span> : null}
          </>
        }
        actions={
          closed ? (
            <span className="text-xs text-muted-foreground">
              Updated <RelativeTime value={proposal.updatedAt} className="font-mono" />
            </span>
          ) : (
            <>
              <RejectDialog proposalId={proposal.id} />
              <AcceptDialog proposal={proposal} selectedOptionId={focusId} />
            </>
          )
        }
      />

      <ProposalStatusBanner proposal={proposal} />

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_22rem] xl:grid-cols-[minmax(0,1fr)_24rem]">
        <div className="flex min-w-0 flex-col gap-6">
          {focus ? (
            <RecommendedOption option={focus} recommended={focus.id === proposal.recommendedOptionId} />
          ) : (
            <p className="text-sm text-muted-foreground">This proposal has no options.</p>
          )}

          {others > 0 ? (
            <Section
              title="All options"
              description={`${proposal.options.length} ranked against the project. Choose one to inspect its plan.`}
            >
              <OptionsTable
                options={proposal.options}
                recommendedId={proposal.recommendedOptionId}
                selectedId={focusId}
                onChoose={setChosenId}
                readOnly={closed}
              />
            </Section>
          ) : null}

          {focus ? (
            <Section
              title="Plan"
              description={`${focus.plan.length} steps, run one at a time with a checkpoint after each.`}
            >
              <PlanRail steps={focus.plan} />
            </Section>
          ) : null}
        </div>

        <div className="lg:sticky lg:top-16">
          <NegotiationPanel proposal={proposal} readOnly={closed} />
        </div>
      </div>
    </Page>
  )
}
