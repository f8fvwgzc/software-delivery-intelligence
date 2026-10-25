import { aliasesOf, type FieldAlias } from '@/features/delivery-cd/lib/field-errors'

import type { Cluster, UpsertClusterRequest } from '../api/types'

/** The cluster editor's state. `namespaces` is the comma list as typed. */
export interface ClusterDraft {
  name: string
  kubeconfigSecret: string
  context: string
  defaultNamespace: string
  namespaces: string
}

export const EMPTY_CLUSTER: ClusterDraft = { name: '', kubeconfigSecret: '', context: '', defaultNamespace: '', namespaces: '' }

export function toClusterDraft(cluster: Cluster | undefined): ClusterDraft {
  if (!cluster) return EMPTY_CLUSTER
  return {
    name: cluster.name,
    kubeconfigSecret: cluster.kubeconfigSecret,
    context: cluster.context ?? '',
    defaultNamespace: cluster.defaultNamespace ?? '',
    namespaces: cluster.namespaces.join(', '),
  }
}

/** `shop, payments  ,` → [`shop`, `payments`]; commas, spaces and new lines all separate. */
export function splitNamespaces(value: string): string[] {
  return [...new Set(value.split(/[\s,]+/).map((namespace) => namespace.trim()).filter(Boolean))]
}

export function toClusterInput(draft: ClusterDraft): UpsertClusterRequest {
  return {
    name: draft.name.trim(),
    kubeconfigSecret: draft.kubeconfigSecret,
    context: draft.context.trim() || null,
    defaultNamespace: draft.defaultNamespace.trim() || null,
    namespaces: splitNamespaces(draft.namespaces),
  }
}

const DNS_LABEL = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/
const isNamespace = (value: string) => value.length <= 63 && DNS_LABEL.test(value)

export function validateCluster(draft: ClusterDraft): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!draft.name.trim()) errors.name = 'Give the cluster a name.'
  if (!draft.kubeconfigSecret) errors.kubeconfigSecret = 'Choose the workspace secret that holds its kubeconfig, or store one.'
  const fallback = draft.defaultNamespace.trim()
  if (fallback && !isNamespace(fallback)) errors.defaultNamespace = 'A namespace is up to 63 lower case letters, digits and hyphens.'
  const invalid = splitNamespaces(draft.namespaces).find((namespace) => !isNamespace(namespace))
  if (invalid) errors.namespaces = `${invalid} is not a namespace name: lower case letters, digits and hyphens.`
  return errors
}

export const CLUSTER_FIELDS: FieldAlias[] = [
  { field: 'name', aliases: ['name'] },
  { field: 'kubeconfigSecret', aliases: [...aliasesOf('kubeconfigSecret'), 'kubeconfig', 'secret'] },
  { field: 'context', aliases: ['context'] },
  { field: 'defaultNamespace', aliases: aliasesOf('defaultNamespace') },
  { field: 'namespaces', aliases: ['namespaces'] },
]

export const clusterControlId = (field: string) => `cluster-${field}`
