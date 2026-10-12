import { useQuery } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldSeparator } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { useSaveEnvironment } from '../../api/mutations'
import { cdQueries } from '../../api/queries'
import type { Environment } from '../../api/types'
import { DEPLOYER_KINDS, DEPLOYERS, deployerOf } from '../../lib/deployers'
import { controlId, fieldAliases, toDraft, toInput, validateDraft, withKind, type EnvironmentDraft } from '../../lib/environment-form'
import { toSaveError, type SaveError } from '../../lib/field-errors'
import { registryPath } from '../../lib/labels'
import { ConfigFields } from './config-fields'
import { SwitchField } from './switch-field'

/** The combobox cannot carry ''; this value stands for the workspace default registry. */
const DEFAULT_REGISTRY = '__default__'

const KIND_OPTIONS: ComboboxOption[] = DEPLOYER_KINDS.map((kind) => ({
  value: kind,
  label: DEPLOYERS[kind].label,
  description: DEPLOYERS[kind].description,
}))

interface EnvironmentEditorProps {
  projectId: string
  /** The environment to edit; omitted to add one, which appends it after the last stage. */
  environment?: Environment
  open: boolean
  onOpenChange: (open: boolean) => void
  onCloseAutoFocus?: (event: Event) => void
}

/** Adds or edits one stage of the pipeline: where it is, which deployer takes a release there, and its gates (admin). */
export function EnvironmentEditor({ projectId, environment, open, onOpenChange, onCloseAutoFocus }: EnvironmentEditorProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl" onCloseAutoFocus={onCloseAutoFocus}>
        {/* Mounted per open, so the form starts from the environment as it is now. */}
        {open ? <EnvironmentForm projectId={projectId} environment={environment} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

interface EnvironmentFormProps {
  projectId: string
  environment?: Environment
  onDone: () => void
}

function EnvironmentForm({ projectId, environment, onDone }: EnvironmentFormProps) {
  const save = useSaveEnvironment(projectId)
  // Read only while the editor is open; the board itself never asks for the registries.
  const registries = useQuery(cdQueries.registries())
  const [draft, setDraft] = useState<EnvironmentDraft>(() => toDraft(environment))
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const deployer = deployerOf(draft.kind)
  const clientErrors = submitted ? validateDraft(draft) : {}
  const errors = refused?.field ? { ...clientErrors, [refused.field]: refused.message } : clientErrors

  /** Any edit answers the server's last refusal: its message goes until the next save. */
  const change = (patch: Partial<EnvironmentDraft>) => {
    setDraft((current) => ({ ...current, ...patch }))
    setRefused(null)
  }

  const kindOptions = DEPLOYERS[draft.kind as keyof typeof DEPLOYERS]
    ? KIND_OPTIONS
    : [{ value: draft.kind, label: draft.kind, description: 'Not a deployer this version knows' }, ...KIND_OPTIONS]

  const defaultRegistry = registries.data?.find((registry) => registry.isDefault)
  const registryOptions: ComboboxOption[] = [
    {
      value: DEFAULT_REGISTRY,
      label: 'Workspace default',
      description: defaultRegistry ? registryPath(defaultRegistry) : 'Whichever registry is the default',
    },
    ...(registries.data ?? []).map((registry) => ({
      value: registry.id,
      label: registry.name,
      description: registryPath(registry),
      keywords: [registry.kind],
    })),
  ]

  const focusField = (field: string) => document.getElementById(controlId(field))?.focus()

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    const firstInvalid = Object.keys(validateDraft(draft))[0]
    if (firstInvalid) {
      // Focus lands on the first control with an error, so the message next to it is read out.
      focusField(firstInvalid)
      return
    }
    save.mutate(
      { id: environment?.id, input: toInput(draft) },
      {
        onSuccess: (saved) => {
          toast.success(environment ? `${saved.name} saved` : `${saved.name} added to the pipeline`)
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, fieldAliases(draft))
          setRefused(next)
          if (next.field) focusField(next.field)
        },
      },
    )
  }

  const fieldProps = (field: string) => ({
    id: controlId(field),
    'aria-invalid': errors[field] ? true : undefined,
    'aria-describedby': errors[field] ? `${controlId(field)}-error` : undefined,
  })

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>{environment ? `Edit ${environment.name}` : 'Add environment'}</DialogTitle>
        <DialogDescription>
          A stage of the pipeline: where a release runs and which deployer takes it there. Secrets are referenced by name.
        </DialogDescription>
      </DialogHeader>

      {/* The fields scroll inside the dialog; the actions stay in reach. */}
      <div className="-mx-4 flex max-h-[min(34rem,calc(100svh-14rem))] flex-col gap-5 overflow-y-auto px-4 py-1">
        {refused && !refused.field ? (
          <Alert variant="destructive">
            <AlertTitle>The environment was not saved</AlertTitle>
            <AlertDescription>{refused.message}</AlertDescription>
          </Alert>
        ) : null}

        <FieldGroup className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={errors.name ? true : undefined}>
            <FieldLabel htmlFor={controlId('name')}>Name</FieldLabel>
            <Input
              {...fieldProps('name')}
              value={draft.name}
              onChange={(event) => change({ name: event.target.value })}
              placeholder="staging"
              autoComplete="off"
              spellCheck={false}
              maxLength={60}
              autoFocus={!environment}
            />
            {errors.name ? (
              <FieldError id={`${controlId('name')}-error`} className="text-xs">
                {errors.name}
              </FieldError>
            ) : null}
          </Field>
          <Field data-invalid={errors.kind ? true : undefined}>
            <FieldLabel htmlFor={controlId('kind')}>Deployer</FieldLabel>
            <ComboboxField
              {...fieldProps('kind')}
              value={draft.kind}
              onValueChange={(kind) => {
                setDraft((current) => withKind(current, kind))
                setRefused(null)
              }}
              options={kindOptions}
              searchPlaceholder="Search deployers"
              emptyText="No deployer matches."
            />
            {errors.kind ? (
              <FieldError id={`${controlId('kind')}-error`} className="text-xs">
                {errors.kind}
              </FieldError>
            ) : deployer.description ? (
              <FieldDescription className="text-xs">{deployer.description}.</FieldDescription>
            ) : null}
          </Field>
        </FieldGroup>

        <ConfigFields
          kind={draft.kind}
          config={draft.config}
          onChange={(key, value) => change({ config: { ...draft.config, [key]: value } })}
          errors={errors}
        />

        <FieldSeparator />

        <FieldGroup className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={errors.url ? true : undefined}>
            <FieldLabel htmlFor={controlId('url')}>
              URL <span className="font-normal text-muted-foreground">optional</span>
            </FieldLabel>
            <Input
              {...fieldProps('url')}
              type="url"
              inputMode="url"
              value={draft.url}
              onChange={(event) => change({ url: event.target.value })}
              placeholder="https://staging.example.com"
              autoComplete="off"
              spellCheck={false}
            />
            {errors.url ? (
              <FieldError id={`${controlId('url')}-error`} className="text-xs">
                {errors.url}
              </FieldError>
            ) : (
              <FieldDescription className="text-xs">Where the environment answers.</FieldDescription>
            )}
          </Field>
          <Field data-invalid={errors.healthPath ? true : undefined}>
            <FieldLabel htmlFor={controlId('healthPath')}>
              Health path <span className="font-normal text-muted-foreground">optional</span>
            </FieldLabel>
            <Input
              {...fieldProps('healthPath')}
              value={draft.healthPath}
              onChange={(event) => change({ healthPath: event.target.value })}
              placeholder="/health"
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs md:text-xs"
            />
            {errors.healthPath ? (
              <FieldError id={`${controlId('healthPath')}-error`} className="text-xs">
                {errors.healthPath}
              </FieldError>
            ) : (
              <FieldDescription className="text-xs">Polled for a 2xx after each deploy; skipped without a URL.</FieldDescription>
            )}
          </Field>
          <Field data-invalid={errors.registryId ? true : undefined} className="sm:col-span-2">
            <FieldLabel htmlFor={controlId('registryId')}>Registry</FieldLabel>
            <ComboboxField
              {...fieldProps('registryId')}
              value={draft.registryId || DEFAULT_REGISTRY}
              onValueChange={(value) => change({ registryId: value === DEFAULT_REGISTRY ? '' : value })}
              options={registryOptions}
              loading={registries.isPending}
              searchPlaceholder="Search registries"
              emptyText="No registry matches."
            />
            {errors.registryId ? (
              <FieldError id={`${controlId('registryId')}-error`} className="text-xs">
                {errors.registryId}
              </FieldError>
            ) : null}
          </Field>
        </FieldGroup>

        <FieldGroup className="gap-4">
          <SwitchField
            id={controlId('protected')}
            label="Protected"
            description="Production semantics: every deployment here needs an approval, whatever the other policies say."
            checked={draft.protected}
            onCheckedChange={(value) => change({ protected: value })}
          />
          <SwitchField
            id={controlId('autoPromote')}
            label="Auto promote"
            description="Promote into this stage as soon as the previous one succeeds. The run still passes policy and approval."
            checked={draft.autoPromote}
            onCheckedChange={(value) => change({ autoPromote: value })}
          />
        </FieldGroup>
      </div>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={save.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={save.isPending}>
          {environment ? 'Save environment' : 'Add environment'}
        </Button>
      </DialogFooter>
    </form>
  )
}
