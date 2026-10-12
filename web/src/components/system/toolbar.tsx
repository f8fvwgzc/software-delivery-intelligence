import type { ComponentProps } from 'react'

import { Separator } from '@/components/ui/separator'
import { cn } from '@/lib/utils'

/**
 * A 32px strip of ghost controls: filters, toggles, a search field, counts.
 * Nothing in it is filled; the primary action of a page lives in its header.
 */
export function Toolbar({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="toolbar"
      role="toolbar"
      className={cn('flex min-h-8 min-w-0 flex-wrap items-center gap-1.5', className)}
      {...props}
    />
  )
}

/** Controls that belong together; groups separate with a hairline. */
export function ToolbarGroup({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="toolbar-group" className={cn('flex items-center gap-1', className)} {...props} />
}

export function ToolbarSeparator({ className }: { className?: string }) {
  return <Separator orientation="vertical" className={cn('mx-1 h-4!', className)} />
}

/** Pushes what follows to the right end. */
export function ToolbarSpacer() {
  return <span className="flex-1" aria-hidden />
}

/** 12px muted count or hint inside the toolbar. */
export function ToolbarText({ className, ...props }: ComponentProps<'span'>) {
  return <span className={cn('px-1 text-xs tabular-nums text-muted-foreground', className)} {...props} />
}
