import { useSuspenseQuery } from '@tanstack/react-query'
import { HistoryIcon } from 'lucide-react'
import { useState } from 'react'

import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'
import { useAuth } from '@/features/auth/lib/auth-context'
import { count } from '@/lib/intl'

import { gitopsQueries } from '../../api/queries'
import type { Application, HistoryEntry } from '../../api/types'
import { rollbackGate } from '../../lib/permissions'
import type { RequestedOperation } from '../operation-notice'
import { RollbackDialog } from '../rollback-dialog'
import { HistoryRow } from './history-row'

interface HistoryListProps {
  application: Application
  repoUrl?: string
  onRequested: (requested: RequestedOperation) => void
}

/** The syncs that were not dry runs, newest first, each one a way back. */
export function HistoryList({ application, repoUrl, onRequested }: HistoryListProps) {
  const { data: entries } = useSuspenseQuery(gitopsQueries.history(application.id))
  const { currentWorkspace } = useAuth()
  // The dialog keeps its entry while it closes; `open` alone says whether it shows.
  const [open, setOpen] = useState(false)
  const [target, setTarget] = useState<HistoryEntry | undefined>(undefined)

  if (entries.length === 0) {
    return (
      <PanelEmpty
        icon={HistoryIcon}
        title="No syncs yet"
        description="Every sync that is not a dry run is kept here with its commit and its source, and can be rolled back to."
      />
    )
  }

  const gate = rollbackGate(currentWorkspace.myRole, application)
  // The newest successful sync of what the cluster runs is where it already is.
  const current = entries.find((entry) => entry.phase === 'succeeded')
  const reasonFor = (entry: HistoryEntry) => {
    if (gate) return gate
    // A state that never deployed is not one to go back to.
    if (entry.phase !== 'succeeded') return 'Only a sync that succeeded can be rolled back to.'
    return entry === current && entry.revision === application.syncedRevision ? 'The cluster already runs this sync.' : null
  }

  return (
    <>
      <RowList aria-label="Sync history">
        {entries.map((entry) => (
          <li key={entry.id}>
            <HistoryRow
              entry={entry}
              repoUrl={repoUrl}
              rollbackReason={reasonFor(entry)}
              onRollback={(picked) => {
                setTarget(picked)
                setOpen(true)
              }}
            />
          </li>
        ))}
      </RowList>
      <FrameFooter>
        <span className="tabular-nums">{count(entries.length, 'sync', 'syncs')}</span>
        <span className="ml-auto">The last 50 are kept.</span>
      </FrameFooter>
      <RollbackDialog application={application} entry={target} open={open} onOpenChange={setOpen} onRequested={onRequested} />
    </>
  )
}
