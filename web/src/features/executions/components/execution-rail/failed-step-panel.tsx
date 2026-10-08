import { OctagonXIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Textarea } from '@/components/ui/textarea'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { useResolveFailure } from '../../api/mutations'
import { CancelDialog } from '../execution-header/cancel-dialog'

interface FailedStepPanelProps {
  executionId: string
  executionNumber: number
  failure: components['schemas']['Failure']
}

const INSTRUCTION = '__instruction'

/** The paused step expanded in place: cause, raw output, and the ways out. */
export function FailedStepPanel({ executionId, executionNumber, failure }: FailedStepPanelProps) {
  const resolve = useResolveFailure(executionId)
  const recommended = failure.options.find((option) => option.recommended) ?? failure.options[0]
  const [choice, setChoice] = useState<string>(recommended?.id ?? INSTRUCTION)
  const [instruction, setInstruction] = useState('')
  const needsInstruction = choice === INSTRUCTION && instruction.trim().length === 0

  const submit = () => {
    const body = choice === INSTRUCTION ? { instruction: instruction.trim() } : { optionId: choice }
    resolve.mutate(body, { onSuccess: () => toast.success('Replanning from the last checkpoint') })
  }

  return (
    <div className="flex flex-col gap-4 rounded-lg bg-destructive/[0.03] p-4 ring-1 ring-destructive/20">
      <div className="flex flex-col gap-1.5">
        <div className="flex flex-wrap items-center gap-2">
          <OctagonXIcon className="size-4 text-destructive" />
          <Badge variant="outline" className="border-destructive/30 font-mono text-xs text-destructive">
            {failure.code}
          </Badge>
          <span className="text-sm font-medium">{failure.message}</span>
        </div>
        <p className="text-sm leading-relaxed text-muted-foreground">{failure.diagnosis}</p>
      </div>

      {failure.missingPermissions.length > 0 && (
        <DetailRow label="Missing permissions">
          <MonoList items={failure.missingPermissions} tone="destructive" />
        </DetailRow>
      )}

      {failure.rawOutput && (
        <DetailRow label="Raw output">
          <pre
            className="max-h-48 overflow-auto rounded-md bg-muted/60 p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap text-foreground/90"
            style={{ scrollbarGutter: 'stable' }}
          >
            {failure.rawOutput}
          </pre>
        </DetailRow>
      )}

      {failure.blockedSteps.length > 0 && (
        <DetailRow label={`Blocked until resolved (${failure.blockedSteps.length})`}>
          <MonoList items={failure.blockedSteps} />
        </DetailRow>
      )}

      <Field>
        <FieldLabel>How do you want to continue?</FieldLabel>
        <RadioGroup value={choice} onValueChange={setChoice} className="gap-1.5">
          {failure.options.map((option) => (
            <RemediationChoice key={option.id} option={option} selected={choice === option.id} />
          ))}
          <label
            className={cn(
              'flex cursor-pointer flex-col gap-2 rounded-md px-3 py-2 ring-1 ring-border transition-colors hover:bg-muted/40',
              choice === INSTRUCTION && 'bg-brand/[0.04] ring-brand/40',
            )}
          >
            <span className="flex items-center gap-2.5">
              <RadioGroupItem value={INSTRUCTION} />
              <span className="text-sm font-medium">Give an instruction</span>
            </span>
            {choice === INSTRUCTION && (
              <Textarea
                value={instruction}
                onChange={(event) => setInstruction(event.target.value)}
                placeholder="Describe what the planner should change, e.g. use a smaller SKU in the same region."
                rows={3}
                className="ml-6.5"
                autoFocus
              />
            )}
          </label>
        </RadioGroup>
      </Field>

      <div className="flex flex-wrap items-center justify-end gap-2">
        <CancelDialog executionId={executionId} executionNumber={executionNumber} intent="stop">
          <Button variant="outline" disabled={resolve.isPending}>
            Stop
          </Button>
        </CancelDialog>
        <Button onClick={submit} loading={resolve.isPending} disabled={needsInstruction}>
          Continue
        </Button>
      </div>
    </div>
  )
}

interface RemediationChoiceProps {
  option: components['schemas']['RemediationOption']
  selected: boolean
}

function RemediationChoice({ option, selected }: RemediationChoiceProps) {
  return (
    <label
      className={cn(
        'flex cursor-pointer items-start gap-2.5 rounded-md px-3 py-2 ring-1 ring-border transition-colors hover:bg-muted/40',
        selected && 'bg-brand/[0.04] ring-brand/40',
      )}
    >
      <RadioGroupItem value={option.id} className="mt-0.5" />
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="flex flex-wrap items-center gap-2 text-sm font-medium">
          {option.label}
          {option.recommended && (
            <Badge variant="outline" className="h-4 border-brand/30 px-1.5 text-2xs font-normal text-brand">
              Recommended
            </Badge>
          )}
        </span>
        <span className="text-xs text-muted-foreground">{option.description}</span>
        {option.effect && <span className="text-2xs text-muted-foreground/80">Effect: {option.effect}</span>}
      </span>
    </label>
  )
}

function DetailRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs text-muted-foreground">{label}</span>
      {children}
    </div>
  )
}

function MonoList({ items, tone }: { items: string[]; tone?: 'destructive' }) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((item) => (
        <code
          key={item}
          className={cn(
            'rounded-sm bg-muted px-1.5 py-0.5 font-mono text-2xs',
            tone === 'destructive' && 'bg-destructive/10 text-destructive',
          )}
        >
          {item}
        </code>
      ))}
    </div>
  )
}
