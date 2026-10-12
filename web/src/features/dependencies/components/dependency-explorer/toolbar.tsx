import { SearchIcon, XIcon } from 'lucide-react'

import { ComboboxField } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'

import type { EcosystemCount } from '../../api/queries'
import { ALL_ECOSYSTEMS, ecosystemLabel } from '../../lib/labels'
import { count } from '@/lib/intl'

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

/** Ecosystem picker, search, and the "n of total" count. Ghost controls only. */
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
  const options = [
    { value: ALL_ECOSYSTEMS, label: 'All ecosystems' },
    ...counts.map((row) => ({
      value: row.ecosystem,
      label: ecosystemLabel(row.ecosystem),
      description: `${count(row.count, 'package', 'packages')}`,
    })),
  ]

  return (
    <div className="flex flex-wrap items-center gap-2">
      <ComboboxField
        size="sm"
        className="w-44"
        aria-label="Filter by ecosystem"
        value={ecosystem}
        onValueChange={onEcosystemChange}
        options={options}
        searchPlaceholder="Search ecosystems"
        emptyText="No ecosystem matches."
      />

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
