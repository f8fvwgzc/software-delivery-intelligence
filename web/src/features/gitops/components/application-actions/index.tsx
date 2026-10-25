import { PencilIcon, RefreshCwIcon, RotateCwIcon, Trash2Icon, UploadIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { Button } from '@/components/ui/button'
import { ButtonGroup } from '@/components/ui/button-group'
import { useAuth } from '@/features/auth/lib/auth-context'

import { useRefreshApplication } from '../../api/mutations'
import type { Application } from '../../api/types'
import { syncStatus } from '../../lib/labels'
import { adminGate, refreshGate, syncGate } from '../../lib/permissions'
import { ApplicationEditor } from '../application-editor'
import { DeleteApplicationDialog } from '../delete-application-dialog'
import type { RequestedOperation } from '../operation-notice'
import { SyncDialog } from '../sync-dialog'

type Dialog = 'sync' | 'edit' | 'delete'

interface ApplicationActionsProps {
  application: Application
  onRequested: (requested: RequestedOperation) => void
}

/**
 * The application page's header actions. Refresh and Hard refresh compare
 * now (member); Sync asks for an execution (approver); Edit and Delete are
 * an admin's. A control that is off says why instead of disappearing.
 */
export function ApplicationActions({ application, onRequested }: ApplicationActionsProps) {
  const { currentWorkspace } = useAuth()
  const role = currentWorkspace.myRole
  const refresh = useRefreshApplication(application.id)
  const [dialog, setDialog] = useState<Dialog | null>(null)

  const refreshReason = refreshGate(role)
  const syncReason = syncGate(role, application)
  const editReason = adminGate(role, 'edit')
  const deleteReason = adminGate(role, 'delete')
  const pendingHard = refresh.isPending ? refresh.variables?.hard : undefined

  const runRefresh = (hard: boolean) =>
    refresh.mutate(
      { hard },
      {
        onSuccess: (refreshed) =>
          toast.success(hard ? `${refreshed.name} rendered again and compared` : `${refreshed.name} compared`, {
            description: `${syncStatus(refreshed.syncStatus).label}.${refreshed.conditions[0] ? ` ${refreshed.conditions[0]}` : ''}`,
          }),
      },
    )

  const onOpenChange = (open: boolean) => {
    if (!open) setDialog(null)
  }

  return (
    <>
      <ButtonGroup aria-label="Refresh">
        <DisabledReason reason={refreshReason}>
          <Button
            variant="outline"
            size="sm"
            onClick={() => runRefresh(false)}
            loading={pendingHard === false}
            disabled={refreshReason !== null || refresh.isPending}
            title="Compare with the cluster now, from the manifests already rendered for this commit."
          >
            <RefreshCwIcon data-icon="inline-start" />
            Refresh
          </Button>
        </DisabledReason>
        <DisabledReason reason={refreshReason}>
          <Button
            variant="outline"
            size="sm"
            onClick={() => runRefresh(true)}
            loading={pendingHard === true}
            disabled={refreshReason !== null || refresh.isPending}
            title="Render the source again, even when the commit did not change, then compare."
          >
            <RotateCwIcon data-icon="inline-start" />
            Hard refresh
          </Button>
        </DisabledReason>
      </ButtonGroup>

      <DisabledReason reason={syncReason}>
        <Button size="sm" onClick={() => setDialog('sync')} disabled={syncReason !== null}>
          <UploadIcon data-icon="inline-start" />
          Sync
        </Button>
      </DisabledReason>

      <DisabledReason reason={editReason}>
        <Button variant="outline" size="sm" onClick={() => setDialog('edit')} disabled={editReason !== null}>
          <PencilIcon data-icon="inline-start" />
          Edit
        </Button>
      </DisabledReason>

      <DisabledReason reason={deleteReason}>
        <Button
          variant="ghost"
          size="sm"
          className="text-destructive hover:text-destructive"
          onClick={() => setDialog('delete')}
          disabled={deleteReason !== null}
        >
          <Trash2Icon data-icon="inline-start" />
          Delete
        </Button>
      </DisabledReason>

      <SyncDialog application={application} open={dialog === 'sync'} onOpenChange={onOpenChange} onRequested={onRequested} />
      <ApplicationEditor projectId={application.projectId} application={application} open={dialog === 'edit'} onOpenChange={onOpenChange} />
      <DeleteApplicationDialog application={application} open={dialog === 'delete'} onOpenChange={onOpenChange} />
    </>
  )
}
