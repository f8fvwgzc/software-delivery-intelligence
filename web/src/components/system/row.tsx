import type { ComponentProps, ReactNode } from 'react'
import { Link } from 'react-router'

import { cn } from '@/lib/utils'

export interface RowProps {
  /** Makes the row a link. */
  to?: string
  /** Makes the row a button. */
  onClick?: () => void
  /** Fires on hover and focus, before the click: prefetch the destination here. */
  onIntent?: () => void
  /** Icon, avatar or status dot, 16px. */
  leading?: ReactNode
  title: ReactNode
  /** Second line, 12px muted; nodes are laid out inline with 8px gaps. */
  meta?: ReactNode
  /** Right-aligned: a time, a count, a tag, a small button. */
  trailing?: ReactNode
  /** Highlighted row (selected, current). */
  active?: boolean
  /** 36px single line (`sm`) or 44px two lines (`md`). */
  size?: 'sm' | 'md'
  /** Title in mono (paths, keys). */
  mono?: boolean
  className?: string
  'aria-label'?: string
  'data-testid'?: string
}

const SIZE_CLASS = { sm: 'min-h-9 py-1.5', md: 'min-h-11 py-2' }

/**
 * One list row: leading glyph, title and meta in the middle, trailing at
 * the right. Interactive when `to` or `onClick` is set; the hover is a
 * faint muted wash, the active row is tinted with the brand at 5%.
 */
export function Row({ to, onClick, onIntent, leading, title, meta, trailing, active = false, size = 'md', mono = false, className, ...rest }: RowProps) {
  const interactive = Boolean(to || onClick)
  const body = (
    <>
      {leading ? <span className="flex size-4 shrink-0 items-center justify-center [&>svg]:size-4">{leading}</span> : null}
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className={cn('truncate text-sm leading-5', mono && 'font-mono text-xs')}>{title}</span>
        {meta ? <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-xs leading-4 text-muted-foreground">{meta}</span> : null}
      </span>
      {trailing ? <span className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">{trailing}</span> : null}
    </>
  )
  const classes = cn(
    'flex w-full items-center gap-3 px-3 text-left outline-none',
    SIZE_CLASS[size],
    interactive && 'hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
    active && 'bg-brand/5 hover:bg-brand/8',
    className,
  )

  if (to) {
    return (
      <Link
        data-slot="row"
        to={to}
        className={classes}
        aria-current={active ? 'true' : undefined}
        onMouseEnter={onIntent}
        onFocus={onIntent}
        {...rest}
      >
        {body}
      </Link>
    )
  }
  if (onClick) {
    return (
      <button
        data-slot="row"
        type="button"
        onClick={onClick}
        onMouseEnter={onIntent}
        onFocus={onIntent}
        className={classes}
        aria-pressed={active || undefined}
        {...rest}
      >
        {body}
      </button>
    )
  }
  return (
    <div data-slot="row" className={classes} {...rest}>
      {body}
    </div>
  )
}

interface RowListProps extends ComponentProps<'ul'> {
  /** Hairline between rows (default) or none. */
  divided?: boolean
  /** Fade rows in with an 80ms stagger when the list first appears (CSS only). */
  stagger?: boolean
}

/** The `ul` rows live in; wrap each `Row` in an `li`. */
export function RowList({ divided = true, stagger = false, className, ...props }: RowListProps) {
  return (
    <ul
      data-slot="row-list"
      className={cn('flex min-w-0 flex-col', divided && 'divide-y divide-border', stagger && 'row-list-stagger', className)}
      {...props}
    />
  )
}
