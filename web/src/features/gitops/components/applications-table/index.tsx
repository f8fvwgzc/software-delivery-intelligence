import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { ShipWheelIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useAuth } from '@/features/auth/lib/auth-context'
import { count } from '@/lib/intl'

import { gitopsQueries } from '../../api/queries'
import { sourceRepoUrl } from '../../lib/repo'
import { ApplicationRow } from './application-row'
import { AUTOMATION_CLASS, DESTINATION_CLASS, HEAD_CLASS } from './columns'

interface ApplicationsTableProps {
  projectId: string
  /** The "New application" trigger, for the empty state. */
  emptyAction?: ReactNode
}

/** The project's applications by name, with how each compares with its cluster. */
export function ApplicationsTable({ projectId, emptyAction }: ApplicationsTableProps) {
  const { data } = useSuspenseQuery(gitopsQueries.applications(projectId))
  // Already loaded by the project layout: only the repository URL for the commit links.
  const { data: project } = useQuery(gitopsQueries.project(projectId))
  const { currentWorkspace } = useAuth()
  const applications = data.toSorted((a, b) => a.name.localeCompare(b.name))

  if (applications.length === 0) {
    return (
      <PanelEmpty
        icon={ShipWheelIcon}
        title="No applications yet"
        description="An application is a path in a repository, rendered as Kubernetes manifests and kept in step with one namespace of a cluster. Declare one, then sync it through an execution."
        action={emptyAction}
      />
    )
  }

  const outOfSync = applications.filter((application) => application.syncStatus === 'out_of_sync').length

  return (
    <>
      <Table aria-label="Applications">
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Application</TableHead>
            <TableHead className={HEAD_CLASS}>Sync</TableHead>
            <TableHead className={HEAD_CLASS}>Health</TableHead>
            <TableHead className={`${HEAD_CLASS} ${DESTINATION_CLASS}`}>Destination</TableHead>
            <TableHead className={HEAD_CLASS}>Revision</TableHead>
            <TableHead className={`${HEAD_CLASS} ${AUTOMATION_CLASS}`}>Automation</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {applications.map((application) => (
            <ApplicationRow
              key={application.id}
              application={application}
              repoUrl={sourceRepoUrl(application.source, project, currentWorkspace.githubHost)}
            />
          ))}
        </TableBody>
      </Table>
      <FrameFooter>
        <span className="tabular-nums">{count(applications.length, 'application', 'applications')}</span>
        {outOfSync > 0 ? <span className="tabular-nums">{outOfSync} out of sync</span> : null}
        <span className="ml-auto">Compared with the cluster every few minutes; changes appear live.</span>
      </FrameFooter>
    </>
  )
}
