import type { TagTone } from '@/components/system/tag'

import type { InspectedBinary, ReleaseInspection } from '../api/types'

/*
 * CONTRACT.md §22.1: what reverse-engineering a release image found. The
 * report is the gateway's `InspectionReport`; these are the words and
 * tones the release list and the report sheet show for it.
 */

export type Severity = 'info' | 'low' | 'medium' | 'high' | 'critical'

export const SEVERITIES: readonly Severity[] = ['critical', 'high', 'medium', 'low', 'info']

export const SEVERITY_TONE: Record<Severity, TagTone> = {
  critical: 'destructive',
  high: 'destructive',
  medium: 'warning',
  low: 'neutral',
  info: 'neutral',
}

export function severityTone(severity: string | null | undefined): TagTone {
  return SEVERITY_TONE[severity as Severity] ?? 'neutral'
}

/** The tag a release shows for its last inspection. */
export function inspectionSummary(inspection: ReleaseInspection): { label: string; tone: TagTone } {
  if (inspection.status === 'passed') return { label: 'Image clean', tone: 'success' }
  if (inspection.status === 'incomplete') return { label: 'Inspection incomplete', tone: 'warning' }
  const n = inspection.findingCount
  return {
    label: `${n} ${n === 1 ? 'finding' : 'findings'}${inspection.maxSeverity ? `, ${inspection.maxSeverity}` : ''}`,
    tone: severityTone(inspection.maxSeverity),
  }
}

/** Hardening properties in the order the report sheet shows them, with short labels. */
export const HARDENING_LABELS: Record<string, string> = {
  pie: 'PIE',
  nx: 'NX',
  relro: 'RELRO',
  stack_protector: 'Canary',
  fortify: 'Fortify',
  rpath: 'RPATH',
  runpath: 'RUNPATH',
  stripped: 'Stripped',
}

/**
 * The tone of one hardening state. `rpath`/`runpath` read the other way
 * round: having one is the risk, not the protection.
 */
export function hardeningTone(property: string, state: string): TagTone {
  const inverted = property === 'rpath' || property === 'runpath'
  if (state === 'not_applicable' || state === 'unknown') return 'neutral'
  if (state === 'partial') return 'warning'
  const good = inverted ? state === 'disabled' : state === 'enabled'
  if (property === 'stripped') return good ? 'success' : 'neutral'
  return good ? 'success' : 'warning'
}

export function languageLabel(binary: InspectedBinary) {
  const language = binary.language === 'go' ? 'Go' : binary.language === 'rust' ? 'Rust' : 'C/C++'
  return binary.toolchain ? `${language}, ${binary.toolchain}` : language
}

/** `binary.no_pie` or `binary.*`, the analyzer's and the execution service's check. */
export function isRulePattern(pattern: string) {
  return /^[a-z0-9_]+(\.[a-z0-9_]+)*(\.\*)?$/.test(pattern) && pattern.length <= 66
}

export const INSPECTION_KEYS: ReadonlySet<string> = new Set(['inspect', 'inspect_fail_on', 'inspect_ignore'])
