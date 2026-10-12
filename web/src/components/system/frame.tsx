import { useId, type ComponentProps, type ReactNode } from 'react'

import { cn } from '@/lib/utils'

export interface FrameProps extends Omit<ComponentProps<'section'>, 'title'> {
  /** 12px header title. Rendered as an `h2` unless `as` says otherwise. */
  title?: ReactNode
  /** One muted sentence next to the title; truncates rather than wraps. */
  description?: ReactNode
  /** Tags and counts, right after the title. */
  meta?: ReactNode
  /** Ghost controls, right-aligned. */
  actions?: ReactNode
  /** Heading level of the title; sub-frames use `h3`. */
  as?: 'h2' | 'h3'
  /** Body padding: `none` for tables and lists that draw their own rows. */
  padding?: 'none' | 'sm' | 'md'
  /** Drops the outer hairline: a bare header row over content. */
  bare?: boolean
  /** Clips the body (rounded tables); off when something inside must stick. */
  clip?: boolean
  children?: ReactNode
}

const PADDING_CLASS = { none: '', sm: 'p-2', md: 'p-3' }

/**
 * The structural unit of every page: a hairline-bordered section with a
 * 32px header row (title, meta tags, actions) and a body. Frames nest; a
 * sub-frame keeps the same header but a smaller radius and no outer ring
 * of its own beyond a top hairline.
 */
export function Frame({
  title,
  description,
  meta,
  actions,
  as: Heading = 'h2',
  padding = 'md',
  bare = false,
  clip = true,
  className,
  children,
  ...props
}: FrameProps) {
  const id = useId()
  const hasHeader = title || meta || actions || description

  return (
    <section
      data-slot="frame"
      aria-labelledby={title ? id : undefined}
      className={cn(
        'group/frame flex min-w-0 flex-col bg-background',
        !bare && 'rounded-lg ring-1 ring-border',
        clip && !bare && 'overflow-hidden',
        className,
      )}
      {...props}
    >
      {hasHeader ? (
        <FrameHeader>
          {title ? (
            <Heading id={id} className="shrink-0 text-xs font-medium text-foreground">
              {title}
            </Heading>
          ) : null}
          {description ? (
            <p className="hidden min-w-0 truncate text-xs text-muted-foreground sm:block" title={typeof description === 'string' ? description : undefined}>
              {description}
            </p>
          ) : null}
          {meta ? <div className="flex shrink-0 items-center gap-1.5">{meta}</div> : null}
          {actions ? <div className="ml-auto flex shrink-0 items-center gap-1">{actions}</div> : null}
        </FrameHeader>
      ) : null}
      <div data-slot="frame-body" className={cn('flex min-w-0 flex-1 flex-col', PADDING_CLASS[padding])}>
        {children}
      </div>
    </section>
  )
}

/** The 32px header row on its own, for frames that build a custom one. */
export function FrameHeader({ className, ...props }: ComponentProps<'header'>) {
  return (
    <header
      data-slot="frame-header"
      className={cn(
        'flex min-h-8 shrink-0 items-center gap-2 border-b px-3 py-1 group-[.is-bare]/frame:px-0',
        className,
      )}
      {...props}
    />
  )
}

/** A hairline between two bodies inside one frame. */
export function FrameDivider({ className, ...props }: ComponentProps<'hr'>) {
  return <hr data-slot="frame-divider" className={cn('border-0 border-t', className)} {...props} />
}

/** One muted line at the bottom of a frame: counts, sync times. */
export function FrameFooter({ className, ...props }: ComponentProps<'footer'>) {
  return (
    <footer
      data-slot="frame-footer"
      className={cn('flex min-h-8 shrink-0 items-center gap-2 border-t px-3 py-1 text-2xs text-muted-foreground', className)}
      {...props}
    />
  )
}
