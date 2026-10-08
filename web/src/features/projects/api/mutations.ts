import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { api, unwrap } from '@/lib/api/client'

import { projectKeys } from './keys'
import type { Project, ProjectMode } from './queries'

/** `PATCH /projects/{id}/mode`. The returned project replaces the cached detail at once. */
export function useSetProjectMode(project: Pick<Project, 'id' | 'name'>) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (mode: ProjectMode) =>
      unwrap(
        api.PATCH('/api/v1/projects/{id}/mode', {
          params: { path: { id: project.id } },
          body: { mode },
        }),
      ),
    onSuccess: (updated) => {
      queryClient.setQueryData(projectKeys.detail(project.id), updated)
      queryClient.invalidateQueries({ queryKey: projectKeys.lists() })
      toast.success(
        updated.mode === 'execute'
          ? `Execute mode on for ${project.name}`
          : `${project.name} is back in observe mode`,
      )
    },
  })
}

/** `POST /projects/{id}/scans`. Everything known about the project is refetched. */
export function useStartScan(project: Pick<Project, 'id' | 'name'>) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: () =>
      unwrap(api.POST('/api/v1/projects/{id}/scans', { params: { path: { id: project.id } } })),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: projectKeys.detail(project.id) })
      queryClient.invalidateQueries({ queryKey: projectKeys.lists() })
      toast.success(`Scan queued for ${project.name}`)
    },
  })
}

/** `DELETE /projects/{id}`. The caller decides where to navigate afterwards. */
export function useDeleteProject(project: Pick<Project, 'id' | 'name'>) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async () => {
      await api.DELETE('/api/v1/projects/{id}', { params: { path: { id: project.id } } })
    },
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: projectKeys.detail(project.id) })
      queryClient.invalidateQueries({ queryKey: projectKeys.lists() })
      toast.success(`${project.name} disconnected`)
    },
  })
}
