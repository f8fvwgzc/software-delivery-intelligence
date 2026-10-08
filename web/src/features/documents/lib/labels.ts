import type { components } from '@/lib/api/schema'

export type DocumentKind = components['schemas']['DocumentKind']

/** The eight kinds that can be generated, in menu order. */
export const DOCUMENT_KINDS = [
  'readme',
  'architecture',
  'deployment_guide',
  'runbook',
  'adr',
  'uat_plan',
  'executive_report',
  'engineering_report',
] as const satisfies readonly DocumentKind[]

export const DOCUMENT_KIND_LABEL: Record<DocumentKind, string> = {
  unspecified: 'Document',
  readme: 'README',
  architecture: 'Architecture document',
  deployment_guide: 'Deployment guide',
  runbook: 'Runbook',
  adr: 'Architecture decision record',
  uat_plan: 'UAT plan',
  executive_report: 'Executive report',
  engineering_report: 'Engineering report',
}

/** Short tag shown in the list. */
export const DOCUMENT_KIND_SHORT: Record<DocumentKind, string> = {
  unspecified: 'Doc',
  readme: 'README',
  architecture: 'Architecture',
  deployment_guide: 'Deploy guide',
  runbook: 'Runbook',
  adr: 'ADR',
  uat_plan: 'UAT plan',
  executive_report: 'Exec report',
  engineering_report: 'Eng report',
}

/** File-safe name for the .md download. */
export function documentFileName(title: string, kind: DocumentKind) {
  const base = (title || kind)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return `${base || kind}.md`
}
