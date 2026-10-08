import { projectKeys } from '@/features/projects/api/queries'

/** Lives under the project family, so a rescan refreshes it along with everything else. */
export const dependencyKeys = {
  list: (projectId: string) => [...projectKeys.detail(projectId), 'dependencies'] as const,
}
