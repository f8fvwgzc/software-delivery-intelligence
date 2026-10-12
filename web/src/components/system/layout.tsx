import type { ComponentProps } from 'react'

import { cn } from '@/lib/utils'

type Gap = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 8

const GAP_CLASS: Record<Gap, string> = {
  0: 'gap-0',
  1: 'gap-1',
  2: 'gap-2',
  3: 'gap-3',
  4: 'gap-4',
  5: 'gap-5',
  6: 'gap-6',
  8: 'gap-8',
}

interface StackProps extends ComponentProps<'div'> {
  /** Spacing step on the 4px grid. */
  gap?: Gap
}

/** Vertical flex column. The default gap (4 = 16px) is the page rhythm. */
export function Stack({ gap = 4, className, ...props }: StackProps) {
  return <div data-slot="stack" className={cn('flex min-w-0 flex-col', GAP_CLASS[gap], className)} {...props} />
}

interface InlineProps extends ComponentProps<'div'> {
  gap?: Gap
  align?: 'start' | 'center' | 'baseline' | 'end'
  justify?: 'start' | 'between' | 'end'
  wrap?: boolean
}

const ALIGN_CLASS = { start: 'items-start', center: 'items-center', baseline: 'items-baseline', end: 'items-end' }
const JUSTIFY_CLASS = { start: 'justify-start', between: 'justify-between', end: 'justify-end' }

/** Horizontal flex row, centred by default. */
export function Inline({ gap = 2, align = 'center', justify = 'start', wrap = false, className, ...props }: InlineProps) {
  return (
    <div
      data-slot="inline"
      className={cn('flex min-w-0', GAP_CLASS[gap], ALIGN_CLASS[align], JUSTIFY_CLASS[justify], wrap && 'flex-wrap', className)}
      {...props}
    />
  )
}
