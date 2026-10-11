import { Trash2Icon } from 'lucide-react'

import { RISK } from '@/components/shared/data/risk'
import { ComboboxField } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

import { ACTION_TYPES, ADAPTERS } from '../../lib/labels'
import type { DraftStep } from '../../lib/plan-template'
import { DependsOnSelect } from './depends-on-select'

const ACTION_TYPE_OPTIONS = ACTION_TYPES.map((action) => ({
  value: action.actionType,
  label: action.actionType,
  description: `${action.label} · ${RISK[action.risk].label}`,
  disabled: action.risk === 'blocked',
}))

const ADAPTER_OPTIONS = ADAPTERS.map((adapter) => ({ value: adapter, label: adapter }))

interface StepEditorRowProps {
  index: number
  step: DraftStep
  /** Keys of every other step, offered as dependencies. */
  otherKeys: string[]
  error?: string
  onChange: (patch: Partial<DraftStep>) => void
  onRemove: () => void
  canRemove: boolean
}

export function StepEditorRow({ index, step, otherKeys, error, onChange, onRemove, canRemove }: StepEditorRowProps) {
  return (
    <div className="flex flex-col gap-1.5 rounded-lg p-2 ring-1 ring-border" aria-invalid={error ? true : undefined}>
      <div className="grid grid-cols-[2.5rem_6rem_minmax(0,1fr)] items-center gap-2 sm:grid-cols-[2.5rem_6rem_minmax(0,1fr)_minmax(0,1fr)]">
        <span className="font-mono text-xs tabular-nums text-muted-foreground">{index + 1}.</span>
        <Input
          aria-label={`Step ${index + 1} key`}
          value={step.key}
          onChange={(event) => onChange({ key: event.target.value })}
          placeholder="key"
          className="h-7 font-mono text-xs"
          aria-invalid={error ? true : undefined}
        />
        <Input
          aria-label={`Step ${index + 1} title`}
          value={step.title}
          onChange={(event) => onChange({ title: event.target.value })}
          placeholder="Title"
          className="h-7 text-xs"
        />
        <div className="col-span-3 flex items-center gap-2 sm:col-span-1">
          <ComboboxField
            size="sm"
            mono
            className="min-w-0 flex-1"
            aria-label={`Step ${index + 1} action type`}
            value={step.actionType}
            onValueChange={(actionType) => onChange({ actionType })}
            options={ACTION_TYPE_OPTIONS}
            searchPlaceholder="Search action types"
            emptyText="No action type matches."
          />
        </div>
      </div>
      <div className="grid grid-cols-[2.5rem_minmax(0,1fr)_minmax(0,1fr)_1.75rem] items-center gap-2">
        <span className="text-2xs text-muted-foreground">after</span>
        <DependsOnSelect options={otherKeys} value={step.dependsOn} onChange={(dependsOn) => onChange({ dependsOn })} />
        <ComboboxField
          size="sm"
          mono
          aria-label={`Step ${index + 1} adapter`}
          value={step.adapter}
          onValueChange={(adapter) => onChange({ adapter })}
          options={ADAPTER_OPTIONS}
          searchPlaceholder="Search adapters"
          emptyText="No adapter matches."
        />
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onRemove}
          disabled={!canRemove}
          aria-label={`Remove step ${index + 1}`}
          className="text-muted-foreground"
        >
          <Trash2Icon />
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  )
}
