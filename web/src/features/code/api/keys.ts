import { projectKeys } from '@/features/projects/api/queries'

/**
 * All hang off `projectKeys.detail(projectId)`: a rescan moves the commit,
 * so the tree, every cached file and its highlight tokens go stale together.
 */
export const codeKeys = {
  tree: (projectId: string) => [...projectKeys.detail(projectId), 'tree'] as const,
  file: (projectId: string, path: string) => [...projectKeys.detail(projectId), 'file', path] as const,
  tokens: (projectId: string, path: string, commitSha: string, lang: string) =>
    [...codeKeys.file(projectId, path), 'tokens', commitSha, lang] as const,
}
