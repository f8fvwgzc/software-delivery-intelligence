import { useSuspenseQuery } from '@tanstack/react-query'
import { Outlet, useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'

import { projectQueries } from '../api/queries'
import { ProjectHeader, ProjectHeaderSkeleton } from '../components/project-header'
import { ProjectTabs, ProjectTabsSkeleton } from '../components/project-tabs'

/**
 * Set on the layout root to the band's rendered height, so sticky things in
 * the sections (the code page's tree pane and file header) can sit under it.
 */
const BAND_HEIGHT_VAR = '--project-band'

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
 * Ref callback for the band: keeps `--project-band` on the layout root (its
 * parent) equal to the band's height, which changes when the tldr wraps or
 * the skeleton gives way to the real header.
 */
function observeBandHeight(band: HTMLDivElement | null) {
  if (!band) return
  const layout = band.parentElement
  const observer = new ResizeObserver(() => {
    layout?.style.setProperty(BAND_HEIGHT_VAR, `${band.getBoundingClientRect().height}px`)
  })
  observer.observe(band)
  return () => {
    observer.disconnect()
    layout?.style.removeProperty(BAND_HEIGHT_VAR)
  }
}

/**
 * `/projects/:projectId`. The band at the top is shared by every section and
 * stays on screen under the app header while the section scrolls; each
 * section is its own <Page> so it keeps the standard frame.
 */
export default function ProjectLayout() {
  const { projectId = '' } = useParams()

  return (
    <div className="flex flex-1 flex-col">
      <div ref={observeBandHeight} className="sticky top-(--header-h) z-10 bg-background">
        <div className="flex w-full min-w-0 flex-col gap-3 px-4 pt-4 md:px-6 md:pt-5">
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
