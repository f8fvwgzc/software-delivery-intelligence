import { useSuspenseQuery } from '@tanstack/react-query'

import type { components } from '@/lib/api/schema'

import { proposalMessagesQuery } from '../../api/queries'
import { ConstraintsList } from './constraints-list'
import { MessageList } from './message-list'

type Constraint = components['schemas']['Constraint']

interface ThreadBodyProps {
  proposalId: string
  constraints: Constraint[]
  /** The message being sent right now, shown before the server echoes it. */
  pendingContent?: string
}

/** Loads the thread once and feeds both the messages and the constraints. */
export function ThreadBody({ proposalId, constraints, pendingContent }: ThreadBodyProps) {
  const { data: messages } = useSuspenseQuery(proposalMessagesQuery(proposalId))

  // Message id → 1-based position, so a constraint can say "from message 3".
  const positions = new Map(messages.map((message, index) => [message.id, index + 1]))

  return (
    <>
      <MessageList messages={messages} pendingContent={pendingContent} />
      {constraints.length > 0 ? <ConstraintsList constraints={constraints} positions={positions} /> : null}
    </>
  )
}
