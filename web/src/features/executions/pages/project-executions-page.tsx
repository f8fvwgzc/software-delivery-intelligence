import { useParams } from 'react-router'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { ExecutionsTable, ExecutionsTableSkeleton } from '../components/executions-table'
import { NewDryRunDialog } from '../components/new-dry-run-dialog'

/** Executions of one project, rendered inside the project layout. */
export default function ProjectExecutionsPage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Executions"
        description="Runs of accepted plans for this project. Live rows refresh on their own."
        actions={<NewDryRunDialog projectId={projectId} variant="outline" size="sm" />}
      />
      <Panel title="Executions" padding="none" skeleton={<ExecutionsTableSkeleton showProject={false} />}>
        <ExecutionsTable
          projectId={projectId}
          emptyAction={<NewDryRunDialog projectId={projectId} variant="outline" size="sm" />}
        />
      </Panel>
    </Page>
  )
}
