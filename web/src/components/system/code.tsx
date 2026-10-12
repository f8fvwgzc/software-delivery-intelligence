import type { ComponentProps } from 'react'

import { cn } from '@/lib/utils'

interface CodeProps extends ComponentProps<'pre'> {
  /** Wrap long lines instead of scrolling. */
  wrap?: boolean
  /** Tighter block for a single line. */
  size?: 'sm' | 'md'
}

/** A mono block: 12px, muted wash, hairline, horizontal scroll. */
export function Code({ wrap = false, size = 'md', className, children, ...props }: CodeProps) {
  return (
    <pre
      data-slot="code"
      className={cn(
        'min-w-0 rounded-md bg-muted/60 font-mono text-xs leading-5 text-foreground/90 ring-1 ring-border ring-inset',
        size === 'md' ? 'px-3 py-2' : 'px-2 py-1',
        wrap ? 'break-words whitespace-pre-wrap' : 'overflow-x-auto',
        className,
      )}
      {...props}
    >
      <code>{children}</code>
    </pre>
  )
}

/** Inline mono token: a path, a sha, a key. */
export function InlineCode({ className, ...props }: ComponentProps<'code'>) {
  return <code className={cn('rounded-[4px] bg-muted px-1 py-px font-mono text-[0.92em]', className)} {...props} />
}
