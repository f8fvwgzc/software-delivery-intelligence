import { useRef, useState } from 'react'
import { useParams } from 'react-router'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { DeploymentRunsTable } from '../components/deployment-runs-table'
import { AddEnvironmentButton } from '../components/environment-editor/add-environment-button'
import { NewReleaseDialog } from '../components/new-release-dialog'
import { PipelineBoard, PipelineBoardSkeleton } from '../components/pipeline-board'
import { PipelineStats } from '../components/pipeline-stats'
import { ReleasesList } from '../components/releases-list'
import { SetupFiles } from '../components/setup-files'

/**
 * /projects/:projectId/pipeline — the CD board (CONTRACT.md §22). Three
 * requests on first paint: the pipeline (stats and stages share it), the
 * releases and the deployment runs. Registries and secrets are read when an
 * editor opens, the setup files when they are asked for.
 */
export default function PipelinePage() {
  const { projectId = '' } = useParams()
  const { currentWorkspace } = useAuth()
  const canAdmin = hasRole(currentWorkspace.myRole, 'admin')
  const canRelease = hasRole(currentWorkspace.myRole, 'member')
  const [setupShown, setSetupShown] = useState(false)
  const setupRef = useRef<HTMLElement>(null)

  const generateSetup = () => {
    setSetupShown(true)
    // Focus moves to the panel that now loads, which also brings it into view.
    setupRef.current?.focus()
  }

  return (
    <Page>
      <PageHeader
        title="Pipeline"
        description="One image, moved through the environments in order. Every promotion and rollback is an execution: permission, policy, approval, act, verify, checkpoint."
        actions={canAdmin ? <AddEnvironmentButton projectId={projectId} variant="outline" size="sm" /> : null}
      />

      <PipelineStats projectId={projectId} />

      <Panel
        title="Environments"
        description="In promotion order. A release moves left to right; a protected stage always waits for an approval."
        skeleton={<PipelineBoardSkeleton />}
      >
        <PipelineBoard projectId={projectId} onGenerateSetup={generateSetup} />
      </Panel>

      <Panel
        title="Releases"
        description="One immutable image per commit, newest first."
        padding="none"
        actions={canRelease ? <NewReleaseDialog projectId={projectId} variant="ghost" size="xs" /> : null}
      >
        <ReleasesList projectId={projectId} emptyAction={canRelease ? <NewReleaseDialog projectId={projectId} /> : null} />
      </Panel>

      <Panel title="Deployment history" description="Promotions and rollbacks, newest first. Running ones update live." padding="none">
        <DeploymentRunsTable projectId={projectId} />
      </Panel>

      <SetupFiles ref={setupRef} projectId={projectId} shown={setupShown} onShow={() => setSetupShown(true)} />
    </Page>
  )
}
