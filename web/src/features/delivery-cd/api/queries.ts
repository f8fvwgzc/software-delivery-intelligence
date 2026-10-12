import { infiniteQueryOptions, queryOptions, type InfiniteData } from '@tanstack/react-query'

import { projectQueries } from '@/features/projects/api/queries'
import { api, unwrap } from '@/lib/api/client'
import { fallbackPoll } from '@/lib/live/connection-store'

import { isRunOpen } from '../lib/labels'
import { artifactKeys, cdKeys, registryKeys, secretKeys, type DeploymentRunFilters } from './keys'
import type { DeploymentRun } from './types'

/** Fallback cadence while a deployment runs and the live stream is down. */
const LIVE_INTERVAL = 3_000
/** Rows per page; a full page means there may be an earlier one. */
export const RELEASES_PAGE_SIZE = 20
export const RUNS_PAGE_SIZE = 20
/** Registries, secrets and generated files only change through an admin's own mutation, which invalidates them. */
const CONFIG_STALE_MS = 60_000

function hasOpenRun(data: InfiniteData<DeploymentRun[]> | undefined) {
  return data?.pages.some((page) => page.some(isRunOpen)) ?? false
}

/*
 * CONTRACT.md §22. Live through the `deployment`, `release` and
 * `environment` entities on the platform stream; polling is only the
 * fallback while something is in flight and the stream is down.
 */
export const cdQueries = {
  /** The board: stages in promotion order with what runs, what could be promoted, and the 30-day numbers. */
  pipeline: (projectId: string) =>
    queryOptions({
      queryKey: cdKeys.pipeline(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/pipeline', { params: { path: { id: projectId } }, signal })),
      refetchInterval: (query) =>
        query.state.data?.stages.some((stage) => stage.lastRun && isRunOpen(stage.lastRun)) ? fallbackPoll(LIVE_INTERVAL) : false,
    }),

  /** Releases newest first, paged backwards by `before` (the oldest row's `createdAt`). */
  releases: (projectId: string) =>
    infiniteQueryOptions({
      queryKey: cdKeys.releasePages(projectId),
      queryFn: ({ pageParam, signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/releases', {
            params: { path: { id: projectId }, query: { limit: RELEASES_PAGE_SIZE, ...(pageParam ? { before: pageParam } : {}) } },
            signal,
          }),
        ),
      initialPageParam: '' as string,
      getNextPageParam: (lastPage) => (lastPage.length === RELEASES_PAGE_SIZE ? lastPage[lastPage.length - 1].createdAt : undefined),
    }),

  /** One release with its notes; the list carries summaries (CONTRACT.md §21). An image never changes once built. */
  release: (releaseId: string) =>
    queryOptions({
      queryKey: cdKeys.release(releaseId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/releases/{id}', { params: { path: { id: releaseId } }, signal })),
      staleTime: 5 * 60_000,
    }),

  /** Promotions and rollbacks newest first, paged backwards by `before`. */
  runs: (projectId: string, filters: DeploymentRunFilters = {}) =>
    infiniteQueryOptions({
      queryKey: cdKeys.runPages(projectId, filters),
      queryFn: ({ pageParam, signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/deployment-runs', {
            params: {
              path: { id: projectId },
              query: {
                limit: RUNS_PAGE_SIZE,
                ...(filters.environmentId ? { environmentId: filters.environmentId } : {}),
                ...(pageParam ? { before: pageParam } : {}),
              },
            },
            signal,
          }),
        ),
      initialPageParam: '' as string,
      getNextPageParam: (lastPage) => (lastPage.length === RUNS_PAGE_SIZE ? lastPage[lastPage.length - 1].createdAt : undefined),
      refetchInterval: (query) => (hasOpenRun(query.state.data) ? fallbackPoll(LIVE_INTERVAL) : false),
    }),

  /** The setup files for the deployers this project uses, generated from its facts. Fetched only when asked for. */
  artifacts: (projectId: string) =>
    queryOptions({
      queryKey: artifactKeys.project(projectId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/delivery/artifacts', { params: { path: { id: projectId } }, signal })),
      staleTime: 5 * 60_000,
    }),

  /** Where images live, per workspace. */
  registries: () =>
    queryOptions({
      queryKey: registryKeys.list(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/registries', { signal })),
      staleTime: CONFIG_STALE_MS,
    }),

  /** Secret names and hints; a value is never returned. Admin only: callers enable it for admins alone. */
  secrets: (workspaceId: string) =>
    queryOptions({
      queryKey: secretKeys.list(workspaceId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/workspaces/{id}/secrets', { params: { path: { id: workspaceId } }, signal })),
      staleTime: CONFIG_STALE_MS,
    }),

  /**
   * The project behind the board (repository, default branch): the project
   * feature's own detail query, which the project layout already keeps
   * fresh, so reading it here never costs a request of its own.
   */
  project: (projectId: string) =>
    queryOptions({ ...projectQueries.detail(projectId), staleTime: 5 * 60_000, refetchInterval: false }),
}
