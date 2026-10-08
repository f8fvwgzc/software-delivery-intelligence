import { LightbulbIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { ProposalRow } from './proposal-row'

type Proposal = components['schemas']['Proposal']

interface ProposalsTableProps {
  proposals: Proposal[]
  /** Project names by id; omitted when the list is scoped to one project. */
  projectNames?: Map<string, string>
  /** Rendered inside the empty state, usually the "New proposal" button. */
  emptyAction?: ReactNode
}

/** Dense list of proposals, best-ranked option inline. */
export function ProposalsTable({ proposals, projectNames, emptyAction }: ProposalsTableProps) {
  if (proposals.length === 0) {
    return (
      <EmptyState
        icon={LightbulbIcon}
        title="No proposals yet"
        description="Generate one to see ranked, evidence-backed options for a project."
        action={emptyAction}
      />
    )
  }

  const showProject = projectNames !== undefined

  return (
    <div className="overflow-x-auto rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-28 px-3 text-xs font-medium text-muted-foreground">Kind</TableHead>
            <TableHead className="px-3 text-xs font-medium text-muted-foreground">Proposal</TableHead>
            <TableHead className="w-32 px-3 text-xs font-medium text-muted-foreground">Status</TableHead>
            <TableHead className="w-72 px-3 text-xs font-medium text-muted-foreground">Recommended</TableHead>
            <TableHead className="w-16 px-3 text-right text-xs font-medium text-muted-foreground">Rev</TableHead>
            <TableHead className="w-28 px-3 text-xs font-medium text-muted-foreground">Updated</TableHead>
            {showProject ? (
              <TableHead className="w-40 px-3 text-xs font-medium text-muted-foreground">Project</TableHead>
            ) : null}
          </TableRow>
        </TableHeader>
        <TableBody>
          {proposals.map((proposal) => (
            <ProposalRow
              key={proposal.id}
              proposal={proposal}
              projectName={showProject ? (projectNames.get(proposal.projectId) ?? proposal.projectId) : undefined}
            />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
