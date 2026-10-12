import { useQueries, useSuspenseQuery } from '@tanstack/react-query'
import { CirclePlayIcon, RocketIcon, WorkflowIcon } from 'lucide-react'
import { useParams } from 'react-router'

import { RelativeTime } from '@/components/shared/data'
import { Page, PageHeader } from '@/components/shared/page'
import { FrameFooter } from '@/components/system/frame'
import { Panel, PanelEmpty } from '@/components/system/panel'

import { DEPLOYMENTS_LIMIT, RUNS_LIMIT, deliveryQueries } from '../api/queries'
import { DeploymentStatsStrip, DeploymentsSkeleton, DeploymentsTable } from '../components/deployments-table'
import { PipelinesList, PipelinesSkeleton } from '../components/pipelines-list'
import { RefreshDeploymentsButton } from '../components/refresh-deployments-button'
import { RefreshRunsButton } from '../components/refresh-runs-button'
import { RunStats } from '../components/run-stats'
import { RunsSkeleton, RunsTable } from '../components/runs-table'

interface RegionProps {
  projectId: string
}

function PipelinesRegion({ projectId }: RegionProps) {
  const { data: pipelines } = useSuspenseQuery(deliveryQueries.pipelines(projectId))

  if (pipelines.length === 0) {
    return (
      <PanelEmpty
        icon={WorkflowIcon}
        title="No pipelines found"
        description="The scan looks for workflow definitions such as .github/workflows in the default branch. None were found."
      />
    )
  }

  return <PipelinesList pipelines={pipelines} />
}

function RunsRegion({ projectId }: RegionProps) {
  const { data } = useSuspenseQuery(deliveryQueries.runs(projectId))

  if (data.runs.length === 0) {
    return (
      <PanelEmpty
        icon={CirclePlayIcon}
        title="No runs yet"
        description="Nothing has run at the provider for this repository, or the runs have not been synced yet."
        action={<RefreshRunsButton projectId={projectId} />}
      />
    )
  }

  return (
    <>
      <div className="border-b p-3">
        <RunStats stats={data.stats} />
      </div>
      <RunsTable runs={data.runs} />
      {data.syncedAt ? (
        <FrameFooter>
          Synced <RelativeTime value={data.syncedAt} className="font-mono" />
        </FrameFooter>
      ) : null}
    </>
  )
}

function DeploymentsRegion({ projectId }: RegionProps) {
  const { data } = useSuspenseQuery(deliveryQueries.deployments(projectId))

  if (data.deployments.length === 0) {
    return (
      <PanelEmpty
        icon={RocketIcon}
        title="No deployments observed"
        description="GitHub deployments and deploy workflow runs show up here once the provider reports them."
        action={<RefreshDeploymentsButton projectId={projectId} />}
      />
    )
  }

  return (
    <>
      <div className="border-b p-3">
        <DeploymentStatsStrip stats={data.stats} />
      </div>
      <DeploymentsTable deployments={data.deployments} />
    </>
  )
}

export default function DeliveryPage() {
  const { projectId = '' } = useParams()

  // Start all three fetches at once. Each panel below subscribes with
  // Suspense inside its own boundary and dedupes onto these requests, so a
  // slow one never blocks the others.
  useQueries({
    queries: [deliveryQueries.pipelines(projectId), deliveryQueries.runs(projectId), deliveryQueries.deployments(projectId)],
  })

  return (
    <Page>
      <PageHeader title="Delivery" description="Pipelines defined in the repository, what they ran, and what was deployed." />

      <Panel title="Pipelines" description="Workflow definitions found in the default branch." skeleton={<PipelinesSkeleton />}>
        <PipelinesRegion projectId={projectId} />
      </Panel>

      <Panel
        title="Pipeline runs"
        description={`The last ${RUNS_LIMIT} runs at the provider, newest first.`}
        padding="none"
        actions={<RefreshRunsButton projectId={projectId} />}
        skeleton={<RunsSkeleton />}
      >
        <RunsRegion projectId={projectId} />
      </Panel>

      <Panel
        title="Deployments"
        description={`The last ${DEPLOYMENTS_LIMIT} deployments observed, newest first.`}
        padding="none"
        actions={<RefreshDeploymentsButton projectId={projectId} />}
        skeleton={<DeploymentsSkeleton />}
      >
        <DeploymentsRegion projectId={projectId} />
      </Panel>
    </Page>
  )
}
