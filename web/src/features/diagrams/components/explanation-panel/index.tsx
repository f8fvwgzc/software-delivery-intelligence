import { Markdown } from '@/components/shared/data/markdown'
import { Section } from '@/components/shared/page'

import type { Diagram } from '../../api/queries'
import { EvidenceTable } from './evidence-table'
import { count } from '@/lib/intl'

interface ExplanationPanelProps {
  diagram: Diagram
}

/** The reasoner's explanation, then every edge with the file that proves it. */
export function ExplanationPanel({ diagram }: ExplanationPanelProps) {
  const explanation = diagram.explanation.trim()
  const edges = diagram.spec?.edges ?? []
  const nodes = diagram.spec?.nodes ?? []

  return (
    <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <Section title="What it shows" description="Written at generation time, with the limits of its evidence.">
        <div className="rounded-lg bg-card px-4 py-4 ring-1 ring-border">
          {explanation.length === 0 ? (
            <p className="text-sm text-muted-foreground">No explanation was written for this diagram.</p>
          ) : (
            <Markdown className="max-w-[72ch]">{explanation}</Markdown>
          )}
        </div>
      </Section>
      <Section
        title="Evidence"
        description={`${count(edges.length, 'edge', 'edges')}; each names the file that shows it, or is inferred.`}
      >
        <EvidenceTable projectId={diagram.projectId} edges={edges} nodes={nodes} />
      </Section>
    </div>
  )
}
