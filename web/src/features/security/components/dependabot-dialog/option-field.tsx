import { Tag } from '@/components/system/tag'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldContent, FieldDescription, FieldLabel } from '@/components/ui/field'

import type { DependabotOptionState } from '../../lib/dependabot'

interface OptionFieldProps {
  id: string
  option: DependabotOptionState
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  disabled?: boolean
}

/** One of the dialog's three options. One that is already on is ticked, locked, and says so. */
export function OptionField({ id, option, checked, onCheckedChange, disabled = false }: OptionFieldProps) {
  return (
    <Field orientation="horizontal">
      <Checkbox
        id={id}
        checked={checked}
        onCheckedChange={(value) => onCheckedChange(value === true)}
        disabled={disabled || option.on}
        aria-describedby={`${id}-description`}
      />
      <FieldContent>
        <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <FieldLabel htmlFor={id}>{option.label}</FieldLabel>
          {option.on && option.note ? <Tag tone="success">{option.note}</Tag> : null}
        </span>
        <FieldDescription id={`${id}-description`} className="text-xs">
          {option.description}
          {!option.on && option.note ? ` ${option.note}` : null}
        </FieldDescription>
      </FieldContent>
    </Field>
  )
}
