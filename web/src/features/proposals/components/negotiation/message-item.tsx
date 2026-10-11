import { RefreshCwIcon } from 'lucide-react'

import { Markdown } from '@/components/shared/data/markdown'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Badge } from '@/components/ui/badge'
import { Spinner } from '@/components/ui/spinner'
import type { components } from '@/lib/api/schema'

import { EVIDENCE_KIND, MESSAGE_ROLE_LABEL } from '../../lib/labels'

type Message = components['schemas']['Message']

export function MessageItem({ message }: { message: Message }) {
  if (message.role === 'system' || message.role === 'unspecified') {
    return (
      <li className="px-2 text-center text-2xs text-muted-foreground">
        {message.content}
      </li>
    )
  }

  if (message.role === 'user') {
    return (
      <li className="flex flex-col items-end gap-1">
        <div className="max-w-[85%] rounded-lg rounded-tr-sm bg-muted px-3 py-2 text-sm whitespace-pre-wrap">
          {message.content}
        </div>
        <Meta role={message.role} at={message.createdAt} />
      </li>
    )
  }

  return (
    <li className="flex flex-col gap-1.5">
      {message.changedRecommendation ? (
        <p className="inline-flex items-center gap-1.5 text-2xs text-brand">
          <RefreshCwIcon className="size-3" aria-hidden="true" />
          Recommendation changed
        </p>
      ) : null}
      <Markdown className="max-w-none [&_p]:my-1.5 [&_ul]:my-1.5">{message.content}</Markdown>
      {message.references.length > 0 ? (
        <ul className="flex flex-wrap gap-1" aria-label="References">
          {message.references.map((reference, index) => {
            const kind = EVIDENCE_KIND[reference.kind]
            return (
              <li key={`${reference.refId}-${index}`}>
                <Badge
                  variant="outline"
                  title={reference.detail || undefined}
                  className="h-5 max-w-56 gap-1 px-1.5 text-2xs font-normal text-muted-foreground"
                >
                  <kind.icon aria-label={kind.label} />
                  <span className="truncate">{reference.label}</span>
                </Badge>
              </li>
            )
          })}
        </ul>
      ) : null}
      <Meta role={message.role} at={message.createdAt} />
    </li>
  )
}

/** The user's turn while the reply is being written. */
export function PendingMessageItem({ content }: { content: string }) {
  return (
    <>
      <li className="flex flex-col items-end gap-1 opacity-70">
        <div className="max-w-[85%] rounded-lg rounded-tr-sm bg-muted px-3 py-2 text-sm whitespace-pre-wrap">{content}</div>
      </li>
      <li className="inline-flex items-center gap-2 text-xs text-muted-foreground" aria-live="polite">
        <Spinner className="size-3.5" />
        Re-ranking the options
      </li>
    </>
  )
}

function Meta({ role, at }: { role: Message['role']; at: string }) {
  return (
    <span className="text-2xs text-muted-foreground">
      {MESSAGE_ROLE_LABEL[role]} · <RelativeTime value={at} className="font-mono" />
    </span>
  )
}
