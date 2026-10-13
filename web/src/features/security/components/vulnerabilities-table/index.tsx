import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { RefreshCwIcon, ShieldCheckIcon, ShieldOffIcon } from 'lucide-react'
import { useRef, useState } from 'react'

import { routes } from '@/app/router/routes'
import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { useWindowRows } from '@/components/system/virtual-rows'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { pullRequestQueries } from '@/features/reviews/api/queries'
import { count } from '@/lib/intl'

import { useRefreshSecurity } from '../../api/mutations'
import { securityQueries } from '../../api/queries'
import { sourceMeta } from '../../lib/labels'
import { COLUMN_COUNT, HEAD_CLASS, ROW_HEIGHT } from './columns'
import { ALL_SEVERITIES, SeverityFilter } from './severity-filter'
import { VulnerabilityRow } from './vulnerability-row'

/**
 * The vulnerable dependencies, most severe first as the server sends them,
 * narrowed by severity. The list can hold 300 rows: past 200 only the ones
 * on screen are in the DOM (the table scrolls with the page; spacer rows
 * keep its height).
 */
export function VulnerabilitiesTable({ projectId }: { projectId: string }) {
  const { data: overview } = useSuspenseQuery(securityQueries.overview(projectId))
  const refresh = useRefreshSecurity(projectId)
  const [picked, setPicked] = useState(ALL_SEVERITIES)
  const bodyRef = useRef<HTMLTableSectionElement>(null)
  // The open pull requests, only if the pull requests tab already read them: "Fix: #n" then leads to that review.
  const { data: pullRequests } = useQuery({ ...pullRequestQueries.list(projectId, 'open'), enabled: false })

  const all = overview.vulnerabilities
  // A severity that is gone after a refetch falls back to everything.
  const severity = overview.bySeverity.some((row) => row.severity === picked && row.count > 0) ? picked : ALL_SEVERITIES
  const shown = severity === ALL_SEVERITIES ? all : all.filter((vulnerability) => vulnerability.severity === severity)
  const rows = useWindowRows({ count: shown.length, estimateSize: ROW_HEIGHT, listRef: bodyRef })

  if (overview.source === 'none') {
    return (
      <PanelEmpty
        icon={ShieldOffIcon}
        title="The vulnerabilities could not be read"
        description={overview.sourceNote ?? 'Neither Dependabot nor the OSV lookup answered for this project.'}
        action={
          <Button variant="outline" size="sm" onClick={() => refresh.mutate()} loading={refresh.isPending}>
            <RefreshCwIcon data-icon="inline-start" />
            Refresh
          </Button>
        }
      />
    )
  }

  if (all.length === 0) {
    return (
      <PanelEmpty
        icon={ShieldCheckIcon}
        title="No known vulnerabilities"
        description={
          overview.source === 'dependabot'
            ? 'Dependabot has no open alert for this repository.'
            : 'No advisory matches the exact versions this project’s manifests name. Version ranges are not looked up.'
        }
      />
    )
  }

  const fixHref = (number: number | null | undefined) => {
    if (number == null) return null
    const reviewId = pullRequests?.pullRequests.find((pullRequest) => pullRequest.number === number)?.latestReviewId
    return reviewId ? routes.review(reviewId) : routes.projectPullRequests(projectId)
  }

  return (
    <>
      <div className="flex min-h-9 flex-wrap items-center justify-between gap-x-3 gap-y-1.5 border-b px-3 py-1.5">
        <SeverityFilter counts={overview.bySeverity} total={all.length} value={severity} onChange={setPicked} />
        <span className="ml-auto text-xs tabular-nums text-muted-foreground" aria-live="polite">
          {severity === ALL_SEVERITIES ? count(all.length, 'vulnerability', 'vulnerabilities') : `${shown.length.toLocaleString()} of ${all.length.toLocaleString()}`}
        </span>
      </div>

      <Table data-virtual={rows.virtual || undefined}>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Severity</TableHead>
            <TableHead className={HEAD_CLASS}>Package</TableHead>
            <TableHead className={HEAD_CLASS}>Version</TableHead>
            <TableHead className={HEAD_CLASS}>Advisory</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden lg:table-cell`}>Manifest</TableHead>
            <TableHead className={`${HEAD_CLASS} text-right`}>Fix</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody ref={bodyRef}>
          {rows.virtual && rows.offsetTop > 0 ? <SpacerRow height={rows.offsetTop} /> : null}
          {rows.items.map((item) => {
            const vulnerability = shown[item.index]
            if (!vulnerability) return null
            return (
              <VulnerabilityRow
                // An advisory can hit one package in two manifests.
                key={`${vulnerability.id}:${vulnerability.package}:${vulnerability.manifestPath ?? ''}:${item.index}`}
                vulnerability={vulnerability}
                fixHref={fixHref(vulnerability.fixPullRequest)}
                index={item.index}
                measure={rows.virtual ? rows.measure : undefined}
              />
            )
          })}
          {rows.virtual && rows.offsetBottom > 0 ? <SpacerRow height={rows.offsetBottom} /> : null}
        </TableBody>
      </Table>

      <FrameFooter>
        <span>From {sourceMeta(overview.source).label}, most severe first.</span>
        {overview.truncated ? <span className="ml-auto">More exist than were read.</span> : null}
      </FrameFooter>
    </>
  )
}

/** Keeps the table as tall as every row while only a window of them is rendered. */
function SpacerRow({ height }: { height: number }) {
  return (
    <TableRow aria-hidden className="border-0 hover:bg-transparent" style={{ height }}>
      <td colSpan={COLUMN_COUNT} className="p-0" />
    </TableRow>
  )
}
