import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { api, unwrap } from '@/lib/api/client'

import { applySettingsPatch } from '../lib/settings'
import { settingsKeys } from './keys'
import type { Settings, UpdateSettingsRequest } from './queries'

/**
 * `PATCH /settings` for one section (or both). The cache flips at once and
 * rolls back if the gateway refuses; the response then replaces it, since
 * a service may normalise what it was sent. Each form calls this hook
 * itself so its Save button has its own `isPending`.
 */
export function useUpdateSettings() {
  const queryClient = useQueryClient()
  const key = settingsKeys.settings()

  return useMutation({
    mutationFn: (patch: UpdateSettingsRequest) => unwrap(api.PATCH('/api/v1/settings', { body: patch })),
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: key })
      const previous = queryClient.getQueryData<Settings>(key)
      if (previous) queryClient.setQueryData<Settings>(key, applySettingsPatch(previous, patch))
      return { previous }
    },
    onError: (_error, _patch, context) => {
      if (context?.previous) queryClient.setQueryData(key, context.previous)
    },
    onSuccess: (settings, patch) => {
      queryClient.setQueryData(key, settings)
      const section = patch.knowledge && patch.intelligence ? 'Settings' : patch.knowledge ? 'Scanner settings' : 'Ranking settings'
      toast.success(`${section} saved`)
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.all })
    },
  })
}
