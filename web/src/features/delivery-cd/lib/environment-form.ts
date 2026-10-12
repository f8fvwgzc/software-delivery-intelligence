import type { Environment, UpsertEnvironmentRequest } from '../api/types'
import { visibleFields, type ConfigField } from './deployers'
import { aliasesOf, type FieldAlias } from './field-errors'

/** The editor's state: every value as the controls hold it. */
export interface EnvironmentDraft {
  name: string
  kind: string
  config: Record<string, string>
  url: string
  healthPath: string
  /** '' for the workspace default. */
  registryId: string
  protected: boolean
  autoPromote: boolean
}

export const EMPTY_DRAFT: EnvironmentDraft = {
  name: '',
  kind: 'github_actions',
  config: {},
  url: '',
  healthPath: '',
  registryId: '',
  protected: false,
  autoPromote: false,
}

export function toDraft(environment: Environment | undefined): EnvironmentDraft {
  if (!environment) return EMPTY_DRAFT
  return {
    name: environment.name,
    kind: environment.kind,
    config: { ...environment.config },
    url: environment.url ?? '',
    healthPath: environment.healthPath ?? '',
    registryId: environment.registryId ?? '',
    protected: environment.protected,
    autoPromote: environment.autoPromote,
  }
}

/** The form's key for a config field: `config.workflow`. */
export const configFieldKey = (key: string) => `config.${key}`

/** Only the keys the chosen deployer reads, without the ones left empty: its defaults then apply. */
function prunedConfig(draft: EnvironmentDraft) {
  const config: Record<string, string> = {}
  for (const field of visibleFields(draft.kind, draft.config)) {
    const value = draft.config[field.key]?.trim()
    if (value) config[field.key] = value
  }
  return config
}

/** `position` is left out on purpose: an update keeps the stage where it is, a create appends it. */
export function toInput(draft: EnvironmentDraft): UpsertEnvironmentRequest {
  return {
    name: draft.name.trim(),
    kind: draft.kind,
    config: prunedConfig(draft),
    url: draft.url.trim() || null,
    healthPath: draft.healthPath.trim() || null,
    registryId: draft.registryId || null,
    protected: draft.protected,
    autoPromote: draft.autoPromote,
  }
}

/** A deployer switch keeps what the two kinds share (the GitOps pair) and drops the rest. */
export function withKind(draft: EnvironmentDraft, kind: string): EnvironmentDraft {
  const next: EnvironmentDraft = { ...draft, kind, config: {} }
  for (const field of visibleFields(kind, draft.config)) {
    const value = draft.config[field.key]
    if (value !== undefined) next.config[field.key] = value
  }
  return next
}

function isHttpUrl(value: string) {
  try {
    const { protocol } = new URL(value)
    return protocol === 'https:' || protocol === 'http:'
  } catch {
    return false
  }
}

function configError(field: ConfigField, raw: string | undefined): string | null {
  const value = raw?.trim() ?? ''
  if (!value) return field.required ? `${field.label} is required.` : null
  if (field.type === 'number' && !/^\d+$/.test(value)) return `${field.label} must be a whole number.`
  if (field.type === 'json') {
    try {
      const parsed: unknown = JSON.parse(value)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return `${field.label} must be a JSON object.`
    } catch {
      return `${field.label} is not valid JSON.`
    }
  }
  if (field.key === 'url' && !isHttpUrl(value)) return `${field.label} must start with http:// or https://.`
  return null
}

/** What can be checked before asking the server; it has the last word on everything else. Keyed by form field. */
export function validateDraft(draft: EnvironmentDraft): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!draft.name.trim()) errors.name = 'Give the environment a name.'
  const url = draft.url.trim()
  if (url && !isHttpUrl(url)) errors.url = 'The URL must start with http:// or https://.'
  const healthPath = draft.healthPath.trim()
  if (healthPath && !healthPath.startsWith('/')) errors.healthPath = 'The health path starts with a slash, for example /health.'
  if (healthPath && !url) errors.url = 'A health path needs the URL it is checked on.'
  for (const field of visibleFields(draft.kind, draft.config)) {
    const message = configError(field, draft.config[field.key])
    if (message) errors[configFieldKey(field.key)] = message
  }
  return errors
}

/**
 * The names a server message may use for each field of the form as it
 * stands. The services name a deployer setting as `config.<key>`, so a bare
 * `url` always means the environment's own URL, never a deployer's.
 */
export function fieldAliases(draft: EnvironmentDraft): FieldAlias[] {
  const top = ['name', 'kind', 'url', 'healthPath', 'registryId', 'protected', 'autoPromote'].map((field) => ({
    field,
    aliases: field === 'registryId' ? [...aliasesOf(field), 'registry'] : aliasesOf(field),
  }))
  const config = visibleFields(draft.kind, draft.config).map((field) => ({
    field: configFieldKey(field.key),
    aliases: [configFieldKey(field.key)],
  }))
  return [...top, ...config]
}

/** DOM id of the control for a form field, so a refused save can move focus to it. */
export const controlId = (field: string) => `environment-${field.replaceAll('.', '-')}`
