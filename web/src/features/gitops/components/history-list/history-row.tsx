import { Undo2Icon } from 'lucide-react'

import { ExecutionLink } from '@/components/shared/data/execution-link'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { Row } from '@/components/system/row'
import { Tag, TagDot } from '@/components/system/tag'
import { Button } from '@/components/ui/button'

import type { HistoryEntry } from '../../api/types'
import { formatDuration, operationPhase } from '../../lib/labels'
import { Initiator } from '../initiator'
import { RevisionLink } from '../revision-link'

interface HistoryRowProps {
  entry: HistoryEntry
  repoUrl?: string
  /** Why "Roll back" is off, or null when it is live. */
  rollbackReason: string | null
  onRollback: (entry: HistoryEntry) => void
}

/** The overrides synced with the commit: two entries of one commit differ only by these. */
function overrides(entry: HistoryEntry): string[] {
  const { kustomizeImages, helmParameters } = entry.source
  return [...kustomizeImages, ...Object.entries(helmParameters).map(([key, value]) => `${key}=${value}`)]
}

function seconds(startedAt: string, finishedAt: string): number | null {
  const ms = new Date(finishedAt).getTime() - new Date(startedAt).getTime()
  return Number.isFinite(ms) && ms >= 0 ? ms / 1000 : null
}

/** One sync: the commit and its overrides, how it ended, who asked (or the sync policy), how long it took, and the way back to it. */
export function HistoryRow({ entry, repoUrl, rollbackReason, onRollback }: HistoryRowProps) {
  const phase = operationPhase(entry.phase)
  const took = seconds(entry.startedAt, entry.finishedAt)
  const synced = overrides(entry)

  return (
    <Row
      leading={<TagDot tone={phase.tone} />}
      title={
        <span className="inline-flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 align-bottom">
          <RevisionLink revision={entry.revision} repoUrl={repoUrl} className="text-xs" />
          <Tag tone={phase.tone}>{phase.label}</Tag>
          {entry.kind === 'rollback' ? <Tag variant="outline">Rollback</Tag> : null}
          {entry.automated ? <Tag variant="outline">Automated</Tag> : null}
        </span>
      }
      meta={
        <>
          <Initiator value={entry.initiatedBy} />
          {took !== null ? <span className="font-mono tabular-nums">{formatDuration(took)}</span> : null}
          <RelativeTime value={entry.finishedAt} className="tabular-nums" />
          {entry.executionId ? <ExecutionLink executionId={entry.executionId}>Execution</ExecutionLink> : null}
          {synced.length > 0 ? (
            <span className="basis-full truncate font-mono text-xs" title={synced.join('\n')}>
              {synced.join(' · ')}
            </span>
          ) : null}
          {entry.message ? (
            <span className={`basis-full ${entry.phase === 'succeeded' ? '' : 'text-destructive'}`}>{entry.message}</span>
          ) : null}
        </>
      }
      trailing={
        <DisabledReason reason={rollbackReason}>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => onRollback(entry)}
            disabled={rollbackReason !== null}
            aria-label={`Roll back to ${entry.revision.slice(0, 7)}`}
          >
            <Undo2Icon data-icon="inline-start" />
            Roll back
          </Button>
        </DisabledReason>
      }
    />
  )
}
