import { Suspense } from 'react'
import { Link, useLocation } from 'react-router'

import { routes } from '@/app/router/routes'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'
import { useAuth } from '@/features/auth/lib/auth-context'
import { useIsMobile } from '@/hooks/use-mobile'
import { cn } from '@/lib/utils'

import { ApprovalsBadge } from './approvals-badge'
import { visibleNavigation, type NavItem } from './navigation'
import { SidebarStatus } from './sidebar-status'
import { WorkspaceSwitcher } from './workspace-switcher'

function isActive(item: NavItem, pathname: string) {
  if (item.to === '/') return pathname === '/'
  return item.prefix ? pathname.startsWith(item.to) : pathname === item.to
}

/** 36px rows; the active one is a brand-tinted pill, the rest are plain text. */
const ROW_CLASS =
  'h-(--sidebar-row) rounded-md px-2.5 text-sm text-sidebar-foreground/85 hover:bg-sidebar-accent/70 data-active:bg-brand/8 data-active:font-medium data-active:text-brand data-active:hover:bg-brand/10 [&_svg]:text-muted-foreground data-active:[&_svg]:text-brand'

/**
 * Permanent sidebar: the workspace switcher, then one calm list of pages
 * with 11px section labels between the groups, then the health line. Below
 * `md` it becomes an off-canvas sheet opened from the header.
 */
export function AppSidebar() {
  const { pathname } = useLocation()
  const { currentWorkspace } = useAuth()
  const isMobile = useIsMobile()
  const groups = visibleNavigation(currentWorkspace.myRole)

  return (
    <Sidebar
      collapsible={isMobile ? 'offcanvas' : 'none'}
      className="sticky top-0 h-svh shrink-0 self-start border-r border-sidebar-border"
    >
      <SidebarHeader className="h-(--header-h) justify-center border-b border-sidebar-border px-2 py-0">
        <WorkspaceSwitcher />
      </SidebarHeader>
      <SidebarContent className="gap-0 px-2 py-2">
        <nav aria-label="Main">
          <SidebarMenu className="gap-0.5">
            {groups.map((group, groupIndex) => (
              <li key={group.label} className="flex flex-col gap-0.5">
                <span
                  className={cn(
                    'flex h-6 items-center px-2.5 text-2xs font-medium tracking-wide text-muted-foreground/80 select-none',
                    groupIndex > 0 && 'mt-2',
                  )}
                  aria-hidden
                >
                  {group.label}
                </span>
                <ul className="flex flex-col gap-0.5" aria-label={group.label}>
                  {group.items.map((item) => (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton asChild isActive={isActive(item, pathname)} tooltip={item.label} className={ROW_CLASS}>
                        <Link to={item.to}>
                          <item.icon />
                          <span>{item.label}</span>
                        </Link>
                      </SidebarMenuButton>
                      {item.to === routes.approvals() && (
                        <Suspense fallback={null}>
                          <ApprovalsBadge />
                        </Suspense>
                      )}
                    </SidebarMenuItem>
                  ))}
                </ul>
              </li>
            ))}
          </SidebarMenu>
        </nav>
      </SidebarContent>

      <SidebarFooter className="border-t border-sidebar-border px-2 py-1.5">
        <Suspense fallback={null}>
          <SidebarStatus />
        </Suspense>
      </SidebarFooter>
    </Sidebar>
  )
}
