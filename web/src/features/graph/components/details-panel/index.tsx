import { XIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { useIsMobile } from '@/hooks/use-mobile'

interface DetailsPanelProps {
  open: boolean
  onClose: () => void
  title: string
  /** Show the close control (desktop); off when the panel is the board's summary. */
  closable?: boolean
  children: ReactNode
}

/**
 * Right-hand details: a sheet on phones, a fixed aside beside the canvas
 * otherwise. The body is a native scroller on purpose: Radix ScrollArea lays
 * its content out as a table, which lets truncated text widen the panel.
 */
export function DetailsPanel({ open, onClose, title, closable = true, children }: DetailsPanelProps) {
  const isMobile = useIsMobile()

  if (isMobile) {
    return (
      <Sheet
        open={open}
        onOpenChange={(next) => {
          if (!next) onClose()
        }}
      >
        <SheetContent side="right" className="gap-0 p-0 sm:max-w-md">
          <SheetHeader className="h-12 shrink-0 justify-center border-b pr-12">
            <SheetTitle className="text-sm">{title}</SheetTitle>
            <SheetDescription className="sr-only">Attributes and impact analysis for the selected node.</SheetDescription>
          </SheetHeader>
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4">{children}</div>
        </SheetContent>
      </Sheet>
    )
  }

  if (!open) return null

  return (
    <aside aria-label={title} className="flex w-80 shrink-0 flex-col overflow-hidden rounded-lg bg-card ring-1 ring-border">
      <div className="flex h-10 shrink-0 items-center justify-between gap-2 border-b pr-2 pl-4">
        <h2 className="text-sm font-medium">{title}</h2>
        {closable ? (
          <Button variant="ghost" size="icon-sm" aria-label="Close details" onClick={onClose}>
            <XIcon />
          </Button>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4 [scrollbar-gutter:stable]">{children}</div>
    </aside>
  )
}
