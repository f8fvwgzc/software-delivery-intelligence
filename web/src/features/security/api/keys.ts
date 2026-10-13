import { SECURITY_ROOT } from '@/lib/live/event-families'

/*
 * Its own root, by project: the overview is computed per request from
 * GitHub (and OSV), so the live stream reaches it through the gated lane
 * (lib/live/event-families.ts) and only for the project an event names.
 */
export const securityKeys = {
  all: [SECURITY_ROOT] as const,
  project: (projectId: string) => [...securityKeys.all, projectId] as const,
  overview: (projectId: string) => [...securityKeys.project(projectId), 'overview'] as const,
}
