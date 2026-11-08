import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { cn } from '@/lib/utils'

import { deployerOf, visibleFields, type ConfigField } from '../../lib/deployers'
import { configFieldKey, controlId } from '../../lib/environment-form'
import { INSPECTION_KEYS } from '../../lib/inspection'
import { useSecretField } from '../../lib/use-secret-field'
import { ConfigControl } from './config-control'

interface ConfigFieldsProps {
  kind: string
  config: Record<string, string>
  onChange: (key: string, value: string) => void
  /** Messages by form field (`config.workflow`), from the client check or the server. */
  errors: Record<string, string>
}

/** The settings of the chosen deployer (exactly the config keys CONTRACT.md §22 lists for its kind), then the image inspection's (§22.1). */
export function ConfigFields({ kind, config, onChange, errors }: ConfigFieldsProps) {
  const deployer = deployerOf(kind)
  const fields = visibleFields(kind, config)
  const deployerFields = fields.filter((field) => !INSPECTION_KEYS.has(field.key))
  const inspectionFields = fields.filter((field) => INSPECTION_KEYS.has(field.key))
  // The secret names are read only for a deployer that has a secret to pick.
  const secretField = useSecretField(fields.some((field) => field.type === 'secret'))

  const render = (field: ConfigField) => {
    const formKey = configFieldKey(field.key)
    const id = controlId(formKey)
    const error = errors[formKey]
    const wide = field.type === 'secret' || field.type === 'json' || field.key === 'inspect_ignore'
    return (
      <Field key={field.key} data-invalid={error ? true : undefined} className={cn(wide && 'sm:col-span-2')}>
        <FieldLabel htmlFor={id}>
          {field.label}
          {field.required ? null : <span className="font-normal text-muted-foreground">optional</span>}
        </FieldLabel>
        <ConfigControl
          id={id}
          field={field}
          value={config[field.key] ?? ''}
          onChange={(value) => onChange(field.key, value)}
          secretField={secretField}
          invalid={Boolean(error)}
          describedBy={error ? `${id}-error` : field.description ? `${id}-hint` : undefined}
        />
        {error ? (
          <FieldError id={`${id}-error`} className="text-xs">
            {error}
          </FieldError>
        ) : field.description ? (
          <FieldDescription id={`${id}-hint`} className="text-xs">
            {field.description}
          </FieldDescription>
        ) : null}
      </Field>
    )
  }

  return (
    <>
      {deployerFields.length > 0 ? (
        <FieldSet>
          <FieldLegend variant="label" className="mb-3">
            {deployer.label} settings
          </FieldLegend>
          <FieldGroup className="grid gap-4 sm:grid-cols-2">{deployerFields.map(render)}</FieldGroup>
        </FieldSet>
      ) : null}
      <FieldSet>
        <FieldLegend variant="label" className="mb-3">
          Before the deploy
        </FieldLegend>
        <FieldGroup className="grid gap-4 sm:grid-cols-2">{inspectionFields.map(render)}</FieldGroup>
      </FieldSet>
    </>
  )
}
