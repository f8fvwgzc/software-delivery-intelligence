import { useEffect, useState } from 'react'

export type MotionLib = typeof import('./motion-lib')

let loaded: MotionLib | null = null
const loadMotion = () => import('./motion-lib').then((lib) => (loaded = lib))

/**
 * The motion runtime, or null until its chunk has landed. The chunk is
 * requested after the first render of whoever asks, so it never blocks a
 * paint (CONTRACT.md §21); every later caller gets it synchronously.
 */
export function useMotionLib(): MotionLib | null {
  const [lib, setLib] = useState<MotionLib | null>(loaded)

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

  return lib
}
