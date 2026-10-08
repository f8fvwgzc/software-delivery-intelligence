import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar'

export function Brand() {
  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton asChild size="lg" className="hover:bg-transparent active:bg-transparent">
          <Link to={routes.overview()}>
            <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-brand text-brand-foreground">
              <svg viewBox="0 0 32 32" fill="none" className="size-4" aria-hidden>
                <circle cx="10" cy="11" r="2.5" fill="currentColor" />
                <circle cx="22" cy="11" r="2.5" fill="currentColor" fillOpacity=".7" />
                <circle cx="16" cy="22" r="2.5" fill="currentColor" />
                <path
                  d="M10 11 16 22 22 11"
                  stroke="currentColor"
                  strokeOpacity=".55"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </span>
            <span className="flex min-w-0 flex-col leading-tight">
              <span className="truncate text-sm font-semibold">Delivery Intelligence</span>
              <span className="truncate text-2xs text-muted-foreground">Observe · Propose · Execute</span>
            </span>
          </Link>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
