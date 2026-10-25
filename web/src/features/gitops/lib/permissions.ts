import type { Role } from '@/features/auth/api/types'
import { hasRole } from '@/features/auth/lib/roles'

import type { Application } from '../api/types'
import { isOperationRunning } from './labels'

/*
 * Role gates of the GitOps pages (CONTRACT.md §24.1). Each answers the
 * sentence shown next to a control that is off, or null when it is live.
 * The gateway refuses the calls anyway; these keep a person from pressing a
 * button that can only fail.
 */

/** `POST /applications/{id}/refresh` is a member's request. */
export function refreshGate(role: Role): string | null {
  return hasRole(role, 'member') ? null : 'Refreshing an application needs the member role.'
}

/** One operation per application at a time: the service refuses a second sync while one runs. */
function busy(application: Pick<Application, 'operation'>): string | null {
  return isOperationRunning(application.operation) ? 'An operation is running on this application. Wait for it to end.' : null
}

/** `POST /applications/{id}/sync` creates an execution that changes a cluster: approver. */
export function syncGate(role: Role, application: Pick<Application, 'operation'>): string | null {
  if (!hasRole(role, 'approver')) return 'Syncing needs the approver role.'
  return busy(application)
}

/** `POST /applications/{id}/rollback`: approver, like a sync. */
export function rollbackGate(role: Role, application: Pick<Application, 'operation'>): string | null {
  if (!hasRole(role, 'approver')) return 'Rolling back needs the approver role.'
  return busy(application)
}

/** Declaring, changing and deleting applications and clusters is an admin's. */
export function adminGate(role: Role, action: 'edit' | 'delete'): string | null {
  if (hasRole(role, 'admin')) return null
  return action === 'edit' ? 'Changing an application needs the admin role.' : 'Deleting an application needs the admin role.'
}
