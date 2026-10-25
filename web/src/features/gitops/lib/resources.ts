import type { ResourceDiff, ResourceStatus } from '../api/types'

/** `apps/Deployment:shop/web`: the same identity the service tracks (`sdi.dev/tracking-id`). */
export function resourceKey(resource: Pick<ResourceStatus, 'group' | 'kind' | 'namespace' | 'name'>): string {
  return `${resource.group}/${resource.kind}:${resource.namespace ?? ''}/${resource.name}`
}

/** `shop/web`, or `web` for a cluster-scoped resource. */
export function resourceName(resource: Pick<ResourceStatus, 'namespace' | 'name'>): string {
  return resource.namespace ? `${resource.namespace}/${resource.name}` : resource.name
}

/** `apps/v1`, or `v1` for the core group. */
export function apiVersion(resource: Pick<ResourceStatus, 'group' | 'version'>): string {
  return resource.group ? `${resource.group}/${resource.version}` : resource.version
}

/**
 * Kinds in the order a sync applies them (the engine's kind order, after
 * Helm's): what others depend on first. Kinds not listed follow, by name.
 */
const KIND_ORDER = [
  'Namespace',
  'NetworkPolicy',
  'ResourceQuota',
  'LimitRange',
  'PodSecurityPolicy',
  'PodDisruptionBudget',
  'ServiceAccount',
  'Secret',
  'SecretList',
  'ConfigMap',
  'StorageClass',
  'PersistentVolume',
  'PersistentVolumeClaim',
  'CustomResourceDefinition',
  'ClusterRole',
  'ClusterRoleList',
  'ClusterRoleBinding',
  'ClusterRoleBindingList',
  'Role',
  'RoleList',
  'RoleBinding',
  'RoleBindingList',
  'Service',
  'DaemonSet',
  'Pod',
  'ReplicationController',
  'ReplicaSet',
  'Deployment',
  'HorizontalPodAutoscaler',
  'StatefulSet',
  'Job',
  'CronJob',
  'IngressClass',
  'Ingress',
  'APIService',
]

const rank = (kind: string) => {
  const index = KIND_ORDER.indexOf(kind)
  return index === -1 ? KIND_ORDER.length : index
}

export interface ResourceGroup {
  kind: string
  resources: ResourceStatus[]
  /** Resources of this kind that are not synced (hooks excluded, as for the application). */
  outOfSync: number
  /** Resources of this kind whose health is degraded or missing. */
  unhealthy: number
}

const UNHEALTHY = new Set(['degraded', 'missing'])

/** Resources grouped by kind, kinds in sync order, each group by namespace then name. */
export function groupByKind(resources: readonly ResourceStatus[]): ResourceGroup[] {
  const byKind = new Map<string, ResourceStatus[]>()
  for (const resource of resources) {
    const group = byKind.get(resource.kind)
    if (group) group.push(resource)
    else byKind.set(resource.kind, [resource])
  }
  return [...byKind.entries()]
    .toSorted(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
    .map(([kind, members]) => ({
      kind,
      resources: members.toSorted(
        (a, b) => a.syncWave - b.syncWave || (a.namespace ?? '').localeCompare(b.namespace ?? '') || a.name.localeCompare(b.name),
      ),
      outOfSync: members.filter((resource) => !resource.hook && resource.syncStatus !== 'synced').length,
      unhealthy: members.filter((resource) => UNHEALTHY.has(resource.healthStatus ?? '')).length,
    }))
}

/** The diff of one resource out of the application's diff, by the same identity. */
export function findResourceDiff(diffs: readonly ResourceDiff[], resource: ResourceStatus): ResourceDiff | undefined {
  const key = resourceKey(resource)
  return diffs.find((diff) => resourceKey(diff) === key)
}
