import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { projectKeys } from '@/features/projects/api/keys'
import { api, unwrap } from '@/lib/api/client'

import { githubKeys } from './keys'
import type { ConnectProjectRequest, Project } from './types'

/**
 * `POST /projects`: connects a repository and queues its first scan.
 * Invalidates the whole projects family (lists, pickers) and the repository
 * list, which then marks the row as connected. Navigation is the caller's.
 */
export function useConnectRepository() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (body: ConnectProjectRequest): Promise<Project> => unwrap(api.POST('/api/v1/projects', { body })),
    onSuccess: async (project) => {
      toast.success(`Connected ${project.repoFullName}`, {
        description: 'The first scan has been queued.',
      })
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: projectKeys.all }),
        queryClient.invalidateQueries({ queryKey: githubKeys.repos() }),
      ])
    },
  })
}
