import { SearchIcon, XIcon } from 'lucide-react'

import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { count } from '@/lib/intl'

interface TreeFilterProps {
  value: string
  onChange: (value: string) => void
  /** Rows shown for the current filter; `null` when nothing is filtered. */
  matches: number | null
}

/** Narrows the tree to paths containing the text. */
export function TreeFilter({ value, onChange, matches }: TreeFilterProps) {
  return (
    <div className="flex flex-col gap-1.5">
      <InputGroup className="h-7">
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput
          type="search"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="Filter by path"
          aria-label="Filter files by path"
          autoComplete="off"
          spellCheck={false}
          className="h-7 text-sm md:text-sm"
        />
        {value ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton size="icon-xs" aria-label="Clear filter" onClick={() => onChange('')}>
              <XIcon />
            </InputGroupButton>
          </InputGroupAddon>
        ) : null}
      </InputGroup>
      {matches !== null ? (
        <p className="px-1 text-2xs tabular-nums text-muted-foreground" aria-live="polite">
          {matches === 0 ? 'No path matches' : `${count(matches, 'match', 'matches')}`}
        </p>
      ) : null}
    </div>
  )
}
