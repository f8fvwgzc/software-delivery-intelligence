import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
import type { Role } from '@/features/auth/api/types'
import { ROLE_DESCRIPTION, ROLE_LABEL, rolesBelow } from '@/features/auth/lib/roles'

import { useUpdateMemberRole } from '../../api/mutations'
import type { Member } from '../../api/types'

/** The combobox cannot carry `null`; this value stands for "no override". */
const DERIVED = 'derived'

interface RoleOverrideSelectProps {
  workspaceId: string
  member: Member
}

/** Narrows one member below the role GitHub implies; never widens (CONTRACT.md §18). */
export function RoleOverrideSelect({ workspaceId, member }: RoleOverrideSelectProps) {
  const update = useUpdateMemberRole(workspaceId)

  const options: ComboboxOption[] = [
    { value: DERIVED, label: `Derived (${ROLE_LABEL[member.derivedRole]})`, description: 'No override; follow GitHub.' },
    ...rolesBelow(member.derivedRole)
      .reverse()
      .map((role) => ({ value: role, label: ROLE_LABEL[role], description: ROLE_DESCRIPTION[role] })),
  ]

  const change = (value: string) => {
    update.mutate({ userId: member.user.id, overrideRole: value === DERIVED ? null : (value as Role) })
  }

  return (
    <ComboboxField
      size="sm"
      className="w-44"
      aria-label={`Role override for ${member.user.login}`}
      value={member.overrideRole ?? DERIVED}
      onValueChange={change}
      options={options}
      loading={update.isPending}
      disabled={options.length === 1}
      searchPlaceholder="Search roles"
      emptyText="No role matches."
    />
  )
}
