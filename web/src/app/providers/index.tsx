import type { ReactNode } from 'react'

import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'

import { QueryProvider } from './query-provider'
import { ThemeProvider } from './theme-provider'

/**
 * Everything that does not need the router. The session (`AuthProvider`)
 * and the event stream (`LiveEventsProvider`) are mounted by the router's
 * root and guarded layouts, since they navigate and depend on the session.
 */
export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider>
      <QueryProvider>
        <TooltipProvider delayDuration={200}>{children}</TooltipProvider>
        <Toaster position="bottom-right" />
      </QueryProvider>
    </ThemeProvider>
  )
}
