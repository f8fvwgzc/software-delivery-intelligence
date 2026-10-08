import { SearchIcon, XIcon } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Kbd } from '@/components/ui/kbd'

interface NodeSearchProps {
  value: string
  onChange: (value: string) => void
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)
}

/** Label search with a `/` shortcut, like every list in the app. */
export function NodeSearch({ value, onChange }: NodeSearchProps) {
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey) return
      if (isTypingTarget(event.target)) return
      event.preventDefault()
      inputRef.current?.focus()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  return (
    <InputGroup className="w-full sm:w-56">
      <InputGroupAddon>
        <SearchIcon />
      </InputGroupAddon>
      <InputGroupInput
        ref={inputRef}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="Find a node"
        aria-label="Search nodes by label"
        autoComplete="off"
        spellCheck={false}
      />
      <InputGroupAddon align="inline-end">
        {value ? (
          <InputGroupButton size="icon-xs" aria-label="Clear search" onClick={() => onChange('')}>
            <XIcon />
          </InputGroupButton>
        ) : (
          <Kbd>/</Kbd>
        )}
      </InputGroupAddon>
    </InputGroup>
  )
}
