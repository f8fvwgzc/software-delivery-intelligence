import { useSuspenseInfiniteQuery, useSuspenseQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import { FrameFooter } from '@/components/system/frame'
import { Button } from '@/components/ui/button'
import { count } from '@/lib/intl'

import { projectPickerQuery, proposalPagesQuery } from '../api/queries'
import { ProposalsTable } from './proposals-table'

interface ProposalsListProps {
  /** Scope to one project; the project column is dropped. */
  projectId?: string
  emptyAction?: ReactNode
}

/**
 * Loads the proposals (and, unscoped, the project names) and renders the
 * table. Lives under a QueryBoundary so the page header paints first.
 */
export function ProposalsList({ projectId, emptyAction }: ProposalsListProps) {
  return projectId ? (
    <ScopedList projectId={projectId} emptyAction={emptyAction} />
  ) : (
    <GlobalList emptyAction={emptyAction} />
  )
}

/** "Load earlier" under the table while the gateway has older pages. */
function LoadEarlier({ query, loaded }: { query: ReturnType<typeof useSuspenseInfiniteQuery<unknown[]>>; loaded: number }) {
  if (!query.hasNextPage) return null
  return (
    <FrameFooter className="justify-between">
      <span className="tabular-nums">{count(loaded, 'proposal', 'proposals')} loaded, more before</span>
      <Button variant="ghost" size="xs" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
        Load earlier
      </Button>
    </FrameFooter>
  )
}

function ScopedList({ projectId, emptyAction }: { projectId: string; emptyAction?: ReactNode }) {
  const query = useSuspenseInfiniteQuery(proposalPagesQuery(projectId))
  const proposals = query.data.pages.flat()
  return (
    <>
      <ProposalsTable proposals={proposals} emptyAction={emptyAction} />
      <LoadEarlier query={query} loaded={proposals.length} />
    </>
  )
}

function GlobalList({ emptyAction }: { emptyAction?: ReactNode }) {
  const query = useSuspenseInfiniteQuery(proposalPagesQuery())
  const { data: projects } = useSuspenseQuery(projectPickerQuery())
  const proposals = query.data.pages.flat()
  const names = new Map(projects.map((project) => [project.id, project.repoFullName]))
  return (
    <>
      <ProposalsTable proposals={proposals} projectNames={names} emptyAction={emptyAction} />
      <LoadEarlier query={query} loaded={proposals.length} />
    </>
  )
}
