import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'
import { fallbackPoll } from '@/lib/live/connection-store'

import { projectKeys } from './keys'

export { projectKeys }

export type Project = components['schemas']['Project']
export type ProjectStatus = components['schemas']['ProjectStatus']
export type ProjectMode = components['schemas']['ProjectMode']
export type Scan = components['schemas']['Scan']
export type ScanStage = components['schemas']['ScanStage']
export type ScanStatus = components['schemas']['ScanStatus']
export type StageStatus = components['schemas']['StageStatus']
export type MaturityScore = components['schemas']['MaturityScore']
export type MaturityAxis = components['schemas']['MaturityAxis']
export type ProjectConfig = components['schemas']['ProjectConfig']

/** A project whose scan has not settled yet. */
export function isProjectBusy(status: ProjectStatus | undefined) {
  return status === 'pending' || status === 'scanning'
}

/** A scan that is still queued or running. */
export function isScanActive(status: ScanStatus | undefined) {
  return status === 'queued' || status === 'running'
}

const BUSY_POLL_MS = 5_000
export const SCAN_POLL_MS = 2_000
/** Maturity, config and scan history only move with a rescan, which invalidates the project family. */
const COMMIT_STALE_MS = 5 * 60_000

/**
 * Query options for projects. The platform event stream keeps these fresh;
 * lists and details only poll (while a scan is in flight) as a fallback
 * when that stream is down. Fallback polling continues in a background tab:
 * a scan takes seconds and the page must be current the moment the user
 * comes back.
 */
export const projectQueries = {
  list: () =>
    queryOptions({
      queryKey: projectKeys.list(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects', { signal })),
      refetchInterval: (query) =>
        query.state.data?.some((project) => isProjectBusy(project.status)) ? fallbackPoll(BUSY_POLL_MS) : false,
    }),

  detail: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.detail(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}', { params: { path: { id: projectId } }, signal })),
      refetchInterval: (query) => (isProjectBusy(query.state.data?.status) ? fallbackPoll(BUSY_POLL_MS) : false),
    }),

  scans: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.scans(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/scans', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),

  /** One scan. Its own SSE stream (`/scans/{id}/stream`) is consumed by ScanProgress; this only polls as a fallback. */
  scan: (scanId: string) =>
    queryOptions({
      queryKey: projectKeys.scan(scanId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/scans/{id}', { params: { path: { id: scanId } }, signal })),
      refetchInterval: (query) => (isScanActive(query.state.data?.status) ? fallbackPoll(SCAN_POLL_MS) : false),
    }),

  /** Delivery maturity (CONTRACT.md §13): eight axes, findings, recommended patterns. */
  maturity: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.maturity(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/maturity', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),

  /** What `.sdi.yml` declares (CONTRACT.md §12), or `present: false`. */
  config: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.config(projectId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/projects/{id}/config', { params: { path: { id: projectId } }, signal })),
      staleTime: COMMIT_STALE_MS,
    }),
}
