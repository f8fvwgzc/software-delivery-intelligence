import { useSuspenseQuery } from '@tanstack/react-query'

import { Markdown } from '@/components/shared/data'
import { Separator } from '@/components/ui/separator'

import { documentQuery } from '../../api/queries'
import { ViewerToolbar } from './toolbar'

/** Right pane: the selected document, rendered. Suspends on its own. */
export function DocumentViewer({ documentId }: { documentId: string }) {
  const { data: document } = useSuspenseQuery(documentQuery(documentId))

  return (
    <article className="flex min-w-0 flex-col rounded-lg bg-card ring-1 ring-border">
      <ViewerToolbar document={document} />
      <Separator />
      <div className="px-4 py-4 md:px-6 md:py-5">
        {document.markdown.trim().length === 0 ? (
          <p className="text-sm text-muted-foreground">This document is empty.</p>
        ) : (
          <Markdown>{document.markdown}</Markdown>
        )}
      </div>
    </article>
  )
}
