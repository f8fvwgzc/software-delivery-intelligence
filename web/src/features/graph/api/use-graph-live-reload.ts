import { useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'

import { usePlatformEvents, type PlatformEvent } from '@/lib/live/platform-events'

import { graphKeys } from './keys'

/**
 * An open map reloads itself when a scan of its project succeeds: only that
 * event rebuilds the graph, so the progress events before it cost nothing.
 * Feature-local on purpose: the shared invalidation lanes stay as they are
 * (`lib/live/event-families.ts`).
 */
export function useGraphLiveReload(projectId: string) {
  const queryClient = useQueryClient()
  const listener = useCallback(
    (event: PlatformEvent) => {
      if (event.entity !== 'scan' || event.status !== 'succeeded' || event.projectId !== projectId) return
      void queryClient.invalidateQueries({ queryKey: graphKeys.project(projectId) })
    },
    [queryClient, projectId],
  )
  usePlatformEvents(listener)
}
