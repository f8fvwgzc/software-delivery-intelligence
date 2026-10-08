import type { Project } from '@/features/projects/api/queries'

/** Link to a file on the project's default branch, or nothing when the path is unknown. */
export function sourceFileUrl(project: Pick<Project, 'repoUrl' | 'defaultBranch'>, path: string): string | null {
  if (!path) return null
  const base = project.repoUrl.replace(/\/+$/, '')
  const branch = encodeURIComponent(project.defaultBranch)
  const encodedPath = path.split('/').map(encodeURIComponent).join('/')
  return `${base}/blob/${branch}/${encodedPath}`
}

export function shortSha(sha: string) {
  return sha.slice(0, 7)
}
