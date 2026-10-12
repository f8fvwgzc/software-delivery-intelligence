import { MoreHorizontalIcon, PencilIcon, Trash2Icon, Undo2Icon } from 'lucide-react'
import { useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

import type { PipelineStage } from '../../api/types'
import { DeleteEnvironmentDialog } from '../delete-environment-dialog'
import { EnvironmentEditor } from '../environment-editor'
import { RollbackDialog } from '../rollback-dialog'

type Action = 'rollback' | 'edit' | 'delete'

interface StageMenuProps {
  projectId: string
  stage: PipelineStage
  /** The viewer holds the approver role: may roll back. */
  canPromote: boolean
  /** The viewer holds the admin role: may edit and delete. */
  canAdmin: boolean
}

/**
 * The overflow menu of a stage. Rollback for approvers; Edit and Delete for
 * admins, and only for environments made here: one declared in `.sdi.yml`
 * is changed in the repository. With nothing to offer it renders nothing.
 */
export function StageMenu({ projectId, stage, canPromote, canAdmin }: StageMenuProps) {
  const [action, setAction] = useState<Action | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const { environment } = stage
  const editable = canAdmin && environment.source !== 'config'

  if (!canPromote && !editable) return null

  const onOpenChange = (open: boolean) => {
    if (!open) setAction(null)
  }
  // The menu item that opened the dialog is gone when it closes; focus returns to the menu button.
  const restoreFocus = (event: Event) => {
    event.preventDefault()
    triggerRef.current?.focus()
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button ref={triggerRef} variant="ghost" size="icon-xs" aria-label={`Actions for ${environment.name}`}>
            <MoreHorizontalIcon />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-44">
          {canPromote ? (
            <DropdownMenuGroup>
              <DropdownMenuItem disabled={!stage.current} onSelect={() => setAction('rollback')}>
                <Undo2Icon />
                Roll back
              </DropdownMenuItem>
            </DropdownMenuGroup>
          ) : null}
          {canPromote && editable ? <DropdownMenuSeparator /> : null}
          {editable ? (
            <DropdownMenuGroup>
              <DropdownMenuItem onSelect={() => setAction('edit')}>
                <PencilIcon />
                Edit
              </DropdownMenuItem>
              <DropdownMenuItem variant="destructive" onSelect={() => setAction('delete')}>
                <Trash2Icon />
                Delete
              </DropdownMenuItem>
            </DropdownMenuGroup>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>

      <RollbackDialog projectId={projectId} stage={stage} open={action === 'rollback'} onOpenChange={onOpenChange} onCloseAutoFocus={restoreFocus} />
      <EnvironmentEditor
        projectId={projectId}
        environment={environment}
        open={action === 'edit'}
        onOpenChange={onOpenChange}
        onCloseAutoFocus={restoreFocus}
      />
      <DeleteEnvironmentDialog
        projectId={projectId}
        environment={environment}
        open={action === 'delete'}
        onOpenChange={onOpenChange}
        onCloseAutoFocus={restoreFocus}
      />
    </>
  )
}
