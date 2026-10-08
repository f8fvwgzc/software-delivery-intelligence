import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { identityKeys, policyKeys } from './keys'

type Policy = components['schemas']['Policy']
type PolicyDecision = components['schemas']['PolicyDecision']

export interface UpdatePolicyVariables {
  actionType: string
  decision: PolicyDecision
}

/**
 * Changes one policy's decision. The row flips immediately and rolls back
 * if the gateway refuses (e.g. a policy that cannot be relaxed).
 */
export function useUpdatePolicy() {
  const queryClient = useQueryClient()
  const listKey = policyKeys.list()

  return useMutation({
    mutationFn: ({ actionType, decision }: UpdatePolicyVariables) =>
      unwrap(api.PATCH('/api/v1/policies/{actionType}', { params: { path: { actionType } }, body: { decision } })),
    onMutate: async ({ actionType, decision }) => {
      await queryClient.cancelQueries({ queryKey: listKey })
      const previous = queryClient.getQueryData<Policy[]>(listKey)
      queryClient.setQueryData<Policy[]>(listKey, (policies) =>
        policies?.map((policy) => (policy.actionType === actionType ? { ...policy, decision } : policy)),
      )
      return { previous }
    },
    onError: (_error, _variables, context) => {
      if (context?.previous) queryClient.setQueryData(listKey, context.previous)
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: policyKeys.all })
    },
  })
}

export interface GrantPermissionVariables {
  identityId: string
  permissions: string[]
  ttlMinutes?: number
}

export function useGrantPermission() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ identityId, permissions, ttlMinutes }: GrantPermissionVariables) =>
      unwrap(
        api.POST('/api/v1/identities/{id}/grants', {
          params: { path: { id: identityId } },
          body: { permissions, ttlMinutes: ttlMinutes ?? null },
        }),
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: identityKeys.all })
    },
  })
}
