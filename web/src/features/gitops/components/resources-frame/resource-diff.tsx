import { useQuery } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'

import { describeError } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'

import { gitopsQueries } from '../../api/queries'
import type { ResourceDiff as ResourceDiffEntry, ResourceStatus } from '../../api/types'
import { diffStats, parseUnifiedDiff } from '../../lib/diff'
import { findResourceDiff, resourceName } from '../../lib/resources'
import { DiffLines } from './diff-lines'

interface ResourceDiffProps {
  applicationId: string
  resource: ResourceStatus
  id: string
}

const NOTE = 'px-1 py-2 text-xs text-muted-foreground'

/**
 * One resource's diff out of the application's. The first resource opened
 * asks for the diff of all of them, once; the others read the same answer.
 */
export function ResourceDiff({ applicationId, resource, id }: ResourceDiffProps) {
  const query = useQuery(gitopsQueries.diff(applicationId))

  return (
    <div id={id} className="flex flex-col gap-2 px-3 pt-1 pb-3 pl-10">
      {query.isPending ? (
        <div role="status" aria-label="Loading the diff" className="flex flex-col gap-1.5 py-1">
          <Skeleton className="h-3 w-3/5" />
          <Skeleton className="h-3 w-2/5" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ) : query.isError ? (
        <DiffError error={query.error} retry={() => query.refetch()} retrying={query.isFetching} />
      ) : (
        <DiffBody resource={resource} entry={findResourceDiff(query.data.resources, resource)} />
      )}
    </div>
  )
}

function DiffBody({ resource, entry }: { resource: ResourceStatus; entry: ResourceDiffEntry | undefined }) {
  const diff = entry?.diff
  if (!entry) return <p className={NOTE}>The diff does not list this resource yet. Refresh the application, then open it again.</p>
  if (!diff?.trim()) {
    if (resource.syncStatus === 'missing') return <p className={NOTE}>Not in the cluster yet: a sync creates it.</p>
    if (resource.syncStatus === 'extraneous') return <p className={NOTE}>In the cluster and no longer desired: a sync with prune deletes it.</p>
    return <p className={NOTE}>No differences: the cluster matches the repository.</p>
  }
  const lines = parseUnifiedDiff(diff)
  const { added, removed } = diffStats(lines)
  return (
    <>
      <p className="flex items-center gap-3 text-2xs text-muted-foreground">
        <span>live → desired</span>
        <span className="tabular-nums text-success">+{added}</span>
        <span className="tabular-nums text-destructive">−{removed}</span>
        {resource.kind === 'Secret' ? <span>Secret values are masked on both sides.</span> : null}
      </p>
      <DiffLines lines={lines} label={`Diff of ${resource.kind} ${resourceName(resource)}`} />
    </>
  )
}

function DiffError({ error, retry, retrying }: { error: unknown; retry: () => void; retrying: boolean }) {
  const described = describeError(error)
  return (
    <div role="alert" className="flex flex-wrap items-center gap-2 py-1 text-xs">
      <span className="text-destructive">{described.title}.</span>
      <span className="min-w-0 text-muted-foreground">{described.description}</span>
      <Button variant="outline" size="xs" onClick={retry} loading={retrying}>
        <RefreshCwIcon data-icon="inline-start" />
        Try again
      </Button>
    </div>
  )
}
