import { useSuspenseQuery } from '@tanstack/react-query'
import { ErrorBoundary } from 'react-error-boundary'
import { Link, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { BreadcrumbSeparator } from '@/components/ui/breadcrumb'
import { projectQueries } from '@/features/projects/api/queries'

import { diagramQuery } from '../api/queries'

function ProjectName({ projectId }: { projectId: string }) {
  const { data: project } = useSuspenseQuery(projectQueries.detail(projectId))
  return <>{project.name}</>
}

/**
 * Breadcrumb for `/diagrams/:diagramId`: the project (linking to its
 * diagrams), then the diagram's title. Shares the detail query with the page.
 */
export default function DiagramCrumb() {
  const { diagramId = '' } = useParams()
  const { data: diagram } = useSuspenseQuery(diagramQuery(diagramId))

  return (
    <span className="inline-flex max-w-96 items-center gap-1.5">
      <Link
        to={routes.projectDiagrams(diagram.projectId)}
        className="truncate font-normal text-muted-foreground transition-colors hover:text-foreground"
      >
        <ErrorBoundary fallback={<>Project</>} resetKeys={[diagram.projectId]}>
          <ProjectName projectId={diagram.projectId} />
        </ErrorBoundary>
      </Link>
      <BreadcrumbSeparator />
      <span className="truncate">{diagram.title}</span>
    </span>
  )
}
