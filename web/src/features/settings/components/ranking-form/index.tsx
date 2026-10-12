import { useId, useState, type FormEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'

import { useUpdateSettings } from '../../api/mutations'
import type { IntelligenceSettings, IntelligenceSettingsPatch, ScoreWeights } from '../../api/queries'
import {
  EMPTY_WEIGHTS,
  REASONERS,
  sumWeights,
  WEIGHT_FIELDS,
  weightsInRange,
  weightsSumToOne,
  type Reasoner,
  type WeightKey,
} from '../../lib/settings'

interface Draft {
  weights: Record<WeightKey, string>
  reasoner: string
  model: string
  costCeilingUsd: string
  autoReview: boolean
}

function toDraft(settings: IntelligenceSettings): Draft {
  const weights = settings.defaultWeights ?? EMPTY_WEIGHTS
  return {
    weights: Object.fromEntries(WEIGHT_FIELDS.map((field) => [field.key, String(weights[field.key])])) as Record<
      WeightKey,
      string
    >,
    reasoner: settings.reasoner,
    model: settings.model,
    costCeilingUsd: String(settings.costCeilingUsd),
    autoReview: settings.autoReview,
  }
}

function parseWeights(draft: Draft['weights']): ScoreWeights {
  return Object.fromEntries(
    WEIGHT_FIELDS.map((field) => [field.key, draft[field.key].trim() === '' ? Number.NaN : Number(draft[field.key])]),
  ) as ScoreWeights
}

function parseCost(value: string): number | null {
  const parsed = Number(value)
  return value.trim() !== '' && Number.isFinite(parsed) && parsed >= 0 ? parsed : null
}

function sameWeights(a: ScoreWeights, b: ScoreWeights | null | undefined) {
  return b != null && WEIGHT_FIELDS.every((field) => a[field.key] === b[field.key])
}

/** Changed fields only; null when a field is invalid. */
function toPatch(draft: Draft, settings: IntelligenceSettings): IntelligenceSettingsPatch | null {
  const weights = parseWeights(draft.weights)
  if (!weightsInRange(weights) || !weightsSumToOne(weights)) return null
  const cost = parseCost(draft.costCeilingUsd)
  if (cost === null) return null
  const model = draft.model.trim()
  if (!model) return null

  const patch: IntelligenceSettingsPatch = {}
  if (!sameWeights(weights, settings.defaultWeights)) patch.defaultWeights = weights
  if (draft.reasoner !== settings.reasoner) patch.reasoner = draft.reasoner
  if (model !== settings.model) patch.model = model
  if (cost !== settings.costCeilingUsd) patch.costCeilingUsd = cost
  if (draft.autoReview !== settings.autoReview) patch.autoReview = draft.autoReview
  return patch
}

const REASONER_LABEL: Record<Reasoner, string> = { heuristic: 'Heuristic', claude: 'Claude' }

/** Ranking defaults of the intelligence service: the seven weights, the reasoner, its model, the cost ceiling and auto review. */
export function RankingForm({ settings }: { settings: IntelligenceSettings }) {
  const id = useId()
  const [draft, setDraft] = useState<Draft>(() => toDraft(settings))
  const update = useUpdateSettings()

  const weights = parseWeights(draft.weights)
  const inRange = weightsInRange(weights)
  const sum = inRange ? sumWeights(weights) : Number.NaN
  const sumValid = inRange && weightsSumToOne(weights)
  const patch = toPatch(draft, settings)
  const dirty = patch !== null && Object.keys(patch).length > 0
  const claudeAvailable = settings.apiKeyConfigured

  const setWeight = (key: WeightKey, value: string) =>
    setDraft((current) => ({ ...current, weights: { ...current.weights, [key]: value } }))

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!patch || !dirty) return
    update.mutate({ intelligence: patch })
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-5 rounded-lg p-4 ring-1 ring-border">
      <FieldSet>
        <FieldLegend variant="label">Default weights</FieldLegend>
        <FieldDescription className="text-xs">
          How much each signal counts when options are ranked. They must sum to 1.
        </FieldDescription>
        <FieldGroup className="gap-3">
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {WEIGHT_FIELDS.map((field) => {
              const value = weights[field.key]
              const invalid = !Number.isFinite(value) || value < 0 || value > 1
              return (
                <Field key={field.key} data-invalid={invalid || undefined}>
                  <FieldLabel htmlFor={`${id}-${field.key}`} className="text-xs font-normal">
                    {field.label}
                  </FieldLabel>
                  <Input
                    id={`${id}-${field.key}`}
                    type="number"
                    inputMode="decimal"
                    min={0}
                    max={1}
                    step={0.05}
                    value={draft.weights[field.key]}
                    onChange={(event) => setWeight(field.key, event.target.value)}
                    aria-invalid={invalid || undefined}
                    className="tabular-nums"
                  />
                </Field>
              )
            })}
          </div>
          <p
            aria-live="polite"
            className={cn('text-xs tabular-nums', sumValid ? 'text-muted-foreground' : 'text-destructive')}
          >
            Sum {Number.isFinite(sum) ? sum.toFixed(3) : '—'}
            {sumValid ? null : inRange ? ' · must be 1.000' : ' · every weight is 0 to 1'}
          </p>
        </FieldGroup>
      </FieldSet>

      <FieldGroup className="gap-4">
        <Field>
          <FieldLabel id={`${id}-reasoner-label`}>Reasoner</FieldLabel>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={0}
            value={draft.reasoner}
            onValueChange={(value) => {
              if (value) setDraft((current) => ({ ...current, reasoner: value }))
            }}
            aria-labelledby={`${id}-reasoner-label`}
          >
            {REASONERS.map((reasoner) => (
              <ToggleGroupItem
                key={reasoner}
                value={reasoner}
                disabled={reasoner === 'claude' && !claudeAvailable}
                className="px-3 font-normal data-[state=on]:font-medium"
              >
                {REASONER_LABEL[reasoner]}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <FieldDescription className="text-xs">
            {claudeAvailable
              ? 'Claude negotiates in plain language; heuristic ranks without a model.'
              : 'Claude needs ANTHROPIC_API_KEY on the intelligence service. Until then, heuristic.'}
          </FieldDescription>
        </Field>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={draft.model.trim() === '' || undefined}>
            <FieldLabel htmlFor={`${id}-model`}>Model</FieldLabel>
            <Input
              id={`${id}-model`}
              value={draft.model}
              onChange={(event) => setDraft((current) => ({ ...current, model: event.target.value }))}
              spellCheck={false}
              autoComplete="off"
              aria-invalid={draft.model.trim() === '' || undefined}
              className="font-mono"
            />
            <FieldDescription className="text-xs">Used by the Claude reasoner.</FieldDescription>
          </Field>
          <Field data-invalid={parseCost(draft.costCeilingUsd) === null || undefined}>
            <FieldLabel htmlFor={`${id}-cost`}>Cost ceiling</FieldLabel>
            <Input
              id={`${id}-cost`}
              type="number"
              inputMode="decimal"
              min={0}
              step={1}
              value={draft.costCeilingUsd}
              onChange={(event) => setDraft((current) => ({ ...current, costCeilingUsd: event.target.value }))}
              aria-invalid={parseCost(draft.costCeilingUsd) === null || undefined}
              className="tabular-nums"
            />
            <FieldDescription className="text-xs">USD per month. Options above it lose ranking unless a budget says otherwise.</FieldDescription>
          </Field>
        </div>

        <Field orientation="horizontal">
          <Switch
            id={`${id}-auto-review`}
            checked={draft.autoReview}
            onCheckedChange={(checked) => setDraft((current) => ({ ...current, autoReview: checked }))}
          />
          <div className="flex flex-col gap-0.5">
            <FieldLabel htmlFor={`${id}-auto-review`} className="font-normal">
              Auto review pull requests
            </FieldLabel>
            <FieldDescription className="text-xs">Starts a review when a PR is opened or pushed; never publishes.</FieldDescription>
          </div>
        </Field>
      </FieldGroup>

      <div className="flex items-center justify-end gap-3">
        <Button type="submit" size="sm" loading={update.isPending} disabled={!dirty}>
          Save ranking
        </Button>
      </div>
    </form>
  )
}
