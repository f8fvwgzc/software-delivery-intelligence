import type { ReactNode } from 'react'

import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'

import { LiveEventsProvider } from './live-events-provider'
import { QueryProvider } from './query-provider'
import { ThemeProvider } from './theme-provider'

export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider>
      <QueryProvider>
        <LiveEventsProvider>
          <TooltipProvider delayDuration={200}>{children}</TooltipProvider>
        </LiveEventsProvider>
        <Toaster position="bottom-right" />
      </QueryProvider>
    </ThemeProvider>
  )
}
