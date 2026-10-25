import { FileUpIcon, XIcon } from 'lucide-react'
import { useId, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent } from 'react'

import { normalizeSecretName, SECRET_NAME_HINT, SECRET_NAME_PATTERN } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { formatNumber } from '@/lib/intl'

/** A kubeconfig is a few kilobytes; anything near this is not one. */
const MAX_BYTES = 1024 * 1024
/** Every kubeconfig, YAML or JSON, names its clusters. */
const LOOKS_LIKE_KUBECONFIG = /\bclusters\b/

interface Received {
  value: string
  /** Where it came from: a file name, or "pasted". */
  from: string
}

interface StoreKubeconfigProps {
  /** Stores the secret and resolves once it exists. */
  onStore: (secret: { name: string; value: string }) => Promise<unknown>
  /** Names that exist already: storing one of them replaces its value. */
  taken: readonly string[]
  /** Called with the new name once stored, or with nothing on cancel. */
  onDone: (name?: string) => void
}

/**
 * Stores a kubeconfig as a workspace secret without leaving the cluster
 * editor. It is pasted or read from a file, held in memory, sent once and
 * never rendered: the box below only ever says how much it received. A
 * kubeconfig spans many lines, which a password input would join into one.
 * Not a `<form>`: it sits inside the editor's own.
 */
export function StoreKubeconfig({ onStore, taken, onDone }: StoreKubeconfigProps) {
  const id = useId()
  const fileRef = useRef<HTMLInputElement>(null)
  const [name, setName] = useState('')
  const [received, setReceived] = useState<Received | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const nameError = name && !SECRET_NAME_PATTERN.test(name) ? SECRET_NAME_HINT : null
  const contentError = !received
    ? null
    : received.value.length > MAX_BYTES
      ? 'This is larger than any kubeconfig. Choose the kubeconfig file itself.'
      : !LOOKS_LIKE_KUBECONFIG.test(received.value)
        ? 'This is not a kubeconfig: it names no clusters.'
        : null
  const ready = SECRET_NAME_PATTERN.test(name) && received !== null && contentError === null
  const replaces = taken.includes(name)

  const receive = (value: string, from: string) => {
    setError(null)
    setReceived(value.trim() ? { value, from } : null)
  }

  const readFile = async (file: File | undefined) => {
    if (!file) return
    if (file.size > MAX_BYTES) {
      setReceived(null)
      setError('This file is larger than any kubeconfig.')
      return
    }
    receive(await file.text(), file.name)
  }

  const onPaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    // The text never enters the box: it would be on screen otherwise.
    event.preventDefault()
    receive(event.clipboardData.getData('text/plain'), 'pasted')
  }
  const onDrop = (event: DragEvent<HTMLTextAreaElement>) => {
    event.preventDefault()
    const file = event.dataTransfer.files[0]
    if (file) void readFile(file)
    else receive(event.dataTransfer.getData('text/plain'), 'dropped')
  }
  const onNameKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return
    // Enter must not submit the editor this sits in.
    event.preventDefault()
    void save()
  }

  const save = async () => {
    if (!ready || saving || !received) return
    setSaving(true)
    setError(null)
    try {
      await onStore({ name, value: received.value })
      setReceived(null)
      onDone(name)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'The kubeconfig could not be stored.')
    } finally {
      setSaving(false)
    }
  }

  const lines = received ? received.value.split('\n').length : 0
  const status = received
    ? `Received ${received.from === 'pasted' || received.from === 'dropped' ? `(${received.from})` : `from ${received.from}`}: ${formatNumber(
        new Blob([received.value]).size,
      )} bytes, ${formatNumber(lines)} lines. It is not shown.`
    : 'Paste it here or choose the file. It is kept in memory, sent once to be stored encrypted, and never shown.'

  return (
    <div role="group" aria-label="Store a kubeconfig as a workspace secret" className="flex flex-col gap-3 rounded-md bg-muted/50 p-3 ring-1 ring-border ring-inset">
      <Field data-invalid={nameError ? true : undefined}>
        <FieldLabel htmlFor={`${id}-name`} className="text-xs">
          Secret name
        </FieldLabel>
        <Input
          id={`${id}-name`}
          value={name}
          onChange={(event) => setName(normalizeSecretName(event.target.value))}
          onKeyDown={onNameKeyDown}
          placeholder="KUBECONFIG_PRODUCTION"
          autoComplete="off"
          spellCheck={false}
          autoFocus
          aria-invalid={nameError ? true : undefined}
          aria-describedby={`${id}-name-hint`}
          className="bg-background font-mono text-xs md:text-xs"
        />
        <FieldDescription id={`${id}-name-hint`} className="text-xs">
          {nameError ?? (replaces ? `${name} exists already; storing replaces its value.` : SECRET_NAME_HINT)}
        </FieldDescription>
      </Field>

      <Field data-invalid={contentError ? true : undefined}>
        <FieldLabel htmlFor={`${id}-kubeconfig`} className="text-xs">
          Kubeconfig
        </FieldLabel>
        <div className="flex items-start gap-2">
          <Textarea
            id={`${id}-kubeconfig`}
            // Always empty: what was pasted or dropped is held in state, never rendered.
            value=""
            onChange={() => undefined}
            onPaste={onPaste}
            onDrop={onDrop}
            placeholder={received ? 'Kubeconfig received' : 'Paste the kubeconfig'}
            rows={2}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={contentError ? true : undefined}
            aria-describedby={`${id}-kubeconfig-hint`}
            className="min-h-14 flex-1 resize-none bg-background text-xs md:text-xs"
          />
          <div className="flex shrink-0 flex-col gap-1.5">
            <Button type="button" variant="outline" size="sm" onClick={() => fileRef.current?.click()}>
              <FileUpIcon data-icon="inline-start" />
              Choose file
            </Button>
            {received ? (
              <Button type="button" variant="ghost" size="sm" onClick={() => setReceived(null)}>
                <XIcon data-icon="inline-start" />
                Clear
              </Button>
            ) : null}
          </div>
          <input
            ref={fileRef}
            type="file"
            className="sr-only"
            tabIndex={-1}
            aria-hidden
            onChange={(event) => {
              void readFile(event.target.files?.[0])
              event.target.value = ''
            }}
          />
        </div>
        {contentError ? (
          <FieldError id={`${id}-kubeconfig-hint`} className="text-xs">
            {contentError}
          </FieldError>
        ) : (
          <FieldDescription id={`${id}-kubeconfig-hint`} className="text-xs" aria-live="polite">
            {status}
          </FieldDescription>
        )}
      </Field>

      {error ? (
        <FieldError className="text-xs" role="alert">
          {error}
        </FieldError>
      ) : null}
      <div className="flex items-center gap-2">
        <Button type="button" size="sm" onClick={save} loading={saving} disabled={!ready}>
          Store secret
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={() => onDone()} disabled={saving}>
          Cancel
        </Button>
      </div>
    </div>
  )
}
