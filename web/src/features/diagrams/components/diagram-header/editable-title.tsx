import { CheckIcon, PencilIcon, XIcon } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

interface EditableTitleProps {
  value: string
  onSave: (title: string) => void
  saving: boolean
}

/** The h1, with a pencil that turns it into an input. Enter saves, Escape cancels, blur saves when changed. */
export function EditableTitle({ value, onSave, saving }: EditableTitleProps) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(value)

  const start = () => {
    setDraft(value)
    setEditing(true)
  }
  const cancel = () => setEditing(false)
  const commit = () => {
    const next = draft.trim()
    setEditing(false)
    if (next.length > 0 && next !== value) onSave(next)
  }
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') commit()
    if (event.key === 'Escape') cancel()
  }

  if (editing) {
    return (
      <form
        className="flex min-w-0 items-center gap-1.5"
        onSubmit={(event) => {
          event.preventDefault()
          commit()
        }}
      >
        <Input
          autoFocus
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={onKeyDown}
          onBlur={commit}
          aria-label="Diagram title"
          maxLength={120}
          className="h-8 max-w-md text-base font-semibold tracking-tight"
        />
        <Button type="submit" variant="ghost" size="icon-sm" aria-label="Save title" onMouseDown={(event) => event.preventDefault()}>
          <CheckIcon />
        </Button>
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Cancel" onMouseDown={(event) => event.preventDefault()} onClick={cancel}>
          <XIcon />
        </Button>
      </form>
    )
  }

  return (
    <div className="flex min-w-0 items-center gap-1.5">
      <h1 className="truncate text-lg font-semibold leading-7 tracking-tight">{value}</h1>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="Rename diagram" onClick={start} loading={saving}>
            <PencilIcon className="size-3.5 text-muted-foreground" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Rename</TooltipContent>
      </Tooltip>
    </div>
  )
}
