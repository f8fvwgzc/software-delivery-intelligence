import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { codeKeys } from './keys'

export { codeKeys }

export type RepoTree = components['schemas']['RepoTree']
export type RepoTreeEntry = components['schemas']['RepoTreeEntry']
export type RepoFile = components['schemas']['RepoFile']

/** Content is pinned to the scanned commit, so it only changes with a rescan. */
const COMMIT_STALE_MS = 10 * 60_000

export const codeQueries = {
  /** Every entry of the repository at the scanned commit (CONTRACT.md §15). */
  tree: (projectId: string) =>
    queryOptions({
      queryKey: codeKeys.tree(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/tree', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),

  /** One file's text, with the ids of the facts detected from it. */
  file: (projectId: string, path: string) =>
    queryOptions({
      queryKey: codeKeys.file(projectId, path),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/file', { params: { path: { id: projectId }, query: { path } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),
}
