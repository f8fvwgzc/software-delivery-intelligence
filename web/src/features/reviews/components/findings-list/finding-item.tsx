import { CheckIcon } from 'lucide-react'
import type { CSSProperties } from 'react'
import { toast } from 'sonner'

import { Markdown } from '@/components/shared/data/markdown'
import { Person } from '@/components/shared/data/person'
import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'

import { useDecideFinding, useDraftFix } from '../../api/mutations'
import type { Finding } from '../../api/types'
import { DECISION, formatLocation, isExample } from '../../lib/labels'
import { decideGate, draftFixGate } from '../../lib/permissions'
import { DismissDialog } from './dismiss-dialog'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'
import { DraftFixButton } from './draft-fix'
import { FindingSuggestion } from './finding-suggestion'

interface FindingItemProps {
  finding: Finding
  reviewId: string
  decidable: boolean
  /** Position in the windowed list and its measurer, when the list is long. */
  index?: number
  measure?: (element: HTMLElement | null) => void
  style?: CSSProperties
}

/** One finding: where, which rule, the evidence, the suggested replacement (or the way to draft one), and the decision. */
export function FindingItem({ finding, reviewId, decidable, index, measure, style }: FindingItemProps) {
  const { currentWorkspace } = useAuth()
  const decide = useDecideFinding(reviewId)
  const draft = useDraftFix(reviewId)
  const decision = DECISION[finding.decision] ?? DECISION.pending
  const pending = finding.decision === 'pending' || finding.decision === 'unspecified'
  const accepting = decide.isPending && decide.variables?.decision === 'accepted'
  const dismissing = decide.isPending && decide.variables?.decision === 'dismissed'
  const canDecide = decidable && pending
  const decideReason = decideGate(currentWorkspace.myRole)
  // The server says where a fix can be drafted: a finding without a replacement yet (none at all, or only the
  // analyzer's example) that points at lines of a text file.
  const canDraft = finding.canDraftFix && (!finding.suggestion || isExample(finding.suggestionSource))

  const accept = () => {
    decide.mutate(
      { findingId: finding.id, decision: 'accepted' },
      { onSuccess: () => toast.success('Finding accepted', { description: 'It will be posted as a comment when the review is published.' }) },
    )
  }

  return (
    <li className="flex flex-col gap-2 border-b px-3 py-2.5 last:border-b-0" data-decision={finding.decision} data-index={index} ref={measure} style={style}>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="text-sm font-medium">{finding.title}</span>
        <Tag mono variant="outline">
          {finding.rule || finding.category}
        </Tag>
        {!pending ? (
          <Tag tone={decision.tone} dot className="ml-auto">
            {decision.label}
          </Tag>
        ) : null}
      </div>
      <code className="truncate font-mono text-xs text-muted-foreground" title={finding.path}>
        {formatLocation(finding.path, finding.line, finding.endLine)}
      </code>
      {finding.detail ? <Markdown className="text-xs [&_p]:my-1.5">{finding.detail}</Markdown> : null}
      {finding.suggestion ? <FindingSuggestion finding={finding} /> : null}
      {!pending && finding.decisionNote ? (
        <p className="text-xs text-muted-foreground">
          {finding.decidedBy ? (
            <>
              <Person value={finding.decidedBy} avatar={false} className="font-medium text-foreground/80" />
              {': '}
            </>
          ) : null}
          {finding.decisionNote}
        </p>
      ) : null}
      {canDecide || canDraft ? (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          {canDecide ? (
            <DisabledReason reason={decideReason}>
              <span className="flex items-center gap-2">
                <Button size="xs" onClick={accept} loading={accepting} disabled={dismissing || decideReason !== null}>
                  <CheckIcon data-icon="inline-start" />
                  Accept
                </Button>
                <DismissDialog finding={finding} decide={decide} disabled={decideReason !== null} />
              </span>
            </DisabledReason>
          ) : null}
          {canDraft ? <DraftFixButton finding={finding} draft={draft} disabledReason={draftFixGate(currentWorkspace.myRole)} /> : null}
        </div>
      ) : null}
      {/* A refused draft names the switch to turn on: it stays under the finding it was asked for. */}
      {draft.isError ? <RefusalNote error={draft.error} /> : null}
    </li>
  )
}
