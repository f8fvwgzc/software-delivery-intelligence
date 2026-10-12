import type React from 'react'
import { Outlet, useLocation } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { PageTransition } from '@/components/system/page-transition'
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar'

import { AppHeader } from './app-header'
import { AppSidebar } from './app-sidebar'

/**
 * Persistent frame: sidebar on the left, header on top, page below.
 * The <QueryBoundary> around <Outlet /> is the Suspense boundary that shows
 * while a lazy route chunk loads, and the error boundary for page errors.
 * The transition keys on the pathname, so every navigation fades the page in.
 */
export function AppShell() {
  const { pathname } = useLocation()

  return (
    <SidebarProvider style={{ '--sidebar-width': '13.5rem' } as React.CSSProperties}>
      <AppSidebar />
      <SidebarInset className="min-w-0">
        <AppHeader />
        <PageTransition routeKey={pathname}>
          <QueryBoundary>
            <Outlet />
          </QueryBoundary>
        </PageTransition>
      </SidebarInset>
    </SidebarProvider>
  )
}
