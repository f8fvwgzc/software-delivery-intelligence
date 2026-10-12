import { infiniteQueryOptions, queryOptions } from '@tanstack/react-query'

import { projectQueries } from '@/features/projects/api/queries'
import { api, unwrap } from '@/lib/api/client'

import { proposalKeys } from './keys'

/** Rows per page of the proposals table; a full page means there may be an earlier one. */
export const PROPOSALS_PAGE_SIZE = 50

export const proposalsQuery = (projectId?: string) =>
  queryOptions({
    queryKey: proposalKeys.list({ projectId }),
    queryFn: ({ signal }) =>
      unwrap(
        api.GET('/api/v1/proposals', {
          params: { query: projectId ? { projectId } : {} },
          signal,
        }),
      ),
  })

/** The proposals table: newest first, paged backwards with `before` (the oldest row's `createdAt`). */
export const proposalPagesQuery = (projectId?: string) =>
  infiniteQueryOptions({
    queryKey: proposalKeys.pages({ projectId }),
    queryFn: ({ pageParam, signal }) =>
      unwrap(
        api.GET('/api/v1/proposals', {
          params: {
            query: { limit: PROPOSALS_PAGE_SIZE, ...(projectId ? { projectId } : {}), ...(pageParam ? { before: pageParam } : {}) },
          },
          signal,
        }),
      ),
    initialPageParam: '' as string,
    getNextPageParam: (lastPage) => (lastPage.length === PROPOSALS_PAGE_SIZE ? lastPage[lastPage.length - 1].createdAt : undefined),
  })

export const proposalQuery = (proposalId: string) =>
  queryOptions({
    queryKey: proposalKeys.detail(proposalId),
    queryFn: ({ signal }) => unwrap(api.GET('/api/v1/proposals/{id}', { params: { path: { id: proposalId } }, signal })),
  })

export const proposalMessagesQuery = (proposalId: string) =>
  queryOptions({
    queryKey: proposalKeys.messages(proposalId),
    queryFn: ({ signal }) =>
      unwrap(api.GET('/api/v1/proposals/{id}/messages', { params: { path: { id: proposalId } }, signal })),
  })

/** The generated files of one option, complete and ready to commit. Kept off the proposal body. */
export const optionArtifactsQuery = (proposalId: string, optionId: string) =>
  queryOptions({
    queryKey: proposalKeys.artifacts(proposalId, optionId),
    queryFn: ({ signal }) =>
      unwrap(
        api.GET('/api/v1/proposals/{id}/options/{optionId}/artifacts', {
          params: { path: { id: proposalId, optionId } },
          signal,
        }),
      ),
  })

/** Every connected project, for pickers: the projects feature's own list query, so one request serves every picker and page. */
export const projectPickerQuery = () => queryOptions({ ...projectQueries.list(), staleTime: 60_000 })
