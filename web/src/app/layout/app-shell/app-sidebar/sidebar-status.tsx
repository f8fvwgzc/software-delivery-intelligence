import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import type { StatusTone } from '@/components/shared/data'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from '@/components/ui/sidebar'
import { githubQueries } from '@/features/github/api/queries'
import { systemQueries } from '@/features/system/api/queries'
import { DOT_CLASS, overallHealth } from '@/features/system/lib/labels'
import { useLiveEvents } from '@/lib/live/use-live-events'
import { cn } from '@/lib/utils'

const POLL_INTERVAL = 30_000

/**
 * Footer of the sidebar: one health dot, one line, and who the GitHub token
 * belongs to. Polls the gateway; collapses to the dot in icon mode.
 */
export function SidebarStatus() {
  const { state, isMobile } = useSidebar()
  const status = useQuery({ ...systemQueries.status(), refetchInterval: POLL_INTERVAL })
  const account = useQuery(githubQueries.account())
  const live = useLiveEvents()

  const compact = !isMobile && state === 'collapsed'
  const health: { tone: StatusTone; label: string } = status.data
    ? overallHealth(status.data)
    : status.isError
      ? { tone: 'destructive', label: 'Gateway unreachable' }
      : { tone: 'neutral', label: 'Checking services' }
  const login = account.data?.tokenConfigured ? account.data.login : null
  const liveLabel = live.connected ? 'Live updates on' : 'Live updates off, polling'
  const tooltip = [health.label, liveLabel, login].filter(Boolean).join(' · ')

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton asChild size="sm" tooltip={tooltip} className="h-8 rounded-md px-2.5 text-xs text-muted-foreground hover:text-foreground">
          <Link to={routes.system()} aria-label={`${health.label}. ${liveLabel}. Open system page`}>
            <span className="flex size-4 shrink-0 items-center justify-center">
              <span aria-hidden className={cn('size-1.5 rounded-full', DOT_CLASS[health.tone])} />
            </span>
            {compact ? null : (
              <>
                <span className="truncate">{health.label}</span>
                <span
                  aria-hidden
                  title={liveLabel}
                  className={cn(
                    'size-1.5 shrink-0 rounded-full transition-colors duration-150',
                    live.connected ? 'bg-success motion-safe:animate-pulse' : 'bg-muted-foreground/40',
                  )}
                />
                {login ? (
                  <Avatar size="sm" className="ml-auto size-5" title={login}>
                    <AvatarImage src={account.data?.avatarUrl} alt="" />
                    <AvatarFallback className="text-2xs">{login.slice(0, 2).toUpperCase()}</AvatarFallback>
                  </Avatar>
                ) : null}
              </>
            )}
          </Link>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
