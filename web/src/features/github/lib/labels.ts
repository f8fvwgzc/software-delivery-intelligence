import type { StatusTone } from '@/components/shared/data'

import type { GithubRepo } from '../api/types'

/** Environment variable the knowledge service reads (CONTRACT.md §3). */
export const TOKEN_ENV_VAR = 'GITHUB_TOKEN'

/** Command whose output `scripts/dev.sh` exports as the token. */
export const TOKEN_FALLBACK_COMMAND = 'gh auth token'

/** Private repositories get a warning-toned dot, public a neutral one. */
export function visibilityTone(repo: Pick<GithubRepo, 'isPrivate'>): StatusTone {
  return repo.isPrivate ? 'warning' : 'neutral'
}

export function visibilityLabel(repo: Pick<GithubRepo, 'isPrivate'>) {
  return repo.isPrivate ? 'Private' : 'Public'
}

/** Status dot fills, keyed by tone. */
export const DOT_CLASS: Record<StatusTone, string> = {
  neutral: 'bg-muted-foreground/50',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

/** Two-letter avatar fallback. */
export function initials(login: string) {
  return login.slice(0, 2).toUpperCase()
}
