import { useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { BotIcon, HistoryIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'

import { activityQueries } from '../../api/queries'
import type { ReviewActivity } from '../../api/types'
import { activityMeta, actorAvatarUrl } from '../../lib/labels'

interface ActivityFeedProps {
  projectId?: string
}

/** The review loop as a timeline: when, who, what, on which PR. */
export function ActivityFeed({ projectId }: ActivityFeedProps) {
  const query = useSuspenseInfiniteQuery(activityQueries.list(projectId ? { projectId } : {}))
  const entries = query.data.pages.flat()

  if (entries.length === 0) {
    return (
      <PanelEmpty
        icon={HistoryIcon}
        title="Nothing has happened yet"
        description="Starting a review, deciding a finding and publishing all leave a line here."
      />
    )
  }

  return (
    <>
      <ol className="relative flex flex-col py-1 before:absolute before:top-4 before:bottom-4 before:left-[27px] before:w-px before:bg-border" aria-label="Review activity">
        {entries.map((entry) => (
          <ActivityRow key={entry.id} entry={entry} />
        ))}
      </ol>
      {query.hasNextPage ? (
        <FrameFooter className="justify-center">
          <Button variant="ghost" size="xs" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
            Load earlier
          </Button>
        </FrameFooter>
      ) : null}
    </>
  )
}

function ActivityRow({ entry }: { entry: ReviewActivity }) {
  const action = activityMeta(entry.action)
  const avatar = actorAvatarUrl(entry.actor)

  return (
    <li className="relative flex items-start gap-3 px-3 py-2">
      <span className="w-14 shrink-0 pt-0.5 text-right font-mono text-2xs tabular-nums text-muted-foreground">
        <RelativeTime value={entry.at} />
      </span>
      <Avatar size="sm" className="z-10 size-5 shrink-0 ring-2 ring-background" title={entry.actor}>
        <AvatarImage src={avatar} alt="" />
        <AvatarFallback className="text-[9px]">
          {avatar ? entry.actor.slice(0, 2).toUpperCase() : <BotIcon className="size-3" />}
        </AvatarFallback>
      </Avatar>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm leading-5">
          <span className="font-medium">{entry.actor || 'system'}</span>
          <Tag tone={action.tone}>{action.label}</Tag>
          <Link to={routes.review(entry.reviewId)} className="inline-flex min-w-0 items-baseline gap-1 text-muted-foreground hover:text-foreground hover:underline">
            <span className="font-mono text-xs tabular-nums">#{entry.prNumber}</span>
            <span className="truncate text-xs">{entry.projectName}</span>
          </Link>
        </div>
        {entry.detail ? <p className="text-xs leading-4 text-muted-foreground">{entry.detail}</p> : null}
      </div>
    </li>
  )
}
