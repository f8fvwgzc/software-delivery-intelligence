import type { ReactNode } from 'react'

import { Field, FieldContent, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'

/**
 * The few pieces both GitOps editors share: the ARIA wiring of a control to
 * its message, the message itself (the error when there is one, else the
 * hint), and a switch with its one honest line.
 */

/** Props for the control of `id`: invalid when there is an error, described by the error or the hint. */
export function controlProps(id: string, error: string | undefined, hasHint = false) {
  return {
    id,
    'aria-invalid': error ? true : undefined,
    'aria-describedby': error ? `${id}-error` : hasHint ? `${id}-hint` : undefined,
  } as const
}

/** The error under a control, or its hint. Ids match `controlProps`. */
export function FieldMessage({ id, error, hint }: { id: string; error?: string; hint?: ReactNode }) {
  if (error) {
    return (
      <FieldError id={`${id}-error`} className="text-xs">
        {error}
      </FieldError>
    )
  }
  if (!hint) return null
  return (
    <FieldDescription id={`${id}-hint`} className="text-xs">
      {hint}
    </FieldDescription>
  )
}

/** "optional" after a label, in the label's own line. */
export function Optional() {
  return <span className="font-normal text-muted-foreground">optional</span>
}

interface SwitchFieldProps {
  id: string
  label: string
  description: string
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  disabled?: boolean
}

/** A setting that is on or off: label and one sentence at the left, the switch at the right. */
export function SwitchField({ id, label, description, checked, onCheckedChange, disabled = false }: SwitchFieldProps) {
  return (
    <Field orientation="horizontal" data-disabled={disabled || undefined}>
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <FieldDescription id={`${id}-hint`} className="text-xs">
          {description}
        </FieldDescription>
      </FieldContent>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} disabled={disabled} aria-describedby={`${id}-hint`} />
    </Field>
  )
}
