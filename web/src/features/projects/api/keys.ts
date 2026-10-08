/**
 * Query key factory for everything project-scoped.
 *
 * Other features build their keys on top of `projectKeys.detail(id)` so one
 * invalidation of that family (after a rescan, an answer, a mode change)
 * refreshes every panel that shows data of that project.
 */
export const projectKeys = {
  all: ['projects'] as const,
  lists: () => [...projectKeys.all, 'list'] as const,
  list: () => [...projectKeys.lists()] as const,
  details: () => [...projectKeys.all, 'detail'] as const,
  detail: (projectId: string) => [...projectKeys.details(), projectId] as const,
  scans: (projectId: string) => [...projectKeys.detail(projectId), 'scans'] as const,
  /** Scans are addressed by their own id; they do not live under a project key. */
  scan: (scanId: string) => [...projectKeys.all, 'scan', scanId] as const,
}
