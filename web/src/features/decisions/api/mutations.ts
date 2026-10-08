import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { decisionKeys } from './keys'

type CreateDecisionRequest = components['schemas']['CreateDecisionRequest']

/** POST /decisions — records a decision by hand. */
export function useCreateDecision() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateDecisionRequest) => unwrap(api.POST('/api/v1/decisions', { body })),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: decisionKeys.all })
      // Policy agreement is part of every ranking; proposals may re-rank.
      queryClient.invalidateQueries({ queryKey: ['proposals'] })
    },
  })
}

/** POST /decisions/{id}/supersede — marks a decision as no longer in force. */
export function useSupersedeDecision() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (variables: { id: string; supersededBy?: string }) =>
      unwrap(
        api.POST('/api/v1/decisions/{id}/supersede', {
          params: { path: { id: variables.id } },
          body: { supersededBy: variables.supersededBy ?? null },
        }),
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: decisionKeys.all })
      queryClient.invalidateQueries({ queryKey: ['proposals'] })
    },
  })
}
