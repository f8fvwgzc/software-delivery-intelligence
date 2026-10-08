import { useSuspenseQuery } from '@tanstack/react-query'
import { PackageIcon } from 'lucide-react'
import { useParams } from 'react-router'

import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'

import { dependencyQueries } from '../api/queries'
import { DependabotStatus } from '../components/dependabot-status'
import { DependencyExplorer, DependencyExplorerSkeleton } from '../components/dependency-explorer'

/** The one data region of the page; suspends inside its own boundary. */
function DependenciesPanel({ projectId }: { projectId: string }) {
  const { data } = useSuspenseQuery(dependencyQueries.list(projectId))

  if (data.dependencies.length === 0) {
    return (
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <EmptyState
          icon={PackageIcon}
          title="No dependencies recorded"
          description="Either the repository declares none in a manifest the scan recognises, or it has not been scanned yet."
        />
        <DependabotStatus status={data.dependabot} />
      </div>
    )
  }

  return <DependencyExplorer data={data} />
}

export default function DependenciesPage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Dependencies"
        description="Every declared package, where it was read from, and whether updates are automated."
      />
      <QueryBoundary fallback={<DependencyExplorerSkeleton />}>
        <DependenciesPanel projectId={projectId} />
      </QueryBoundary>
    </Page>
  )
}
