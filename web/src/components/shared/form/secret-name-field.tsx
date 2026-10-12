import { PlusIcon } from 'lucide-react'
import { useId, useState, type KeyboardEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { ComboboxField, type ComboboxOption } from './combobox-field'
import { normalizeSecretName, SECRET_NAME_HINT, SECRET_NAME_PATTERN } from './secret-name'

/** The combobox cannot carry ''; this value stands for "no secret". */
const NONE = '__none__'

export interface SecretOption {
  name: string
  /** Last characters of the value, for recognition only. */
  hint?: string | null
}

export interface SecretNameFieldProps {
  /** Id of the control, so a `FieldLabel htmlFor` names it. */
  id: string
  /** The chosen secret name; '' for none. */
  value: string
  onValueChange: (name: string) => void
  /** The workspace's secrets: names and hints, never values. */
  secrets: readonly SecretOption[]
  loading?: boolean
  /** Set when the names could not be read: the field then takes a typed name. */
  unavailable?: boolean
  /** Offers "None", for a deployer that works without this secret. */
  clearable?: boolean
  /** Stores a new secret and resolves once it exists; omitted when the viewer may not add one. */
  onAdd?: (secret: { name: string; value: string }) => Promise<unknown>
  disabled?: boolean
  'aria-invalid'?: boolean
  'aria-describedby'?: string
}

/**
 * Picks a workspace secret by name. Deployers and registries reference
 * secrets, never hold them, so this is a list of names with "Add secret"
 * beside it: a name and a write-only value, stored without leaving the
 * form that needs it.
 */
export function SecretNameField({
  id,
  value,
  onValueChange,
  secrets,
  loading = false,
  unavailable = false,
  clearable = false,
  onAdd,
  disabled = false,
  'aria-invalid': ariaInvalid,
  'aria-describedby': ariaDescribedBy,
}: SecretNameFieldProps) {
  const [adding, setAdding] = useState(false)

  const options: ComboboxOption[] = secrets.map((secret) => ({
    value: secret.name,
    label: secret.name,
    description: secret.hint ? `ends in ${secret.hint}` : undefined,
  }))
  // A name the config already holds stays selectable even when the secret was deleted since.
  if (value && !secrets.some((secret) => secret.name === value)) {
    options.unshift({ value, label: value, description: 'Not set in this workspace' })
  }
  if (clearable) options.unshift({ value: NONE, label: 'None', description: 'Runs without this secret' })

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex min-w-0 items-center gap-2">
        {unavailable ? (
          <Input
            id={id}
            value={value}
            onChange={(event) => onValueChange(normalizeSecretName(event.target.value))}
            placeholder="SECRET_NAME"
            autoComplete="off"
            spellCheck={false}
            disabled={disabled}
            aria-invalid={ariaInvalid || undefined}
            aria-describedby={ariaDescribedBy}
            className="font-mono text-xs md:text-xs"
          />
        ) : (
          <ComboboxField
            id={id}
            mono
            value={value || (clearable ? NONE : '')}
            onValueChange={(next) => onValueChange(next === NONE ? '' : next)}
            options={options}
            placeholder="Choose a secret"
            searchPlaceholder="Search secrets"
            emptyText="No secret with that name."
            loading={loading}
            disabled={disabled}
            aria-invalid={ariaInvalid}
            aria-describedby={ariaDescribedBy}
          />
        )}
        {onAdd && !adding ? (
          <Button type="button" variant="outline" className="shrink-0" onClick={() => setAdding(true)} disabled={disabled}>
            <PlusIcon data-icon="inline-start" />
            Add secret
          </Button>
        ) : null}
      </div>
      {onAdd && adding ? (
        <AddSecretInline
          onAdd={onAdd}
          taken={secrets.map((secret) => secret.name)}
          onDone={(name) => {
            setAdding(false)
            if (name) onValueChange(name)
          }}
        />
      ) : null}
    </div>
  )
}

interface AddSecretInlineProps {
  onAdd: (secret: { name: string; value: string }) => Promise<unknown>
  /** Names that exist already: saving one of these would replace its value. */
  taken: readonly string[]
  /** Called with the new name once stored, or with nothing on cancel. */
  onDone: (name?: string) => void
}

/** Not a `<form>`: it sits inside the editor's own form. Enter in either input saves. */
function AddSecretInline({ onAdd, taken, onDone }: AddSecretInlineProps) {
  const id = useId()
  const [name, setName] = useState('')
  const [secretValue, setSecretValue] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const replaces = taken.includes(name)
  const nameError = name && !SECRET_NAME_PATTERN.test(name) ? SECRET_NAME_HINT : null
  const ready = SECRET_NAME_PATTERN.test(name) && secretValue.length > 0

  const save = async () => {
    if (!ready || saving) return
    setSaving(true)
    setError(null)
    try {
      await onAdd({ name, value: secretValue })
      setSecretValue('')
      onDone(name)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'The secret could not be stored.')
    } finally {
      setSaving(false)
    }
  }

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return
    // Enter must not submit the form this sits in.
    event.preventDefault()
    void save()
  }

  return (
    <div role="group" aria-label="Add a workspace secret" className="flex flex-col gap-3 rounded-md bg-muted/50 p-3 ring-1 ring-border ring-inset">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field data-invalid={nameError ? true : undefined}>
          <FieldLabel htmlFor={`${id}-name`} className="text-xs">
            Name
          </FieldLabel>
          <Input
            id={`${id}-name`}
            value={name}
            onChange={(event) => setName(normalizeSecretName(event.target.value))}
            onKeyDown={onKeyDown}
            placeholder="JENKINS_TOKEN"
            autoComplete="off"
            spellCheck={false}
            autoFocus
            aria-invalid={nameError ? true : undefined}
            aria-describedby={`${id}-hint`}
            className="bg-background font-mono text-xs md:text-xs"
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${id}-value`} className="text-xs">
            Value
          </FieldLabel>
          <Input
            id={`${id}-value`}
            type="password"
            value={secretValue}
            onChange={(event) => setSecretValue(event.target.value)}
            onKeyDown={onKeyDown}
            autoComplete="new-password"
            spellCheck={false}
            aria-describedby={`${id}-hint`}
            className="bg-background font-mono text-xs md:text-xs"
          />
        </Field>
      </div>
      {error ? (
        <FieldError className="text-xs">{error}</FieldError>
      ) : (
        <FieldDescription id={`${id}-hint`} className="text-xs">
          {nameError ?? (replaces ? `${name} exists already; saving replaces its value.` : 'Stored encrypted. The value is never shown again.')}
        </FieldDescription>
      )}
      <div className="flex items-center gap-2">
        <Button type="button" size="sm" onClick={save} loading={saving} disabled={!ready}>
          Save secret
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={() => onDone()} disabled={saving}>
          Cancel
        </Button>
      </div>
    </div>
  )
}
