import { useSuspenseQuery } from '@tanstack/react-query'
import { BookOpenIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Markdown } from '@/components/shared/data/markdown'
import { QueryBoundary } from '@/components/shared/feedback'
import { Skeleton } from '@/components/ui/skeleton'

import { codeQueries } from '../../api/queries'
import { baseName } from '../../lib/path'

interface ReadmePanelProps {
  projectId: string
  path: string
}

function ReadmeContent({ projectId, path }: ReadmePanelProps) {
  const { data: file } = useSuspenseQuery(codeQueries.file(projectId, path))
  if (file.binary) return null
  return <Markdown className="max-w-[80ch] px-5 py-4">{file.content}</Markdown>
}

function ReadmeSkeleton() {
  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <Skeleton className="h-5 w-48" />
      <Skeleton className="h-3 w-full max-w-[60ch]" />
      <Skeleton className="h-3 w-full max-w-[52ch]" />
      <Skeleton className="h-3 w-full max-w-[40ch]" />
    </div>
  )
}

/** The directory's README, rendered under its listing like GitHub does. */
export function ReadmePanel({ projectId, path }: ReadmePanelProps) {
  return (
    <section className="flex flex-col border-t" aria-label="README">
      <header className="flex h-10 items-center gap-2 border-b bg-muted/30 px-3">
        <BookOpenIcon className="size-4 text-muted-foreground" aria-hidden="true" />
        <Link
          to={routes.projectCode(projectId, path)}
          className="rounded-sm font-mono text-xs outline-none hover:underline hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {baseName(path)}
        </Link>
      </header>
      <QueryBoundary fallback={<ReadmeSkeleton />}>
        <ReadmeContent projectId={projectId} path={path} />
      </QueryBoundary>
    </section>
  )
}
