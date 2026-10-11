import { MessagesSquareIcon } from 'lucide-react'
import { useEffect, useRef } from 'react'

import type { components } from '@/lib/api/schema'

import { MessageItem, PendingMessageItem } from './message-item'

type Message = components['schemas']['Message']

interface MessageListProps {
  messages: Message[]
  pendingContent?: string
}

export function MessageList({ messages, pendingContent }: MessageListProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const count = messages.length + (pendingContent ? 1 : 0)

  // DOM sync, not derived state: keep the newest turn in view. Only the
  // thread's own viewport scrolls; `scrollIntoView` would also scroll the
  // document, which on a single-column layout jumps the page to the thread.
  useEffect(() => {
    const viewport = viewportRef.current
    if (count === 0 || !viewport) return
    viewport.scrollTop = viewport.scrollHeight
  }, [count])

  if (count === 0) {
    return (
      <div className="flex flex-col items-center gap-1.5 px-4 py-8 text-center">
        <MessagesSquareIcon className="size-4 text-muted-foreground" />
        <p className="text-sm">No messages yet</p>
        <p className="max-w-56 text-xs text-muted-foreground">
          Ask for something cheaper, more private or more specific. The options re-rank with every turn.
        </p>
      </div>
    )
  }

  return (
    // A plain scroll container: Radix ScrollArea sizes its viewport to the
    // content, so a max-height on it never clips and the thread would run
    // under the constraints and weights below.
    <div ref={viewportRef} className="max-h-[28rem] min-h-40 overflow-y-auto [scrollbar-gutter:stable]">
      <ol className="flex flex-col gap-3 px-4 py-3">
        {messages.map((message) => (
          <MessageItem key={message.id} message={message} />
        ))}
        {pendingContent ? <PendingMessageItem content={pendingContent} /> : null}
      </ol>
    </div>
  )
}
