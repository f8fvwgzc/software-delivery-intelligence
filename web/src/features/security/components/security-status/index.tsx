import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatStrip } from '@/components/system/stat'
import { Tag } from '@/components/system/tag'
import { Skeleton } from '@/components/ui/skeleton'

import { securityQueries } from '../../api/queries'
import type { SecurityOverview } from '../../api/types'
import { alertsMeta, dependabotPullRequests, ecosystemLabel, securityUpdatesMeta, severityBreakdown, sourceMeta } from '../../lib/labels'

const NONE = '—'

/** One cell of the strip: a small-caps label, one 20px line that carries the answer, one muted line under it. */
function Cell({ label, children, detail }: { label: string; children: ReactNode; detail: ReactNode }) {
  return (
    // `data-slot="stat"`: the strip draws its hairlines and padding around cells of that name.
    <div data-slot="stat" className="flex min-w-0 flex-col gap-1">
      <span className="truncate text-2xs font-medium tracking-wide text-muted-foreground [font-variant-caps:all-small-caps]">{label}</span>
      <div className="flex min-h-5 min-w-0 flex-wrap items-center gap-1.5 text-sm leading-5 font-medium">{children}</div>
      <div className="min-w-0 text-xs leading-4 text-muted-foreground">{detail}</div>
    </div>
  )
}

function total(overview: SecurityOverview) {
  const counted = overview.bySeverity.reduce((sum, row) => sum + row.count, 0)
  return Math.max(counted, overview.vulnerabilities.length)
}

/**
 * Where the list comes from, how much is open, and what Dependabot does on
 * the repository. Reads the same query as the table below (one request);
 * while that loads or fails the strip keeps its shape, and the panel under
 * it carries the error.
 */
export function SecurityStatus({ projectId }: { projectId: string }) {
  const { data: overview, isPending } = useQuery(securityQueries.overview(projectId))
  const placeholder = isPending ? <Skeleton className="h-4 w-24" /> : NONE
  const detailPlaceholder = isPending ? <Skeleton className="my-0.5 h-3 w-32 max-w-full" /> : null

  const source = overview ? sourceMeta(overview.source) : null
  // Neither source answered: how much is open is not known, which is not the same as nothing.
  const unread = overview?.source === 'none'
  const open = overview ? total(overview) : 0
  const breakdown = overview ? severityBreakdown(overview.bySeverity) : ''
  const alerts = overview ? alertsMeta(overview.alerts) : null
  const updates = overview ? securityUpdatesMeta(overview.securityUpdates) : null

  return (
    <div className="flex flex-col gap-2">
      <StatStrip>
        <Cell
          label="Source"
          detail={
            overview ? (
              <>
                Read <RelativeTime value={overview.checkedAt} className="font-mono tabular-nums" />
              </>
            ) : (
              detailPlaceholder
            )
          }
        >
          {source ? source.label : placeholder}
        </Cell>

        <Cell
          label="Open vulnerabilities"
          detail={
            overview
              ? unread
                ? 'Could not be read'
                : open === 0
                  ? 'None in the list'
                  : overview.truncated
                    ? `${breakdown}; more exist than were read`
                    : breakdown
              : detailPlaceholder
          }
        >
          {overview ? (
            unread ? (
              NONE
            ) : (
              <span className="text-lg leading-5 font-semibold tabular-nums tracking-tight">{open.toLocaleString()}</span>
            )
          ) : (
            placeholder
          )}
        </Cell>

        <Cell
          label="Dependabot"
          detail={
            overview ? (
              overview.openDependabotPullRequests > 0 ? (
                <Link
                  to={routes.projectPullRequests(projectId)}
                  className="rounded-sm outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  {dependabotPullRequests(overview.openDependabotPullRequests)}
                </Link>
              ) : (
                'No open Dependabot pull requests'
              )
            ) : (
              detailPlaceholder
            )
          }
        >
          {alerts && updates ? (
            <>
              <Tag tone={alerts.tone} dot>
                {alerts.label}
              </Tag>
              <Tag tone={updates.tone} dot>
                {updates.label}
              </Tag>
            </>
          ) : (
            placeholder
          )}
        </Cell>

        <Cell
          label="Configuration file"
          detail={
            overview ? (
              overview.configPresent ? (
                overview.configEcosystems.length > 0 ? (
                  <span title={overview.configEcosystems.map(ecosystemLabel).join(', ')} className="block truncate">
                    Updates {overview.configEcosystems.map(ecosystemLabel).join(', ')}
                  </span>
                ) : (
                  'Names no ecosystem'
                )
              ) : (
                'Not in the repository'
              )
            ) : (
              detailPlaceholder
            )
          }
        >
          {overview ? (
            overview.configPresent ? (
              <span className="truncate font-mono text-xs font-normal" title={overview.configPath ?? undefined}>
                {overview.configPath ?? '.github/dependabot.yml'}
              </span>
            ) : (
              'None'
            )
          ) : (
            placeholder
          )}
        </Cell>
      </StatStrip>

      {/* Why the list is not Dependabot's, in the server's own words. With no list at all, the panel below says it where the list would be. */}
      {overview?.sourceNote && !unread ? (
        <p role="note" className="max-w-[90ch] px-0.5 text-xs leading-5 text-muted-foreground">
          {overview.sourceNote}
        </p>
      ) : null}
    </div>
  )
}
