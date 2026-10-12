import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { normalizeSecretName, SECRET_NAME_HINT, SECRET_NAME_PATTERN } from '@/components/shared/form'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useSetSecret } from '@/features/delivery-cd/api/mutations'
import type { SecretInfo } from '@/features/delivery-cd/api/types'
import { aliasesOf, toSaveError, type SaveError } from '@/features/delivery-cd/lib/field-errors'

const FIELDS = ['name', 'value', 'description'].map((field) => ({ field, aliases: aliasesOf(field) }))

interface SetSecretDialogProps {
  workspaceId: string
  /** The secret whose value is replaced; omitted to add one. */
  secret?: SecretInfo
  /** Names that exist already: saving one of them replaces its value. */
  taken: readonly string[]
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Stores a workspace secret (admin). The value is write-only: after saving, only its last four characters are ever shown. */
export function SetSecretDialog({ workspaceId, secret, taken, open, onOpenChange }: SetSecretDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        {/* Mounted per open, so a value typed earlier never comes back. */}
        {open ? <SetSecretForm workspaceId={workspaceId} secret={secret} taken={taken} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

interface SetSecretFormProps {
  workspaceId: string
  secret?: SecretInfo
  taken: readonly string[]
  onDone: () => void
}

function SetSecretForm({ workspaceId, secret, taken, onDone }: SetSecretFormProps) {
  const id = useId()
  const setSecret = useSetSecret(workspaceId)
  const [name, setName] = useState(secret?.name ?? '')
  const [value, setValue] = useState('')
  const [description, setDescription] = useState(secret?.description ?? '')
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const replaces = !secret && taken.includes(name)
  const nameError =
    refused?.field === 'name' ? refused.message : submitted && !SECRET_NAME_PATTERN.test(name) ? (name ? SECRET_NAME_HINT : 'Give the secret a name.') : null
  const valueError = refused?.field === 'value' ? refused.message : submitted && !value ? 'Enter the value to store.' : null

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    if (!SECRET_NAME_PATTERN.test(name)) {
      document.getElementById(`${id}-name`)?.focus()
      return
    }
    if (!value) {
      document.getElementById(`${id}-value`)?.focus()
      return
    }
    setSecret.mutate(
      { name, value, description },
      {
        onSuccess: (stored) => {
          toast.success(`${stored.name} stored`, { description: 'Encrypted. The value is not shown again.' })
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, FIELDS)
          setRefused(next)
          if (next.field) document.getElementById(`${id}-${next.field}`)?.focus()
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>{secret ? `Replace ${secret.name}` : 'Set secret'}</DialogTitle>
        <DialogDescription>
          Stored encrypted for this workspace. Deployers and registries refer to it by name; the value is never returned, here
          or through the API.
        </DialogDescription>
      </DialogHeader>
      {refused && !refused.field ? (
        <Alert variant="destructive">
          <AlertTitle>The secret was not stored</AlertTitle>
          <AlertDescription>{refused.message}</AlertDescription>
        </Alert>
      ) : null}
      <FieldGroup className="gap-4">
        <Field data-invalid={nameError ? true : undefined} data-disabled={secret ? true : undefined}>
          <FieldLabel htmlFor={`${id}-name`}>Name</FieldLabel>
          <Input
            id={`${id}-name`}
            value={name}
            onChange={(event) => {
              setName(normalizeSecretName(event.target.value))
              setRefused(null)
            }}
            placeholder="JENKINS_TOKEN"
            autoComplete="off"
            spellCheck={false}
            maxLength={64}
            disabled={Boolean(secret)}
            autoFocus={!secret}
            aria-invalid={nameError ? true : undefined}
            aria-describedby={`${id}-name-hint`}
            className="font-mono text-xs md:text-xs"
          />
          {nameError ? (
            <FieldError id={`${id}-name-hint`} className="text-xs">
              {nameError}
            </FieldError>
          ) : (
            <FieldDescription id={`${id}-name-hint`} className="text-xs">
              {replaces ? `${name} exists already; saving replaces its value.` : SECRET_NAME_HINT}
            </FieldDescription>
          )}
        </Field>
        <Field data-invalid={valueError ? true : undefined}>
          <FieldLabel htmlFor={`${id}-value`}>Value</FieldLabel>
          <Input
            id={`${id}-value`}
            type="password"
            value={value}
            onChange={(event) => {
              setValue(event.target.value)
              setRefused(null)
            }}
            autoComplete="new-password"
            spellCheck={false}
            autoFocus={Boolean(secret)}
            aria-invalid={valueError ? true : undefined}
            aria-describedby={`${id}-value-hint`}
            className="font-mono text-xs md:text-xs"
          />
          {valueError ? (
            <FieldError id={`${id}-value-hint`} className="text-xs">
              {valueError}
            </FieldError>
          ) : (
            <FieldDescription id={`${id}-value-hint`} className="text-xs">
              Write-only. To change it later, set the secret again.
            </FieldDescription>
          )}
        </Field>
        <Field>
          <FieldLabel htmlFor={`${id}-description`}>
            Description <span className="font-normal text-muted-foreground">optional</span>
          </FieldLabel>
          <Input
            id={`${id}-description`}
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="What uses it, and who can rotate it"
            autoComplete="off"
            maxLength={200}
          />
        </Field>
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={setSecret.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={setSecret.isPending}>
          {secret ? 'Replace value' : 'Store secret'}
        </Button>
      </DialogFooter>
    </form>
  )
}
