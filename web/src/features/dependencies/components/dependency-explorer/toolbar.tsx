import { SearchIcon, XIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

import type { EcosystemCount } from '../../api/queries'
import { ALL_ECOSYSTEMS, ecosystemLabel } from '../../lib/labels'

interface DependencyToolbarProps {
  counts: EcosystemCount[]
  ecosystem: string
  onEcosystemChange: (value: string) => void
  search: string
  onSearchChange: (value: string) => void
  shown: number
  total: number
  isFiltered: boolean
  onClear: () => void
}

/** Ecosystem select, search, and the "n of total" count. Ghost controls only. */
export function DependencyToolbar({
  counts,
  ecosystem,
  onEcosystemChange,
  search,
  onSearchChange,
  shown,
  total,
  isFiltered,
  onClear,
}: DependencyToolbarProps) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Select value={ecosystem} onValueChange={onEcosystemChange}>
        <SelectTrigger size="sm" className="w-44" aria-label="Filter by ecosystem">
          <SelectValue />
        </SelectTrigger>
        <SelectContent position="popper">
          <SelectGroup>
            <SelectItem value={ALL_ECOSYSTEMS}>All ecosystems</SelectItem>
            {counts.map((row) => (
              <SelectItem key={row.ecosystem} value={row.ecosystem}>
                {ecosystemLabel(row.ecosystem)}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>

      <InputGroup className="h-7 w-full sm:w-64">
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput
          type="search"
          value={search}
          onChange={(event) => onSearchChange(event.target.value)}
          placeholder="Package or manifest path"
          aria-label="Search dependencies"
          className="h-7 text-sm md:text-sm"
        />
        {search ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton size="icon-xs" aria-label="Clear search" onClick={() => onSearchChange('')}>
              <XIcon />
            </InputGroupButton>
          </InputGroupAddon>
        ) : null}
      </InputGroup>

      <div className="ml-auto flex items-center gap-2 text-xs text-muted-foreground">
        <span className="tabular-nums">
          {isFiltered ? `${shown.toLocaleString()} of ${total.toLocaleString()}` : total.toLocaleString()}
        </span>
        {isFiltered ? (
          <Button variant="link" size="xs" className="h-auto p-0 text-xs text-muted-foreground" onClick={onClear}>
            Reset filters
          </Button>
        ) : null}
      </div>
    </div>
  )
}
