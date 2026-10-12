/*
 * Degraded-gateway scenarios: a 502 from the edge, a failed fetch (offline
 * or gateway down), and a slow answer. Each panel is a real QueryBoundary
 * over a query whose first attempt fails and whose retry succeeds, so the
 * error copy, the retry button and the recovery path are all exercised.
 */
import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { useEffect, useId, useRef } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Frame } from '@/components/system/frame'
import { PanelSkeleton } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'
import { ApiError } from '@/lib/api/errors'

import { Scenario, type Done } from '../scenario'


const SLOW_MS = 2_000

/**
 * Attempts per query, kept outside the component: the error boundary
 * unmounts the panel on failure, so a ref would start over on every retry.
 */
const attempts = new Map<string, number>()

/** Fails the first `failures` attempts the way `fail` says, then answers. */
function useFlakyQuery(key: string, fail: () => Error, failures = 1) {
  return useSuspenseQuery({
    queryKey: ['stress', key],
    queryFn: async () => {
      const attempt = (attempts.get(key) ?? 0) + 1
      attempts.set(key, attempt)
      if (attempt <= failures) throw fail()
      return { attempts: attempt, at: new Date().toISOString() }
    },
    retry: false,
    staleTime: Infinity,
    gcTime: 0,
  })
}

function Recovered({ attempts }: { attempts: number }) {
  return (
    <p className="flex items-center gap-2 px-3 py-4 text-xs">
      <Tag tone="success">recovered</Tag>
      <span className="text-muted-foreground">answered on attempt {attempts}</span>
    </p>
  )
}

function GatewayDown({ run }: { run: string }) {
  const { data } = useFlakyQuery(`502-${run}`, () => new ApiError(502, { code: 'upstream_unavailable', message: 'No gateway replica answered' }))
  return <Recovered attempts={data.attempts} />
}

function FetchFailed({ run }: { run: string }) {
  const { data } = useFlakyQuery(`offline-${run}`, () => new TypeError('Failed to fetch'))
  return <Recovered attempts={data.attempts} />
}

function SlowAnswer() {
  const query = useQuery({
    queryKey: ['stress', 'slow'],
    queryFn: () => new Promise<{ at: string }>((resolve) => setTimeout(() => resolve({ at: new Date().toISOString() }), SLOW_MS)),
    staleTime: Infinity,
    gcTime: 0,
  })
  if (query.isPending) return <PanelSkeleton rows={2} />
  if (query.isError) return <p className="px-3 py-4 text-xs text-destructive">{query.error.message}</p>
  return (
    <p className="flex items-center gap-2 px-3 py-4 text-xs">
      <Tag tone="success">loaded</Tag>
      <span className="text-muted-foreground">after {SLOW_MS} ms behind a skeleton</span>
    </p>
  )
}

function ErrorsBody({ done }: { done: Done }) {
  // One counter set per mount of the scenario ("Run again" starts over); stable across the boundary's retries.
  const run = useId()
  const reported = useRef(false)
  useEffect(() => {
    if (reported.current) return
    reported.current = true
    done({ panels: 3, 'slow answer ms': SLOW_MS }, { 'each panel has its own boundary': true })
  }, [done])

  return (
    <div className="grid gap-3 lg:grid-cols-3">
      <Frame as="h3" title="502 from the edge" description="First attempt answers 502; retry recovers." padding="none" data-error-panel="502">
        <QueryBoundary compact fallback={<PanelSkeleton rows={2} />}>
          <GatewayDown run={run} />
        </QueryBoundary>
      </Frame>
      <Frame as="h3" title="Failed fetch" description="First attempt throws like an offline browser; retry recovers." padding="none" data-error-panel="offline">
        <QueryBoundary compact fallback={<PanelSkeleton rows={2} />}>
          <FetchFailed run={run} />
        </QueryBoundary>
      </Frame>
      <Frame as="h3" title="Slow network" description={`Answers after ${SLOW_MS / 1000} s; a skeleton holds the space.`} padding="none" data-error-panel="slow">
        <SlowAnswer />
      </Frame>
    </div>
  )
}

export function ErrorsScenario() {
  return (
    <Scenario id="errors" title="Gateway down, offline, slow" description="Each panel fails or stalls on its own and offers its own way out; the page never goes blank.">
      {(done) => <ErrorsBody done={done} />}
    </Scenario>
  )
}
