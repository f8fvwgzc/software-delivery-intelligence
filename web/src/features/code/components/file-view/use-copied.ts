import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

const COPIED_FOR_MS = 1_500

/** Clipboard write with a short "Copied" confirmation state. */
export function useCopied() {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(timer.current), [])

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), COPIED_FOR_MS)
    } catch {
      toast.error('Could not copy', { description: 'Your browser blocked clipboard access.' })
    }
  }

  return { copied, copy }
}
