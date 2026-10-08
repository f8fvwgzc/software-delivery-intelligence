import { TechChips } from '@/components/shared/data/tech-chip'
import { cn } from '@/lib/utils'

import type { DependabotStatus as DependabotStatusData } from '../../api/queries'
import { ecosystemLabel } from '../../lib/labels'

interface DependabotStatusProps {
  status: DependabotStatusData | null | undefined
}

/** Whether automated updates are set up, and how much of them is waiting on review. */
export function DependabotStatus({ status }: DependabotStatusProps) {
  const configured = status?.configured ?? false

  return (
    <div className="flex flex-col gap-3 rounded-lg p-4 ring-1 ring-border">
      <div className="flex items-center gap-2">
        <span
          className={cn('size-1.5 shrink-0 rounded-full', configured ? 'bg-success' : 'bg-warning')}
          aria-hidden
        />
        <span className="text-sm font-medium">{configured ? 'Dependabot configured' : 'Dependabot not configured'}</span>
      </div>

      {configured && status ? (
        <dl className="flex flex-col gap-2 text-xs">
          <div className="flex items-baseline justify-between gap-3">
            <dt className="text-muted-foreground">Open pull requests</dt>
            <dd className="text-sm tabular-nums">{status.openPullRequests.toLocaleString()}</dd>
          </div>
          <div className="flex items-baseline justify-between gap-3">
            <dt className="shrink-0 text-muted-foreground">Config</dt>
            <dd className="min-w-0 truncate font-mono text-muted-foreground" title={status.configPath}>
              {status.configPath || 'unknown'}
            </dd>
          </div>
          <div className="flex flex-col gap-1">
            <dt className="text-muted-foreground">Updates</dt>
            <dd>
              {status.ecosystems.length > 0 ? (
                <TechChips items={status.ecosystems.map(ecosystemLabel)} max={6} />
              ) : (
                <span className="text-muted-foreground">No ecosystems listed</span>
              )}
            </dd>
          </div>
        </dl>
      ) : (
        <p className="text-xs text-muted-foreground">
          No <span className="font-mono">.github/dependabot.yml</span> was found. The weekly updates pattern
          in Proposals sets one up.
        </p>
      )}
    </div>
  )
}
