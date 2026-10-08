import { useMutation, useQueryClient } from '@tanstack/react-query'

import { executionKeys } from '@/features/executions/api/keys'
import { api, unwrap } from '@/lib/api/client'

import { approvalKeys } from './keys'

export interface DecideApprovalVariables {
  approvalId: string
  approve: boolean
  note?: string
}

/** Approves or rejects one approval. Executions change state too, so both families refresh. */
export function useDecideApproval() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ approvalId, approve, note }: DecideApprovalVariables) =>
      unwrap(
        api.POST('/api/v1/approvals/{id}/decide', {
          params: { path: { id: approvalId } },
          body: { approve, note: note?.trim() || null },
        }),
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: approvalKeys.all })
      queryClient.invalidateQueries({ queryKey: executionKeys.all })
    },
  })
}
