import { useSuspenseQueries, useSuspenseQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import { projectPickerQuery, proposalsQuery } from '../api/queries'
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

function ScopedList({ projectId, emptyAction }: { projectId: string; emptyAction?: ReactNode }) {
  const { data } = useSuspenseQuery(proposalsQuery(projectId))
  return <ProposalsTable proposals={data} emptyAction={emptyAction} />
}

function GlobalList({ emptyAction }: { emptyAction?: ReactNode }) {
  const [proposals, projects] = useSuspenseQueries({
    queries: [proposalsQuery(), projectPickerQuery()],
  })
  const names = new Map(projects.data.map((project) => [project.id, project.repoFullName]))
  return <ProposalsTable proposals={proposals.data} projectNames={names} emptyAction={emptyAction} />
}
