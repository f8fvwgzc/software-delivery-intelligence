import { KeyRoundIcon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { useSetReasonerKey } from '../../api/mutations'
import type { DataControls } from '../../api/types'

interface ReasonerKeyFormProps {
  workspaceId: string
  controls: DataControls
  disabled: boolean
}

/**
 * The workspace's own model key (BYOK). Stored encrypted, never returned: the
 * only trace is the hint. Its own form, so saving a key never rides along
 * with the controls patch.
 */
export function ReasonerKeyForm({ workspaceId, controls, disabled }: ReasonerKeyFormProps) {
  const id = useId()
  const [apiKey, setApiKey] = useState('')
  const setKey = useSetReasonerKey(workspaceId)

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const trimmed = apiKey.trim()
    if (!trimmed) return
    setKey.mutate(trimmed, { onSuccess: () => setApiKey('') })
  }

  const status = controls.reasonerKeyConfigured
    ? `A key is stored${controls.reasonerKeyHint ? `, ending in ${controls.reasonerKeyHint}` : ''}. It is never shown again; paste a new one to replace it.`
    : 'No key stored. Without one the reasoner cannot reach Claude, whatever the switch says.'

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <Field>
        <FieldLabel htmlFor={id}>Model API key</FieldLabel>
        <div className="flex gap-2">
          <Input
            id={id}
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder={controls.reasonerKeyConfigured ? 'Replace the stored key' : 'sk-ant-…'}
            disabled={disabled || setKey.isPending}
            className="font-mono text-xs"
          />
          <Button
            type="submit"
            size="sm"
            variant="outline"
            className="h-8 shrink-0"
            loading={setKey.isPending && setKey.variables !== ''}
            disabled={disabled || apiKey.trim() === ''}
          >
            <KeyRoundIcon data-icon="inline-start" />
            Store key
          </Button>
        </div>
        <FieldDescription className="text-xs">{status}</FieldDescription>
      </Field>
      {controls.reasonerKeyConfigured ? (
        <div>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="text-muted-foreground"
            loading={setKey.isPending && setKey.variables === ''}
            disabled={disabled}
            onClick={() => setKey.mutate('')}
          >
            Remove stored key
          </Button>
        </div>
      ) : null}
    </form>
  )
}
