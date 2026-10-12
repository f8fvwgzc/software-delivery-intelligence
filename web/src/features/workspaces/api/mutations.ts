import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { api, unwrap, WORKSPACE_HEADER } from '@/lib/api/client'
import { ApiError, type ApiErrorBody } from '@/lib/api/errors'
import { workspaceStore } from '@/lib/workspace-store'

import { workspaceKeys } from './keys'
import type {
  DataControls,
  Member,
  MembersResponse,
  PurgeResult,
  Role,
  UpdateDataControlsRequest,
  WorkspaceDeleted,
} from './types'
import { count } from '@/lib/intl'

/** `PATCH /workspaces/{id}/members/{userId}`: narrows one member's role, or clears the override. */
export function useUpdateMemberRole(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = workspaceKeys.members(workspaceId)

  return useMutation({
    mutationFn: ({ userId, overrideRole }: { userId: string; overrideRole: Role | null }): Promise<Member> =>
      unwrap(
        api.PATCH('/api/v1/workspaces/{id}/members/{userId}', {
          params: { path: { id: workspaceId, userId } },
          body: { overrideRole },
        }),
      ),
    onSuccess: (member) => {
      queryClient.setQueryData<MembersResponse>(key, (current) =>
        current
          ? { ...current, members: current.members.map((row) => (row.user.id === member.user.id ? member : row)) }
          : current,
      )
      toast.success(`${member.user.login}: ${member.effectiveRole}`)
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** `POST /workspaces/{id}/members/sync`: re-reads org membership and teams from GitHub. */
export function useSyncMembers(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = workspaceKeys.members(workspaceId)

  return useMutation({
    mutationFn: (): Promise<MembersResponse> =>
      unwrap(api.POST('/api/v1/workspaces/{id}/members/sync', { params: { path: { id: workspaceId } } })),
    onSuccess: (members) => {
      queryClient.setQueryData(key, members)
      toast.success(`Synced ${count(members.members.length, 'member', 'members')} from GitHub`)
    },
  })
}

/** `PATCH /workspaces/{id}/data-controls`: only the changed fields travel. */
export function useUpdateDataControls(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = workspaceKeys.dataControls(workspaceId)

  return useMutation({
    mutationFn: (patch: UpdateDataControlsRequest): Promise<DataControls> =>
      unwrap(api.PATCH('/api/v1/workspaces/{id}/data-controls', { params: { path: { id: workspaceId } }, body: patch })),
    onSuccess: (controls) => {
      queryClient.setQueryData(key, controls)
      toast.success('Data controls saved')
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** `PUT /workspaces/{id}/data-controls/reasoner-key`: stores (or, with an empty key, removes) the workspace's own model key. */
export function useSetReasonerKey(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = workspaceKeys.dataControls(workspaceId)

  return useMutation({
    mutationFn: (apiKey: string): Promise<DataControls> =>
      unwrap(
        api.PUT('/api/v1/workspaces/{id}/data-controls/reasoner-key', {
          params: { path: { id: workspaceId } },
          body: { apiKey },
        }),
      ),
    onSuccess: (controls, apiKey) => {
      queryClient.setQueryData(key, controls)
      toast.success(apiKey ? 'Model key stored' : 'Model key removed')
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** A browser download of a Blob, through a transient anchor. */
function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  anchor.rel = 'noopener'
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}

/**
 * `POST /workspaces/{id}/data/export`: the gateway streams one JSON
 * document; it is read as a Blob (never parsed, it can be large) and saved
 * as a file. Plain `fetch`, since the typed client would buffer and parse.
 */
export function useExportData(workspaceId: string, slug: string) {
  return useMutation({
    mutationFn: async () => {
      const response = await fetch(`/api/v1/workspaces/${encodeURIComponent(workspaceId)}/data/export`, {
        method: 'POST',
        headers: { accept: 'application/json', [WORKSPACE_HEADER]: workspaceId },
      })
      if (!response.ok) {
        let body: Partial<ApiErrorBody> | undefined
        try {
          body = await response.json()
        } catch {
          body = undefined
        }
        throw new ApiError(response.status, body)
      }
      const blob = await response.blob()
      const stamp = new Date().toISOString().slice(0, 10)
      saveBlob(blob, `sdi-export-${slug}-${stamp}.json`)
      return blob.size
    },
    onSuccess: (bytes) => {
      toast.success('Export downloaded', { description: `${(bytes / 1024).toFixed(0)} KB of JSON` })
    },
  })
}

/** `POST /workspaces/{id}/data/purge-code-cache`: cached file contents and trees are deleted (audited `data.purge`). */
export function usePurgeCodeCache(workspaceId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (): Promise<PurgeResult> =>
      unwrap(api.POST('/api/v1/workspaces/{id}/data/purge-code-cache', { params: { path: { id: workspaceId } } })),
    onSuccess: (result) => {
      toast.success('Code cache purged', {
        description: `${result.filesDeleted.toLocaleString()} files and ${result.treesDeleted.toLocaleString()} trees removed.`,
      })
      // The code browser now has to refetch from GitHub.
      queryClient.invalidateQueries({ queryKey: ['code'] })
      queryClient.invalidateQueries({ queryKey: workspaceKeys.audits(workspaceId) })
    },
  })
}

/**
 * `DELETE /workspaces/{id}`: every schema is cascaded (CONTRACT.md §19). The
 * session is then ended and the browser reloads on the login page, so no
 * cache or stream can still point at the workspace that is gone.
 */
export function useDeleteWorkspace(workspaceId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (): Promise<WorkspaceDeleted> =>
      unwrap(api.DELETE('/api/v1/workspaces/{id}', { params: { path: { id: workspaceId } } })),
    onSuccess: async () => {
      try {
        await api.POST('/api/v1/auth/logout')
      } catch {
        // The cookie may already be invalid; the reload below ends the session either way.
      }
      workspaceStore.set(null)
      queryClient.clear()
      window.location.assign(routes.login())
    },
  })
}
