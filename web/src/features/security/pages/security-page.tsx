import { useParams } from 'react-router'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { SecurityActions } from '../components/security-actions'
import { SecurityStatus } from '../components/security-status'
import { VulnerabilitiesTable } from '../components/vulnerabilities-table'
import { VulnerabilitiesSkeleton } from '../components/vulnerabilities-table/skeleton'

/**
 * /projects/:projectId/security — the project's vulnerable dependencies and
 * the state of Dependabot on its repository (CONTRACT.md §23.4). One request
 * on first paint: the header's actions, the status strip and the table all
 * read the same overview.
 */
export default function SecurityPage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Security"
        description="The dependencies with a known vulnerability, the version that fixes each one, and whether Dependabot watches this repository."
        // Keyed by project: what the actions remember (a setup in progress) belongs to one repository.
        actions={<SecurityActions key={projectId} projectId={projectId} />}
      />

      <SecurityStatus projectId={projectId} />

      <Panel title="Vulnerabilities" description="Open now, most severe first." padding="none" skeleton={<VulnerabilitiesSkeleton />}>
        <VulnerabilitiesTable key={projectId} projectId={projectId} />
      </Panel>
    </Page>
  )
}
