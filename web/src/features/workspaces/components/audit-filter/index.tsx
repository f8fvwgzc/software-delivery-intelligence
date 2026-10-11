import { useId } from 'react'

import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { ACTION_PREFIXES } from '../../lib/labels'

interface AuditFilterProps {
  value: string
  onChange: (value: string) => void
}

/** Radix reads an empty value as "nothing pressed"; "All" needs a value of its own. */
const ALL = 'all'
const toToggle = (prefix: string) => (prefix === '' ? ALL : prefix)
const fromToggle = (toggle: string) => (toggle === ALL ? '' : toggle)

/** Quick prefixes as a toggle row, plus a free prefix for anything else (`approval.dec`). */
export function AuditFilter({ value, onChange }: AuditFilterProps) {
  const id = useId()
  const preset = ACTION_PREFIXES.some((option) => option.value === value) ? toToggle(value) : 'custom'

  return (
    <div className="flex flex-wrap items-end gap-3">
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        spacing={1}
        value={preset}
        onValueChange={(next) => {
          // An empty value is the pressed item released again: keep the filter as it is.
          if (next) onChange(fromToggle(next))
        }}
        aria-label="Action family"
      >
        {ACTION_PREFIXES.map((option) => (
          <ToggleGroupItem key={option.value} value={toToggle(option.value)} className="h-7 px-2 text-xs">
            {option.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Field className="w-56">
        <FieldLabel htmlFor={id} className="sr-only">
          Action prefix
        </FieldLabel>
        <Input
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="Action prefix, e.g. approval."
          className="h-7 font-mono text-xs"
          autoComplete="off"
          spellCheck={false}
        />
      </Field>
    </div>
  )
}
