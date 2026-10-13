import { useQuery } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { ExecutionLink } from '@/components/shared/data/execution-link'
import { Button } from '@/components/ui/button'

import { useRefreshSecurity } from '../../api/mutations'
import { securityQueries } from '../../api/queries'
import type { SecurityOverview } from '../../api/types'
import { isFullyOn } from '../../lib/dependabot'
import { DependabotDialog } from '../dependabot-dialog'

/**
 * The execution that turns Dependabot on has ended: the repository's
 * settings are what changed, so GitHub is asked once more instead of
 * waiting out the server's five-minute cache. Not when the answer on screen
 * is itself a fresh read: then there is nothing newer to get.
 */
function useRefreshWhenConfigured(overview: SecurityOverview | undefined, refreshed: SecurityOverview | undefined, refresh: () => void) {
  const executionId = overview?.configureExecutionId
  const previous = useRef(executionId)
  const fresh = overview !== undefined && refreshed?.checkedAt === overview.checkedAt
  useEffect(() => {
    if (previous.current && !executionId && !fresh) refresh()
    previous.current = executionId
  }, [executionId, fresh, refresh])
}

/**
 * The page's header actions: "Refresh" asks GitHub again, and either the
 * way to the execution that is turning Dependabot on or the button that
 * starts one. They read the page's one query; until it answered there is
 * nothing to act on, and the panel below carries the loading and the error.
 */
export function SecurityActions({ projectId }: { projectId: string }) {
  const { data: overview } = useQuery(securityQueries.overview(projectId))
  const refresh = useRefreshSecurity(projectId)
  useRefreshWhenConfigured(overview, refresh.data, refresh.mutate)

  if (!overview) return null

  return (
    <>
      <Button variant="outline" size="sm" onClick={() => refresh.mutate()} loading={refresh.isPending}>
        <RefreshCwIcon data-icon="inline-start" />
        Refresh
      </Button>
      {overview.configureExecutionId ? (
        <span className="inline-flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          Dependabot is being turned on.
          <ExecutionLink executionId={overview.configureExecutionId}>Open the execution</ExecutionLink>
        </span>
      ) : isFullyOn(overview) ? null : (
        <DependabotDialog projectId={projectId} overview={overview} />
      )}
    </>
  )
}
