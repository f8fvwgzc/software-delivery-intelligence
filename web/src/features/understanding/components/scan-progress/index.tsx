import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { AlertCircleIcon } from 'lucide-react'
import { useEffect } from 'react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { isScanActive, projectKeys, projectQueries, type Project } from '@/features/projects/api/queries'

import { shortSha } from '../../lib/github'
import { SCAN_STATUS } from '../../lib/labels'
import { ScanCounts } from './scan-counts'
import { StageRail } from './stage-rail'

interface ScanProgressProps {
  project: Project
  scanId: string
}

/**
 * Live view of one scan, polled every 2s while it runs. When it settles,
 * the project family is refetched so the page swaps to the real content
 * without waiting for the slower project poll.
 */
export function ScanProgress({ project, scanId }: ScanProgressProps) {
  const queryClient = useQueryClient()
  const { data: scan } = useSuspenseQuery(projectQueries.scan(scanId))

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
