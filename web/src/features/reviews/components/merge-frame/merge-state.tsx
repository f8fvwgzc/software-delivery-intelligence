import { Tag } from '@/components/system/tag'
import { count } from '@/lib/intl'

import type { MergeReadiness } from '../../api/types'
import { readinessTags } from '../../lib/merge'

/** GitHub's side as tags, with the names of the checks that failed or are still running under them. */
export function MergeState({ readiness }: { readiness: MergeReadiness }) {
  return (
    <div className="flex flex-col gap-2">
      <ul aria-label="State on GitHub" className="flex flex-wrap gap-1.5">
        {readinessTags(readiness).map((tag) => (
          <li key={tag.key} className="flex">
            <Tag tone={tag.tone} variant={tag.quiet ? 'outline' : 'tint'} dot={tag.live} live={tag.live} title={tag.title}>
              {tag.label}
            </Tag>
          </li>
        ))}
      </ul>
      <CheckNames label="Failed" names={readiness.failedChecks} total={readiness.checksFailed} />
      <CheckNames label="Still running" names={readiness.pendingChecks} total={readiness.checksPending} />
    </div>
  )
}

/** The server names at most 20 checks; the count says how many there are. */
function CheckNames({ label, names, total }: { label: string; names: readonly string[]; total: number }) {
  if (names.length === 0) return null
  const rest = total - names.length

  return (
    <p className="text-xs leading-4 text-muted-foreground">
      {label}: <span className="font-mono text-foreground/80">{names.join(', ')}</span>
      {rest > 0 ? ` and ${count(rest, 'more', 'more')}` : ''}
    </p>
  )
}
