import { useSuspenseQuery } from '@tanstack/react-query'
import { WalletIcon } from 'lucide-react'
import { useState } from 'react'
import { Link, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { PlanRail, RelativeTime, StatusBadge } from '@/components/shared/data'
import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Tag } from '@/components/system/tag'
import { ButtonGroup } from '@/components/ui/button-group'

import { proposalQuery } from '../api/queries'
import { AcceptDialog } from '../components/accept-dialog'
import { ArtifactsPanel, ArtifactsSkeleton, DownloadAllButton } from '../components/artifacts'
import { NegotiationPanel } from '../components/negotiation'
import { OptionsTable } from '../components/options-table'
import { ProposalStatusBanner } from '../components/proposal-status-banner'
import { RecommendedOption } from '../components/recommended-option'
import { RejectDialog } from '../components/reject-dialog'
import { budgetOf } from '../lib/budget'
import { formatMonthlyCost } from '../lib/format'
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
  const budget = budgetOf(proposal)

  return (
    <Page>
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
            {budget !== null ? (
              <Tag variant="outline" className="tabular-nums">
                <WalletIcon aria-hidden="true" className="size-3" />
                Budget {formatMonthlyCost(budget)}
              </Tag>
            ) : null}
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
            <ButtonGroup aria-label="Decide on this proposal">
              <RejectDialog proposalId={proposal.id} />
              <AcceptDialog proposal={proposal} selectedOptionId={focusId} />
            </ButtonGroup>
          )
        }
      />

      <ProposalStatusBanner proposal={proposal} />

      <div className="grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem] xl:grid-cols-[minmax(0,1fr)_24rem]">
        <div className="flex min-w-0 flex-col gap-4">
          {focus ? (
            <RecommendedOption
              option={focus}
              recommended={focus.id === proposal.recommendedOptionId}
              budgetMonthlyUsd={budget}
            />
          ) : (
            <p className="text-sm text-muted-foreground">This proposal has no options.</p>
          )}

          {others > 0 ? (
            <Frame
              title="All options"
              description={`${proposal.options.length} ranked against the project. Choose one to inspect its plan and files.`}
              padding="none"
            >
              <OptionsTable
                options={proposal.options}
                recommendedId={proposal.recommendedOptionId}
                selectedId={focusId}
                onChoose={setChosenId}
                readOnly={closed}
                budgetMonthlyUsd={budget}
              />
            </Frame>
          ) : null}

          {focus ? (
            <Frame
              title="Plan"
              description={`${focus.plan.length} steps, run one at a time with a checkpoint after each.`}
            >
              <PlanRail steps={focus.plan} />
            </Frame>
          ) : null}

          {focus ? (
            <Frame
              title="Files"
              padding="none"
              description={
                focus.artifactCount === 0
                  ? 'This option adds no files to the repository.'
                  : `${focus.artifactCount} generated ${focus.artifactCount === 1 ? 'file' : 'files'}, complete and ready to commit through a pull request.`
              }
              actions={
                focus.artifactCount > 0 ? (
                  <DownloadAllButton proposalId={proposal.id} optionId={focus.id} artifactCount={focus.artifactCount} />
                ) : null
              }
            >
              {focus.artifactCount > 0 ? (
                <QueryBoundary fallback={<ArtifactsSkeleton files={Math.min(focus.artifactCount, 8)} />}>
                  <ArtifactsPanel key={focus.id} proposalId={proposal.id} optionId={focus.id} />
                </QueryBoundary>
              ) : (
                <p className="p-3 text-xs text-muted-foreground">Nothing to review here; the plan only runs actions.</p>
              )}
            </Frame>
          ) : null}
        </div>

        {/* Sticky, but never taller than the viewport: the panel scrolls on its own past that. */}
        <div className="lg:sticky lg:top-[calc(var(--header-h)+1rem)] lg:max-h-[calc(100svh-var(--header-h)-2rem)] lg:overflow-y-auto">
          <NegotiationPanel proposal={proposal} readOnly={closed} />
        </div>
      </div>
    </Page>
  )
}
