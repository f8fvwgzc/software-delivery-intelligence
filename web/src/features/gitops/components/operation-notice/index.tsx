import { CircleAlertIcon } from 'lucide-react'

import { ExecutionLink } from '@/components/shared/data/execution-link'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Tag } from '@/components/system/tag'

import type { Application } from '../../api/types'
import { isOperationRunning, operationKind, shortSha } from '../../lib/labels'
import { Initiator } from '../initiator'

/** An execution this page started and has not seen run yet. */
export interface RequestedOperation {
  kind: 'sync' | 'preview' | 'rollback'
  executionId: string
  /** The revision a rollback goes back to. */
  revision?: string
}

const GATED = 'Its execution previews the change, then waits for an approval and a timed cluster:write grant before anything reaches the cluster.'

function requestedText({ kind, revision }: RequestedOperation) {
  switch (kind) {
    case 'preview':
      return 'Preview requested. Its execution lists what a sync would create, change or prune, and applies nothing.'
    case 'rollback':
      return `Rollback${revision ? ` to ${shortSha(revision)}` : ''} requested. ${GATED}`
    default:
      return `Sync requested. ${GATED}`
  }
}

const LINE = 'flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg px-3 py-2 text-xs ring-1'

/**
 * What is happening to the application now: the operation in flight with
 * its execution, a request this page just made, or how the last operation
 * failed. Nothing when all is quiet: the history says the rest.
 */
export function OperationNotice({ application, requested }: { application: Application; requested: RequestedOperation | null }) {
  const { operation } = application

  if (operation && isOperationRunning(operation)) {
    return (
      <div role="status" className={`${LINE} ring-border`}>
        <Tag tone="info" dot live>
          {operationKind(operation.kind)} {operation.phase === 'terminating' ? 'terminating' : 'running'}
        </Tag>
        {operation.revision ? <span className="font-mono">{shortSha(operation.revision)}</span> : null}
        <span className="text-muted-foreground">
          started <RelativeTime value={operation.startedAt} /> by <Initiator value={operation.initiatedBy} />
        </span>
        {operation.message ? <span className="min-w-0 text-muted-foreground">{operation.message}</span> : null}
        {operation.executionId ? (
          <ExecutionLink executionId={operation.executionId} className="ml-auto">
            Open the execution
          </ExecutionLink>
        ) : null}
      </div>
    )
  }

  // A request whose execution has not reached its sync step yet (preview, approval, grant).
  if (requested && operation?.executionId !== requested.executionId) {
    return (
      <div role="status" className={`${LINE} ring-border`}>
        <span className="min-w-0 text-muted-foreground">{requestedText(requested)}</span>
        <ExecutionLink executionId={requested.executionId} className="ml-auto">
          Open the execution
        </ExecutionLink>
      </div>
    )
  }

  if (operation?.finishedAt && (operation.phase === 'failed' || operation.phase === 'error')) {
    return (
      <div role="alert" className={`${LINE} bg-destructive/[0.03] ring-destructive/20`}>
        <CircleAlertIcon className="size-3.5 shrink-0 text-destructive" aria-hidden />
        <span className="text-destructive">
          The last {operationKind(operation.kind).toLowerCase()} {operation.phase === 'error' ? 'ended in an error' : 'failed'}
        </span>
        <span className="text-muted-foreground">
          <RelativeTime value={operation.finishedAt} />
        </span>
        {operation.message ? <span className="min-w-0 basis-full text-foreground sm:basis-auto">{operation.message}</span> : null}
        {operation.executionId ? (
          <ExecutionLink executionId={operation.executionId} className="ml-auto">
            Open the execution
          </ExecutionLink>
        ) : null}
      </div>
    )
  }

  return null
}
