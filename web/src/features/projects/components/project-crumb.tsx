import { useSuspenseQuery } from '@tanstack/react-query'
import { ErrorBoundary } from 'react-error-boundary'
import { useParams } from 'react-router'

import { projectQueries } from '../api/queries'

function ProjectName({ projectId }: { projectId: string }) {
  const { data: project } = useSuspenseQuery(projectQueries.detail(projectId))
  return <>{project.name}</>
}

/**
 * Breadcrumb for `/projects/:projectId`. Shares the detail query with the
 * layout, so it never costs an extra request. If the project cannot be
 * loaded the page shows the error; the crumb just falls back to a word.
 */
export default function ProjectCrumb() {
  const { projectId } = useParams()
  if (!projectId) return <>Project</>

  return (
    <ErrorBoundary fallback={<>Project</>} resetKeys={[projectId]}>
      <ProjectName projectId={projectId} />
    </ErrorBoundary>
  )
}
