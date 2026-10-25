import { useId, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { FieldGroup } from '@/components/ui/field'
import { count } from '@/lib/intl'

import { useSyncApplication } from '../../api/mutations'
import type { Application } from '../../api/types'
import { destinationLabel, shortSha } from '../../lib/labels'
import { CheckField } from '../check-field'
import type { RequestedOperation } from '../operation-notice'

interface SyncDialogProps {
  application: Application
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The execution the request started, for the page to link to. */
  onRequested: (requested: RequestedOperation) => void
}

/**
 * Asks for a sync (approver). Nothing reaches the cluster from here: the
 * answer is an execution that previews, waits for an approval and a timed
 * `cluster:write` grant, syncs and waits for health (CONTRACT.md §24.7).
 * With "Dry run" the execution only previews.
 */
export function SyncDialog({ application, open, onOpenChange, onRequested }: SyncDialogProps) {
  const sync = useSyncApplication(application.id)
  return (
    <Dialog open={open} onOpenChange={(next) => (sync.isPending ? undefined : onOpenChange(next))}>
      <DialogContent className="sm:max-w-md">
        {open ? <SyncForm application={application} sync={sync} onDone={() => onOpenChange(false)} onRequested={onRequested} /> : null}
      </DialogContent>
    </Dialog>
  )
}

interface SyncFormProps {
  application: Application
  sync: ReturnType<typeof useSyncApplication>
  onDone: () => void
  onRequested: (requested: RequestedOperation) => void
}

/** How the cluster compares now, in one sentence: what the sync is about to change. */
function CurrentState({ application }: { application: Application }) {
  if (application.syncStatus === 'synced') return <>The cluster matches the repository now; a sync applies the same state again.</>
  if (application.outOfSyncCount === 0) return <>The last comparison did not say what differs.</>
  return (
    <>
      {count(application.outOfSyncCount, 'resource differs', 'resources differ')} from the repository
      {application.revision ? (
        <>
          {' '}
          at <span className="font-mono">{shortSha(application.revision)}</span>
        </>
      ) : null}
      .
    </>
  )
}

function SyncForm({ application, sync, onDone, onRequested }: SyncFormProps) {
  const id = useId()
  const navigate = useNavigate()
  const [prune, setPrune] = useState(false)
  const [dryRun, setDryRun] = useState(false)
  const extraneous = application.resources.filter((resource) => resource.syncStatus === 'extraneous').length
  const destination = destinationLabel(application.clusterName, application.namespace)

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    sync.mutate(
      { prune, dryRun },
      {
        onSuccess: ({ executionId }) => {
          onDone()
          onRequested({ kind: dryRun ? 'preview' : 'sync', executionId })
          toast.success(dryRun ? `Preview of ${application.name} started` : `Sync of ${application.name} requested`, {
            description: dryRun ? 'It applies nothing.' : 'The execution asks for an approval before anything reaches the cluster.',
            action: { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) },
          })
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>Sync {application.name}</DialogTitle>
        <DialogDescription>
          {dryRun ? (
            <>Creates an execution that only previews what a sync to {destination} would create, change or prune. Nothing is applied.</>
          ) : (
            <>
              Creates an execution that previews the change, waits for an approval and a timed{' '}
              <span className="font-mono text-xs">cluster:write</span> grant, syncs {destination}, then waits for the application to be
              healthy. Nothing reaches the cluster before the approval.
            </>
          )}
        </DialogDescription>
      </DialogHeader>

      <p className="text-xs text-muted-foreground">
        <CurrentState application={application} />
      </p>

      <FieldGroup className="gap-3">
        <CheckField
          id={`${id}-prune`}
          label="Prune"
          description={
            extraneous > 0
              ? `Also delete what left the repository: ${count(extraneous, 'resource is', 'resources are')} in the cluster and no longer desired.`
              : 'Also delete resources that left the repository. None do now.'
          }
          checked={prune}
          onCheckedChange={setPrune}
          disabled={sync.isPending}
        />
        <CheckField
          id={`${id}-dry-run`}
          label="Dry run"
          description="Only preview: the execution lists what would be created, changed or pruned, needs no approval, and applies nothing."
          checked={dryRun}
          onCheckedChange={setDryRun}
          disabled={sync.isPending}
        />
      </FieldGroup>

      {sync.isError ? <RefusalNote error={sync.error} /> : null}

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={sync.isPending}>
          Not now
        </Button>
        <Button type="submit" loading={sync.isPending}>
          {dryRun ? 'Start preview' : 'Request sync'}
        </Button>
      </DialogFooter>
    </form>
  )
}
