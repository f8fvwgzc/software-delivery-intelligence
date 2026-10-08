import type { StatusTone } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

export type PatternSource = components['schemas']['PatternSource']
export type PatternStatus = components['schemas']['PatternStatus']

/** Categories from CONTRACT §5.6, in catalog order. */
export const PATTERN_CATEGORIES = [
  'deployment',
  'ci_cd',
  'testing',
  'infrastructure',
  'security',
  'documentation',
  'dependency',
] as const

export type PatternCategory = (typeof PATTERN_CATEGORIES)[number]

const CATEGORY_LABEL: Record<PatternCategory, string> = {
  deployment: 'Deployment',
  ci_cd: 'CI/CD',
  testing: 'Testing',
  infrastructure: 'Infrastructure',
  security: 'Security',
  documentation: 'Documentation',
  dependency: 'Dependencies',
}

/** Categories are free strings on the wire; unknown ones are shown as-is. */
export function categoryLabel(category: string) {
  return (CATEGORY_LABEL as Record<string, string>)[category] ?? category
}

export const PATTERN_SOURCE: Record<PatternSource, { label: string; tone: StatusTone }> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  catalog: { label: 'Catalog', tone: 'info' },
  learned: { label: 'Learned', tone: 'brand' },
}

const CLOUD_LABEL: Record<string, string> = {
  azure: 'Azure',
  aws: 'AWS',
  gcp: 'Google Cloud',
  any: 'Any cloud',
  self_hosted: 'Self-hosted',
  github: 'GitHub',
}

export function cloudLabel(cloud: string) {
  return CLOUD_LABEL[cloud] ?? cloud
}

/** `serverless_container` → "serverless container". */
export function humanize(value: string) {
  return value.replace(/_/g, ' ')
}
