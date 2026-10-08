import { useSuspenseQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Skeleton } from '@/components/ui/skeleton'

import { documentsQuery } from '../api/queries'
import { DocumentList } from '../components/document-list'
import { DocumentListSkeleton } from '../components/document-list/skeleton'
import { DocumentViewer } from '../components/document-viewer'
import { DocumentViewerSkeleton } from '../components/document-viewer/skeleton'
import { GenerateMenu } from '../components/generate-menu'

/** /projects/:projectId/documents — generated documents, list left, reader right. */
export default function DocumentsPage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Documents"
        description="Written from the facts the scan collected. Every document names how many it used."
      />
      <QueryBoundary fallback={<DocumentsSkeleton />}>
        <DocumentsBody projectId={projectId} />
      </QueryBoundary>
    </Page>
  )
}

function DocumentsBody({ projectId }: { projectId: string }) {
  const { data: documents } = useSuspenseQuery(documentsQuery(projectId))
  const [selectedId, setSelectedId] = useState<string | null>(null)

  // Fall back to the newest document; a stale selection (deleted id) is ignored.
  const newest = documents.toSorted((a, b) => b.generatedAt.localeCompare(a.generatedAt))[0]
  const activeId =
    selectedId && documents.some((document) => document.id === selectedId) ? selectedId : newest?.id

  const menu = <GenerateMenu projectId={projectId} onGenerated={setSelectedId} />

  return (
    <div className="grid items-start gap-6 lg:grid-cols-[18rem_minmax(0,1fr)]">
      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs tabular-nums text-muted-foreground">
            {documents.length} {documents.length === 1 ? 'document' : 'documents'}
          </span>
          {menu}
        </div>
        <DocumentList documents={documents} selectedId={activeId} onSelect={setSelectedId} emptyAction={menu} />
      </div>
      {activeId ? (
        <QueryBoundary fallback={<DocumentViewerSkeleton />}>
          <DocumentViewer key={activeId} documentId={activeId} />
        </QueryBoundary>
      ) : null}
    </div>
  )
}

function DocumentsSkeleton() {
  return (
    <div className="grid items-start gap-6 lg:grid-cols-[18rem_minmax(0,1fr)]">
      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <Skeleton className="h-3 w-20" />
          <Skeleton className="h-7 w-24" />
        </div>
        <DocumentListSkeleton />
      </div>
      <DocumentViewerSkeleton />
    </div>
  )
}
