import type { StatusTone } from '@/components/shared/data'

import type { Role, WorkspaceKind } from '../api/types'

/** Lowest to highest; each role includes the ones before it (CONTRACT.md §18). */
export const ROLE_ORDER: readonly Role[] = ['viewer', 'member', 'approver', 'admin', 'owner']

export const ROLE_LABEL: Record<Role, string> = {
  unspecified: 'Unset',
  viewer: 'Viewer',
  member: 'Member',
  approver: 'Approver',
  admin: 'Admin',
  owner: 'Owner',
}

/** What the role may do, in the words of the contract. */
export const ROLE_DESCRIPTION: Record<Role, string> = {
  unspecified: 'No role resolved.',
  viewer: 'Read everything, change nothing.',
  member: 'Connect, scan, propose, negotiate, dry-run executions.',
  approver: 'Decide approvals, run docker and GitHub executions, grant permissions.',
  admin: 'Policies, settings, members and data controls.',
  owner: 'Everything, including deleting the workspace.',
}

export const ROLE_TONE: Record<Role, StatusTone> = {
  unspecified: 'neutral',
  viewer: 'neutral',
  member: 'info',
  approver: 'success',
  admin: 'brand',
  owner: 'warning',
}

export function roleRank(role: Role): number {
  return ROLE_ORDER.indexOf(role)
}

/** True when `role` is at least `required`. */
export function hasRole(role: Role, required: Role): boolean {
  return roleRank(role) >= roleRank(required)
}

/** Roles strictly below `role`: what an admin may narrow a member to. */
export function rolesBelow(role: Role): Role[] {
  return ROLE_ORDER.slice(0, Math.max(0, roleRank(role)))
}

export const KIND_LABEL: Record<WorkspaceKind, string> = {
  unspecified: 'Workspace',
  personal: 'Personal',
  organization: 'Organisation',
}

/** Two-letter avatar fallback. */
export function initials(name: string) {
  return name.slice(0, 2).toUpperCase()
}
