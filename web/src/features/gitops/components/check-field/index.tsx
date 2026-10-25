import type { ReactNode } from 'react'

import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldContent, FieldDescription, FieldLabel } from '@/components/ui/field'

interface CheckFieldProps {
  id: string
  label: string
  description: ReactNode
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  disabled?: boolean
}

/** A choice of one request (prune, dry run, cascade): the box, its label, and what it does in one sentence. */
export function CheckField({ id, label, description, checked, onCheckedChange, disabled = false }: CheckFieldProps) {
  return (
    <Field orientation="horizontal">
      <Checkbox
        id={id}
        checked={checked}
        onCheckedChange={(value) => onCheckedChange(value === true)}
        disabled={disabled}
        aria-describedby={`${id}-description`}
      />
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <FieldDescription id={`${id}-description`} className="text-xs">
          {description}
        </FieldDescription>
      </FieldContent>
    </Field>
  )
}
