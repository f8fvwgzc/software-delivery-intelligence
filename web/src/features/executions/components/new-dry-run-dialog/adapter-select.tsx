import { ComboboxField } from '@/components/shared/form'

import { ADAPTER_CHOICES, type Adapter } from '../../lib/labels'

interface AdapterSelectProps {
  id?: string
  value: Adapter
  onChange: (adapter: Adapter) => void
  invalid?: boolean
  /** Id of the hint or error that describes the control. */
  describedBy?: string
}

const OPTIONS = ADAPTER_CHOICES.map((choice) => ({
  value: choice.value,
  label: choice.label,
  description: choice.description,
}))

/** The runtime for the whole run (CONTRACT.md §5.4): dry run, local Docker or GitHub. */
export function AdapterSelect({ id, value, onChange, invalid, describedBy }: AdapterSelectProps) {
  return (
    <ComboboxField
      id={id}
      value={value}
      onValueChange={(next) => onChange(next as Adapter)}
      options={OPTIONS}
      aria-invalid={invalid}
      aria-describedby={describedBy}
      searchPlaceholder="Search runtimes"
      emptyText="No runtime matches."
    />
  )
}
