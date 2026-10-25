import type { components } from '@/lib/api/schema'

/*
 * GitOps DTOs come from the gateway's OpenAPI contract (CONTRACT.md §24).
 * The schema already narrows every status to a union; nothing is widened
 * or re-declared here. Optional strings the service leaves empty arrive as
 * `null`.
 */

type Schemas = components['schemas']

export type Application = Schemas['Application']
export type ApplicationSource = Schemas['ApplicationSource']
export type ApplicationSourceInput = Schemas['ApplicationSourceInput']
export type ApplicationOperation = Schemas['ApplicationOperation']
export type ResourceStatus = Schemas['ResourceStatus']
export type ResourceDiff = Schemas['ResourceDiff']
export type ApplicationDiff = Schemas['ApplicationDiff']
export type Manifests = Schemas['Manifests']
export type HistoryEntry = Schemas['HistoryEntry']
export type SyncPolicy = Schemas['SyncPolicy']
export type SyncPolicyInput = Schemas['SyncPolicyInput']
export type UpsertApplicationRequest = Schemas['UpsertApplicationRequest']
export type SyncApplicationRequest = Schemas['SyncApplicationRequest']
export type RollbackApplicationRequest = Schemas['RollbackApplicationRequest']
export type OperationStarted = Schemas['OperationStarted']
export type DeletedApplication = Schemas['DeletedApplication']
export type Cluster = Schemas['Cluster']
export type UpsertClusterRequest = Schemas['UpsertClusterRequest']

export type SyncStatus = Schemas['SyncStatus']
export type HealthStatus = Schemas['HealthStatus']
export type OperationPhase = Schemas['OperationPhase']
export type OperationKind = Schemas['OperationKind']
export type SourceType = Schemas['SourceType']
export type ClusterStatus = Schemas['ClusterStatus']
