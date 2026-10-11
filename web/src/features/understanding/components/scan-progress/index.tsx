import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { AlertCircleIcon } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import {
  isScanActive,
  projectKeys,
  projectQueries,
  SCAN_POLL_MS,
  type Project,
  type Scan,
} from '@/features/projects/api/queries'
import { useEventSource } from '@/lib/live/use-event-source'

import { shortSha } from '../../lib/github'
import { SCAN_STATUS } from '../../lib/labels'
import { ScanCounts } from './scan-counts'
import { StageRail } from './stage-rail'

interface ScanProgressProps {
  project: Project
  scanId: string
}

function scanStreamUrl(scanId: string) {
  return `/api/v1/scans/${encodeURIComponent(scanId)}/stream`
}

/**
 * Live view of one scan. The gateway streams the whole `Scan` on every
 * change (`GET /scans/{id}/stream`), which lands straight in the query
 * cache; the 2s poll only runs while that stream is down. When the scan
 * settles, the project family is refetched so the page swaps to the real
 * content at once.
 */
export function ScanProgress({ project, scanId }: ScanProgressProps) {
  const queryClient = useQueryClient()
  const scanKey = projectKeys.scan(scanId)
  const lastState = useRef<string>('connecting')

  const stream = useEventSource<Scan>({
    url: scanStreamUrl(scanId),
    onMessage: (scan) => {
      if (scan.id === scanId) queryClient.setQueryData(scanKey, scan)
    },
    onStateChange: (state) => {
      // One fetch on each edge re-evaluates `refetchInterval`, so the poll
      // stops the moment the stream is open and resumes when it drops. Only
      // the first drop is an edge: every backoff attempt after it reports
      // `reconnecting` again and must not cost a fetch of its own.
      const edge = state === 'open' || (state === 'reconnecting' && lastState.current !== 'reconnecting')
      lastState.current = state
      if (edge) queryClient.invalidateQueries({ queryKey: scanKey })
    },
  })

  // A paused stream means the tab is hidden: nothing should fetch then, so
  // the poll only runs while the stream is actually down.
  const streamDown = stream.state !== 'open' && stream.state !== 'paused'
  const { data: scan } = useSuspenseQuery({
    ...projectQueries.scan(scanId),
    refetchInterval: (query) => (isScanActive(query.state.data?.status) && streamDown ? SCAN_POLL_MS : false),
  })

  const settled = !isScanActive(scan.status)
  useEffect(() => {
    if (!settled) return
    queryClient.invalidateQueries({ queryKey: projectKeys.detail(project.id) })
    queryClient.invalidateQueries({ queryKey: projectKeys.lists() })
  }, [settled, project.id, queryClient])

  const status = SCAN_STATUS[scan.status]
  const heading =
    scan.status === 'failed'
      ? `Scan of ${project.name} failed`
      : scan.status === 'succeeded'
        ? `Scan of ${project.name} finished`
        : `Scanning ${project.name}`

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-6 rounded-lg p-4 ring-1 ring-border lg:grid-cols-[minmax(0,1fr)_14rem]">
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <h2 className="text-sm font-medium">{heading}</h2>
            <StatusBadge tone={status.tone} live={status.live}>
              {status.label}
            </StatusBadge>
            <span className="font-mono text-xs text-muted-foreground">
              {scan.commitSha ? shortSha(scan.commitSha) : null}
              {scan.startedAt ? (
                <>
                  {scan.commitSha ? ' · ' : null}
                  started <RelativeTime value={scan.startedAt} />
                </>
              ) : null}
            </span>
            {!settled ? (
              <span className="ml-auto text-2xs text-muted-foreground" aria-live="polite">
                {stream.state === 'open' ? 'live' : stream.state === 'closed' ? 'stream unavailable, polling' : 'polling'}
              </span>
            ) : null}
          </div>

          {scan.stages.length > 0 ? (
            <StageRail stages={scan.stages} />
          ) : (
            <p className="text-sm text-muted-foreground">Waiting for the first stage to report.</p>
          )}
        </div>

        <ScanCounts scan={scan} />
      </div>

      {scan.status === 'failed' ? (
        <Alert variant="destructive">
          <AlertCircleIcon />
          <AlertTitle>The scan did not complete</AlertTitle>
          <AlertDescription>
            {scan.error || 'No error text was recorded.'} Fix the cause if it is on your side, then use Rescan above.
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}

export { ScanProgressSkeleton } from './scan-progress-skeleton'
