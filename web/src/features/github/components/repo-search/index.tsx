import { SearchIcon, XIcon } from 'lucide-react'

import { Field, FieldLabel } from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'

interface RepoSearchProps {
  value: string
  onChange: (value: string) => void
}

/** Name filter for the repository list. The caller defers the value. */
export function RepoSearch({ value, onChange }: RepoSearchProps) {
  return (
    <Field className="max-w-sm">
      <FieldLabel htmlFor="repo-search" className="sr-only">
        Filter repositories by name
      </FieldLabel>
      <InputGroup>
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput
          id="repo-search"
          type="search"
          autoComplete="off"
          spellCheck={false}
          placeholder="Filter by name, e.g. api or infra"
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
        {value ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton size="icon-xs" aria-label="Clear filter" onClick={() => onChange('')}>
              <XIcon />
            </InputGroupButton>
          </InputGroupAddon>
        ) : null}
      </InputGroup>
    </Field>
  )
}
