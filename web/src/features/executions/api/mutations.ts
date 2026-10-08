import { useMutation, useQueryClient } from '@tanstack/react-query'

import { approvalKeys } from '@/features/approvals/api/keys'
import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { executionKeys } from './keys'

type CreateExecutionRequest = components['schemas']['CreateExecutionRequest']
type FaultInjection = components['schemas']['FaultInjection']
type ResolveFailureRequest = components['schemas']['ResolveFailureRequest']

export function useCreateExecution() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateExecutionRequest) => unwrap(api.POST('/api/v1/executions', { body })),
    onSuccess: (execution) => {
      queryClient.setQueryData(executionKeys.detail(execution.id), execution)
      queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
    },
  })
}

function useExecutionAction<TVariables>(
  executionId: string,
  mutationFn: (variables: TVariables) => Promise<unknown>,
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: executionKeys.detail(executionId) })
      queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
      queryClient.invalidateQueries({ queryKey: approvalKeys.all })
    },
  })
}

export function useStartExecution(executionId: string) {
  return useExecutionAction(executionId, (fault: FaultInjection | null) =>
    unwrap(api.POST('/api/v1/executions/{id}/start', { params: { path: { id: executionId } }, body: { fault } })),
  )
}

export function useResolveFailure(executionId: string) {
  return useExecutionAction(executionId, (body: ResolveFailureRequest) =>
    unwrap(api.POST('/api/v1/executions/{id}/resolve', { params: { path: { id: executionId } }, body })),
  )
}

export function useCancelExecution(executionId: string) {
  return useExecutionAction(executionId, (reason: string) =>
    unwrap(api.POST('/api/v1/executions/{id}/cancel', { params: { path: { id: executionId } }, body: { reason } })),
  )
}
