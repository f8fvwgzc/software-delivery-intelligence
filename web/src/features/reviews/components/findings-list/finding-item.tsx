import { CheckIcon } from 'lucide-react'
import type { CSSProperties } from 'react'
import { toast } from 'sonner'

import { Markdown } from '@/components/shared/data/markdown'
import { Person } from '@/components/shared/data/person'
import { Code } from '@/components/system/code'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'

import { useDecideFinding } from '../../api/mutations'
import type { Finding } from '../../api/types'
import { DECISION, formatLocation } from '../../lib/labels'
import { DismissDialog } from './dismiss-dialog'

interface FindingItemProps {
  finding: Finding
  reviewId: string
  decidable: boolean
  /** Position in the windowed list and its measurer, when the list is long. */
  index?: number
  measure?: (element: HTMLElement | null) => void
  style?: CSSProperties
}

/** One finding: where, which rule, the evidence, the suggested replacement, and the decision. */
export function FindingItem({ finding, reviewId, decidable, index, measure, style }: FindingItemProps) {
  const decide = useDecideFinding(reviewId)
  const decision = DECISION[finding.decision] ?? DECISION.pending
  const pending = finding.decision === 'pending' || finding.decision === 'unspecified'
  const accepting = decide.isPending && decide.variables?.decision === 'accepted'
  const dismissing = decide.isPending && decide.variables?.decision === 'dismissed'

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
      {finding.suggestion ? (
        <div className="flex flex-col gap-1">
          <span className="text-2xs font-medium text-muted-foreground">Suggested replacement</span>
          <Code>{finding.suggestion}</Code>
        </div>
      ) : null}
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
      {decidable && pending ? (
        <div className="flex items-center gap-2 pt-0.5">
          <Button size="xs" onClick={accept} loading={accepting} disabled={dismissing}>
            <CheckIcon data-icon="inline-start" />
            Accept
          </Button>
          <DismissDialog finding={finding} decide={decide} />
        </div>
      ) : null}
    </li>
  )
}
