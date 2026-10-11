import { useSuspenseQuery } from '@tanstack/react-query'
import { Outlet, useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'

import { projectQueries } from '../api/queries'
import { ProjectHeader, ProjectHeaderSkeleton } from '../components/project-header'
import { ProjectTabs, ProjectTabsSkeleton } from '../components/project-tabs'

/** Header and tab strip for one project. The child routes render below them. */
function ProjectBand({ projectId }: { projectId: string }) {
  const { data: project } = useSuspenseQuery(projectQueries.detail(projectId))

  return (
    <>
      <ProjectHeader project={project} />
      <ProjectTabs project={project} />
    </>
  )
}

function ProjectBandSkeleton() {
  return (
    <>
      <ProjectHeaderSkeleton />
      <ProjectTabsSkeleton />
    </>
  )
}

/**
 * `/projects/:projectId`. The band at the top is shared by every section;
 * each section is its own <Page> so it keeps the standard frame.
 */
export default function ProjectLayout() {
  const { projectId = '' } = useParams()

  return (
    <div className="flex flex-1 flex-col">
      <div className="bg-background">
        <div className="flex w-full min-w-0 flex-col gap-4 px-4 pt-5 md:px-6 md:pt-6">
          <QueryBoundary fallback={<ProjectBandSkeleton />}>
            <ProjectBand projectId={projectId} />
          </QueryBoundary>
        </div>
      </div>
      {/* Each section loads and fails on its own; the band above stays put. */}
      <QueryBoundary>
        <Outlet />
      </QueryBoundary>
    </div>
  )
}
