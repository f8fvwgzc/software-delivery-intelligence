import type { Project } from '@/features/projects/api/queries'

import type { ApplicationSource } from '../api/types'

const OWNER_NAME = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/

/**
 * The GitHub URL of the repository an application renders: the project's
 * own when the source names none (or names the project's), else the named
 * `owner/name` on the workspace's GitHub host. Undefined when it cannot be
 * told, and the revision then stays plain text.
 */
export function sourceRepoUrl(
  source: Pick<ApplicationSource, 'repo'>,
  project: Pick<Project, 'repoFullName' | 'repoUrl'> | undefined,
  githubHost: string | undefined,
): string | undefined {
  const repo = source.repo?.trim()
  if (!repo || (project && repo.toLowerCase() === project.repoFullName.toLowerCase())) return project?.repoUrl || undefined
  if (!OWNER_NAME.test(repo)) return undefined
  const host = githubHost?.trim() || 'github.com'
  return `https://${host}/${repo}`
}

/** `owner/name` as the source names it, or the project's when it names none. */
export function sourceRepoName(source: Pick<ApplicationSource, 'repo'>, project: Pick<Project, 'repoFullName'> | undefined): string {
  return source.repo?.trim() || project?.repoFullName || 'This project’s repository'
}

/** A 40-character hexadecimal string: a commit, which gets a link and the short form. */
export function isCommitSha(value: string | null | undefined): boolean {
  return Boolean(value && /^[0-9a-f]{40}$/i.test(value))
}
