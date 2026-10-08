import { Field, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { categoryLabel, PATTERN_CATEGORIES } from '../../lib/labels'

interface PatternFiltersProps {
  /** '' means every category. */
  category: string
  onCategoryChange: (category: string) => void
  includeDeprecated: boolean
  onIncludeDeprecatedChange: (value: boolean) => void
  /** True while a filter change is being applied. */
  busy?: boolean
}

/** Toolbar: a category toggle group plus the deprecated switch. */
export function PatternFilters({
  category,
  onCategoryChange,
  includeDeprecated,
  onIncludeDeprecatedChange,
  busy = false,
}: PatternFiltersProps) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2" aria-busy={busy || undefined}>
      <ToggleGroup
        type="single"
        size="sm"
        variant="outline"
        spacing={1}
        value={category || 'all'}
        onValueChange={(value) => onCategoryChange(value === 'all' || value === '' ? '' : value)}
        aria-label="Category"
        className="flex-wrap"
      >
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        {PATTERN_CATEGORIES.map((option) => (
          <ToggleGroupItem key={option} value={option}>
            {categoryLabel(option)}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Field orientation="horizontal" className="w-auto gap-2">
        <Switch
          id="include-deprecated"
          size="sm"
          checked={includeDeprecated}
          onCheckedChange={onIncludeDeprecatedChange}
        />
        <FieldLabel htmlFor="include-deprecated" className="text-xs font-normal text-muted-foreground">
          Include deprecated
        </FieldLabel>
      </Field>
    </div>
  )
}
