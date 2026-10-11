import { UsersIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { Member } from '../../api/types'
import { MemberRow } from './member-row'

export { MembersTableSkeleton } from './skeleton'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

interface MembersTableProps {
  workspaceId: string
  members: Member[]
  currentUserId: string
  canEdit: boolean
}

/** Dense member list: who, what GitHub says, what applies, and the one control an admin has. */
export function MembersTable({ workspaceId, members, currentUserId, canEdit }: MembersTableProps) {
  if (members.length === 0) {
    return (
      <EmptyState
        icon={UsersIcon}
        title="No members synced yet"
        description="Members arrive from GitHub organisation membership and teams. Sync to read them now."
      />
    )
  }

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Member</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden md:table-cell`}>GitHub</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden lg:table-cell`}>Teams</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden sm:table-cell`}>Derived</TableHead>
            <TableHead className={HEAD_CLASS}>Effective</TableHead>
            <TableHead className={HEAD_CLASS}>Override</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden text-right xl:table-cell`}>Synced</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {members.map((member) => (
            <MemberRow
              key={member.user.id}
              workspaceId={workspaceId}
              member={member}
              canEdit={canEdit}
              self={member.user.id === currentUserId}
            />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
