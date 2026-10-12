import { Trash2Icon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Workspace } from '@/features/auth/api/types'

import { useDeleteWorkspace } from '../../api/mutations'
import { count } from '@/lib/intl'

interface DeleteDialogProps {
  workspace: Pick<Workspace, 'id' | 'slug' | 'name' | 'projectCount'>
  /** Only the owner may delete (CONTRACT.md §18). */
  isOwner: boolean
}

/** The real delete: every schema cascades, then the session ends. The slug has to be typed. */
export function DeleteDialog({ workspace, isOwner }: DeleteDialogProps) {
  const id = useId()
  const [open, setOpen] = useState(false)
  const [typed, setTyped] = useState('')
  const remove = useDeleteWorkspace(workspace.id)
  const matches = typed.trim() === workspace.slug

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!matches) return
    remove.mutate()
  }

  const trigger = (
    <AlertDialogTrigger asChild>
      <Button variant="destructive" size="sm" disabled={!isOwner}>
        <Trash2Icon data-icon="inline-start" />
        Delete workspace
      </Button>
    </AlertDialogTrigger>
  )

  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (remove.isPending) return
        setOpen(next)
        if (!next) setTyped('')
      }}
    >
      {isOwner ? (
        trigger
      ) : (
        <Tooltip>
          {/* A disabled button fires no pointer events; the wrapper carries the tooltip. */}
          <TooltipTrigger asChild>
            <span tabIndex={0} className="inline-flex rounded-lg">
              {trigger}
            </span>
          </TooltipTrigger>
          <TooltipContent>Only the workspace owner can delete it</TooltipContent>
        </Tooltip>
      )}
      <AlertDialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-destructive/10 text-destructive">
              <Trash2Icon />
            </AlertDialogMedia>
            <AlertDialogTitle>Delete {workspace.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              Every row this workspace owns in every service is deleted in one go:{' '}
              {count(workspace.projectCount, 'project', 'projects')}, facts, proposals, decisions, executions, members,
              settings and the audit trail. GitHub is not touched. You are signed out afterwards.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <Field>
            <FieldLabel htmlFor={id}>
              Type <span className="font-mono">{workspace.slug}</span> to confirm
            </FieldLabel>
            <Input
              id={id}
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              autoFocus
              className="font-mono"
            />
            <FieldDescription className="text-xs">This cannot be undone; export first if in doubt.</FieldDescription>
          </Field>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>Keep workspace</AlertDialogCancel>
            <Button type="submit" variant="destructive" disabled={!matches} loading={remove.isPending}>
              Delete everything
            </Button>
          </AlertDialogFooter>
        </form>
      </AlertDialogContent>
    </AlertDialog>
  )
}
