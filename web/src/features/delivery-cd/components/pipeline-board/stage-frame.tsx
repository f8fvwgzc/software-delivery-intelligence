import { ArrowRightIcon, ExternalLinkIcon, LockIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Frame, FrameFooter } from '@/components/system/frame'
import { Tag, TagDot } from '@/components/system/tag'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import type { PipelineStage } from '../../api/types'
import { deployerOf } from '../../lib/deployers'
import { deploymentKind, hostOf, runStatus, safeHttpUrl, stageHealth } from '../../lib/labels'
import { StageMenu } from './stage-menu'
import { StageNext } from './stage-next'
import { StageRunning } from './stage-running'

const ICON_LINK_CLASS =
  'inline-flex size-6 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50'

interface StageFrameProps {
  projectId: string
  stage: PipelineStage
  /** The first stage has no connector before it, so it names its own auto promotion. */
  first: boolean
  canPromote: boolean
  canAdmin: boolean
  repoUrl?: string
}

/**
 * One environment of the pipeline. Header: its name, the lock of a
 * protected stage, its health, the link to where it answers, the menu.
 * Body: what runs, what is next, and the last run with its way into the
 * execution that carried it.
 */
export function StageFrame({ projectId, stage, first, canPromote, canAdmin, repoUrl }: StageFrameProps) {
  const { environment, lastRun } = stage
  const deployer = deployerOf(environment.kind)
  const health = stageHealth(stage.health)
  const url = safeHttpUrl(environment.url)
  const status = lastRun ? runStatus(lastRun) : null

  return (
    <Frame
      as="h3"
      padding="none"
      className="h-full min-w-0 flex-1 rounded-md"
      title={
        <span className="block max-w-40 truncate" title={environment.name}>
          {environment.name}
        </span>
      }
      meta={
        <>
          {environment.protected ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span tabIndex={0} className="inline-flex rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                  <LockIcon className="size-3 text-muted-foreground" aria-hidden />
                  <span className="sr-only">Protected</span>
                </span>
              </TooltipTrigger>
              <TooltipContent>Protected: every deployment needs an approval.</TooltipContent>
            </Tooltip>
          ) : null}
          <span className="inline-flex items-center" title={health.label}>
            <TagDot tone={health.tone} />
            <span className="sr-only">{health.label}</span>
          </span>
        </>
      }
      actions={
        <>
          {url ? (
            <a href={url} target="_blank" rel="noreferrer" className={ICON_LINK_CLASS} aria-label={`Open ${environment.name} at ${hostOf(url)}`} title={hostOf(url)}>
              <ExternalLinkIcon className="size-3" aria-hidden />
            </a>
          ) : null}
          <StageMenu projectId={projectId} stage={stage} canPromote={canPromote} canAdmin={canAdmin} />
        </>
      }
    >
      <div className="flex min-w-0 flex-wrap items-center gap-1.5 border-b px-3 py-2">
        <Tag>{deployer.tag}</Tag>
        {environment.source === 'config' ? <Tag variant="outline">managed by .sdi.yml</Tag> : null}
        {first && environment.autoPromote ? (
          <Tag variant="outline" title="Every new release is promoted here, still gated by policy">
            <span className="inline-flex items-center gap-1">
              auto
              <ArrowRightIcon className="size-3" aria-hidden />
              <span className="sr-only">promote</span>
            </span>
          </Tag>
        ) : null}
      </div>
      <StageRunning stage={stage} repoUrl={repoUrl} />
      <StageNext projectId={projectId} stage={stage} canPromote={canPromote} repoUrl={repoUrl} />
      {/* Wraps rather than overflows when a long status meets a narrow column. */}
      <FrameFooter className="flex-wrap gap-x-1.5 gap-y-1">
        {lastRun && status ? (
          <>
            {/* The way into the run: its approvals, logs and, when it is paused, the ways out. */}
            <Link
              to={routes.execution(lastRun.executionId)}
              className="rounded-[4px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              aria-label={`Last run: ${status.label}. Open its execution`}
            >
              <Tag tone={status.tone} dot live={status.live}>
                {status.label}
              </Tag>
            </Link>
            <span className="min-w-0 truncate">
              {deploymentKind(lastRun.kind).label.toLowerCase()} of <span className="font-mono">{lastRun.releaseVersion}</span>
            </span>
            <RelativeTime value={lastRun.finishedAt ?? lastRun.createdAt} className="ml-auto shrink-0 font-mono tabular-nums" />
          </>
        ) : (
          'No deployment yet.'
        )}
      </FrameFooter>
    </Frame>
  )
}
