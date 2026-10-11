import { projectKeys } from '@/features/projects/api/queries'

/**
 * Both hang off `projectKeys.detail(projectId)`: a rescan moves the commit,
 * so the tree and every cached file go stale together.
 */
export const codeKeys = {
  tree: (projectId: string) => [...projectKeys.detail(projectId), 'tree'] as const,
  file: (projectId: string, path: string) => [...projectKeys.detail(projectId), 'file', path] as const,
}
