import type { DiagramSpec } from '../api/queries'

/** `LR` by default; `TB` for sequence and module maps unless the spec says otherwise (CONTRACT.md §17). */
export function resolveDirection(spec: DiagramSpec | null | undefined, kind: string): 'LR' | 'TB' {
  if (spec?.direction === 'TB' || spec?.direction === 'LR') return spec.direction
  return kind === 'sequence' || kind === 'module_map' ? 'TB' : 'LR'
}
