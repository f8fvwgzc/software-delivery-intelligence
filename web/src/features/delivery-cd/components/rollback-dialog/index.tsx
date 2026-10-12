import { useInfiniteQuery } from '@tanstack/react-query'
import { Undo2Icon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
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
import { Textarea } from '@/components/ui/textarea'

import { useRollback } from '../../api/mutations'
import { cdQueries } from '../../api/queries'
import type { PipelineStage } from '../../api/types'
import { releaseSource, shortSha } from '../../lib/labels'

/** The combobox cannot carry ''; this value lets the service pick the previous successful release. */
const PREVIOUS = '__previous__'

interface RollbackDialogProps {
  projectId: string
  stage: PipelineStage
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Where focus goes when the dialog closes (it is opened from a menu that is gone by then). */
  onCloseAutoFocus?: (event: Event) => void
}

/** Puts an earlier release back into a stage. It asks first: a rollback changes what is running. */
export function RollbackDialog({ projectId, stage, open, onOpenChange, onCloseAutoFocus }: RollbackDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="sm:max-w-md" onCloseAutoFocus={onCloseAutoFocus}>
        {/* Mounted per open, so the form starts from the defaults every time. */}
        {open ? <RollbackForm projectId={projectId} stage={stage} onDone={() => onOpenChange(false)} /> : null}
      </AlertDialogContent>
    </AlertDialog>
  )
}

interface RollbackFormProps {
  projectId: string
  stage: PipelineStage
  onDone: () => void
}

function RollbackForm({ projectId, stage, onDone }: RollbackFormProps) {
  const id = useId()
  const navigate = useNavigate()
  const rollback = useRollback(projectId)
  // The releases panel on the same page already holds these pages: no request of its own.
  const releases = useInfiniteQuery(cdQueries.releases(projectId))
  const [releaseId, setReleaseId] = useState(PREVIOUS)
  const [reason, setReason] = useState('')
  const { environment, current, lastRun } = stage

  const known = releases.data?.pages.flat() ?? []
  const previous = known.find((release) => release.id === lastRun?.previousReleaseId)
  const options: ComboboxOption[] = [
    {
      value: PREVIOUS,
      label: 'Previous successful release',
      description: previous ? `${previous.version}, the one this stage ran before` : 'The one this stage ran before',
    },
    ...known
      .filter((release) => release.id !== current?.id)
      .map((release) => ({
        value: release.id,
        label: release.version,
        description: `${shortSha(release.commitSha)} · ${releaseSource(release.source).toLowerCase()}`,
        keywords: [release.commitSha, release.tag],
      })),
  ]

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    rollback.mutate(
      { environmentId: environment.id, releaseId: releaseId === PREVIOUS ? null : releaseId, reason },
      {
        onSuccess: (run) => {
          onDone()
          const executionId = run.executionId
          toast.success(`Rolling ${environment.name} back to ${run.releaseVersion || 'the previous release'}`, {
            description: 'The run asks for an approval before anything changes.',
            action: executionId ? { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) } : undefined,
          })
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4">
      <AlertDialogHeader>
        <AlertDialogMedia className="bg-warning/10 text-warning">
          <Undo2Icon />
        </AlertDialogMedia>
        <AlertDialogTitle>Roll back {environment.name}?</AlertDialogTitle>
        <AlertDialogDescription>
          {current ? (
            <>
              <span className="font-mono text-xs">{current.version}</span> is replaced by the release you pick.{' '}
            </>
          ) : null}
          Same plan as a promotion, as <span className="font-mono text-xs">deploy.rollback</span>: an approval is requested, then
          the image is verified, deployed and health-checked.
        </AlertDialogDescription>
      </AlertDialogHeader>
      <FieldGroup className="gap-4">
        <Field>
          <FieldLabel htmlFor={`${id}-release`}>Release</FieldLabel>
          <ComboboxField
            id={`${id}-release`}
            mono
            value={releaseId}
            onValueChange={setReleaseId}
            options={options}
            loading={releases.isPending}
            searchPlaceholder="Search versions and commits"
            emptyText="No release matches."
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${id}-reason`}>Reason</FieldLabel>
          <Textarea
            id={`${id}-reason`}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            placeholder="What is wrong with the running release"
            rows={3}
            maxLength={500}
          />
          <FieldDescription className="text-xs">Optional. It is shown to the approver and kept with the run.</FieldDescription>
        </Field>
      </FieldGroup>
      <AlertDialogFooter>
        <AlertDialogCancel type="button" disabled={rollback.isPending}>
          Keep running
        </AlertDialogCancel>
        <Button type="submit" variant="destructive" loading={rollback.isPending}>
          Request rollback
        </Button>
      </AlertDialogFooter>
    </form>
  )
}
