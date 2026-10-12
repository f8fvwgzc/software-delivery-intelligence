import type { ComponentProps, ReactNode } from 'react'

import { cn } from '@/lib/utils'

export type TagTone = 'neutral' | 'info' | 'success' | 'warning' | 'destructive' | 'brand'

/** Tint per tone: a faint fill, a hairline of the same hue, readable text. */
const TONE_CLASS: Record<TagTone, string> = {
  neutral: 'bg-muted text-muted-foreground ring-border',
  info: 'bg-info/8 text-info ring-info/15',
  success: 'bg-success/8 text-success ring-success/15',
  warning: 'bg-warning/10 text-warning ring-warning/20',
  destructive: 'bg-destructive/8 text-destructive ring-destructive/15',
  brand: 'bg-brand/8 text-brand ring-brand/15',
}

/** 6px dot fill per tone; the dot and the tag read the same. */
export const TAG_DOT_CLASS: Record<TagTone, string> = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

export interface TagProps extends Omit<ComponentProps<'span'>, 'children'> {
  tone?: TagTone
  children: ReactNode
  /** Leading 6px status dot. */
  dot?: boolean
  /** Pulses the dot for in-flight states. */
  live?: boolean
  /** Mono text for keys, rules and action types. */
  mono?: boolean
  /** Outline only, no fill: for counts and secondary labels. */
  variant?: 'tint' | 'outline'
}

/**
 * The small label of the app: 11px, 4px radius, 20px tall, tinted by tone.
 * Every lifecycle state, rule id and count is one of these. Never a pill.
 */
export function Tag({ tone = 'neutral', dot = false, live = false, mono = false, variant = 'tint', className, children, ...props }: TagProps) {
  return (
    <span
      data-slot="tag"
      data-tone={tone}
      className={cn(
        'inline-flex h-5 max-w-full shrink-0 items-center gap-1.5 rounded-[4px] px-1.5 text-2xs font-medium whitespace-nowrap ring-1 ring-inset',
        variant === 'tint' ? TONE_CLASS[tone] : 'bg-transparent text-muted-foreground ring-border',
        mono && 'font-mono font-normal',
        className,
      )}
      {...props}
    >
      {dot ? <TagDot tone={tone} live={live} /> : null}
      <span className="truncate">{children}</span>
    </span>
  )
}

/** The 6px dot on its own, with the optional pulse. */
export function TagDot({ tone, live = false, className }: { tone: TagTone; live?: boolean; className?: string }) {
  return (
    <span className={cn('relative flex size-1.5 shrink-0', className)} aria-hidden>
      {live ? (
        <span className={cn('absolute inline-flex size-full rounded-full opacity-60 motion-safe:animate-ping', TAG_DOT_CLASS[tone])} />
      ) : null}
      <span className={cn('relative inline-flex size-1.5 rounded-full', TAG_DOT_CLASS[tone])} />
    </span>
  )
}
