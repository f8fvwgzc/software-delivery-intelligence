import { useId, useState, type FormEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { useUpdateDataControls } from '../../api/mutations'
import type { DataControls, UpdateDataControlsRequest } from '../../api/types'
import { CODE_CACHE_OPTIONS, CODE_TO_MODEL_OPTIONS, codeToModelMode, REASONER_OPTIONS, type CodeToModelMode } from '../../lib/labels'
import { ReasonerKeyForm } from './reasoner-key-form'

interface Draft {
  codeCache: string
  codeCacheTtlHours: string
  minimalChunks: boolean
  reasoner: string
  codeToModel: CodeToModelMode
  auditRetentionDays: string
}

const TTL = { min: 1, max: 8760 }
const RETENTION = { min: 0, max: 3650 }

function toDraft(controls: DataControls): Draft {
  return {
    codeCache: controls.codeCache,
    codeCacheTtlHours: String(controls.codeCacheTtlHours),
    minimalChunks: controls.minimalChunks,
    reasoner: controls.reasoner,
    codeToModel: codeToModelMode(controls.codeToModel),
    auditRetentionDays: String(controls.auditRetentionDays),
  }
}

function parseInteger(value: string, range: { min: number; max: number }): number | null {
  if (!/^\d+$/.test(value.trim())) return null
  const parsed = Number(value)
  return parsed >= range.min && parsed <= range.max ? parsed : null
}

/** Changed fields only; null while a number is out of range. */
function toPatch(draft: Draft, controls: DataControls): UpdateDataControlsRequest | null {
  const ttl = parseInteger(draft.codeCacheTtlHours, TTL)
  const retention = parseInteger(draft.auditRetentionDays, RETENTION)
  if (ttl === null || retention === null) return null

  const patch: UpdateDataControlsRequest = {}
  if (draft.codeCache !== controls.codeCache) patch.codeCache = draft.codeCache
  if (ttl !== controls.codeCacheTtlHours) patch.codeCacheTtlHours = ttl
  if (draft.minimalChunks !== controls.minimalChunks) patch.minimalChunks = draft.minimalChunks
  if (draft.reasoner !== controls.reasoner) patch.reasoner = draft.reasoner
  if (draft.codeToModel !== codeToModelMode(controls.codeToModel)) patch.codeToModel = draft.codeToModel
  if (retention !== controls.auditRetentionDays) patch.auditRetentionDays = retention
  return patch
}

interface DataControlsFormProps {
  workspaceId: string
  controls: DataControls
  /** Below admin the form is read-only; the gateway would refuse anyway. */
  readOnly: boolean
}

/** The §19 switches, and §23's one about source lines. Save sends what changed; the key has its own form below. */
export function DataControlsForm({ workspaceId, controls, readOnly }: DataControlsFormProps) {
  const id = useId()
  const [draft, setDraft] = useState<Draft>(() => toDraft(controls))
  const update = useUpdateDataControls(workspaceId)

  const patch = toPatch(draft, controls)
  const dirty = patch !== null && Object.keys(patch).length > 0
  const ttlInvalid = parseInteger(draft.codeCacheTtlHours, TTL) === null
  const retentionInvalid = parseInteger(draft.auditRetentionDays, RETENTION) === null

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!patch || !dirty) return
    update.mutate(patch)
  }

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => setDraft((current) => ({ ...current, [key]: value }))

  return (
    <div className="flex flex-col gap-5 rounded-lg p-4 ring-1 ring-border">
      <form onSubmit={submit} className="flex flex-col gap-5">
        <fieldset disabled={readOnly} className="flex flex-col gap-5">
          <FieldSet>
            <FieldLegend variant="label">Code cache</FieldLegend>
            <FieldDescription className="text-xs">
              File contents the code browser and diagrams read. Analysis itself never needs them stored.
            </FieldDescription>
            <RadioGroup value={draft.codeCache} onValueChange={(value) => set('codeCache', value)} className="gap-2">
              {CODE_CACHE_OPTIONS.map((option) => (
                <Field key={option.value} orientation="horizontal" className="items-start">
                  <RadioGroupItem id={`${id}-cache-${option.value}`} value={option.value} className="mt-0.5" />
                  <div className="flex flex-col gap-0.5">
                    <FieldLabel htmlFor={`${id}-cache-${option.value}`} className="font-normal">
                      {option.label}
                    </FieldLabel>
                    <FieldDescription className="text-xs">{option.description}</FieldDescription>
                  </div>
                </Field>
              ))}
            </RadioGroup>
            <Field data-invalid={ttlInvalid || undefined}>
              <FieldLabel htmlFor={`${id}-ttl`} className="text-xs font-normal">
                TTL (hours)
              </FieldLabel>
              <Input
                id={`${id}-ttl`}
                type="number"
                inputMode="numeric"
                min={TTL.min}
                max={TTL.max}
                value={draft.codeCacheTtlHours}
                onChange={(event) => set('codeCacheTtlHours', event.target.value)}
                disabled={draft.codeCache !== 'session'}
                aria-invalid={ttlInvalid || undefined}
                className="max-w-48 tabular-nums"
              />
              <FieldDescription className="text-xs">
                {ttlInvalid ? `A whole number from ${TTL.min} to ${TTL.max}.` : 'Applies to the per-commit cache only.'}
              </FieldDescription>
            </Field>
          </FieldSet>

          <Separator />

          <FieldGroup className="gap-4">
            <Field orientation="horizontal">
              <Switch
                id={`${id}-chunks`}
                checked={draft.minimalChunks}
                onCheckedChange={(checked) => set('minimalChunks', checked)}
              />
              <div className="flex flex-col gap-0.5">
                <FieldLabel htmlFor={`${id}-chunks`} className="font-normal">
                  Minimal chunks
                </FieldLabel>
                <FieldDescription className="text-xs">
                  Semantic chunks keep excerpts of at most 400 characters before they are embedded and stored.
                </FieldDescription>
              </div>
            </Field>

            <Field>
              <FieldLabel id={`${id}-reasoner-label`}>Reasoner</FieldLabel>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                spacing={0}
                value={draft.reasoner}
                onValueChange={(value) => {
                  if (value) set('reasoner', value)
                }}
                aria-labelledby={`${id}-reasoner-label`}
              >
                {REASONER_OPTIONS.map((option) => (
                  <ToggleGroupItem key={option.value} value={option.value} className="px-3 font-normal data-[state=on]:font-medium">
                    {option.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <FieldDescription className="text-xs">
                Off sends nothing to a model. Claude sends structured context (facts, options, the spec), never raw files, with
                the key stored below.
              </FieldDescription>
            </Field>

            <Field>
              <FieldLabel id={`${id}-code-to-model-label`}>Source lines to the model</FieldLabel>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                spacing={0}
                value={draft.codeToModel}
                onValueChange={(value) => {
                  if (value) set('codeToModel', codeToModelMode(value))
                }}
                aria-labelledby={`${id}-code-to-model-label`}
                aria-describedby={`${id}-code-to-model-hint`}
              >
                {CODE_TO_MODEL_OPTIONS.map((option) => (
                  <ToggleGroupItem key={option.value} value={option.value} className="px-3 font-normal data-[state=on]:font-medium">
                    {option.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <FieldDescription id={`${id}-code-to-model-hint`} className="text-xs">
                Lets the model write a fix for a review finding. What is sent then: the lines the finding flags and at most 20 lines
                either side of them, 120 lines at most, with every line that looks like a secret replaced by{' '}
                <span className="font-mono">&lt;redacted&gt;</span>. Never a whole file, and never a finding about a secret. It only
                matters while the reasoner is on, and it is off by default.
                {draft.codeToModel === 'changed_lines' && draft.reasoner === 'off' ? ' The reasoner is off, so nothing is sent now.' : null}
              </FieldDescription>
            </Field>

            <Field data-invalid={retentionInvalid || undefined}>
              <FieldLabel htmlFor={`${id}-retention`}>Audit retention (days)</FieldLabel>
              <Input
                id={`${id}-retention`}
                type="number"
                inputMode="numeric"
                min={RETENTION.min}
                max={RETENTION.max}
                value={draft.auditRetentionDays}
                onChange={(event) => set('auditRetentionDays', event.target.value)}
                aria-invalid={retentionInvalid || undefined}
                className="max-w-48 tabular-nums"
              />
              <FieldDescription className="text-xs">
                {retentionInvalid ? `A whole number from ${RETENTION.min} to ${RETENTION.max}.` : '0 keeps entries forever.'}
              </FieldDescription>
            </Field>
          </FieldGroup>
        </fieldset>

        <div className="flex items-center justify-end gap-3">
          {readOnly ? <span className="text-xs text-muted-foreground">Admins change these.</span> : null}
          <Button type="submit" size="sm" loading={update.isPending} disabled={readOnly || !dirty}>
            Save controls
          </Button>
        </div>
      </form>

      <Separator />

      <ReasonerKeyForm workspaceId={workspaceId} controls={controls} disabled={readOnly} />
    </div>
  )
}
