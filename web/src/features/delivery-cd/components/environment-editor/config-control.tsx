import { ComboboxField, SecretNameField, type SecretNameFieldProps } from '@/components/shared/form'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'

import type { ConfigField } from '../../lib/deployers'

interface ConfigControlProps {
  id: string
  field: ConfigField
  value: string
  onChange: (value: string) => void
  /** The workspace's secret names, for `secret` fields. */
  secretField: Pick<SecretNameFieldProps, 'secrets' | 'loading' | 'unavailable' | 'onAdd'>
  invalid: boolean
  describedBy?: string
}

/** The control for one config key, by its type: text, number, a choice, a secret name or a JSON object. */
export function ConfigControl({ id, field, value, onChange, secretField, invalid, describedBy }: ConfigControlProps) {
  const aria = { 'aria-invalid': invalid || undefined, 'aria-describedby': describedBy }

  switch (field.type) {
    case 'secret':
      return (
        <SecretNameField
          id={id}
          value={value}
          onValueChange={onChange}
          clearable={!field.required}
          aria-invalid={invalid}
          aria-describedby={describedBy}
          {...secretField}
        />
      )
    case 'choice':
      return (
        <ComboboxField
          id={id}
          value={value}
          onValueChange={onChange}
          options={field.choices ?? []}
          placeholder={field.placeholder ?? 'Select'}
          aria-invalid={invalid}
          aria-describedby={describedBy}
        />
      )
    case 'json':
      return (
        <Textarea
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={field.placeholder}
          rows={3}
          spellCheck={false}
          className="font-mono text-xs md:text-xs"
          {...aria}
        />
      )
    default:
      return (
        <Input
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={field.placeholder}
          inputMode={field.type === 'number' ? 'numeric' : undefined}
          autoComplete="off"
          spellCheck={false}
          className="font-mono text-xs md:text-xs"
          {...aria}
        />
      )
  }
}
