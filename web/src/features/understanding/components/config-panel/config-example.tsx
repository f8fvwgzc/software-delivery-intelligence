import { CheckIcon, CopyIcon } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'

import { CONFIG_EXAMPLE } from '../../lib/config'

const COPIED_MS = 1_500

/** The minimal `.sdi.yml`, ready to paste, with one line on why it matters. */
export function ConfigExample() {
  const [copied, setCopied] = useState(false)
  const resetTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(resetTimer.current), [])

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(CONFIG_EXAMPLE)
    } catch {
      toast.error('Could not copy', { description: 'Select the block and copy it by hand.' })
      return
    }
    setCopied(true)
    clearTimeout(resetTimer.current)
    resetTimer.current = setTimeout(() => setCopied(false), COPIED_MS)
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-muted-foreground">
        No <span className="font-mono text-foreground/80">.sdi.yml</span> in this repository. Configured values
        answer the open questions automatically and become the budget and test command every proposal uses.
      </p>
      <div className="relative">
        <pre
          className="overflow-x-auto rounded-lg bg-muted/50 p-3 pr-20 font-mono text-xs leading-5 ring-1 ring-border"
          tabIndex={0}
        >
          <code>{CONFIG_EXAMPLE}</code>
        </pre>
        <Button
          type="button"
          variant="outline"
          size="xs"
          onClick={copy}
          className="absolute top-2 right-2 bg-background"
          aria-label={copied ? 'Copied' : 'Copy example .sdi.yml'}
        >
          {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
    </div>
  )
}
