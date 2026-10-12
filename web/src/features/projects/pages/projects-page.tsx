import { useSuspenseQuery } from '@tanstack/react-query'
import { FolderGitIcon, PlusIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Page, PageHeader } from '@/components/shared/page'
import { Panel, PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'

import { projectQueries } from '../api/queries'
import { ProjectsTable, ProjectsTableSkeleton } from '../components/projects-table'

/** The one data region of the page; suspends inside its own boundary. */
function ProjectsPanel() {
  const { data: projects } = useSuspenseQuery(projectQueries.list())

  if (projects.length === 0) {
    return (
      <PanelEmpty
        icon={FolderGitIcon}
        title="No repositories connected"
        description="Connect one of your GitHub repositories and the platform scans it, reads the facts and asks only what it cannot know."
        action={
          <Button asChild variant="outline" size="sm">
            <Link to={routes.connect()}>
              <PlusIcon data-icon="inline-start" />
              Connect repository
            </Link>
          </Button>
        }
      />
    )
  }

  return <ProjectsTable projects={projects} />
}

export default function ProjectsPage() {
  return (
    <Page>
      <PageHeader
        title="Projects"
        description="Connected repositories, what the platform knows about each, and whether it may act."
        actions={
          <Button asChild>
            <Link to={routes.connect()}>
              <PlusIcon data-icon="inline-start" />
              Connect repository
            </Link>
          </Button>
        }
      />
      <Panel title="Connected repositories" padding="none" skeleton={<ProjectsTableSkeleton />}>
        <ProjectsPanel />
      </Panel>
    </Page>
  )
}
