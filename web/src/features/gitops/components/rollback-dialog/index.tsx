import { Undo2Icon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { FieldGroup } from '@/components/ui/field'

import { useRollbackApplication } from '../../api/mutations'
import type { Application, HistoryEntry } from '../../api/types'
import { destinationLabel, shortSha } from '../../lib/labels'
import { CheckField } from '../check-field'
import type { RequestedOperation } from '../operation-notice'

interface RollbackDialogProps {
  application: Application
  /** The entry to go back to; it stays set while the dialog closes. */
  entry: HistoryEntry | undefined
  open: boolean
  onOpenChange: (open: boolean) => void
  onRequested: (requested: RequestedOperation) => void
}

/**
 * Asks for a rollback to one history entry (approver): an execution that
 * previews, waits for an approval and a timed `cluster:write` grant, syncs
 * that entry's source again and waits for health (CONTRACT.md §24.7).
 */
export function RollbackDialog({ application, entry, open, onOpenChange, onRequested }: RollbackDialogProps) {
  const rollback = useRollbackApplication(application.id)
  return (
    <AlertDialog open={open} onOpenChange={(next) => (rollback.isPending ? undefined : onOpenChange(next))}>
      <AlertDialogContent className="data-[size=default]:sm:max-w-md">
        {open && entry ? (
          <RollbackForm application={application} entry={entry} rollback={rollback} onDone={() => onOpenChange(false)} onRequested={onRequested} />
        ) : null}
      </AlertDialogContent>
    </AlertDialog>
  )
}

interface RollbackFormProps {
  application: Application
  entry: HistoryEntry
  rollback: ReturnType<typeof useRollbackApplication>
  onDone: () => void
  onRequested: (requested: RequestedOperation) => void
}

function RollbackForm({ application, entry, rollback, onDone, onRequested }: RollbackFormProps) {
  const id = useId()
  const navigate = useNavigate()
  const [prune, setPrune] = useState(false)
  const short = shortSha(entry.revision)
  const healsBack = application.syncPolicy.automated && application.syncPolicy.selfHeal

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    rollback.mutate(
      { historyId: entry.id, prune },
      {
        onSuccess: ({ executionId }) => {
          onDone()
          onRequested({ kind: 'rollback', executionId, revision: entry.revision })
          toast.success(`Rollback of ${application.name} to ${short} requested`, {
            description: 'The execution asks for an approval before anything reaches the cluster.',
            action: { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) },
          })
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex min-w-0 flex-col gap-4" noValidate>
      <AlertDialogHeader>
        <AlertDialogMedia className="bg-warning/10 text-warning">
          <Undo2Icon />
        </AlertDialogMedia>
        <AlertDialogTitle>
          Roll {application.name} back to <span className="font-mono">{short}</span>?
        </AlertDialogTitle>
        <AlertDialogDescription>
          Creates an execution that previews, waits for an approval and a timed <span className="font-mono text-xs">cluster:write</span>{' '}
          grant, syncs {destinationLabel(application.clusterName, application.namespace)} to the source as it was synced then, and waits
          for the application to be healthy.
        </AlertDialogDescription>
      </AlertDialogHeader>

      <p className="text-xs leading-5 text-muted-foreground">
        The preview renders today’s source at <span className="font-mono">{short}</span>; when the path, images or parameters changed
        since, it differs from what the rollback applies.
      </p>
      {healsBack ? (
        <p className="text-xs leading-5 text-warning">
          Automated sync with self heal is on, so the next reconcile puts the current source back. Turn self heal off first to keep
          the rollback.
        </p>
      ) : null}

      <FieldGroup className="gap-3">
        <CheckField
          id={`${id}-prune`}
          label="Prune"
          description="Also delete the resources the earlier state did not have."
          checked={prune}
          onCheckedChange={setPrune}
          disabled={rollback.isPending}
        />
      </FieldGroup>

      {rollback.isError ? <RefusalNote error={rollback.error} /> : null}

      <AlertDialogFooter>
        <AlertDialogCancel type="button" disabled={rollback.isPending}>
          Keep the current state
        </AlertDialogCancel>
        <Button type="submit" variant="destructive" loading={rollback.isPending}>
          Request rollback
        </Button>
      </AlertDialogFooter>
    </form>
  )
}
