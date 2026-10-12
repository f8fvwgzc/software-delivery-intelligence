import { useSuspenseQuery } from '@tanstack/react-query'
import { ExternalLinkIcon } from 'lucide-react'

import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Panel } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { workspaceQueries } from '../api/queries'
import { DataActions } from '../components/data-actions'
import { DataControlsForm } from '../components/data-controls-form'
import { DataControlsSkeleton } from '../components/data-controls-form/skeleton'
import { StorageTable } from '../components/storage-table'

const TRUST_URL = 'https://github.com/f8fvwgzc/software-delivery-intelligence/blob/main/TRUST.md'

function ControlsPanel() {
  const { currentWorkspace } = useAuth()
  const { data } = useSuspenseQuery(workspaceQueries.dataControls(currentWorkspace.id))
  return (
    <DataControlsForm
      // A workspace switch remounts the form with that workspace's values.
      key={currentWorkspace.id}
      workspaceId={currentWorkspace.id}
      controls={data}
      readOnly={!hasRole(currentWorkspace.myRole, 'admin')}
    />
  )
}

export default function DataPrivacyPage() {
  const { currentWorkspace } = useAuth()

  return (
    <Page>
      <PageHeader
        title="Data & privacy"
        description="What SDI keeps about this workspace, the switches that change it, and the two guarantees: a complete export and a real delete."
        actions={
          <Button asChild variant="ghost" size="sm">
            <a href={TRUST_URL} target="_blank" rel="noreferrer">
              Read TRUST.md
              <ExternalLinkIcon data-icon="inline-end" />
            </a>
          </Button>
        }
      />

      <Frame
        title="What we store"
        description="CONTRACT.md §19, as enforced in code. Nothing is trained on; no telemetry leaves the deployment."
        padding="none"
      >
        <StorageTable />
      </Frame>

      <div className="grid gap-4 lg:grid-cols-12 lg:items-start">
        <Panel
          title="Controls"
          description="Per workspace. Defaults: per-commit cache for 24 hours, whole chunks, reasoner off, audit kept a year."
          className="lg:col-span-7"
          skeleton={<DataControlsSkeleton />}
        >
          <ControlsPanel />
        </Panel>
        <Frame title="Actions" description={`Applies to ${currentWorkspace.name} only.`} className="lg:col-span-5">
          <DataActions workspace={currentWorkspace} />
        </Frame>
      </div>
    </Page>
  )
}
