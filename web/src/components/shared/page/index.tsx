import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface PageProps {
  children: ReactNode
  className?: string
}

/**
 * Standard page frame: full width, consistent gutters (24px, 16px on phones)
 * and vertical rhythm. Long-form readers cap their own text column; the
 * frame never does.
 */
export function Page({ children, className }: PageProps) {
  return (
    <div className={cn('flex w-full min-w-0 flex-1 flex-col gap-6 px-4 py-5 md:px-6 md:py-6', className)}>
      {children}
    </div>
  )
}

interface PageHeaderProps {
  title: ReactNode
  description?: ReactNode
  /** Rendered above the title, e.g. a status badge row. */
  eyebrow?: ReactNode
  /** Right-aligned actions. */
  actions?: ReactNode
  className?: string
}

export function PageHeader({ title, description, eyebrow, actions, className }: PageHeaderProps) {
  return (
    <div className={cn('flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between', className)}>
      <div className="flex min-w-0 flex-col gap-1">
        {eyebrow && <div className="flex flex-wrap items-center gap-2">{eyebrow}</div>}
        <h1 className="truncate text-lg font-semibold leading-7">{title}</h1>
        {description && <p className="max-w-[72ch] text-sm text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  )
}

interface SectionProps {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
}

/** A titled block inside a page. */
export function Section({ title, description, actions, children, className }: SectionProps) {
  return (
    <section className={cn('flex min-w-0 flex-col gap-3', className)}>
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <h2 className="text-sm font-medium">{title}</h2>
          {description && <p className="max-w-[72ch] text-xs text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  )
}
