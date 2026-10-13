import { useEffect, useState, type RefObject } from 'react'

const supported = () => typeof IntersectionObserver !== 'undefined'

/**
 * Whether the element is on screen right now (any part of it). False until
 * the observer's first report, which arrives right after mount; true where
 * `IntersectionObserver` does not exist, so nothing that waits for it stalls.
 */
export function useInViewport(ref: RefObject<Element | null>): boolean {
  const [visible, setVisible] = useState(() => !supported())

  useEffect(() => {
    const element = ref.current
    if (!element || !supported()) return
    const observer = new IntersectionObserver(([entry]) => setVisible(entry?.isIntersecting ?? false))
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])

  return visible
}
