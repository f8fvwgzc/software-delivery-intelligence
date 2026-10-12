import { useQuery } from '@tanstack/react-query'

import { Markdown } from '@/components/shared/data/markdown'
import { Skeleton } from '@/components/ui/skeleton'

import { cdQueries } from '../../api/queries'
import type { Release } from '../../api/types'

/**
 * The notes of a release: the commit subjects since the previous one. The
 * list carries summaries, so when a row has no notes of its own the release
 * is read once, on expand.
 */
export function ReleaseNotes({ release }: { release: Release }) {
  const inline = release.notes.trim()
  const detail = useQuery({ ...cdQueries.release(release.id), enabled: !inline })
  const notes = inline || detail.data?.notes.trim()

  if (notes) return <Markdown className="text-xs [&_p]:my-1.5 [&_ul]:my-1.5">{notes}</Markdown>
  if (!inline && detail.isPending) {
    return (
      <div className="flex flex-col gap-1.5 py-1" role="status" aria-label="Loading the notes">
        <Skeleton className="h-3 w-3/4" />
        <Skeleton className="h-3 w-1/2" />
      </div>
    )
  }
  return (
    <p className="text-xs text-muted-foreground">
      {detail.isError ? 'The notes could not be loaded.' : 'No notes: this is the first release, or nothing was committed since the previous one.'}
    </p>
  )
}
