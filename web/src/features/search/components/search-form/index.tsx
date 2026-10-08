import { SearchIcon, XIcon } from 'lucide-react'

import { Field, FieldLabel } from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { KNOWLEDGE_KIND, SEARCH_KINDS } from '../../lib/labels'
import { ProjectSelect } from './project-select'

interface SearchFormProps {
  query: string
  onQueryChange: (query: string) => void
  projectId: string
  onProjectChange: (projectId: string) => void
  kinds: string[]
  onKindsChange: (kinds: string[]) => void
  /** True while results for the current query are being fetched. */
  searching: boolean
}

/** Search box, project scope and kind toggles. The form never submits; typing searches. */
export function SearchForm({
  query,
  onQueryChange,
  projectId,
  onProjectChange,
  kinds,
  onKindsChange,
  searching,
}: SearchFormProps) {
  return (
    <form
      role="search"
      className="flex flex-col gap-3"
      onSubmit={(event) => event.preventDefault()}
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <InputGroup className="flex-1">
          <InputGroupAddon>
            <SearchIcon />
          </InputGroupAddon>
          <InputGroupInput
            type="text"
            enterKeyHint="search"
            autoFocus
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder="Search code, infrastructure, decisions, documents…"
            aria-label="Search the knowledge base"
            autoComplete="off"
            spellCheck={false}
          />
          <InputGroupAddon align="inline-end">
            {searching ? <Spinner className="size-3.5 text-muted-foreground" /> : null}
            {query.length > 0 ? (
              <InputGroupButton aria-label="Clear search" size="icon-xs" onClick={() => onQueryChange('')}>
                <XIcon />
              </InputGroupButton>
            ) : null}
          </InputGroupAddon>
        </InputGroup>
        <ProjectSelect value={projectId} onChange={onProjectChange} />
      </div>
      <Field orientation="horizontal" className="w-auto gap-2">
        <FieldLabel id="search-kinds-label" className="text-xs font-normal text-muted-foreground">
          Kinds
        </FieldLabel>
        <ToggleGroup
          type="multiple"
          size="sm"
          variant="outline"
          spacing={1}
          value={kinds}
          onValueChange={onKindsChange}
          aria-labelledby="search-kinds-label"
          className="flex-wrap"
        >
          {SEARCH_KINDS.map((kind) => (
            <ToggleGroupItem key={kind} value={kind}>
              {KNOWLEDGE_KIND[kind].label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        {kinds.length > 0 ? (
          <button
            type="button"
            className="text-xs text-muted-foreground underline-offset-4 hover:underline"
            onClick={() => onKindsChange([])}
          >
            Any kind
          </button>
        ) : null}
      </Field>
    </form>
  )
}
