import type { StatusTone } from '@/components/shared/data/status-badge'
import type { ScanStatus } from '@/features/projects/api/queries'

import type { Fact, FactCategory } from '../api/queries'

interface StatusLabel {
  label: string
  tone: StatusTone
  /** The status dot pulses: something is happening right now. */
  live?: boolean
}

export const SCAN_STATUS: Record<ScanStatus, StatusLabel> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  queued: { label: 'Queued', tone: 'neutral' },
  running: { label: 'Running', tone: 'info', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
}

export const FACT_CATEGORY: Record<FactCategory, string> = {
  unspecified: 'Other',
  repository: 'Repository',
  language: 'Languages',
  framework: 'Frameworks',
  datastore: 'Datastores',
  container: 'Containers',
  ci_cd: 'CI/CD',
  iac: 'Infrastructure as code',
  cloud: 'Cloud',
  testing: 'Testing',
  dependency: 'Dependencies',
  security: 'Security',
  documentation: 'Documentation',
  deployment: 'Deployment',
  migration: 'Migrations',
}

/** Display order: what the system is, then how it is built, run and shipped. */
const FACT_CATEGORY_ORDER: FactCategory[] = [
  'repository',
  'language',
  'framework',
  'datastore',
  'dependency',
  'testing',
  'container',
  'ci_cd',
  'iac',
  'cloud',
  'deployment',
  'migration',
  'security',
  'documentation',
  'unspecified',
]

export interface FactGroup {
  category: FactCategory
  label: string
  facts: Fact[]
}

/** Groups facts by category in display order. Stable, so it can be a query `select`. */
export function groupFactsByCategory(facts: Fact[]): FactGroup[] {
  const byCategory = new Map<FactCategory, Fact[]>()
  for (const fact of facts) {
    const bucket = byCategory.get(fact.category)
    if (bucket) {
      bucket.push(fact)
    } else {
      byCategory.set(fact.category, [fact])
    }
  }

  return FACT_CATEGORY_ORDER.flatMap((category) => {
    const bucket = byCategory.get(category)
    return bucket ? [{ category, label: FACT_CATEGORY[category], facts: bucket }] : []
  })
}

/** Inferred facts, least certain first: those are the ones worth a look. */
export function sortByConfidence(facts: Fact[]): Fact[] {
  return facts.toSorted((a, b) => a.confidence - b.confidence)
}
