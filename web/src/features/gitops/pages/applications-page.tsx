import { useParams } from 'react-router'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { ApplicationsTable } from '../components/applications-table'
import { ApplicationsSkeleton } from '../components/applications-table/skeleton'
import { NewApplicationButton } from '../components/new-application-button'

/**
 * /projects/:projectId/applications — the project's GitOps applications
 * (CONTRACT.md §24). One request on first paint: the list. The clusters are
 * read when the editor opens.
 */
export default function ApplicationsPage() {
  const { projectId = '' } = useParams()
  const { currentWorkspace } = useAuth()
  const canAdmin = hasRole(currentWorkspace.myRole, 'admin')

  return (
    <Page>
      <PageHeader
        title="Applications"
        description="What a cluster should run, rendered from a path in a repository and compared with what it runs. A sync is an execution: preview, approval, sync, health check."
        actions={canAdmin ? <NewApplicationButton projectId={projectId} /> : null}
      />
      <Panel
        title="Applications"
        description="Sync says whether the cluster matches the repository; health is the worst of the resources’."
        padding="none"
        skeleton={<ApplicationsSkeleton />}
      >
        <ApplicationsTable key={projectId} projectId={projectId} emptyAction={canAdmin ? <NewApplicationButton projectId={projectId} variant="outline" /> : null} />
      </Panel>
    </Page>
  )
}
