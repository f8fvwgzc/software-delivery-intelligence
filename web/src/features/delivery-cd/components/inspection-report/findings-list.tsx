import { RowList } from '@/components/system/row'
import { Tag } from '@/components/system/tag'

import type { InspectionFinding } from '../../api/types'
import { severityTone } from '../../lib/inspection'

/** Findings most severe first: what, where, the evidence, and what would make it go away. */
export function FindingsList({ findings }: { findings: InspectionFinding[] }) {
  if (findings.length === 0) {
    return <p className="text-xs text-muted-foreground">Nothing found in what was read.</p>
  }
  return (
    <RowList aria-label="Findings">
      {findings.map((finding, index) => (
        <li key={`${finding.rule}-${finding.path}-${index}`} className="flex flex-col gap-1.5 px-3 py-2.5">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <Tag tone={severityTone(finding.severity)} dot>
              {finding.severity}
            </Tag>
            <span className="min-w-0 text-sm font-medium">{finding.title}</span>
            <Tag mono variant="outline" title="Rule id: what inspect_ignore names">
              {finding.rule}
            </Tag>
          </div>
          {finding.path ? (
            <code className="truncate font-mono text-2xs text-muted-foreground" title={finding.path}>
              {finding.path}
            </code>
          ) : null}
          <p className="text-xs leading-relaxed text-muted-foreground">{finding.detail}</p>
          <p className="text-xs leading-relaxed">
            <span className="font-medium">Fix: </span>
            {finding.fix}
          </p>
        </li>
      ))}
    </RowList>
  )
}
