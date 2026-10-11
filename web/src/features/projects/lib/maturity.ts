import type { StatusTone } from '@/components/shared/data/status-badge'

/** `legacy` < 35, `transitional` 35–69, `modern` ≥ 70 (CONTRACT.md §13). */
export type MaturityStage = 'legacy' | 'transitional' | 'modern'

export function maturityStage(score: number): MaturityStage {
  if (score >= 70) return 'modern'
  if (score >= 35) return 'transitional'
  return 'legacy'
}

export const MATURITY_STAGE: Record<MaturityStage, { label: string; tone: StatusTone }> = {
  legacy: { label: 'Legacy', tone: 'warning' },
  transitional: { label: 'Transitional', tone: 'info' },
  modern: { label: 'Modern', tone: 'success' },
}

/** The server's stage word, or the one derived from the score when it is unknown. */
export function stageLabel(stage: string, score: number) {
  const known = MATURITY_STAGE[stage as MaturityStage]
  return known ?? MATURITY_STAGE[maturityStage(score)]
}

export const MATURITY_SEGMENTS = 8

/** How many of the eight segments a 0–100 score fills. */
export function filledSegments(score: number) {
  const clamped = Math.max(0, Math.min(100, score))
  return Math.round((clamped / 100) * MATURITY_SEGMENTS)
}

/** Brand fill opacity for the bar: faint at 0, full at 100. */
export function segmentOpacity(score: number) {
  const clamped = Math.max(0, Math.min(100, score))
  return 0.35 + 0.65 * (clamped / 100)
}
