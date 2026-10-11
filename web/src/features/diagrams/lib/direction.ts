import type { DiagramSpec } from '../api/queries'

export type Direction = 'LR' | 'TB'

/**
 * Top-down for the graph kinds, like gitdiagram, so a repository reads
 * entry point → services → data. The backend currently writes the model
 * default (`LR`) into every builder's spec rather than a choice, so the
 * spec's direction only counts for the kinds where the builder or the
 * reasoner picks one on purpose: `dependency` (fan-in reads better
 * left-to-right) and `custom`. Schemas are always LR and sequences have
 * their own renderer.
 */
export function resolveDirection(spec: DiagramSpec | null | undefined, kind: string): Direction {
  if (kind === 'db_schema') return 'LR'
  if (kind === 'dependency' || kind === 'custom') {
    if (spec?.direction === 'TB' || spec?.direction === 'LR') return spec.direction
    return kind === 'dependency' ? 'LR' : 'TB'
  }
  return 'TB'
}
