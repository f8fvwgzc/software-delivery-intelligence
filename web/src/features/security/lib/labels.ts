import type { TagTone } from '@/components/system/tag'
import { ecosystemLabel as manifestEcosystemLabel } from '@/features/dependencies/lib/labels'
import { count } from '@/lib/intl'

import type {
  AlertsState,
  SecuritySource,
  SecurityUpdatesState,
  SeverityCount,
  VulnerabilityScope,
  VulnerabilitySeverity,
} from '../api/types'

interface StatusMeta {
  label: string
  tone: TagTone
}

export const SEVERITY: Record<VulnerabilitySeverity, StatusMeta> = {
  critical: { label: 'Critical', tone: 'destructive' },
  high: { label: 'High', tone: 'destructive' },
  medium: { label: 'Medium', tone: 'warning' },
  low: { label: 'Low', tone: 'info' },
  unknown: { label: 'Unrated', tone: 'neutral' },
}

/** `severity` is an open string in the contract; a word a newer gateway adds still renders. */
export function severityMeta(severity: string): StatusMeta {
  return SEVERITY[severity as VulnerabilitySeverity] ?? { label: severity, tone: 'neutral' }
}

/** "2 critical, 5 high, 1 medium", in the order the server sent (most severe first); empty when nothing is open. */
export function severityBreakdown(counts: readonly SeverityCount[]): string {
  return counts
    .filter((row) => row.count > 0)
    .map((row) => `${row.count.toLocaleString()} ${severityMeta(row.severity).label.toLowerCase()}`)
    .join(', ')
}

export const SOURCE: Record<SecuritySource, { label: string; description: string }> = {
  dependabot: { label: 'Dependabot alerts', description: 'GitHub’s open alerts for this repository' },
  osv: { label: 'OSV lookup', description: 'The exact versions in the manifests, looked up at OSV' },
  none: { label: 'No source', description: 'Neither Dependabot nor OSV answered' },
}

/** `source` is an open string; an unknown one is shown as it is. */
export function sourceMeta(source: string): { label: string; description: string } {
  return SOURCE[source as SecuritySource] ?? { label: source, description: '' }
}

const ALERTS: Record<AlertsState, StatusMeta> = {
  enabled: { label: 'Alerts on', tone: 'success' },
  disabled: { label: 'Alerts off', tone: 'warning' },
  unknown: { label: 'Alerts unknown', tone: 'neutral' },
}

export function alertsMeta(state: string): StatusMeta {
  return ALERTS[state as AlertsState] ?? ALERTS.unknown
}

const SECURITY_UPDATES: Record<SecurityUpdatesState, StatusMeta> = {
  enabled: { label: 'Security updates on', tone: 'success' },
  paused: { label: 'Security updates paused', tone: 'warning' },
  disabled: { label: 'Security updates off', tone: 'neutral' },
  unknown: { label: 'Security updates unknown', tone: 'neutral' },
}

export function securityUpdatesMeta(state: string): StatusMeta {
  return SECURITY_UPDATES[state as SecurityUpdatesState] ?? SECURITY_UPDATES.unknown
}

/** Advisory ecosystems the dependency manifests name differently. */
const ADVISORY_ECOSYSTEM: Record<string, string> = {
  rubygems: 'RubyGems',
  actions: 'GitHub Actions',
}

/** Display name of an advisory's ecosystem; unknown keys are shown as they are. */
export function ecosystemLabel(key: string) {
  return ADVISORY_ECOSYSTEM[key] ?? manifestEcosystemLabel(key)
}

const SCOPE: Record<VulnerabilityScope, string> = {
  runtime: 'Runtime',
  development: 'Development',
}

export function scopeLabel(scope: string) {
  return SCOPE[scope as VulnerabilityScope] ?? scope
}

/** "3 open Dependabot pull requests". */
export function dependabotPullRequests(open: number) {
  return count(open, 'open Dependabot pull request', 'open Dependabot pull requests')
}
