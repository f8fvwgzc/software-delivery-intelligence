import { PlusIcon, RotateCcwIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'

import type { DraftStep } from '../../lib/plan-template'
import { StepEditorRow } from './step-editor-row'
import { count } from '@/lib/intl'

interface PlanEditorProps {
  steps: DraftStep[]
  errors: Map<number, string>
  onChange: (steps: DraftStep[]) => void
  onReset: () => void
}

export function PlanEditor({ steps, errors, onChange, onReset }: PlanEditorProps) {
  const keys = steps.map((step) => step.key.trim()).filter(Boolean)

  const update = (index: number, patch: Partial<DraftStep>) => {
    const previous = steps[index]
    const next = { ...previous, ...patch }
    const renamed = patch.key !== undefined && patch.key.trim() !== previous.key.trim()

    onChange(
      steps.map((step, position) => {
        if (position === index) return next
        // Keep dependency edges pointing at the renamed step.
        if (renamed && step.dependsOn.includes(previous.key.trim())) {
          return { ...step, dependsOn: step.dependsOn.map((key) => (key === previous.key.trim() ? next.key.trim() : key)) }
        }
        return step
      }),
    )
  }

  const remove = (index: number) => {
    const removedKey = steps[index].key.trim()
    onChange(
      steps
        .filter((_, position) => position !== index)
        .map((step) => ({ ...step, dependsOn: step.dependsOn.filter((key) => key !== removedKey) })),
    )
  }

  const add = () => {
    const last = steps[steps.length - 1]
    onChange([
      ...steps,
      {
        key: `step-${steps.length + 1}`,
        title: '',
        actionType: 'repo.read',
        adapter: 'simulated',
        dependsOn: last?.key.trim() ? [last.key.trim()] : [],
      },
    ])
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          {count(steps.length, 'step', 'steps')}. Each key must be unique; "after" lists the steps that must checkpoint first.
        </span>
        <Button type="button" variant="ghost" size="xs" onClick={onReset} className="text-muted-foreground">
          <RotateCcwIcon data-icon="inline-start" />
          Reset to template
        </Button>
      </div>
      <div className="flex max-h-80 flex-col gap-2 overflow-y-auto pr-1" style={{ scrollbarGutter: 'stable' }}>
        {steps.map((step, index) => (
          <StepEditorRow
            key={index}
            index={index}
            step={step}
            otherKeys={keys.filter((key) => key !== step.key.trim())}
            error={errors.get(index)}
            onChange={(patch) => update(index, patch)}
            onRemove={() => remove(index)}
            canRemove={steps.length > 1}
          />
        ))}
      </div>
      <Button type="button" variant="outline" size="sm" onClick={add} className="self-start">
        <PlusIcon data-icon="inline-start" />
        Add step
      </Button>
    </div>
  )
}
