import { useQuery, useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { PackageIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'
import { Button } from '@/components/ui/button'
import { count } from '@/lib/intl'

import { cdQueries } from '../../api/queries'
import { ReleaseRow } from './release-row'

interface ReleasesListProps {
  projectId: string
  /** The "New release" trigger, for the empty state. */
  emptyAction?: ReactNode
}

/** Releases newest first, one page at a time with "Load earlier" at the foot. */
export function ReleasesList({ projectId, emptyAction }: ReleasesListProps) {
  const query = useSuspenseInfiniteQuery(cdQueries.releases(projectId))
  // Already loaded by the project layout: only the repository URL for the commit links.
  const { data: project } = useQuery(cdQueries.project(projectId))
  const releases = query.data.pages.flat()

  if (releases.length === 0) {
    return (
      <PanelEmpty
        icon={PackageIcon}
        title="No releases yet"
        description="A release is one image at one commit. It is created when the build workflow succeeds on the default branch, when the registry reports a new package, or by hand."
        action={emptyAction}
      />
    )
  }

  return (
    <>
      <RowList stagger aria-label="Releases">
        {releases.map((release) => (
          <li key={release.id}>
            <ReleaseRow release={release} repoUrl={project?.repoUrl} />
          </li>
        ))}
      </RowList>
      {query.hasNextPage ? (
        <FrameFooter className="justify-between">
          <span className="tabular-nums">{count(releases.length, 'release', 'releases')} loaded, more before</span>
          <Button variant="ghost" size="xs" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
            Load earlier
          </Button>
        </FrameFooter>
      ) : null}
    </>
  )
}
