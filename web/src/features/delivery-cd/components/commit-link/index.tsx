import { cn } from '@/lib/utils'

import { shortSha } from '../../lib/labels'

interface CommitLinkProps {
  sha: string
  /** The repository's GitHub URL; without it the sha is plain text. */
  repoUrl?: string
  className?: string
}

/** A short sha in mono that opens the commit on GitHub; the full sha is the tooltip. */
export function CommitLink({ sha, repoUrl, className }: CommitLinkProps) {
  if (!sha) return null
  const classes = cn('font-mono tabular-nums', className)

  if (!repoUrl) {
    return (
      <span className={classes} title={sha}>
        {shortSha(sha)}
      </span>
    )
  }
  return (
    <a
      href={`${repoUrl.replace(/\/$/, '')}/commit/${encodeURIComponent(sha)}`}
      target="_blank"
      rel="noreferrer"
      title={sha}
      aria-label={`Commit ${shortSha(sha)} on GitHub`}
      className={cn(classes, 'rounded-sm underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50')}
    >
      {shortSha(sha)}
    </a>
  )
}
