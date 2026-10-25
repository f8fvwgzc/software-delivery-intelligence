import { useQuery, useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Panel, PanelSkeleton } from '@/components/system/panel'
import { Skeleton } from '@/components/ui/skeleton'
import { useAuth } from '@/features/auth/lib/auth-context'

import { diffKeys, gitopsKeys } from '../api/keys'
import { gitopsQueries } from '../api/queries'
import type { Application } from '../api/types'
import { ApplicationActions } from '../components/application-actions'
import { ApplicationDetails } from '../components/application-details'
import { ApplicationHeader } from '../components/application-header'
import { ConditionsList } from '../components/conditions-list'
import { HistoryList } from '../components/history-list'
import { ManifestsFrame } from '../components/manifests-frame'
import { OperationNotice, type RequestedOperation } from '../components/operation-notice'
import { ResourcesFrame } from '../components/resources-frame'
import { isOperationRunning } from '../lib/labels'
import { sourceRepoUrl } from '../lib/repo'

const GRID_CLASS = 'grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem] xl:grid-cols-[minmax(0,1fr)_26rem]'
/**
 * The aside stays on screen while the resources scroll. On a short window it
 * scrolls by itself, so the history is always reachable; the 1px padding
 * keeps the frames' hairlines from being clipped.
 */
const ASIDE_CLASS =
  'flex min-w-0 flex-col gap-4 lg:sticky lg:top-[calc(var(--header-h)+1rem)] lg:-m-px lg:max-h-[calc(100svh-var(--header-h)-2rem)] lg:overflow-y-auto lg:p-px'

/**
 * /applications/:applicationId — one GitOps application (CONTRACT.md §24.8).
 * Two requests on first paint: the application with its resources, and its
 * history behind its own boundary. The diff is read when a resource's diff
 * is opened, the manifests when they are asked for.
 */
export default function ApplicationPage() {
  const { applicationId = '' } = useParams()

  return (
    <QueryBoundary fallback={<ApplicationPageSkeleton />}>
      <ApplicationView key={applicationId} applicationId={applicationId} />
    </QueryBoundary>
  )
}

/**
 * When an operation ends, what it changed is read again: the history gains
 * an entry and the diff moves. The live stream does this too; this covers
 * the fallback polling, which only watches the application itself.
 */
function useOperationEnded(application: Application, onEnded: () => void) {
  const running = isOperationRunning(application.operation)
  const wasRunning = useRef(running)
  useEffect(() => {
    if (wasRunning.current && !running) onEnded()
    wasRunning.current = running
  }, [running, onEnded])
}

function ApplicationView({ applicationId }: { applicationId: string }) {
  const queryClient = useQueryClient()
  const { currentWorkspace } = useAuth()
  const { data: application } = useSuspenseQuery(gitopsQueries.application(applicationId))
  const { data: project } = useQuery(gitopsQueries.project(application.projectId))
  const [requested, setRequested] = useState<RequestedOperation | null>(null)
  const repoUrl = sourceRepoUrl(application.source, project, currentWorkspace.githubHost)

  const onEnded = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: gitopsKeys.history(applicationId) })
    queryClient.invalidateQueries({ queryKey: diffKeys.application(applicationId) })
  }, [queryClient, applicationId])
  useOperationEnded(application, onEnded)

  return (
    <Page>
      <ApplicationHeader application={application} actions={<ApplicationActions application={application} onRequested={setRequested} />} />
      <ConditionsList conditions={application.conditions} />
      <OperationNotice application={application} requested={requested} />

      <div className={GRID_CLASS}>
        <div className="flex min-w-0 flex-col gap-4">
          <ResourcesFrame application={application} />
          <ManifestsFrame applicationId={application.id} repoUrl={repoUrl} />
        </div>

        <div className={ASIDE_CLASS}>
          <Frame title="Details">
            <ApplicationDetails application={application} project={project} repoUrl={repoUrl} />
          </Frame>
          <Panel title="History" description="Syncs that were not dry runs, newest first." padding="none" skeletonRows={3}>
            <HistoryList application={application} repoUrl={repoUrl} onRequested={setRequested} />
          </Panel>
        </div>
      </div>
    </Page>
  )
}

function ApplicationPageSkeleton() {
  return (
    <Page>
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Skeleton className="h-5 w-16 rounded-[4px]" />
          <Skeleton className="h-5 w-16 rounded-[4px]" />
        </div>
        <Skeleton className="h-5 w-56 max-w-full" />
        <Skeleton className="h-3 w-96 max-w-full" />
      </div>
      <div className={GRID_CLASS}>
        <div className="flex flex-col gap-4">
          <Frame title="Resources" padding="none">
            <PanelSkeleton rows={5} />
          </Frame>
          <Frame title="Manifests" padding="none">
            <PanelSkeleton rows={1} />
          </Frame>
        </div>
        <div className="flex flex-col gap-4">
          <Frame title="Details">
            <PanelSkeleton rows={4} />
          </Frame>
          <Frame title="History" padding="none">
            <PanelSkeleton rows={3} />
          </Frame>
        </div>
      </div>
    </Page>
  )
}
