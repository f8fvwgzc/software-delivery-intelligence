import { useQuery } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel, FieldSeparator } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useAuth } from '@/features/auth/lib/auth-context'
import { toSaveError, type SaveError } from '@/features/delivery-cd/lib/field-errors'

import { useSaveApplication } from '../../api/mutations'
import { gitopsQueries } from '../../api/queries'
import type { Application } from '../../api/types'
import {
  APPLICATION_FIELDS,
  applicationControlId,
  toApplicationDraft,
  toApplicationInput,
  validateApplication,
  type ApplicationDraft,
} from '../../lib/application-form'
import { controlProps, FieldMessage, Optional } from '../form-parts'
import { ClusterField } from './cluster-field'
import { SourceFields } from './source-fields'
import { SyncPolicyFields } from './sync-policy-fields'

interface ApplicationEditorProps {
  projectId: string
  /** The application to edit; omitted to declare one. It stays set while the dialog closes. */
  application?: Application
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Declares or edits an application: where it deploys, what it renders, and whether it syncs on its own (admin). */
export function ApplicationEditor({ projectId, application, open, onOpenChange }: ApplicationEditorProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        {/* Mounted per open, so the form starts from the application as it is now. */}
        {open ? <ApplicationForm projectId={projectId} application={application} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

interface ApplicationFormProps {
  projectId: string
  application?: Application
  onDone: () => void
}

function ApplicationForm({ projectId, application, onDone }: ApplicationFormProps) {
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  const save = useSaveApplication()
  // Read only while the editor is open; the pages themselves never ask for the clusters.
  const clusters = useQuery(gitopsQueries.clusters())
  const { data: project } = useQuery(gitopsQueries.project(projectId))
  const [draft, setDraft] = useState<ApplicationDraft>(() => toApplicationDraft(application))
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const clientErrors = submitted ? validateApplication(draft) : {}
  const errors = refused?.field ? { ...clientErrors, [refused.field]: refused.message } : clientErrors
  const cluster = clusters.data?.find((candidate) => candidate.id === draft.clusterId)

  /** Any edit answers the server's last refusal: its message goes until the next save. */
  const change = (patch: Partial<ApplicationDraft>) => {
    setDraft((current) => ({ ...current, ...patch }))
    setRefused(null)
  }
  const focusField = (field: string) => document.getElementById(applicationControlId(field))?.focus()

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    const firstInvalid = Object.keys(validateApplication(draft))[0]
    if (firstInvalid) {
      // Focus lands on the first control with an error, so the message next to it is read out.
      focusField(firstInvalid)
      return
    }
    save.mutate(
      { id: application?.id, input: toApplicationInput(draft, projectId) },
      {
        onSuccess: (saved) => {
          if (application) {
            toast.success(`${saved.name} saved`)
          } else {
            toast.success(`${saved.name} declared`, {
              description: 'It is compared with the cluster now. Nothing is applied until it is synced.',
              action: { label: 'Open', onClick: () => navigate(routes.application(saved.id)) },
            })
          }
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, APPLICATION_FIELDS)
          setRefused(next)
          if (next.field) focusField(next.field)
        },
      },
    )
  }

  const namespaceHint = cluster?.namespaces.length
    ? `This cluster is watched only in ${cluster.namespaces.join(', ')}.`
    : `Empty uses the cluster’s default namespace${cluster?.defaultNamespace ? `, ${cluster.defaultNamespace}` : ''}.`

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>{application ? `Edit ${application.name}` : 'New application'}</DialogTitle>
        <DialogDescription>
          A path in a repository, rendered as Kubernetes manifests and kept in step with one namespace of a cluster. Saving
          applies nothing: a sync does, through an execution, unless automated sync is on.
        </DialogDescription>
      </DialogHeader>

      {/* The fields scroll inside the dialog; the actions stay in reach. */}
      <div className="-mx-4 flex max-h-[min(38rem,calc(100svh-14rem))] flex-col gap-5 overflow-y-auto px-4 py-1">
        {refused && !refused.field ? (
          <Alert variant="destructive">
            <AlertTitle>The application was not saved</AlertTitle>
            <AlertDescription>{refused.message}</AlertDescription>
          </Alert>
        ) : null}

        <FieldGroup className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={errors.name ? true : undefined} className="sm:col-span-2">
            <FieldLabel htmlFor={applicationControlId('name')}>Name</FieldLabel>
            <Input
              {...controlProps(applicationControlId('name'), errors.name)}
              value={draft.name}
              onChange={(event) => change({ name: event.target.value })}
              placeholder="web"
              autoComplete="off"
              spellCheck={false}
              maxLength={63}
              autoFocus={!application}
            />
            <FieldMessage id={applicationControlId('name')} error={errors.name} />
          </Field>
          <ClusterField
            id={applicationControlId('clusterId')}
            value={draft.clusterId}
            onValueChange={(clusterId) => change({ clusterId })}
            clusters={clusters.data ?? []}
            loading={clusters.isPending}
            failed={clusters.isError}
            error={errors.clusterId}
          />
          <Field data-invalid={errors.namespace ? true : undefined}>
            <FieldLabel htmlFor={applicationControlId('namespace')}>
              Namespace <Optional />
            </FieldLabel>
            <Input
              {...controlProps(applicationControlId('namespace'), errors.namespace, true)}
              value={draft.namespace}
              onChange={(event) => change({ namespace: event.target.value })}
              placeholder={cluster?.defaultNamespace ?? 'default'}
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs md:text-xs"
            />
            <FieldMessage id={applicationControlId('namespace')} error={errors.namespace} hint={namespaceHint} />
          </Field>
        </FieldGroup>

        <FieldSeparator />
        <SourceFields draft={draft} onChange={change} errors={errors} project={project} githubHost={currentWorkspace.githubHost} />
        <FieldSeparator />
        <SyncPolicyFields draft={draft} onChange={change} errors={errors} />
      </div>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={save.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={save.isPending}>
          {application ? 'Save application' : 'Create application'}
        </Button>
      </DialogFooter>
    </form>
  )
}
