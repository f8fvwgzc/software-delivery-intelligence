import type React from 'react'
import { Outlet } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar'

import { AppHeader } from './app-header'
import { AppSidebar } from './app-sidebar'

/**
 * Persistent frame: sidebar on the left, header on top, page below.
 * The <QueryBoundary> around <Outlet /> is the Suspense boundary that shows
 * while a lazy route chunk loads, and the error boundary for page errors.
 */
export function AppShell() {
  return (
    <SidebarProvider style={{ "--sidebar-width": "14.4rem" } as React.CSSProperties}>
      <AppSidebar />
      <SidebarInset className="min-w-0">
        <AppHeader />
        <QueryBoundary>
          <Outlet />
        </QueryBoundary>
      </SidebarInset>
    </SidebarProvider>
  )
}
