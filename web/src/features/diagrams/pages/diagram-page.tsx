import { useSuspenseQuery } from '@tanstack/react-query'
import { AlertCircleIcon, ShapesIcon } from 'lucide-react'
import { lazy, useRef, useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { cn } from '@/lib/utils'

import { useGenerateDiagram, useUpdateDiagram } from '../api/mutations'
import { diagramQuery } from '../api/queries'
import { CANVAS_HEIGHT_CLASS, CanvasSkeleton } from '../components/canvas-skeleton'
import { DiagramHeader, DiagramHeaderSkeleton } from '../components/diagram-header'
import type { CanvasHandle } from '../components/editorial-canvas/types'
import { ExplanationPanel } from '../components/explanation-panel'
import { MermaidSkeleton } from '../components/mermaid-skeleton'
import { codeBrowserPath } from '../lib/code-path'
import { resolveDirection } from '../lib/direction'

// The renderer (dagre) and the Mermaid editor are separate chunks. The
// renderer starts loading as soon as this page evaluates, in parallel with
// the diagram query; Mermaid only when its tab is hovered or opened.
const loadCanvas = () => import('../components/editorial-canvas')
const EditorialCanvas = lazy(loadCanvas)
void loadCanvas()
const loadMermaid = () => import('../components/mermaid-editor')
const MermaidEditor = lazy(loadMermaid)

type DiagramTab = 'editorial' | 'mermaid' | 'explanation'

function DiagramBody({ diagramId }: { diagramId: string }) {
  const { data: diagram } = useSuspenseQuery(diagramQuery(diagramId))
  const navigate = useNavigate()
  const update = useUpdateDiagram(diagramId)
  const regenerate = useGenerateDiagram(diagram.projectId)
  const canvasRef = useRef<CanvasHandle>(null)
  const [tab, setTab] = useState<DiagramTab>('editorial')

  const spec = diagram.spec ?? null
  const hasSpec = (spec?.nodes?.length ?? 0) > 0
  const authoritative = diagram.source === 'mermaid'
  const direction = resolveDirection(spec, diagram.kind)

  const rename = (title: string) => {
    update.mutate({ title }, { onError: (error) => toast.error('Could not rename the diagram', { description: error.message }) })
  }

  const saveMermaid = (mermaid: string) => {
    update.mutate(
      { mermaid },
      {
        onSuccess: () => toast.success('Mermaid saved', { description: 'The source is now authoritative.' }),
        onError: (error) => toast.error('Could not save the Mermaid source', { description: error.message }),
      },
    )
  }

  const runRegenerate = () => {
    regenerate.mutate(
      { kind: diagram.kind, prompt: null },
      {
        onSuccess: (next) => {
          toast.success('Diagram regenerated', { description: `${next.spec?.nodes?.length ?? 0} nodes at ${next.commitSha.slice(0, 7)}.` })
          navigate(routes.diagram(next.id))
        },
        onError: (error) => toast.error('Could not regenerate the diagram', { description: error.message }),
      },
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <DiagramHeader
        diagram={diagram}
        onRename={rename}
        renaming={update.isPending && update.variables?.title !== undefined}
        onRegenerate={runRegenerate}
        regenerating={regenerate.isPending}
        getExporter={() => canvasRef.current}
        onDeleted={() => navigate(routes.projectDiagrams(diagram.projectId), { replace: true })}
      />

      <Tabs value={tab} onValueChange={(value) => setTab(value as DiagramTab)} className="gap-3">
        <TabsList variant="line" className="-mx-1.5 h-9 border-b">
          <TabsTrigger value="editorial">Editorial</TabsTrigger>
          <TabsTrigger value="mermaid" onPointerEnter={() => void loadMermaid()} onFocus={() => void loadMermaid()}>
            Mermaid
          </TabsTrigger>
          <TabsTrigger value="explanation">Explanation</TabsTrigger>
        </TabsList>

        {/* Kept mounted (hidden) so the export menu can serialise the SVG from any tab. */}
        <TabsContent value="editorial" forceMount className="flex flex-col gap-3 data-[state=inactive]:hidden">
          {authoritative ? (
            <Alert>
              <AlertCircleIcon />
              <AlertTitle>Showing the last generated spec</AlertTitle>
              <AlertDescription>
                The Mermaid source is authoritative for this diagram, so this view may be stale. Regenerate to rebuild it.
              </AlertDescription>
            </Alert>
          ) : null}
          <div className={cn('relative overflow-hidden rounded-lg bg-card ring-1 ring-border', CANVAS_HEIGHT_CLASS)}>
            {hasSpec && spec ? (
              // Its own boundary: a layout failure explains itself here and leaves the header and other tabs usable.
              <QueryBoundary fallback={<CanvasSkeleton className="h-full rounded-none ring-0" />}>
                <EditorialCanvas
                  ref={canvasRef}
                  spec={spec}
                  direction={direction}
                  title={diagram.title}
                  onOpenPath={(path) => navigate(codeBrowserPath(diagram.projectId, path))}
                />
              </QueryBoundary>
            ) : (
              <EmptyState
                icon={ShapesIcon}
                title="Nothing to draw"
                description={
                  authoritative
                    ? 'This diagram only has Mermaid source. Open the Mermaid tab to see and edit it.'
                    : 'The generator found no nodes for this kind. Regenerate after a fresh scan, or try another kind.'
                }
                className="h-full border-0"
              />
            )}
          </div>
        </TabsContent>

        <TabsContent value="mermaid">
          <QueryBoundary fallback={<MermaidSkeleton />}>
            <MermaidEditor
              key={`${diagram.id}-${diagram.revision}`}
              source={diagram.mermaid ?? ''}
              authoritative={authoritative}
              onSave={saveMermaid}
              saving={update.isPending && update.variables?.mermaid !== undefined}
            />
          </QueryBoundary>
        </TabsContent>

        <TabsContent value="explanation">
          <ExplanationPanel diagram={diagram} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function DiagramSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <DiagramHeaderSkeleton />
      <div className="flex h-9 items-center gap-4 border-b">
        {['w-16', 'w-16', 'w-20'].map((width) => (
          <div key={width} className={cn('h-3.5 animate-pulse rounded-md bg-muted', width)} />
        ))}
      </div>
      <CanvasSkeleton />
    </div>
  )
}

/** /diagrams/:diagramId — header, then Editorial | Mermaid | Explanation. */
export default function DiagramPage() {
  const { diagramId = '' } = useParams()

  return (
    <Page className="gap-4">
      <QueryBoundary fallback={<DiagramSkeleton />}>
        <DiagramBody diagramId={diagramId} />
      </QueryBoundary>
    </Page>
  )
}
