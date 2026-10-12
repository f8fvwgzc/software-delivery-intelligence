import type { ReactNode } from 'react'

import { Frame } from '@/components/system/frame'
import { cn } from '@/lib/utils'

interface PageProps {
  children: ReactNode
  className?: string
}

/**
 * Standard page frame: full width, consistent gutters (24px, 16px on phones)
 * and a 16px rhythm between frames. Long-form readers cap their own text
 * column; the frame never does.
 */
export function Page({ children, className }: PageProps) {
  return (
    <div className={cn('flex w-full min-w-0 flex-1 flex-col gap-4 px-4 py-4 md:px-6 md:py-5', className)}>
      {children}
    </div>
  )
}

interface PageHeaderProps {
  title: ReactNode
  description?: ReactNode
  /** Rendered above the title, e.g. a status tag row. */
  eyebrow?: ReactNode
  /** Right-aligned actions. */
  actions?: ReactNode
  className?: string
}

/** The page's one `h1` (15px/600) with a muted 12px line and the primary action at the right. */
export function PageHeader({ title, description, eyebrow, actions, className }: PageHeaderProps) {
  return (
    <div className={cn('flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between', className)}>
      <div className="flex min-w-0 flex-col gap-1">
        {eyebrow && <div className="flex flex-wrap items-center gap-2">{eyebrow}</div>}
        <h1 className="truncate text-[15px] leading-6 font-semibold">{title}</h1>
        {description && <p className="max-w-[80ch] text-xs leading-4 text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  )
}

interface SectionProps {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  /** Tags and counts next to the title. */
  meta?: ReactNode
  /** Body padding; `none` for tables and row lists. */
  padding?: 'none' | 'sm' | 'md'
  children: ReactNode
  className?: string
}

/** A titled region of a page. Renders the system `Frame`; new code imports Frame directly. */
export function Section({ title, description, actions, meta, padding = 'md', children, className }: SectionProps) {
  return (
    <Frame title={title} description={description} actions={actions} meta={meta} padding={padding} className={className}>
      {children}
    </Frame>
  )
}
