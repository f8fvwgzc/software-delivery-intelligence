import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { ComboboxField, SecretNameField, type ComboboxOption } from '@/components/shared/form'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldContent, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useSaveRegistry } from '@/features/delivery-cd/api/mutations'
import type { Registry, RegistryKind } from '@/features/delivery-cd/api/types'
import { toSaveError, type SaveError } from '@/features/delivery-cd/lib/field-errors'
import { REGISTRY_AUTH, REGISTRY_KIND_ORDER, REGISTRY_KINDS, registryKind } from '@/features/delivery-cd/lib/registries'
import { useSecretField } from '@/features/delivery-cd/lib/use-secret-field'

import {
  REGISTRY_FIELDS,
  registryControlId,
  toRegistryDraft,
  toRegistryInput,
  validateRegistry,
  withRegistryKind,
  type RegistryDraft,
} from '../../lib/registry-form'

const KIND_OPTIONS: ComboboxOption[] = REGISTRY_KIND_ORDER.map((kind: RegistryKind) => ({
  value: kind,
  label: REGISTRY_KINDS[kind].label,
  description: REGISTRY_KINDS[kind].description,
}))

interface RegistryEditorProps {
  /** The registry to edit; omitted to add one. It stays set while the dialog closes. */
  registry?: Registry
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Adds or edits a registry: which one, where images go in it, and how CI and the verifier sign in (admin). */
export function RegistryEditor({ registry, open, onOpenChange }: RegistryEditorProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        {open ? <RegistryForm registry={registry} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

function RegistryForm({ registry, onDone }: { registry?: Registry; onDone: () => void }) {
  const save = useSaveRegistry()
  const [draft, setDraft] = useState<RegistryDraft>(() => toRegistryDraft(registry))
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const kind = registryKind(draft.kind)
  const bySecret = draft.auth === 'secret'
  // The secret names are read only once the registry signs in with secrets.
  const secretField = useSecretField(bySecret)
  const clientErrors = submitted ? validateRegistry(draft) : {}
  const errors = refused?.field ? { ...clientErrors, [refused.field]: refused.message } : clientErrors

  const change = (patch: Partial<RegistryDraft>) => {
    setDraft((current) => ({ ...current, ...patch }))
    setRefused(null)
  }
  const focusField = (field: string) => document.getElementById(registryControlId(field))?.focus()

  const authOptions: ComboboxOption[] = kind.auth.map((auth) => ({ value: auth, ...REGISTRY_AUTH[auth] }))

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    const firstInvalid = Object.keys(validateRegistry(draft))[0]
    if (firstInvalid) {
      focusField(firstInvalid)
      return
    }
    save.mutate(
      { id: registry?.id, input: toRegistryInput(draft) },
      {
        onSuccess: (saved) => {
          toast.success(registry ? `${saved.name} saved` : `${saved.name} added`)
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, REGISTRY_FIELDS)
          setRefused(next)
          if (next.field) focusField(next.field)
        },
      },
    )
  }

  const fieldProps = (field: string) => ({
    id: registryControlId(field),
    'aria-invalid': errors[field] ? true : undefined,
    'aria-describedby': errors[field] ? `${registryControlId(field)}-error` : undefined,
  })
  const errorOf = (field: string) =>
    errors[field] ? (
      <FieldError id={`${registryControlId(field)}-error`} className="text-xs">
        {errors[field]}
      </FieldError>
    ) : null

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>{registry ? `Edit ${registry.name}` : 'Add registry'}</DialogTitle>
        <DialogDescription>
          Where images are pushed and verified. Credentials are never typed here: a registry names the workspace secrets that
          hold them.
        </DialogDescription>
      </DialogHeader>

      {refused && !refused.field ? (
        <Alert variant="destructive">
          <AlertTitle>The registry was not saved</AlertTitle>
          <AlertDescription>{refused.message}</AlertDescription>
        </Alert>
      ) : null}

      <FieldGroup className="grid gap-4 sm:grid-cols-2">
        <Field data-invalid={errors.name ? true : undefined}>
          <FieldLabel htmlFor={registryControlId('name')}>Name</FieldLabel>
          <Input
            {...fieldProps('name')}
            value={draft.name}
            onChange={(event) => change({ name: event.target.value })}
            placeholder="Production images"
            autoComplete="off"
            maxLength={60}
            autoFocus={!registry}
          />
          {errorOf('name')}
        </Field>
        <Field data-invalid={errors.kind ? true : undefined}>
          <FieldLabel htmlFor={registryControlId('kind')}>Kind</FieldLabel>
          <ComboboxField
            {...fieldProps('kind')}
            value={draft.kind}
            onValueChange={(next) => {
              setDraft((current) => withRegistryKind(current, next))
              setRefused(null)
            }}
            options={KIND_OPTIONS}
            searchPlaceholder="Search registries"
            emptyText="No registry kind matches."
          />
          {errorOf('kind')}
        </Field>
        <Field data-invalid={errors.host ? true : undefined}>
          <FieldLabel htmlFor={registryControlId('host')}>Host</FieldLabel>
          <Input
            {...fieldProps('host')}
            value={draft.host}
            onChange={(event) => change({ host: event.target.value })}
            placeholder={kind.hostPlaceholder}
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs md:text-xs"
          />
          {errorOf('host')}
        </Field>
        <Field data-invalid={errors.namespace ? true : undefined}>
          <FieldLabel htmlFor={registryControlId('namespace')}>Namespace</FieldLabel>
          <Input
            {...fieldProps('namespace')}
            value={draft.namespace}
            onChange={(event) => change({ namespace: event.target.value })}
            placeholder={kind.namespacePlaceholder}
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs md:text-xs"
          />
          {errorOf('namespace') ?? <FieldDescription className="text-xs">Images go to host/namespace/repository.</FieldDescription>}
        </Field>
        <Field data-invalid={errors.auth ? true : undefined} className="sm:col-span-2">
          <FieldLabel htmlFor={registryControlId('auth')}>Sign in with</FieldLabel>
          <ComboboxField
            {...fieldProps('auth')}
            value={draft.auth}
            onValueChange={(auth) => change({ auth })}
            options={authOptions}
            disabled={authOptions.length === 1}
            searchPlaceholder="Search"
          />
          {errorOf('auth') ?? (
            <FieldDescription className="text-xs">{REGISTRY_AUTH[draft.auth as keyof typeof REGISTRY_AUTH]?.description}.</FieldDescription>
          )}
        </Field>
        {bySecret ? (
          <>
            <Field data-invalid={errors.usernameSecret ? true : undefined} className="sm:col-span-2">
              <FieldLabel htmlFor={registryControlId('usernameSecret')}>Username secret</FieldLabel>
              <SecretNameField
                {...fieldProps('usernameSecret')}
                value={draft.usernameSecret}
                onValueChange={(usernameSecret) => change({ usernameSecret })}
                {...secretField}
              />
              {errorOf('usernameSecret')}
            </Field>
            <Field data-invalid={errors.passwordSecret ? true : undefined} className="sm:col-span-2">
              <FieldLabel htmlFor={registryControlId('passwordSecret')}>Password secret</FieldLabel>
              <SecretNameField
                {...fieldProps('passwordSecret')}
                value={draft.passwordSecret}
                onValueChange={(passwordSecret) => change({ passwordSecret })}
                {...secretField}
              />
              {errorOf('passwordSecret') ?? <FieldDescription className="text-xs">A password or an access token.</FieldDescription>}
            </Field>
          </>
        ) : null}
        <Field orientation="horizontal" className="sm:col-span-2">
          <FieldContent>
            <FieldLabel htmlFor={registryControlId('isDefault')}>Workspace default</FieldLabel>
            <FieldDescription id={`${registryControlId('isDefault')}-hint`} className="text-xs">
              Releases of every project go here unless an environment names another registry. There is always exactly one.
            </FieldDescription>
          </FieldContent>
          <Switch
            id={registryControlId('isDefault')}
            checked={draft.isDefault}
            onCheckedChange={(isDefault) => change({ isDefault })}
            // The default can be moved to another registry, not switched off.
            disabled={Boolean(registry?.isDefault)}
            aria-describedby={`${registryControlId('isDefault')}-hint`}
          />
        </Field>
      </FieldGroup>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={save.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={save.isPending}>
          {registry ? 'Save registry' : 'Add registry'}
        </Button>
      </DialogFooter>
    </form>
  )
}
