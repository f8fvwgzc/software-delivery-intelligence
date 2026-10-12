import type { Registry, UpsertRegistryRequest } from '@/features/delivery-cd/api/types'
import { aliasesOf, type FieldAlias } from '@/features/delivery-cd/lib/field-errors'
import { registryKind } from '@/features/delivery-cd/lib/registries'

/** The registry editor's state: every value as the controls hold it. */
export interface RegistryDraft {
  name: string
  kind: string
  host: string
  namespace: string
  auth: string
  usernameSecret: string
  passwordSecret: string
  isDefault: boolean
}

export const EMPTY_REGISTRY: RegistryDraft = {
  name: '',
  kind: 'ghcr',
  host: 'ghcr.io',
  namespace: '',
  auth: 'github_token',
  usernameSecret: '',
  passwordSecret: '',
  isDefault: false,
}

export function toRegistryDraft(registry: Registry | undefined): RegistryDraft {
  if (!registry) return EMPTY_REGISTRY
  return {
    name: registry.name,
    kind: registry.kind,
    host: registry.host,
    namespace: registry.namespace,
    auth: registry.auth,
    usernameSecret: registry.usernameSecret ?? '',
    passwordSecret: registry.passwordSecret ?? '',
    isDefault: registry.isDefault,
  }
}

/** A kind switch brings its fixed host (or clears the last kind's) and its default way to sign in. */
export function withRegistryKind(draft: RegistryDraft, kind: string): RegistryDraft {
  const meta = registryKind(kind)
  const previous = registryKind(draft.kind)
  return {
    ...draft,
    kind,
    host: meta.host || (draft.host === previous.host ? '' : draft.host),
    auth: meta.auth.includes(draft.auth as (typeof meta.auth)[number]) ? draft.auth : meta.auth[0],
  }
}

export function toRegistryInput(draft: RegistryDraft): UpsertRegistryRequest {
  const bySecret = draft.auth === 'secret'
  return {
    name: draft.name.trim(),
    kind: draft.kind,
    host: draft.host.trim().replace(/^https?:\/\//, '').replace(/\/+$/, ''),
    namespace: draft.namespace.trim().replace(/^\/+|\/+$/g, ''),
    auth: draft.auth,
    usernameSecret: bySecret ? draft.usernameSecret || null : null,
    passwordSecret: bySecret ? draft.passwordSecret || null : null,
    isDefault: draft.isDefault,
  }
}

/** What can be checked before asking the server. Keyed by form field. */
export function validateRegistry(draft: RegistryDraft): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!draft.name.trim()) errors.name = 'Give the registry a name.'
  const host = draft.host.trim()
  if (!host) errors.host = 'Enter the registry host, for example ghcr.io.'
  else if (/\s/.test(host)) errors.host = 'A host has no spaces.'
  if (!draft.namespace.trim()) errors.namespace = 'Enter the org, user or project path images are pushed under.'
  if (draft.auth === 'secret') {
    if (!draft.usernameSecret) errors.usernameSecret = 'Choose the secret that holds the username.'
    if (!draft.passwordSecret) errors.passwordSecret = 'Choose the secret that holds the password or token.'
  }
  return errors
}

export const REGISTRY_FIELDS: FieldAlias[] = [
  'name',
  'kind',
  'host',
  'namespace',
  'auth',
  'usernameSecret',
  'passwordSecret',
  'isDefault',
].map((field) => ({ field, aliases: field === 'isDefault' ? [...aliasesOf(field), 'default'] : aliasesOf(field) }))

/** DOM id of the control for a form field, so a refused save can move focus to it. */
export const registryControlId = (field: string) => `registry-${field}`
