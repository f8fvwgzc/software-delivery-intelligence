import { Suspense } from 'react'
import { Link, useLocation } from 'react-router'

import { routes } from '@/app/router/routes'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'

import { ApprovalsBadge } from './approvals-badge'
import { navigation, type NavItem } from './navigation'
import { SidebarStatus } from './sidebar-status'

function isActive(item: NavItem, pathname: string) {
  if (item.to === '/') return pathname === '/'
  return item.prefix ? pathname.startsWith(item.to) : pathname === item.to
}

export function AppSidebar() {
  const { pathname } = useLocation()

  return (
    <Sidebar collapsible="none" className="sticky top-0 h-svh shrink-0 self-start">
      <SidebarContent className="gap-0">
        {navigation.map((group) => (
          <SidebarGroup key={group.label} className="py-1.5">
            <SidebarGroupLabel className="text-2xs tracking-wide uppercase">{group.label}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu className="gap-1">
                {group.items.map((item) => (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton asChild isActive={isActive(item, pathname)} tooltip={item.label}>
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
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>

      <SidebarFooter>
        <Suspense fallback={null}>
          <SidebarStatus />
        </Suspense>
      </SidebarFooter>
    </Sidebar>
  )
}
