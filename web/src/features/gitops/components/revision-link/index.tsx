import { cn } from '@/lib/utils'

import { shortSha } from '../../lib/labels'
import { isCommitSha } from '../../lib/repo'

interface RevisionLinkProps {
  /** A commit SHA, or a branch or tag as the source names it. */
  revision: string | null | undefined
  /** The repository's GitHub URL; without it the revision is plain text. */
  repoUrl?: string
  /** Shown when there is no revision. */
  fallback?: string
  className?: string
}

const LINK_CLASS =
  'rounded-sm underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50'

/**
 * A revision in mono. A commit shows its short SHA and opens on GitHub (the
 * full SHA is the tooltip); a branch or tag opens its tree.
 */
export function RevisionLink({ revision, repoUrl, fallback = '—', className }: RevisionLinkProps) {
  const value = revision?.trim()
  if (!value) return <span className={cn('text-muted-foreground', className)}>{fallback}</span>

  const commit = isCommitSha(value)
  const label = commit ? shortSha(value) : value
  const classes = cn('font-mono tabular-nums', className)
  if (!repoUrl) {
    return (
      <span className={classes} title={value}>
        {label}
      </span>
    )
  }
  const base = repoUrl.replace(/\/$/, '')
  const href = commit ? `${base}/commit/${encodeURIComponent(value)}` : `${base}/tree/${value.split('/').map(encodeURIComponent).join('/')}`
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      title={value}
      aria-label={commit ? `Commit ${label} on GitHub` : `${label} on GitHub`}
      className={cn(classes, LINK_CLASS)}
    >
      {label}
    </a>
  )
}
