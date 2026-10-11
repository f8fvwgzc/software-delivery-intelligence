import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { proposalKeys } from './keys'

type GenerateProposalRequest = components['schemas']['GenerateProposalRequest']
type AcceptProposalRequest = components['schemas']['AcceptProposalRequest']
type OpenPullRequestRequest = components['schemas']['OpenPullRequestRequest']

/** POST /proposals — generates a ranked proposal for a project. */
export function useGenerateProposal() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GenerateProposalRequest) => unwrap(api.POST('/api/v1/proposals', { body })),
    onSuccess: (proposal) => {
      queryClient.setQueryData(proposalKeys.detail(proposal.id), proposal)
      queryClient.invalidateQueries({ queryKey: proposalKeys.lists() })
    },
  })
}

/** POST /proposals/{id}/messages — one negotiation turn. */
export function useNegotiate(proposalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (content: string) =>
      unwrap(
        api.POST('/api/v1/proposals/{id}/messages', {
          params: { path: { id: proposalId } },
          body: { content },
        }),
      ),
    onSuccess: (response) => {
      // The reply carries the re-ranked proposal; seed it so the hero and
      // the thread update together, then refetch to stay honest.
      if (response.proposal) {
        queryClient.setQueryData(proposalKeys.detail(proposalId), response.proposal)
      }
      queryClient.invalidateQueries({ queryKey: proposalKeys.detail(proposalId) })
      queryClient.invalidateQueries({ queryKey: proposalKeys.lists() })
      if (response.recordedDecision) {
        queryClient.invalidateQueries({ queryKey: ['decisions'] })
      }
    },
  })
}

/** POST /proposals/{id}/accept — accepts an option, optionally creating an execution. */
export function useAcceptProposal(proposalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: AcceptProposalRequest) =>
      unwrap(
        api.POST('/api/v1/proposals/{id}/accept', {
          params: { path: { id: proposalId } },
          body,
        }),
      ),
    onSuccess: (proposal) => {
      queryClient.setQueryData(proposalKeys.detail(proposalId), proposal)
      queryClient.invalidateQueries({ queryKey: proposalKeys.all })
      queryClient.invalidateQueries({ queryKey: ['decisions'] })
      if (proposal.executionId) {
        queryClient.invalidateQueries({ queryKey: ['executions'] })
      }
    },
  })
}

/**
 * POST /proposals/{id}/pull-request — accepts an option and opens a pull
 * request with its generated files through a `github`-adapter execution.
 */
export function useOpenPullRequest(proposalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: OpenPullRequestRequest) =>
      unwrap(
        api.POST('/api/v1/proposals/{id}/pull-request', {
          params: { path: { id: proposalId } },
          body,
        }),
      ),
    onSuccess: (proposal) => {
      queryClient.setQueryData(proposalKeys.detail(proposalId), proposal)
      queryClient.invalidateQueries({ queryKey: proposalKeys.all })
      queryClient.invalidateQueries({ queryKey: ['decisions'] })
      queryClient.invalidateQueries({ queryKey: ['executions'] })
    },
  })
}

/** POST /proposals/{id}/reject — rejects with a reason. */
export function useRejectProposal(proposalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (reason: string) =>
      unwrap(
        api.POST('/api/v1/proposals/{id}/reject', {
          params: { path: { id: proposalId } },
          body: { reason },
        }),
      ),
    onSuccess: (proposal) => {
      queryClient.setQueryData(proposalKeys.detail(proposalId), proposal)
      queryClient.invalidateQueries({ queryKey: proposalKeys.all })
    },
  })
}
