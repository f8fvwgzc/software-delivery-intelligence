import type { DiagramGroup, DiagramNode } from '../../api/queries'

/**
 * A tint is a hue token the canvas fills, strokes and writes with:
 * `--diagram-N` at the fill/stroke alphas and `--diagram-N-fg` for text
 * (web/src/index.css; `.dark` lowers the lightness and raises the alphas).
 */
export interface Tint {
  /** CSS colour for fills and strokes. */
  hue: string
  /** CSS colour for text on the fill. */
  fg: string
  /** Which of the six palette slots, or none for the neutral tint. */
  slot: number | null
}

export const TINT_COUNT = 6

export function paletteTint(index: number): Tint {
  const slot = (((index % TINT_COUNT) + TINT_COUNT) % TINT_COUNT) + 1
  return { hue: `var(--diagram-${slot})`, fg: `var(--diagram-${slot}-fg)`, slot }
}

/** Nodes outside any group, drawn in the muted neutrals. */
export const NEUTRAL_TINT: Tint = { hue: 'var(--muted-foreground)', fg: 'var(--foreground)', slot: null }
/** Actors: the one brand placement on the canvas. */
export const BRAND_TINT: Tint = { hue: 'var(--brand)', fg: 'var(--brand)', slot: null }
/** Datastores are always rose, external systems always green, whatever their group. */
export const DATASTORE_TINT = paletteTint(4)
export const EXTERNAL_TINT = paletteTint(2)

/** Alphas as tokens, so the export resolves the theme's values. */
export const FILL_ALPHA = 'var(--diagram-fill-alpha)'
export const STROKE_ALPHA = 'var(--diagram-stroke-alpha)'
export const GROUP_FILL_ALPHA = 'var(--diagram-group-fill-alpha)'
export const GROUP_STROKE_ALPHA = 'var(--diagram-group-stroke-alpha)'

/** Groups take the six tints in order. */
export function groupTints(groups: readonly DiagramGroup[]): Map<string, Tint> {
  const tints = new Map<string, Tint>()
  groups.forEach((group, index) => tints.set(group.id, paletteTint(index)))
  return tints
}

/** A node's tint: its kind first for actors, datastores and externals, then its group's, else neutral. */
export function nodeTint(node: DiagramNode, groups: ReadonlyMap<string, Tint>): Tint {
  if (node.kind === 'actor') return BRAND_TINT
  if (node.kind === 'datastore') return DATASTORE_TINT
  if (node.kind === 'external') return EXTERNAL_TINT
  return (node.groupId && groups.get(node.groupId)) || NEUTRAL_TINT
}
