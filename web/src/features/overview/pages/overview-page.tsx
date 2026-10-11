import { usePrefetchQuery, useSuspenseQueries, useSuspenseQuery } from '@tanstack/react-query'
import { PlugIcon } from 'lucide-react'
import { useMemo } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Button } from '@/components/ui/button'
import { systemQueries } from '@/features/system/api/queries'

import { overviewQueries } from '../api/queries'
import type { Project } from '../api/types'
import { CompactListSkeleton } from '../components/compact-list'
import { ConnectCta } from '../components/connect-cta'
import { MetricStrip, MetricStripSkeleton } from '../components/metric-strip'
import { RecentExecutions } from '../components/recent-executions'
import { RecentProposals } from '../components/recent-proposals'
import { StatusStrip, StatusStripSkeleton } from '../components/status-strip'
import { WorkQueue, WorkQueueSkeleton } from '../components/work-queue'
import { computeOverviewMetrics } from '../lib/metrics'
import { buildWorkQueue } from '../lib/work-queue'

/*
 * Panels: each owns the queries it reads and sits in its own boundary, so a
 * slow or failing list only takes its own region down. The page prefetches
 * every list before suspending on projects, so nothing waterfalls.
 */

function MetricsPanel({ projects }: { projects: Project[] }) {
  const [{ data: approvals }, { data: executions }, { data: proposals }] = useSuspenseQueries({
    queries: [overviewQueries.pendingApprovals(), overviewQueries.executions(), overviewQueries.proposals()],
  })
  const metrics = useMemo(
    () => computeOverviewMetrics({ projects, approvals, executions, proposals }),
    [projects, approvals, executions, proposals],
  )
  return <MetricStrip metrics={metrics} />
}

function StatusPanel() {
  const { data } = useSuspenseQuery(systemQueries.status())
  return <StatusStrip status={data} />
}

function WorkQueuePanel({ projects }: { projects: Project[] }) {
  const [{ data: approvals }, { data: executions }] = useSuspenseQueries({
    queries: [overviewQueries.pendingApprovals(), overviewQueries.executions()],
  })
  const rows = useMemo(() => buildWorkQueue({ projects, approvals, executions }), [projects, approvals, executions])
  return <WorkQueue rows={rows} />
}

function RecentProposalsPanel({ projects }: { projects: Project[] }) {
  const { data } = useSuspenseQuery(overviewQueries.recentProposals())
  const projectNames = useMemo(() => new Map(projects.map((project) => [project.id, project.name])), [projects])
  return <RecentProposals proposals={data} projectNames={projectNames} />
}

function RecentExecutionsPanel() {
  const { data } = useSuspenseQuery(overviewQueries.recentExecutions())
  return <RecentExecutions executions={data} />
}

const DESCRIPTION = 'What the platform knows, what it recommends, and what it is waiting on you for.'

export default function OverviewPage() {
  usePrefetchQuery(overviewQueries.pendingApprovals())
  usePrefetchQuery(overviewQueries.executions())
  usePrefetchQuery(overviewQueries.proposals())
  usePrefetchQuery(systemQueries.status())
  const { data: projects } = useSuspenseQuery(overviewQueries.projects())

  if (projects.length === 0) {
    return (
      <Page>
        <PageHeader title="Overview" description={DESCRIPTION} />
        <ConnectCta />
      </Page>
    )
  }

  return (
    <Page>
      <PageHeader
        title="Overview"
        description={DESCRIPTION}
        actions={
          <Button asChild variant="outline" size="sm">
            <Link to={routes.connect()}>
              <PlugIcon data-icon="inline-start" />
              Connect repository
            </Link>
          </Button>
        }
      />

      <div className="grid gap-3 xl:grid-cols-[minmax(0,3fr)_minmax(0,1fr)]">
        <QueryBoundary fallback={<MetricStripSkeleton />}>
          <MetricsPanel projects={projects} />
        </QueryBoundary>
        <QueryBoundary fallback={<StatusStripSkeleton />}>
          <StatusPanel />
        </QueryBoundary>
      </div>

      <QueryBoundary fallback={<WorkQueueSkeleton />}>
        <WorkQueuePanel projects={projects} />
      </QueryBoundary>

      <div className="grid gap-6 lg:grid-cols-2">
        <QueryBoundary fallback={<CompactListSkeleton title="Recent proposals" />}>
          <RecentProposalsPanel projects={projects} />
        </QueryBoundary>
        <QueryBoundary fallback={<CompactListSkeleton title="Recent executions" />}>
          <RecentExecutionsPanel />
        </QueryBoundary>
      </div>
    </Page>
  )
}
