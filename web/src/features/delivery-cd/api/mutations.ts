import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { artifactKeys, cdKeys, registryKeys, secretKeys } from './keys'
import type {
  CreateReleaseRequest,
  PromoteRequest,
  RollbackRequest,
  SetSecretRequest,
  UpsertEnvironmentRequest,
  UpsertRegistryRequest,
} from './types'

/*
 * Editors surface a refused save next to the field it names, so their
 * mutations opt out of the global error toast (`meta.silent`, see
 * `lib/query-client.ts`). Everything else keeps the toast.
 */
const INLINE_ERRORS = { silent: true } as const

export interface SaveEnvironmentVariables {
  /** Set to update; omitted to create. */
  id?: string
  /** Without `position`: an update keeps the stage where it is, a create appends it. */
  input: UpsertEnvironmentRequest
}

/** `POST /projects/{id}/environments` or `PUT /environments/{id}` (admin). */
export function useSaveEnvironment(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: SaveEnvironmentVariables) =>
      id
        ? unwrap(api.PUT('/api/v1/environments/{id}', { params: { path: { id } }, body: input }))
        : unwrap(api.POST('/api/v1/projects/{id}/environments', { params: { path: { id: projectId } }, body: input })),
    meta: INLINE_ERRORS,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: cdKeys.pipeline(projectId) })
      // The setup files follow the deployer kinds in use.
      queryClient.invalidateQueries({ queryKey: artifactKeys.project(projectId) })
    },
  })
}

/** `DELETE /environments/{id}` (admin). Its runs stay in the history. */
export function useDeleteEnvironment(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (environmentId: string) => unwrap(api.DELETE('/api/v1/environments/{id}', { params: { path: { id: environmentId } } })),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: cdKeys.pipeline(projectId) })
      queryClient.invalidateQueries({ queryKey: cdKeys.releasePages(projectId) })
      queryClient.invalidateQueries({ queryKey: artifactKeys.project(projectId) })
    },
  })
}

/** A deployment run starts a gated execution, which may open an approval. */
function useStartedRun(projectId: string) {
  const queryClient = useQueryClient()
  return () => {
    queryClient.invalidateQueries({ queryKey: cdKeys.pipeline(projectId) })
    queryClient.invalidateQueries({ queryKey: cdKeys.runs() })
    queryClient.invalidateQueries({ queryKey: cdKeys.releasePages(projectId) })
    queryClient.invalidateQueries({ queryKey: ['executions'] })
    queryClient.invalidateQueries({ queryKey: ['approvals'] })
  }
}

export interface PromoteVariables extends PromoteRequest {
  environmentId: string
}

/** `POST /environments/{id}/promote` (approver): answers the deployment run. */
export function usePromote(projectId: string) {
  const onStarted = useStartedRun(projectId)
  return useMutation({
    mutationFn: ({ environmentId, releaseId }: PromoteVariables) =>
      unwrap(
        api.POST('/api/v1/environments/{id}/promote', {
          params: { path: { id: environmentId } },
          body: { releaseId: releaseId || null },
        }),
      ),
    onSuccess: onStarted,
  })
}

export interface RollbackVariables extends RollbackRequest {
  environmentId: string
}

/** `POST /environments/{id}/rollback` (approver): the same plan as `deploy.rollback`. */
export function useRollback(projectId: string) {
  const onStarted = useStartedRun(projectId)
  return useMutation({
    mutationFn: ({ environmentId, releaseId, reason }: RollbackVariables) =>
      unwrap(
        api.POST('/api/v1/environments/{id}/rollback', {
          params: { path: { id: environmentId } },
          body: { releaseId: releaseId || null, reason: reason?.trim() || null },
        }),
      ),
    onSuccess: onStarted,
  })
}

/** `POST /projects/{id}/releases` (member): a manual release, by default at the default branch head. */
export function useCreateRelease(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ commitSha, version }: Pick<CreateReleaseRequest, 'commitSha' | 'version'>) =>
      unwrap(
        api.POST('/api/v1/projects/{id}/releases', {
          params: { path: { id: projectId } },
          body: { commitSha: commitSha?.trim() || null, version: version?.trim() || null },
        }),
      ),
    meta: INLINE_ERRORS,
    onSuccess: (release) => {
      queryClient.setQueryData(cdKeys.release(release.id), release)
      queryClient.invalidateQueries({ queryKey: cdKeys.releasePages(projectId) })
      // A new release is the first stage's candidate.
      queryClient.invalidateQueries({ queryKey: cdKeys.pipeline(projectId) })
    },
  })
}

export interface SaveRegistryVariables {
  /** Set to update; omitted to create. */
  id?: string
  input: UpsertRegistryRequest
}

/** `POST /registries` or `PUT /registries/{id}` (admin). */
export function useSaveRegistry() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: SaveRegistryVariables) =>
      id
        ? unwrap(api.PUT('/api/v1/registries/{id}', { params: { path: { id } }, body: input }))
        : unwrap(api.POST('/api/v1/registries', { body: input })),
    meta: INLINE_ERRORS,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: registryKeys.all })
      // Every board names the registry its releases go to.
      queryClient.invalidateQueries({ queryKey: cdKeys.pipelines() })
    },
  })
}

/** `DELETE /registries/{id}` (admin). */
export function useDeleteRegistry() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (registryId: string) => unwrap(api.DELETE('/api/v1/registries/{id}', { params: { path: { id: registryId } } })),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: registryKeys.all })
      queryClient.invalidateQueries({ queryKey: cdKeys.pipelines() })
    },
  })
}

export interface SetSecretVariables extends SetSecretRequest {
  name: string
}

/**
 * `PUT /workspaces/{id}/secrets/{name}` (admin). The value is write-only:
 * the answer carries the name and a 4-character hint. `gcTime: 0` drops the
 * finished mutation, and with it the value, from the cache at once.
 */
export function useSetSecret(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, value, description }: SetSecretVariables) =>
      unwrap(
        api.PUT('/api/v1/workspaces/{id}/secrets/{name}', {
          params: { path: { id: workspaceId, name } },
          body: { value, description: description?.trim() || null },
        }),
      ),
    meta: INLINE_ERRORS,
    gcTime: 0,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: secretKeys.list(workspaceId) }),
  })
}

/** `DELETE /workspaces/{id}/secrets/{name}` (admin). Deployers that name it fail until it is set again. */
export function useDeleteSecret(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api.DELETE('/api/v1/workspaces/{id}/secrets/{name}', { params: { path: { id: workspaceId, name } } })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: secretKeys.list(workspaceId) }),
  })
}
