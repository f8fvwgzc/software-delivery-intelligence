import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

import { ADAPTER_CHOICES, type Adapter } from '../../lib/labels'

interface AdapterSelectProps {
  id?: string
  value: Adapter
  onChange: (adapter: Adapter) => void
  invalid?: boolean
  /** Id of the hint or error that describes the control. */
  describedBy?: string
}

/** The runtime for the whole run (CONTRACT.md §5.4): dry run, local Docker or GitHub. */
export function AdapterSelect({ id, value, onChange, invalid, describedBy }: AdapterSelectProps) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as Adapter)}>
      <SelectTrigger id={id} className="w-full" aria-invalid={invalid || undefined} aria-describedby={describedBy}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {ADAPTER_CHOICES.map((choice) => (
            <SelectItem key={choice.value} value={choice.value}>
              {choice.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
