import type { components } from '@/lib/api/schema'

export type Member = components['schemas']['Member']
export type MembersResponse = components['schemas']['MembersResponse']
export type AuditEntry = components['schemas']['AuditEntry']
export type DataControls = components['schemas']['DataControls']
export type UpdateDataControlsRequest = components['schemas']['UpdateDataControlsRequest']
export type PurgeResult = components['schemas']['PurgeResult']
export type WorkspaceDeleted = components['schemas']['WorkspaceDeleted']
export type { Role, Workspace } from '@/features/auth/api/types'
