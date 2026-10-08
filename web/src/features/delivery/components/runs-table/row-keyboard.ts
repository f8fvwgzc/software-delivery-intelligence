import type { KeyboardEvent } from 'react'

/**
 * Row-level keys for data tables: arrows move between focusable rows,
 * Enter/Space run the row's action when it has one. Only fires when the row
 * itself has focus, so controls inside keep their own keys.
 */
export function handleRowKeyDown(event: KeyboardEvent<HTMLTableRowElement>, onActivate?: () => void) {
  if (event.target !== event.currentTarget) return

  switch (event.key) {
    case 'ArrowDown':
    case 'ArrowUp': {
      event.preventDefault()
      const body = event.currentTarget.parentElement
      const rows = body ? Array.from(body.querySelectorAll<HTMLTableRowElement>('tr[tabindex="0"]')) : []
      const index = rows.indexOf(event.currentTarget)
      rows[index + (event.key === 'ArrowDown' ? 1 : -1)]?.focus()
      break
    }
    case 'Enter':
    case ' ': {
      if (!onActivate) return
      event.preventDefault()
      onActivate()
      break
    }
    default:
      break
  }
}
