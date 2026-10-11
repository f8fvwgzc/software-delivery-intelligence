import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { projectPickerKeys, proposalKeys } from './keys'

export const proposalsQuery = (projectId?: string) =>
  queryOptions({
    queryKey: proposalKeys.list({ projectId }),
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/proposals', {
          params: { query: projectId ? { projectId } : {} },
        }),
      ),
  })

export const proposalQuery = (proposalId: string) =>
  queryOptions({
    queryKey: proposalKeys.detail(proposalId),
    queryFn: () => unwrap(api.GET('/api/v1/proposals/{id}', { params: { path: { id: proposalId } } })),
  })

export const proposalMessagesQuery = (proposalId: string) =>
  queryOptions({
    queryKey: proposalKeys.messages(proposalId),
    queryFn: () =>
      unwrap(api.GET('/api/v1/proposals/{id}/messages', { params: { path: { id: proposalId } } })),
  })

/** The generated files of one option, complete and ready to commit. Kept off the proposal body. */
export const optionArtifactsQuery = (proposalId: string, optionId: string) =>
  queryOptions({
    queryKey: proposalKeys.artifacts(proposalId, optionId),
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/proposals/{id}/options/{optionId}/artifacts', {
          params: { path: { id: proposalId, optionId } },
        }),
      ),
  })

/**
 * Every connected project, for pickers. The projects feature owns the
 * entity; this lives here only because its `api/queries.ts` does not exist
 * yet. Move the import once it does.
 */
export const projectPickerQuery = () =>
  queryOptions({
    queryKey: projectPickerKeys.list(),
    queryFn: () => unwrap(api.GET('/api/v1/projects')),
    staleTime: 60_000,
  })
