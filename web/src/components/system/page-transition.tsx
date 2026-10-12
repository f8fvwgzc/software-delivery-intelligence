import { useEffect, useState, type ReactNode } from 'react'

type MotionLib = typeof import('./motion-lib')

let loaded: MotionLib | null = null
const loadMotion = () => import('./motion-lib').then((lib) => (loaded = lib))

const ENTER = { opacity: 0, y: 4 }
const SHOWN = { opacity: 1, y: 0 }
/** 150ms fade + 4px rise on page enter (CONTRACT.md §21). */
const TRANSITION = { duration: 0.15, ease: [0.2, 0, 0, 1] as const }

/**
 * Wraps the routed page. The motion runtime loads after the shell; the
 * first page renders in a plain `div` (no remount when the runtime lands),
 * and every navigation from then on fades the new page in. Reduced motion
 * is honoured through MotionConfig.
 */
export function PageTransition({ children, routeKey }: { children: ReactNode; routeKey: string }) {
  const [lib, setLib] = useState<MotionLib | null>(loaded)
  // The route the shell mounted on: it never animates, so the runtime landing does not remount it.
  const [firstKey] = useState(routeKey)

  useEffect(() => {
    if (lib) return
    let active = true
    void loadMotion().then((module) => {
      if (active) setLib(module)
    })
    return () => {
      active = false
    }
  }, [lib])

  if (!lib || routeKey === firstKey) {
    return (
      <div key={routeKey} className="flex min-w-0 flex-1 flex-col">
        {children}
      </div>
    )
  }

  const { LazyMotion, MotionConfig, domAnimation, m } = lib
  return (
    <LazyMotion features={domAnimation} strict>
      <MotionConfig reducedMotion="user">
        <m.div key={routeKey} initial={ENTER} animate={SHOWN} transition={TRANSITION} className="flex min-w-0 flex-1 flex-col">
          {children}
        </m.div>
      </MotionConfig>
    </LazyMotion>
  )
}
