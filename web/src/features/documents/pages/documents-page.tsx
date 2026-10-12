import { useSuspenseQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Skeleton } from '@/components/ui/skeleton'

import { documentsQuery } from '../api/queries'
import { DocumentList } from '../components/document-list'
import { DocumentListSkeleton } from '../components/document-list/skeleton'
import { DocumentViewer } from '../components/document-viewer'
import { DocumentViewerSkeleton } from '../components/document-viewer/skeleton'
import { GenerateMenu } from '../components/generate-menu'

const GRID_CLASS = 'grid items-start gap-4 lg:grid-cols-[18rem_minmax(0,1fr)]'

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
    <div className={GRID_CLASS}>
      <Frame
        title="Documents"
        meta={
          <span className="text-2xs tabular-nums text-muted-foreground">
            {documents.length}
          </span>
        }
        actions={menu}
        padding="none"
      >
        <DocumentList documents={documents} selectedId={activeId} onSelect={setSelectedId} emptyAction={menu} />
      </Frame>
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
    <div className={GRID_CLASS}>
      <Frame title="Documents" actions={<Skeleton className="h-6 w-20" />} padding="none">
        <DocumentListSkeleton />
      </Frame>
      <DocumentViewerSkeleton />
    </div>
  )
}
