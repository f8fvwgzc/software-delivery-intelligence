import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { RadarIcon } from 'lucide-react'
import { useParams } from 'react-router'

import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Panel } from '@/components/system/panel'
import { Skeleton } from '@/components/ui/skeleton'
import { patternsQuery } from '@/features/patterns/api/queries'
import { projectQueries, type Project } from '@/features/projects/api/queries'
import type { components } from '@/lib/api/schema'

import { understandingQueries, type Question } from '../api/queries'
import { ConfigPanel, ConfigPanelSkeleton } from '../components/config-panel'
import { DeepDive, DeepDiveSkeleton } from '../components/deep-dive'
import { DeliveryMaturity, DeliveryMaturitySkeleton } from '../components/delivery-maturity'
import { DetectedFacts, DetectedFactsSkeleton, InferredFactsSkeleton } from '../components/detected-facts'
import { InferredFacts } from '../components/inferred-facts'
import { OpenQuestions, OpenQuestionsSkeleton } from '../components/open-questions'
import { RequestFlow } from '../components/request-flow'
import { ScanProgress, ScanProgressSkeleton } from '../components/scan-progress'
import { TldrCallout, TldrCalloutSkeleton } from '../components/tldr-callout'
import { groupFactsByCategory, isAnsweredByConfig, sortByConfidence } from '../lib/labels'

type Pattern = components['schemas']['Pattern']

/* Panels: each one owns a query and suspends inside its own boundary. */

function TldrPanel({ projectId }: { projectId: string }) {
  const { data } = useSuspenseQuery(understandingQueries.understanding(projectId))
  return <TldrCallout understanding={data} />
}

/** Stable `select`: the catalog as id → name, for the "Propose" links. */
function toPatternNames(patterns: Pattern[]) {
  return new Map(patterns.map((pattern) => [pattern.id, pattern.name]))
}

function MaturityPanel({ projectId }: { projectId: string }) {
  const { data: maturity } = useSuspenseQuery(projectQueries.maturity(projectId))
  // Names are a nicety: the section renders with ids while the catalog loads.
  const { data: patternNames } = useQuery({ ...patternsQuery(), select: toPatternNames })
  return <DeliveryMaturity maturity={maturity} patternNames={patternNames} projectId={projectId} />
}

function ConfigPanelLoader({ project }: { project: Project }) {
  const { data: config } = useSuspenseQuery(projectQueries.config(project.id))
  return <ConfigPanel config={config} project={project} />
}

function DetectedPanel({ project }: { project: Project }) {
  const { data: groups } = useSuspenseQuery({
    ...understandingQueries.facts(project.id, 'detected'),
    select: groupFactsByCategory,
  })
  return <DetectedFacts groups={groups} project={project} />
}

function InferredPanel({ projectId }: { projectId: string }) {
  const { data: facts } = useSuspenseQuery({
    ...understandingQueries.facts(projectId, 'inferred'),
    select: sortByConfidence,
  })
  return <InferredFacts facts={facts} />
}

/** Open questions need a form; answered ones (by the user or by `.sdi.yml`) are the record. */
function splitQuestions(questions: Question[]) {
  const open: Question[] = []
  const answered: Question[] = []
  for (const question of questions) {
    if (question.answeredAt || isAnsweredByConfig(question)) {
      answered.push(question)
    } else {
      open.push(question)
    }
  }
  return { open, answered }
}

function QuestionsPanel({ projectId }: { projectId: string }) {
  const { data } = useSuspenseQuery({ ...understandingQueries.questions(projectId), select: splitQuestions })
  return <OpenQuestions open={data.open} answered={data.answered} projectId={projectId} />
}

function DeepDivePanel({ projectId }: { projectId: string }) {
  const { data } = useSuspenseQuery(understandingQueries.understanding(projectId))
  return (
    <>
      <Frame title="Deep dive" description="Generated sections, one area of the system each.">
        <DeepDive understanding={data} />
      </Frame>
      <Frame title="Request flow" description="The hops a request takes through the system.">
        <RequestFlow hops={data.requestFlow} />
      </Frame>
    </>
  )
}

/* Page states */

function ReadyContent({ project }: { project: Project }) {
  return (
    <>
      <QueryBoundary fallback={<TldrCalloutSkeleton />}>
        <TldrPanel projectId={project.id} />
      </QueryBoundary>

      <div className="grid gap-4 lg:grid-cols-12 lg:items-start">
        <Panel
          title="Delivery maturity"
          description="Eight axes scored from facts only. Each finding names what would raise it."
          className="lg:col-span-7"
          skeleton={<DeliveryMaturitySkeleton />}
        >
          <MaturityPanel projectId={project.id} />
        </Panel>
        <Panel
          title=".sdi.yml"
          description="What the repository declares about how it is delivered."
          className="lg:col-span-5"
          skeleton={<ConfigPanelSkeleton />}
        >
          <ConfigPanelLoader project={project} />
        </Panel>
      </div>

      <div className="grid gap-4 lg:grid-cols-12 lg:items-start">
        <Panel
          title="What was detected"
          description="Read from the repository. Each fact links to the file that proves it."
          className="lg:col-span-7"
          skeleton={<DetectedFactsSkeleton />}
        >
          <DetectedPanel project={project} />
        </Panel>
        <Panel
          title="What was inferred"
          description="Conclusions drawn from the facts, least certain first."
          className="lg:col-span-5"
          skeleton={<InferredFactsSkeleton />}
        >
          <InferredPanel projectId={project.id} />
        </Panel>
      </div>

      <Panel
        title="Open questions"
        description="What the scan could not know. Asked once, remembered afterwards."
        skeleton={<OpenQuestionsSkeleton />}
      >
        <QuestionsPanel projectId={project.id} />
      </Panel>

      <QueryBoundary
        fallback={
          <>
            <Frame title="Deep dive" description="Generated sections, one area of the system each.">
              <DeepDiveSkeleton />
            </Frame>
            <Frame title="Request flow" description="The hops a request takes through the system.">
              <div className="flex items-center gap-2">
                {Array.from({ length: 4 }, (_, index) => (
                  <Skeleton key={index} className="h-5 w-20" />
                ))}
              </div>
            </Frame>
          </>
        }
      >
        <DeepDivePanel projectId={project.id} />
      </QueryBoundary>
    </>
  )
}

function ScanContent({ project }: { project: Project }) {
  if (!project.lastScanId) {
    return (
      <EmptyState
        icon={RadarIcon}
        title="Waiting for the first scan"
        description="The repository is queued. Stages appear here as soon as the scan starts."
      />
    )
  }

  return (
    <QueryBoundary fallback={<ScanProgressSkeleton />}>
      <ScanProgress project={project} scanId={project.lastScanId} />
    </QueryBoundary>
  )
}

export default function UnderstandingPage() {
  const { projectId = '' } = useParams()
  const { data: project } = useSuspenseQuery(projectQueries.detail(projectId))

  return (
    <Page>
      <PageHeader
        title="Understanding"
        description="What was read from the repository, what was inferred from it, and what it still needs from you."
      />
      {project.status === 'ready' ? <ReadyContent project={project} /> : <ScanContent project={project} />}
    </Page>
  )
}
