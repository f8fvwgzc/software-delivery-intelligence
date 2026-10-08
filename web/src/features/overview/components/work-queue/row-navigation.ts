import type { KeyboardEvent } from 'react'

/**
 * Arrow keys and j/k move focus between the row actions (`data-row-link`),
 * so the queue is walkable without tabbing through every cell.
 */
export function handleRowNavigation(event: KeyboardEvent<HTMLElement>) {
  const delta =
    event.key === 'ArrowDown' || event.key === 'j' ? 1 : event.key === 'ArrowUp' || event.key === 'k' ? -1 : 0
  if (delta === 0) return

  const targets = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[data-row-link]'))
  if (targets.length === 0) return

  const current = targets.indexOf(document.activeElement as HTMLElement)
  const next = targets[current === -1 ? (delta > 0 ? 0 : targets.length - 1) : current + delta]
  if (!next) return

  event.preventDefault()
  next.focus()
}
