import { ShieldCheckIcon } from 'lucide-react'

import { PanelEmpty } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'

import type { Finding, FindingSeverity, Review } from '../../api/types'
import { SEVERITY, SEVERITY_ORDER, canDecide } from '../../lib/labels'
import { FindingItem } from './finding-item'

/** Findings grouped by severity, worst first; each group is a sub-frame of the list. */
export function FindingsList({ review }: { review: Review }) {
  if (review.findings.length === 0) {
    return (
      <PanelEmpty
        icon={ShieldCheckIcon}
        title={review.status === 'proposed' || review.status === 'published' ? 'No findings' : 'Findings appear after the analyzers run'}
        description={
          review.status === 'proposed' || review.status === 'published'
            ? 'Every analyzer passed over this diff without a note.'
            : 'Secrets, security, dependencies, Docker, CI, IaC, tests, docs and correctness rules run over the diff.'
        }
      />
    )
  }

  const groups = new Map<FindingSeverity, Finding[]>()
  for (const finding of review.findings) {
    const list = groups.get(finding.severity) ?? []
    list.push(finding)
    groups.set(finding.severity, list)
  }
  const decidable = canDecide(review.status)

  return (
    <div className="flex flex-col">
      {SEVERITY_ORDER.filter((severity) => groups.has(severity)).map((severity) => {
        const items = groups.get(severity) ?? []
        const meta = SEVERITY[severity]
        return (
          <section key={severity} aria-label={`${meta.label} findings`} className="border-b last:border-0">
            <header className="flex h-8 items-center gap-2 bg-muted/40 px-3">
              <Tag tone={meta.tone}>{meta.label}</Tag>
              <span className="text-2xs tabular-nums text-muted-foreground">
                {items.length} {items.length === 1 ? 'finding' : 'findings'}
              </span>
            </header>
            <ol className="divide-y divide-border">
              {items.map((finding) => (
                <FindingItem key={finding.id} finding={finding} reviewId={review.id} decidable={decidable} />
              ))}
            </ol>
          </section>
        )
      })}
    </div>
  )
}
