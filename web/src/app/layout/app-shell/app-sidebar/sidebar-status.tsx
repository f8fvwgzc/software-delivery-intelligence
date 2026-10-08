import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import type { StatusTone } from '@/components/shared/data'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from '@/components/ui/sidebar'
import { githubQueries } from '@/features/github/api/queries'
import { systemQueries } from '@/features/system/api/queries'
import { DOT_CLASS, overallHealth } from '@/features/system/lib/labels'
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

  const compact = !isMobile && state === 'collapsed'
  const health: { tone: StatusTone; label: string } = status.data
    ? overallHealth(status.data)
    : status.isError
      ? { tone: 'destructive', label: 'Gateway unreachable' }
      : { tone: 'neutral', label: 'Checking services' }
  const login = account.data?.tokenConfigured ? account.data.login : null

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton asChild size="sm" tooltip={login ? `${health.label} · ${login}` : health.label}>
          <Link to={routes.system()} aria-label={`${health.label}. Open system page`}>
            <span className="flex size-4 shrink-0 items-center justify-center">
              <span aria-hidden className={cn('size-1.5 rounded-full', DOT_CLASS[health.tone])} />
            </span>
            {compact ? null : (
              <>
                <span className="truncate">{health.label}</span>
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
