import { RelativeTime, StatusBadge } from '@/components/shared/data'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { TableCell, TableRow } from '@/components/ui/table'
import { initials, ROLE_LABEL, ROLE_TONE } from '@/features/auth/lib/roles'

import type { Member } from '../../api/types'
import { githubRoleLabel } from '../../lib/labels'
import { RoleOverrideSelect } from './role-override-select'

interface MemberRowProps {
  workspaceId: string
  member: Member
  /** The viewer is an admin: the override cell is a control. */
  canEdit: boolean
  /** This row is the signed-in user. */
  self: boolean
}

export function MemberRow({ workspaceId, member, canEdit, self }: MemberRowProps) {
  const { user } = member

  return (
    <TableRow className="focus-within:bg-muted/50">
      <TableCell className="px-3 py-2">
        <div className="flex min-w-0 max-w-64 items-center gap-2.5">
          <Avatar size="sm" className="size-6">
            <AvatarImage src={user.avatarUrl} alt="" />
            <AvatarFallback className="text-2xs">{initials(user.login)}</AvatarFallback>
          </Avatar>
          <div className="flex min-w-0 flex-col leading-tight">
            <span className="truncate font-mono text-xs">
              {user.login}
              {self ? <span className="ml-1.5 font-sans text-2xs text-muted-foreground">you</span> : null}
            </span>
            {user.name ? <span className="truncate text-2xs text-muted-foreground">{user.name}</span> : null}
          </div>
        </div>
      </TableCell>
      <TableCell className="hidden px-3 py-2 text-xs text-muted-foreground md:table-cell">
        {githubRoleLabel(member.githubRole)}
      </TableCell>
      <TableCell className="hidden px-3 py-2 lg:table-cell">
        {member.teams.length === 0 ? (
          <span className="text-xs text-muted-foreground">—</span>
        ) : (
          <div className="flex flex-wrap gap-1">
            {member.teams.map((team) => (
              <Badge key={team} variant="outline" className="font-mono text-2xs font-normal">
                {team}
              </Badge>
            ))}
          </div>
        )}
      </TableCell>
      <TableCell className="hidden px-3 py-2 text-xs text-muted-foreground sm:table-cell">
        {ROLE_LABEL[member.derivedRole]}
      </TableCell>
      <TableCell className="px-3 py-2">
        <StatusBadge tone={ROLE_TONE[member.effectiveRole]} dot={false}>
          {ROLE_LABEL[member.effectiveRole]}
        </StatusBadge>
      </TableCell>
      <TableCell className="px-3 py-2">
        {canEdit ? (
          <RoleOverrideSelect workspaceId={workspaceId} member={member} />
        ) : (
          <span className="text-xs text-muted-foreground">
            {member.overrideRole ? `Narrowed to ${ROLE_LABEL[member.overrideRole].toLowerCase()}` : '—'}
          </span>
        )}
      </TableCell>
      <TableCell className="hidden px-3 py-2 text-right text-xs text-muted-foreground xl:table-cell">
        <RelativeTime value={member.syncedAt} />
      </TableCell>
    </TableRow>
  )
}
