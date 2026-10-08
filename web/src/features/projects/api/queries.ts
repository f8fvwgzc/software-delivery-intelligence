import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { projectKeys } from './keys'

export { projectKeys }

export type Project = components['schemas']['Project']
export type ProjectStatus = components['schemas']['ProjectStatus']
export type ProjectMode = components['schemas']['ProjectMode']
export type Scan = components['schemas']['Scan']
export type ScanStage = components['schemas']['ScanStage']
export type ScanStatus = components['schemas']['ScanStatus']
export type StageStatus = components['schemas']['StageStatus']

/** A project whose scan has not settled yet. */
export function isProjectBusy(status: ProjectStatus | undefined) {
  return status === 'pending' || status === 'scanning'
}

/** A scan that is still queued or running. */
export function isScanActive(status: ScanStatus | undefined) {
  return status === 'queued' || status === 'running'
}

const BUSY_POLL_MS = 5_000
const SCAN_POLL_MS = 2_000

/**
 * Query options for projects. Lists and details poll on their own while a
 * scan is in flight, so every consumer sees status changes without wiring
 * polling itself. Polling continues in a background tab: a scan takes
 * seconds and the page must be current the moment the user comes back.
 */
export const projectQueries = {
  list: () =>
    queryOptions({
      queryKey: projectKeys.list(),
      queryFn: () => unwrap(api.GET('/api/v1/projects')),
      refetchInterval: (query) =>
        query.state.data?.some((project) => isProjectBusy(project.status)) ? BUSY_POLL_MS : false,
      refetchIntervalInBackground: true,
    }),

  detail: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.detail(projectId),
      queryFn: () => unwrap(api.GET('/api/v1/projects/{id}', { params: { path: { id: projectId } } })),
      refetchInterval: (query) => (isProjectBusy(query.state.data?.status) ? BUSY_POLL_MS : false),
      refetchIntervalInBackground: true,
    }),

  scans: (projectId: string) =>
    queryOptions({
      queryKey: projectKeys.scans(projectId),
      queryFn: () => unwrap(api.GET('/api/v1/projects/{id}/scans', { params: { path: { id: projectId } } })),
    }),

  scan: (scanId: string) =>
    queryOptions({
      queryKey: projectKeys.scan(scanId),
      queryFn: () => unwrap(api.GET('/api/v1/scans/{id}', { params: { path: { id: scanId } } })),
      refetchInterval: (query) => (isScanActive(query.state.data?.status) ? SCAN_POLL_MS : false),
      refetchIntervalInBackground: true,
    }),
}
