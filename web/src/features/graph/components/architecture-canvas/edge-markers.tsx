import { ARROW_ACCENT_ID, ARROW_ID } from './relation-edge'

interface ArrowProps {
  id: string
  stroke: string
}

function Arrow({ id, stroke }: ArrowProps) {
  return (
    <marker
      id={id}
      viewBox="0 0 10 10"
      refX="9"
      refY="5"
      markerWidth="8"
      markerHeight="8"
      markerUnits="userSpaceOnUse"
      orient="auto-start-reverse"
    >
      <path d="M 1.5 1.5 L 8.5 5 L 1.5 8.5" fill="none" stroke={stroke} strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" />
    </marker>
  )
}

/**
 * Open arrowheads in token colours. React Flow's built-in markers bake the
 * colour into the marker id, which breaks with `var()` values, so the edges
 * reference these instead.
 */
export function EdgeMarkers() {
  return (
    <svg aria-hidden className="absolute size-0 overflow-hidden">
      <defs>
        <Arrow id={ARROW_ID} stroke="var(--arch-edge)" />
        <Arrow id={ARROW_ACCENT_ID} stroke="var(--brand)" />
      </defs>
    </svg>
  )
}
