import { SendHorizontalIcon } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Textarea } from '@/components/ui/textarea'

interface ComposerProps {
  onSend: (content: string) => void
  sending: boolean
  disabled?: boolean
  disabledHint?: string
}

/** Textarea + send. Enter sends, Shift+Enter breaks the line. */
export function Composer({ onSend, sending, disabled = false, disabledHint }: ComposerProps) {
  const [draft, setDraft] = useState('')
  const content = draft.trim()
  const canSend = content.length > 0 && !sending && !disabled

  const send = () => {
    if (!canSend) return
    onSend(content)
    setDraft('')
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault()
      send()
    }
  }

  if (disabled) {
    return <p className="px-4 py-3 text-xs text-muted-foreground">{disabledHint ?? 'Negotiation is closed.'}</p>
  }

  return (
    <form
      className="flex flex-col gap-2 px-4 py-3"
      onSubmit={(event) => {
        event.preventDefault()
        send()
      }}
    >
      <label htmlFor="negotiation-composer" className="sr-only">
        Your message
      </label>
      <Textarea
        id="negotiation-composer"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={onKeyDown}
        placeholder="e.g. we need private networking, or make it cheaper"
        rows={2}
        className="min-h-16 resize-none"
      />
      <div className="flex items-center justify-between gap-2">
        <span className="hidden items-center gap-1 text-2xs text-muted-foreground sm:inline-flex">
          <Kbd>Enter</Kbd> to send · <Kbd>Shift</Kbd>+<Kbd>Enter</Kbd> for a new line
        </span>
        <Button type="submit" size="sm" loading={sending} disabled={!canSend}>
          <SendHorizontalIcon data-icon="inline-start" />
          Send
        </Button>
      </div>
    </form>
  )
}
