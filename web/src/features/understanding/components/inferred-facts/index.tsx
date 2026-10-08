import { SparklesIcon } from 'lucide-react'

import { ScoreBar } from '@/components/shared/data/score-bar'
import { EmptyState } from '@/components/shared/feedback'

import type { Fact } from '../../api/queries'
import { FACT_CATEGORY } from '../../lib/labels'

function InferredFactRow({ fact }: { fact: Fact }) {
  return (
    <li className="flex flex-col gap-1.5 border-b border-border/60 py-3 first:pt-0 last:border-0 last:pb-0">
      <div className="flex items-baseline justify-between gap-3">
        <span className="min-w-0 truncate text-sm" title={fact.label}>
          {fact.label}
          {fact.value ? <span className="text-muted-foreground"> · {fact.value}</span> : null}
        </span>
        <span className="shrink-0 text-2xs text-muted-foreground">{FACT_CATEGORY[fact.category]}</span>
      </div>
      <ScoreBar value={fact.confidence} />
      {fact.evidence ? (
        <code className="truncate text-2xs text-muted-foreground" title={fact.evidence}>
          {fact.evidence}
        </code>
      ) : null}
    </li>
  )
}

/** Conclusions drawn from the detected facts, each with its confidence. */
export function InferredFacts({ facts }: { facts: Fact[] }) {
  if (facts.length === 0) {
    return (
      <EmptyState
        icon={SparklesIcon}
        title="Nothing inferred yet"
        description="Inferences appear once there are enough detected facts to draw them from."
        className="py-8"
      />
    )
  }

  return (
    <ul className="flex flex-col">
      {facts.map((fact) => (
        <InferredFactRow key={fact.id} fact={fact} />
      ))}
    </ul>
  )
}
