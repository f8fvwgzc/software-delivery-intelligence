import { Trash2Icon } from 'lucide-react'
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
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { count } from '@/lib/intl'

import { useDeleteApplication } from '../../api/mutations'
import type { Application } from '../../api/types'
import { destinationLabel } from '../../lib/labels'
import { CheckField } from '../check-field'

interface DeleteApplicationDialogProps {
  application: Application
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Deletes an application (admin). Its resources stay in the cluster,
 * unmanaged, unless "Also delete its resources from the cluster" is ticked.
 * The name is typed to confirm: the cascade cannot be undone.
 */
export function DeleteApplicationDialog({ application, open, onOpenChange }: DeleteApplicationDialogProps) {
  const remove = useDeleteApplication(application)
  return (
    <AlertDialog open={open} onOpenChange={(next) => (remove.isPending ? undefined : onOpenChange(next))}>
      <AlertDialogContent className="data-[size=default]:sm:max-w-md">
        {open ? <DeleteForm application={application} remove={remove} onDone={() => onOpenChange(false)} /> : null}
      </AlertDialogContent>
    </AlertDialog>
  )
}

interface DeleteFormProps {
  application: Application
  remove: ReturnType<typeof useDeleteApplication>
  onDone: () => void
}

function DeleteForm({ application, remove, onDone }: DeleteFormProps) {
  const id = useId()
  const navigate = useNavigate()
  const [cascade, setCascade] = useState(false)
  const [typed, setTyped] = useState('')
  const confirmed = typed.trim() === application.name
  const destination = destinationLabel(application.clusterName, application.namespace)

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!confirmed) {
      document.getElementById(`${id}-name`)?.focus()
      return
    }
    remove.mutate(
      { cascade },
      {
        onSuccess: ({ deletedResources }) => {
          onDone()
          toast.success(`${application.name} deleted`, {
            description: cascade
              ? `${count(deletedResources, 'resource was', 'resources were')} deleted from ${destination}.`
              : 'Its resources stay in the cluster, no longer managed.',
          })
          navigate(routes.projectApplications(application.projectId), { replace: true })
          // In the same render as the navigation: the page that reads them is gone by then.
          remove.forget()
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex min-w-0 flex-col gap-4" noValidate>
      <AlertDialogHeader>
        <AlertDialogMedia className="bg-destructive/10 text-destructive">
          <Trash2Icon />
        </AlertDialogMedia>
        <AlertDialogTitle>Delete {application.name}?</AlertDialogTitle>
        <AlertDialogDescription>
          SDI stops managing it and forgets its history. What runs in {destination} keeps running unless you delete it too.
        </AlertDialogDescription>
      </AlertDialogHeader>

      <FieldGroup className="gap-4">
        <CheckField
          id={`${id}-cascade`}
          label="Also delete its resources from the cluster"
          description={`Deletes the ${count(application.resourceCount, 'resource', 'resources')} it manages from ${destination}. This cannot be undone.`}
          checked={cascade}
          onCheckedChange={setCascade}
          disabled={remove.isPending}
        />
        <Field>
          <FieldLabel htmlFor={`${id}-name`}>
            Type <span className="font-mono text-xs">{application.name}</span> to confirm
          </FieldLabel>
          <Input
            id={`${id}-name`}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            autoFocus
            disabled={remove.isPending}
            aria-describedby={`${id}-name-hint`}
            className="font-mono text-xs md:text-xs"
          />
          <FieldDescription id={`${id}-name-hint`} className="text-xs">
            The application’s name, exactly.
          </FieldDescription>
        </Field>
      </FieldGroup>

      {remove.isError ? <RefusalNote error={remove.error} /> : null}

      <AlertDialogFooter>
        <AlertDialogCancel type="button" disabled={remove.isPending}>
          Keep application
        </AlertDialogCancel>
        <Button type="submit" variant="destructive" loading={remove.isPending} disabled={!confirmed}>
          {cascade ? 'Delete application and resources' : 'Delete application'}
        </Button>
      </AlertDialogFooter>
    </form>
  )
}
