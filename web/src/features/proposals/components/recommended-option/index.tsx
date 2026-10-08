import { AlertTriangleIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import type { components } from '@/lib/api/schema'

import { formatMonthlyCost, formatPercent } from '../../lib/format'
import { EvidenceList } from './evidence-list'
import { ProsCons } from './pros-cons'
import { ScoreBreakdownBars } from './score-breakdown'

type ProposalOption = components['schemas']['ProposalOption']

interface RecommendedOptionProps {
  option: ProposalOption
  /** True when this option is the one the ranker recommends. */
  recommended: boolean
}

/**
 * The hero block: the option in focus with its score, breakdown, evidence
 * and trade-offs. The one place on the page the brand accent is spent.
 */
export function RecommendedOption({ option, recommended }: RecommendedOptionProps) {
  const overall = option.score?.overall ?? 0

  return (
    <article className="flex flex-col gap-5 rounded-lg bg-card p-4 ring-1 ring-border md:p-5">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span>{recommended ? 'Recommended option' : `Option ${option.rank}`}</span>
            <span aria-hidden="true">·</span>
            <span className="font-mono">{option.patternId}</span>
          </div>
          <h2 className="text-base font-semibold leading-6 tracking-tight">{option.title}</h2>
          <p className="max-w-prose text-sm text-muted-foreground">{option.summary}</p>
        </div>
        <div className="flex shrink-0 flex-col items-start gap-0.5 sm:items-end">
          <span className="text-3xl font-semibold leading-none tabular-nums tracking-tight text-brand">
            {formatPercent(overall)}
          </span>
          <span className="text-xs text-muted-foreground">overall score</span>
          <span className="mt-1 text-xs tabular-nums text-muted-foreground">{formatMonthlyCost(option.estimatedMonthlyCostUsd)} estimated</span>
        </div>
      </header>

      <div className="grid gap-5 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
        {option.score ? (
          <ScoreBreakdownBars score={option.score} />
        ) : (
          <p className="text-xs text-muted-foreground">No score breakdown for this option.</p>
        )}
        <ProsCons pros={option.pros} cons={option.cons} />
      </div>

      {option.unmetRequirements.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="mr-1 inline-flex items-center gap-1 text-xs text-warning">
            <AlertTriangleIcon className="size-3.5" />
            Unmet requirements
          </span>
          {option.unmetRequirements.map((requirement) => (
            <Badge
              key={requirement}
              variant="outline"
              className="h-5 border-warning/25 bg-warning/10 px-1.5 font-mono text-2xs font-normal text-warning"
            >
              {requirement}
            </Badge>
          ))}
        </div>
      ) : null}

      <EvidenceList evidence={option.evidence} />
    </article>
  )
}
