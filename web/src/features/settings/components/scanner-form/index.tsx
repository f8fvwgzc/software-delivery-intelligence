import { useId, useState, type FormEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'

import { useUpdateSettings } from '../../api/mutations'
import type { KnowledgeSettings, KnowledgeSettingsPatch } from '../../api/queries'
import { SCANNER_FIELDS, type ScannerNumberKey } from '../../lib/settings'

type Draft = Record<ScannerNumberKey, string> & { cacheGithub: boolean }

function toDraft(settings: KnowledgeSettings): Draft {
  return {
    scanMaxFiles: String(settings.scanMaxFiles),
    scanMaxFileBytes: String(settings.scanMaxFileBytes),
    scanConcurrency: String(settings.scanConcurrency),
    pipelineRunsLimit: String(settings.pipelineRunsLimit),
    cacheGithub: settings.cacheGithub,
  }
}

function parseInteger(value: string, min: number): number | null {
  if (!/^\d+$/.test(value.trim())) return null
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) && parsed >= min ? parsed : null
}

/** Only what changed goes over the wire; omitted fields keep their value. */
function toPatch(draft: Draft, settings: KnowledgeSettings): KnowledgeSettingsPatch | null {
  const patch: KnowledgeSettingsPatch = {}
  for (const field of SCANNER_FIELDS) {
    const parsed = parseInteger(draft[field.key], field.min)
    if (parsed === null) return null
    if (parsed !== settings[field.key]) patch[field.key] = parsed
  }
  if (draft.cacheGithub !== settings.cacheGithub) patch.cacheGithub = draft.cacheGithub
  return patch
}

/** Scanner limits of the knowledge service. Save sends the changed fields only. */
export function ScannerForm({ settings }: { settings: KnowledgeSettings }) {
  const id = useId()
  const [draft, setDraft] = useState<Draft>(() => toDraft(settings))
  const update = useUpdateSettings('Scanner')

  const patch = toPatch(draft, settings)
  const valid = patch !== null
  const dirty = valid && Object.keys(patch).length > 0

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!patch || !dirty) return
    update.mutate({ knowledge: patch })
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-4 rounded-lg p-4 ring-1 ring-border">
      <FieldGroup className="gap-4">
        <div className="grid gap-4 sm:grid-cols-2">
          {SCANNER_FIELDS.map((field) => {
            const invalid = parseInteger(draft[field.key], field.min) === null
            return (
              <Field key={field.key} data-invalid={invalid || undefined}>
                <FieldLabel htmlFor={`${id}-${field.key}`}>{field.label}</FieldLabel>
                <Input
                  id={`${id}-${field.key}`}
                  type="number"
                  inputMode="numeric"
                  min={field.min}
                  step={field.step}
                  value={draft[field.key]}
                  onChange={(event) => setDraft((current) => ({ ...current, [field.key]: event.target.value }))}
                  aria-invalid={invalid || undefined}
                  className="tabular-nums"
                />
                <FieldDescription className="text-xs">
                  {invalid ? `A whole number of at least ${field.min}.` : field.description}
                </FieldDescription>
              </Field>
            )
          })}
        </div>
        <Field orientation="horizontal">
          <Switch
            id={`${id}-cache`}
            checked={draft.cacheGithub}
            onCheckedChange={(checked) => setDraft((current) => ({ ...current, cacheGithub: checked }))}
          />
          <div className="flex flex-col gap-0.5">
            <FieldLabel htmlFor={`${id}-cache`} className="font-normal">
              GitHub cache
            </FieldLabel>
            <FieldDescription className="text-xs">ETag caching of GitHub API responses.</FieldDescription>
          </div>
        </Field>
      </FieldGroup>
      <div className="flex items-center justify-end gap-3">
        <Button type="submit" size="sm" loading={update.isPending} disabled={!dirty}>
          Save scanner
        </Button>
      </div>
    </form>
  )
}
