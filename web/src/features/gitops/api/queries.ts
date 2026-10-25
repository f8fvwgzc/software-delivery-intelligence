import { queryOptions } from '@tanstack/react-query'

import { projectQueries } from '@/features/projects/api/queries'
import { api, unwrap } from '@/lib/api/client'
import { fallbackPoll } from '@/lib/live/connection-store'

import { isOperationRunning } from '../lib/labels'
import { clusterKeys, diffKeys, gitopsKeys, manifestKeys } from './keys'

/** Fallback cadence while an operation runs and the live stream is down. */
const OPERATION_INTERVAL = 3_000
const LIST_INTERVAL = 5_000
/** Rendered from one commit: the same revision always renders the same manifests. */
const RENDERED_STALE_MS = 5 * 60_000
/** Clusters only change through an admin's own mutation, which invalidates them. */
const CONFIG_STALE_MS = 60_000

/*
 * CONTRACT.md §24. Live through the `application` entity on the platform
 * stream (lib/live/event-families.ts); polling is only the fallback while
 * an operation runs and the stream is down. The diff and the manifests are
 * computed by the service on every request, so they are fetched only when
 * someone opens them and never poll.
 */
export const gitopsQueries = {
  /** One project's applications: summaries without resources, the first condition only. */
  applications: (projectId: string) =>
    queryOptions({
      queryKey: gitopsKeys.list(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/applications', { params: { query: { projectId } }, signal })),
      refetchInterval: (query) =>
        query.state.data?.some((application) => isOperationRunning(application.operation)) ? fallbackPoll(LIST_INTERVAL) : false,
    }),

  /** One application with every resource it manages and all its conditions. */
  application: (applicationId: string) =>
    queryOptions({
      queryKey: gitopsKeys.detail(applicationId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/applications/{id}', { params: { path: { id: applicationId } }, signal })),
      refetchInterval: (query) => (isOperationRunning(query.state.data?.operation) ? fallbackPoll(OPERATION_INTERVAL) : false),
    }),

  /** Every sync that was not a dry run, newest first: the last 50 the service keeps. */
  history: (applicationId: string) =>
    queryOptions({
      queryKey: gitopsKeys.history(applicationId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/applications/{id}/history', { params: { path: { id: applicationId } }, signal })),
    }),

  /** Per resource, how the cluster differs from the desired state. One request serves every resource's "Diff". */
  diff: (applicationId: string) =>
    queryOptions({
      queryKey: diffKeys.application(applicationId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/applications/{id}/diff', { params: { path: { id: applicationId } }, signal })),
      staleTime: 30_000,
      retry: false,
    }),

  /** The desired state as rendered at the source's revision, `Secret` values masked. */
  manifests: (applicationId: string) =>
    queryOptions({
      queryKey: manifestKeys.application(applicationId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/applications/{id}/manifests', { params: { path: { id: applicationId } }, signal })),
      staleTime: RENDERED_STALE_MS,
      retry: false,
    }),

  /** The workspace's clusters with the status of their last check. */
  clusters: () =>
    queryOptions({
      queryKey: clusterKeys.list(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/clusters', { signal })),
      staleTime: CONFIG_STALE_MS,
    }),

  /**
   * The project behind an application (repository, default branch, its
   * GitHub URL): the project feature's own detail query, which the project
   * layout keeps fresh, so reading it here rarely costs a request of its own.
   */
  project: (projectId: string) => queryOptions({ ...projectQueries.detail(projectId), staleTime: 5 * 60_000, refetchInterval: false }),
}
