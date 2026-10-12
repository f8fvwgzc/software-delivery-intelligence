import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface KeyValueProps {
  items: { label: ReactNode; value: ReactNode }[]
  /** Two columns on wide screens. */
  columns?: 1 | 2
  className?: string
}

/** Compact definition list: 12px labels, hairlines between rows. */
export function KeyValue({ items, columns = 1, className }: KeyValueProps) {
  return (
    <dl className={cn('grid gap-x-6 text-sm', columns === 2 && 'sm:grid-cols-2', className)}>
      {items.map((item, index) => (
        <div key={index} className="flex min-h-8 items-center justify-between gap-4 border-b border-border py-1.5 last:border-0">
          <dt className="shrink-0 text-xs text-muted-foreground">{item.label}</dt>
          <dd className="min-w-0 truncate text-right text-xs">{item.value}</dd>
        </div>
      ))}
    </dl>
  )
}
