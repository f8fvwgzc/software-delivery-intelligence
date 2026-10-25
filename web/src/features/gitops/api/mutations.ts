import { useMutation, useQueryClient } from '@tanstack/react-query'

import { approvalKeys } from '@/features/approvals/api/keys'
import { executionKeys } from '@/features/executions/api/keys'
import { api, unwrap } from '@/lib/api/client'

import { clusterKeys, diffKeys, gitopsKeys, manifestKeys } from './keys'
import type { Application, UpsertApplicationRequest, UpsertClusterRequest } from './types'

/*
 * Editors and dialogs surface a refusal next to the control it is about, so
 * their mutations opt out of the global error toast (`meta.silent`, see
 * `lib/query-client.ts`). Refresh keeps the toast: it has no dialog.
 */
const INLINE_ERRORS = { silent: true } as const

export interface SaveApplicationVariables {
  /** Set to update; omitted to create. */
  id?: string
  input: UpsertApplicationRequest
}

/** `POST /applications` or `PUT /applications/{id}` (admin). The answer is the application as the service now holds it. */
export function useSaveApplication() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: SaveApplicationVariables) =>
      id
        ? unwrap(api.PUT('/api/v1/applications/{id}', { params: { path: { id } }, body: input }))
        : unwrap(api.POST('/api/v1/applications', { body: input })),
    meta: INLINE_ERRORS,
    onSuccess: (saved) => {
      queryClient.invalidateQueries({ queryKey: gitopsKeys.list(saved.projectId) })
      queryClient.invalidateQueries({ queryKey: gitopsKeys.detail(saved.id) })
      // A new source renders new manifests and a new diff.
      queryClient.invalidateQueries({ queryKey: diffKeys.application(saved.id) })
      queryClient.invalidateQueries({ queryKey: manifestKeys.application(saved.id) })
      // Each cluster counts the applications that deploy to it.
      queryClient.invalidateQueries({ queryKey: clusterKeys.all })
    },
  })
}

export interface DeleteApplicationVariables {
  /** Also delete the resources it manages from the cluster. */
  cascade: boolean
}

/**
 * `DELETE /applications/{id}?cascade=` (admin). Answers how many resources
 * left the cluster. The application's own queries are dropped by `forget`,
 * which the caller runs once it has navigated away: dropped while its page
 * is still mounted, they would be asked for again and answer 404.
 */
export function useDeleteApplication(application: Pick<Application, 'id' | 'projectId'>) {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: ({ cascade }: DeleteApplicationVariables) =>
      unwrap(api.DELETE('/api/v1/applications/{id}', { params: { path: { id: application.id }, query: { cascade } } })),
    meta: INLINE_ERRORS,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: gitopsKeys.list(application.projectId) })
      queryClient.invalidateQueries({ queryKey: clusterKeys.all })
    },
  })
  const forget = () => {
    queryClient.removeQueries({ queryKey: gitopsKeys.detail(application.id) })
    queryClient.removeQueries({ queryKey: gitopsKeys.history(application.id) })
    queryClient.removeQueries({ queryKey: diffKeys.application(application.id) })
    queryClient.removeQueries({ queryKey: manifestKeys.application(application.id) })
  }
  return { ...mutation, forget }
}

/**
 * `POST /applications/{id}/refresh` (member): renders and compares now. The
 * answer is the application with its resources, so it lands in the cache
 * as it is; the diff follows it, and a hard refresh renders anew.
 */
export function useRefreshApplication(applicationId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ hard }: { hard: boolean }) =>
      unwrap(api.POST('/api/v1/applications/{id}/refresh', { params: { path: { id: applicationId } }, body: { hard } })),
    onSuccess: (application, { hard }) => {
      queryClient.setQueryData(gitopsKeys.detail(applicationId), application)
      queryClient.invalidateQueries({ queryKey: gitopsKeys.list(application.projectId) })
      queryClient.invalidateQueries({ queryKey: diffKeys.application(applicationId) })
      if (hard) queryClient.invalidateQueries({ queryKey: manifestKeys.application(applicationId) })
    },
  })
}

/** An operation starts a gated execution, which opens an approval. */
function useStartedExecution() {
  const queryClient = useQueryClient()
  return () => {
    queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
    queryClient.invalidateQueries({ queryKey: approvalKeys.all })
  }
}

/** `POST /applications/{id}/sync` (approver): answers the execution that previews, waits for approval, syncs and checks health. */
export function useSyncApplication(applicationId: string) {
  const onStarted = useStartedExecution()
  return useMutation({
    mutationFn: ({ prune, dryRun }: { prune: boolean; dryRun: boolean }) =>
      unwrap(api.POST('/api/v1/applications/{id}/sync', { params: { path: { id: applicationId } }, body: { prune, dryRun } })),
    meta: INLINE_ERRORS,
    onSuccess: onStarted,
  })
}

/** `POST /applications/{id}/rollback` (approver): syncs a history entry's source again, through an execution. */
export function useRollbackApplication(applicationId: string) {
  const onStarted = useStartedExecution()
  return useMutation({
    mutationFn: ({ historyId, prune }: { historyId: string; prune: boolean }) =>
      unwrap(api.POST('/api/v1/applications/{id}/rollback', { params: { path: { id: applicationId } }, body: { historyId, prune } })),
    meta: INLINE_ERRORS,
    onSuccess: onStarted,
  })
}

export interface SaveClusterVariables {
  id?: string
  input: UpsertClusterRequest
}

/**
 * `POST /clusters` or `PUT /clusters/{id}` (admin). The service reaches the
 * cluster once and answers whether it connected; an unreachable cluster is
 * saved anyway.
 */
export function useSaveCluster() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: SaveClusterVariables) =>
      id
        ? unwrap(api.PUT('/api/v1/clusters/{id}', { params: { path: { id } }, body: input }))
        : unwrap(api.POST('/api/v1/clusters', { body: input })),
    meta: INLINE_ERRORS,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: clusterKeys.all })
      // Applications carry their cluster's name.
      queryClient.invalidateQueries({ queryKey: gitopsKeys.applications() })
    },
  })
}

/** `DELETE /clusters/{id}` (admin). Refused with 412 while an application deploys to it; the dialog shows the sentence. */
export function useDeleteCluster() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (clusterId: string) => unwrap(api.DELETE('/api/v1/clusters/{id}', { params: { path: { id: clusterId } } })),
    meta: INLINE_ERRORS,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: clusterKeys.all }),
  })
}
