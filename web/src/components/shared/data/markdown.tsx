import { lazy, Suspense } from 'react'

import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

interface MarkdownProps {
  children: string
  className?: string
}

/** The markdown engine (react-markdown + remark-gfm) is its own chunk, loaded on first use. */
const MarkdownEngine = lazy(() => import('./markdown-engine'))

function MarkdownSkeleton() {
  return (
    <div className="flex flex-col gap-2 py-1" role="status" aria-label="Loading">
      <Skeleton className="h-3 w-full" />
      <Skeleton className="h-3 w-11/12" />
      <Skeleton className="h-3 w-2/3" />
    </div>
  )
}

/** Renders generated documents and assistant replies. */
export function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={cn('prose-sdi', className)}>
      <Suspense fallback={<MarkdownSkeleton />}>
        <MarkdownEngine>{children}</MarkdownEngine>
      </Suspense>
    </div>
  )
}
