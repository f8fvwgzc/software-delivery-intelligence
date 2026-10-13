import { count } from '@/lib/intl'

import type { Finding, Review } from '../../api/types'
import { formatLocation } from '../../lib/labels'

/** Rows shown before the rest is summed up. */
const SHOWN = 5

/** What `POST /reviews/{id}/apply-fixes` commits: accepted, with a suggestion, not carried by a fix execution yet. */
function readyFixes(findings: readonly Finding[]) {
  return findings.filter((finding) => finding.decision === 'accepted' && Boolean(finding.suggestion) && !finding.fixExecutionId)
}

/** The accepted findings whose suggestion the commit will carry, by title and place. */
export function FixList({ review }: { review: Review }) {
  const fixes = readyFixes(review.findings)
  if (fixes.length === 0) return null
  const rest = fixes.length - SHOWN

  return (
    <div className="flex flex-col gap-1.5">
      <ul aria-label="Fixes in this commit" className="flex flex-col divide-y divide-border rounded-md ring-1 ring-border">
        {fixes.slice(0, SHOWN).map((finding) => (
          <li key={finding.id} className="flex min-w-0 flex-col gap-0.5 px-2.5 py-1.5">
            <span className="truncate text-xs font-medium">{finding.title}</span>
            <code className="truncate font-mono text-2xs text-muted-foreground" title={finding.path}>
              {formatLocation(finding.path, finding.line, finding.endLine)}
            </code>
          </li>
        ))}
      </ul>
      <p className="text-2xs leading-4 text-muted-foreground">
        {rest > 0 ? `And ${count(rest, 'more', 'more')}. ` : ''}
        When two suggestions overlap in one file, the more severe one is committed and the other stays available.
      </p>
    </div>
  )
}
