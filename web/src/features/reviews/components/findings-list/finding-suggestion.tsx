import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Code } from '@/components/system/code'
import { Tag } from '@/components/system/tag'

import type { Finding } from '../../api/types'
import { isExample, suggestionSource } from '../../lib/labels'

/**
 * The code that goes with a finding. A replacement (from the analyzer's rule,
 * or drafted by the workspace's model) is exactly what the finding's lines
 * should become and is what Apply fixes commits; an example is an
 * illustration to adapt by hand and is never committed. Once a fix commit
 * was requested, the execution that carries the replacement is linked.
 */
export function FindingSuggestion({ finding }: { finding: Finding }) {
  const source = suggestionSource(finding.suggestionSource)
  const example = isExample(finding.suggestionSource)

  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="text-2xs font-medium text-muted-foreground">{example ? 'Example to adapt' : 'Suggested replacement'}</span>
        {source ? (
          <Tag tone={source.tone} variant={source.tone === 'neutral' ? 'outline' : 'tint'}>
            {source.label}
          </Tag>
        ) : null}
        {finding.fixExecutionId ? (
          <Link
            to={routes.execution(finding.fixExecutionId)}
            title="The execution that commits this suggestion"
            className="inline-flex rounded-[4px] outline-none hover:[&>[data-slot=tag]]:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <Tag variant="outline">In fix run</Tag>
          </Link>
        ) : null}
      </div>
      <Code>{finding.suggestion}</Code>
    </div>
  )
}
