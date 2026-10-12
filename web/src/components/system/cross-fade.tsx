import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

import { useMotionLib } from './use-motion-lib'

const HIDDEN = { opacity: 0 }
const SHOWN = { opacity: 1 }
/** `--motion-base` (180 ms) on the standard easing: under the 250 ms ceiling of CONTRACT.md §21. */
const TRANSITION = { duration: 0.18, ease: [0.2, 0, 0, 1] as const }

const prefersReducedMotion = () =>
  typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

interface CrossFadeProps {
  /** Identity of what is shown; a new key fades the old content out while the new fades in. */
  swapKey: string
  children: ReactNode
  className?: string
}

/**
 * Swaps a small block of read-only content with a cross-fade: the outgoing
 * content is lifted out of the flow and fades away over the incoming one,
 * so the layout never jumps. Nothing animates on first mount, with reduced
 * motion, or before the motion runtime has landed.
 *
 * Children must be display only: the block remounts once when the runtime
 * arrives, so dialogs and inputs belong outside it.
 */
export function CrossFade({ swapKey, children, className }: CrossFadeProps) {
  const lib = useMotionLib()

  if (!lib || prefersReducedMotion()) {
    return (
      <div data-slot="cross-fade" className={className}>
        {children}
      </div>
    )
  }

  const { AnimatePresence, LazyMotion, domAnimation, m } = lib
  return (
    <LazyMotion features={domAnimation} strict>
      <div data-slot="cross-fade" className={cn('relative', className)}>
        <AnimatePresence initial={false} mode="popLayout">
          <m.div key={swapKey} initial={HIDDEN} animate={SHOWN} exit={HIDDEN} transition={TRANSITION}>
            {children}
          </m.div>
        </AnimatePresence>
      </div>
    </LazyMotion>
  )
}
