import { aliasesOf, type FieldAlias } from '@/features/delivery-cd/lib/field-errors'

import type { Application, SourceType, UpsertApplicationRequest } from '../api/types'

/** The application editor's state: every value as the controls hold it. Lists are one entry per line. */
export interface ApplicationDraft {
  name: string
  clusterId: string
  namespace: string
  repo: string
  revision: string
  path: string
  /** '' is "Detect": decided when the source renders. */
  type: '' | SourceType
  recurse: boolean
  helmReleaseName: string
  helmValueFiles: string
  /** `key=value`, one per line. */
  helmParameters: string
  kustomizeImages: string
  automated: boolean
  selfHeal: boolean
  prune: boolean
  createNamespace: boolean
  /** Digits; '' keeps the service's default of 3. */
  retryLimit: string
}

export const EMPTY_APPLICATION: ApplicationDraft = {
  name: '',
  clusterId: '',
  namespace: '',
  repo: '',
  revision: '',
  path: '',
  type: '',
  recurse: false,
  helmReleaseName: '',
  helmValueFiles: '',
  helmParameters: '',
  kustomizeImages: '',
  automated: false,
  selfHeal: false,
  prune: false,
  createNamespace: false,
  retryLimit: '',
}

const lines = (value: string) =>
  value
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)

export function toApplicationDraft(application: Application | undefined): ApplicationDraft {
  if (!application) return EMPTY_APPLICATION
  const { source, syncPolicy } = application
  return {
    name: application.name,
    clusterId: application.clusterId,
    namespace: application.namespace ?? '',
    repo: source.repo ?? '',
    revision: source.revision ?? '',
    path: source.path ?? '',
    type: source.type ?? '',
    recurse: source.recurse,
    helmReleaseName: source.helmReleaseName ?? '',
    helmValueFiles: source.helmValueFiles.join('\n'),
    helmParameters: Object.entries(source.helmParameters)
      .map(([key, value]) => `${key}=${value}`)
      .join('\n'),
    kustomizeImages: source.kustomizeImages.join('\n'),
    automated: syncPolicy.automated,
    selfHeal: syncPolicy.selfHeal,
    prune: syncPolicy.prune,
    createNamespace: syncPolicy.createNamespace,
    retryLimit: syncPolicy.retryLimit > 0 ? String(syncPolicy.retryLimit) : '',
  }
}

/** `image.tag=1.4.2` → [`image.tag`, `1.4.2`]; the first `=` splits, so values may hold one. */
function parameter(line: string): [string, string] | null {
  const at = line.indexOf('=')
  if (at <= 0) return null
  return [line.slice(0, at).trim(), line.slice(at + 1).trim()]
}

const trimPath = (path: string) => path.trim().replace(/^\/+|\/+$/g, '')

/**
 * The create or update body. An update replaces every field (CONTRACT.md
 * §24.8), so every option travels, including those of the source types the
 * form does not show right now: images a promotion stored on a source whose
 * type is detected must survive an edit of its namespace.
 */
export function toApplicationInput(draft: ApplicationDraft, projectId: string): UpsertApplicationRequest {
  const retryLimit = Number.parseInt(draft.retryLimit, 10)
  return {
    projectId,
    name: draft.name.trim(),
    clusterId: draft.clusterId,
    namespace: draft.namespace.trim() || null,
    source: {
      repo: draft.repo.trim() || null,
      revision: draft.revision.trim() || null,
      path: trimPath(draft.path) || null,
      type: draft.type || null,
      recurse: draft.recurse,
      helmReleaseName: draft.helmReleaseName.trim() || null,
      helmValueFiles: lines(draft.helmValueFiles),
      helmParameters: Object.fromEntries(
        lines(draft.helmParameters)
          .map(parameter)
          .filter((entry): entry is [string, string] => entry !== null),
      ),
      kustomizeImages: lines(draft.kustomizeImages),
    },
    syncPolicy: {
      automated: draft.automated,
      // Self heal and prune only mean something to automated syncs; they are kept as set, not cleared.
      selfHeal: draft.selfHeal,
      prune: draft.prune,
      createNamespace: draft.createNamespace,
      retryLimit: Number.isFinite(retryLimit) ? retryLimit : null,
    },
  }
}

const DNS_LABEL = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/
const OWNER_NAME = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/

/** What can be checked before asking the server. Keyed by form field, in the order the form shows them. */
export function validateApplication(draft: ApplicationDraft): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!draft.name.trim()) errors.name = 'Give the application a name.'
  if (!draft.clusterId) errors.clusterId = 'Choose the cluster it deploys to.'
  const namespace = draft.namespace.trim()
  if (namespace && (namespace.length > 63 || !DNS_LABEL.test(namespace))) {
    errors.namespace = 'A namespace is up to 63 lower case letters, digits and hyphens.'
  }
  const repo = draft.repo.trim()
  if (repo && !OWNER_NAME.test(repo)) errors.repo = 'Write the repository as owner/name.'
  if (/\s/.test(draft.revision.trim())) errors.revision = 'A branch, tag or commit SHA has no spaces.'
  if (trimPath(draft.path).split('/').includes('..')) errors.path = 'The path stays inside the repository: no .. segments.'
  if (lines(draft.helmParameters).some((line) => !parameter(line))) errors.helmParameters = 'Write each parameter as key=value, one per line.'
  if (lines(draft.kustomizeImages).some((line) => /\s/.test(line))) {
    errors.kustomizeImages = 'One image per line: name=newName:tag, name:tag or name@sha256:digest.'
  }
  if (draft.retryLimit.trim() && !/^\d{1,2}$/.test(draft.retryLimit.trim())) errors.retryLimit = 'A whole number of attempts, 0 to 99.'
  return errors
}

/** Form fields and the words a server message may use for them (`source.path`, `cluster_id`, `release name`). */
export const APPLICATION_FIELDS: FieldAlias[] = [
  { field: 'name', aliases: ['name'] },
  { field: 'clusterId', aliases: [...aliasesOf('clusterId'), 'cluster'] },
  { field: 'namespace', aliases: ['namespace'] },
  { field: 'repo', aliases: ['repo', 'source.repo', 'repository'] },
  { field: 'revision', aliases: ['revision', 'source.revision'] },
  { field: 'path', aliases: ['path', 'source.path'] },
  { field: 'type', aliases: ['type', 'source.type', 'source type'] },
  { field: 'helmReleaseName', aliases: [...aliasesOf('helmReleaseName'), 'source.helm_release_name', 'release name'] },
  { field: 'helmValueFiles', aliases: [...aliasesOf('helmValueFiles'), 'source.helm_value_files', 'values file', 'values files'] },
  { field: 'helmParameters', aliases: [...aliasesOf('helmParameters'), 'source.helm_parameters'] },
  { field: 'kustomizeImages', aliases: [...aliasesOf('kustomizeImages'), 'source.kustomize_images', 'images'] },
  { field: 'retryLimit', aliases: [...aliasesOf('retryLimit'), 'sync_policy.retry_limit'] },
]

/** DOM id of the control for a form field, so a refused save can move focus to it. */
export const applicationControlId = (field: string) => `application-${field}`
