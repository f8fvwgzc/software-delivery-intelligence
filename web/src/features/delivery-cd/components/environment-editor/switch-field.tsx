import { Field, FieldContent, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'

interface SwitchFieldProps {
  id: string
  label: string
  description: string
  checked: boolean
  onCheckedChange: (checked: boolean) => void
}

/** A setting that is on or off: label and one sentence at the left, the switch at the right. */
export function SwitchField({ id, label, description, checked, onCheckedChange }: SwitchFieldProps) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <FieldDescription id={`${id}-hint`} className="text-xs">
          {description}
        </FieldDescription>
      </FieldContent>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} aria-describedby={`${id}-hint`} />
    </Field>
  )
}
