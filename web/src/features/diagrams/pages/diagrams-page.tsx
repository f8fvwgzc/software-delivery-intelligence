import { useSuspenseQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { Page, PageHeader } from '@/components/shared/page'
import { FrameFooter } from '@/components/system/frame'
import { Panel } from '@/components/system/panel'

import { diagramsQuery } from '../api/queries'
import { DiagramsTable, DiagramsTableSkeleton } from '../components/diagrams-table'
import { GenerateForm } from '../components/generate-form'
import { count } from '@/lib/intl'

/** /projects/:projectId/diagrams — the generate form, then every diagram as a dense table. */
export default function DiagramsPage() {
  const { projectId = '' } = useParams()
  const navigate = useNavigate()

  return (
    <Page>
      <PageHeader
        title="Diagrams"
        description="Built from the knowledge graph, the repository tree and a few key files. Nodes open the code that implements them; edges name their evidence."
      />
      <GenerateForm projectId={projectId} onGenerated={(diagram) => navigate(routes.diagram(diagram.id))} />
      <Panel title="Diagrams" padding="none" skeleton={<DiagramsTableSkeleton />}>
        <DiagramsBody projectId={projectId} />
      </Panel>
    </Page>
  )
}

function DiagramsBody({ projectId }: { projectId: string }) {
  const { data: diagrams } = useSuspenseQuery(diagramsQuery(projectId))

  return (
    <>
      <DiagramsTable diagrams={diagrams} />
      <FrameFooter>
        <span className="tabular-nums" aria-live="polite">
          {count(diagrams.length, 'diagram', 'diagrams')}
        </span>
      </FrameFooter>
    </>
  )
}
