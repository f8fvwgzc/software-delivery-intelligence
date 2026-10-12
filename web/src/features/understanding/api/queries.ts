import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { understandingKeys } from './keys'

export { understandingKeys }

export type Fact = components['schemas']['Fact']
export type FactKind = components['schemas']['FactKind']
export type FactCategory = components['schemas']['FactCategory']
export type Question = components['schemas']['Question']
export type Understanding = components['schemas']['Understanding']
export type UnderstandingSection = components['schemas']['UnderstandingSection']

/** All three are produced by a scan; answering a question invalidates the project family. */
const COMMIT_STALE_MS = 5 * 60_000

export const understandingQueries = {
  facts: (projectId: string, kind: FactKind) =>
    queryOptions({
      queryKey: understandingKeys.facts(projectId, kind),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/facts', { params: { path: { id: projectId }, query: { kind } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),

  questions: (projectId: string) =>
    queryOptions({
      queryKey: understandingKeys.questions(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/questions', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),

  understanding: (projectId: string) =>
    queryOptions({
      queryKey: understandingKeys.understanding(projectId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/understanding', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),
}
