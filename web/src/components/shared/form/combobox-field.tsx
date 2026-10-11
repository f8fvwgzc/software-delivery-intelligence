import { useState } from 'react'

import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxTrigger,
  ComboboxValue,
} from '@/components/ui/combobox'
import { Spinner } from '@/components/ui/spinner'
import { cn } from '@/lib/utils'

export interface ComboboxOption {
  value: string
  label: string
  /** Second line under the label in the list; also matched by the search. */
  description?: string
  disabled?: boolean
  /** Extra words the search should match, e.g. a repository name or a key. */
  keywords?: string[]
}

interface ComboboxFieldProps {
  /** Id of the trigger, so a `FieldLabel htmlFor` names it. */
  id?: string
  /** '' means nothing chosen: the placeholder shows. */
  value: string
  onValueChange: (value: string) => void
  options: readonly ComboboxOption[]
  placeholder?: string
  searchPlaceholder?: string
  emptyText?: string
  disabled?: boolean
  /** Shows the loader inside the trigger and locks it, like `<Button loading>`. */
  loading?: boolean
  size?: 'sm' | 'default'
  /** Mono labels for keys, codes and adapters. */
  mono?: boolean
  /** Which edge of the trigger the list lines up with. */
  align?: 'start' | 'center' | 'end'
  className?: string
  'aria-label'?: string
  'aria-invalid'?: boolean
  'aria-describedby'?: string
  'data-testid'?: string
}

/** Substring match on the words of an option, never on its id. */
function matchOption(_value: string, search: string, keywords: string[] = []) {
  const needle = search.trim().toLowerCase()
  if (needle === '') return 1
  return keywords.join(' ').toLowerCase().includes(needle) ? 1 : 0
}

/**
 * The one searchable select of the app: a small outlined trigger, a list
 * that filters as you type, a check on the chosen row. Enter picks, Escape
 * closes, arrows move. Works inside dialogs.
 */
export function ComboboxField({
  id,
  value,
  onValueChange,
  options,
  placeholder = 'Select',
  searchPlaceholder = 'Search',
  emptyText = 'No matches.',
  disabled = false,
  loading = false,
  size = 'default',
  mono = false,
  align = 'start',
  className,
  'aria-label': ariaLabel,
  'aria-invalid': ariaInvalid,
  'aria-describedby': ariaDescribedBy,
  'data-testid': testId,
}: ComboboxFieldProps) {
  const [open, setOpen] = useState(false)
  const selected = options.find((option) => option.value === value)

  const choose = (option: ComboboxOption) => {
    if (option.value !== value) onValueChange(option.value)
    setOpen(false)
  }

  return (
    <Combobox open={open} onOpenChange={setOpen}>
      <ComboboxTrigger
        id={id}
        size={size}
        disabled={disabled || loading}
        aria-label={ariaLabel}
        aria-invalid={ariaInvalid || undefined}
        aria-describedby={ariaDescribedBy}
        aria-busy={loading || undefined}
        data-placeholder={selected ? undefined : ''}
        data-loading={loading ? '' : undefined}
        data-testid={testId}
        className={cn(mono && 'font-mono text-xs', 'data-loading:opacity-80', className)}
      >
        {loading ? <Spinner className="size-3 text-muted-foreground" /> : null}
        <ComboboxValue>{selected ? selected.label : placeholder}</ComboboxValue>
      </ComboboxTrigger>
      <ComboboxContent align={align} filter={matchOption}>
        <ComboboxInput placeholder={searchPlaceholder} />
        <ComboboxList>
          <ComboboxEmpty>{emptyText}</ComboboxEmpty>
          <ComboboxGroup>
            {options.map((option) => (
              <ComboboxItem
                key={option.value}
                value={option.value}
                keywords={[option.label, option.description ?? '', ...(option.keywords ?? [])]}
                disabled={option.disabled}
                selected={option.value === value}
                onSelect={() => choose(option)}
                className={cn(mono && 'font-mono text-xs')}
              >
                <span className="flex min-w-0 flex-col">
                  <span className="truncate">{option.label}</span>
                  {option.description ? (
                    <span className="truncate font-sans text-2xs text-muted-foreground">{option.description}</span>
                  ) : null}
                </span>
              </ComboboxItem>
            ))}
          </ComboboxGroup>
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}
