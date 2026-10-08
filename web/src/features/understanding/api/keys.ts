import { projectKeys } from '@/features/projects/api/queries'

import type { FactKind } from './queries'

/**
 * Everything here hangs off `projectKeys.detail(projectId)`, so a rescan
 * or an answered question refreshes the whole understanding at once.
 */
export const understandingKeys = {
  facts: (projectId: string, kind: FactKind) => [...projectKeys.detail(projectId), 'facts', { kind }] as const,
  questions: (projectId: string) => [...projectKeys.detail(projectId), 'questions'] as const,
  understanding: (projectId: string) => [...projectKeys.detail(projectId), 'understanding'] as const,
}
