import {
  CircleHelpIcon,
  CloudIcon,
  ContainerIcon,
  DatabaseIcon,
  DatabaseZapIcon,
  FileCodeIcon,
  FileTextIcon,
  FlaskConicalIcon,
  FolderGitIcon,
  GlobeIcon,
  LayersIcon,
  PackageIcon,
  RocketIcon,
  ServerIcon,
  UserIcon,
  WorkflowIcon,
  type LucideIcon,
} from 'lucide-react'

import type { NodeKind, RelationCategory } from './types'
import { count } from '@/lib/intl'

export interface NodeKindMeta {
  label: string
  /** Lower-case plural for counts: "3 libraries". */
  plural: string
  icon: LucideIcon
}

/** Display order: structural kinds first, then delivery, then supporting. */
export const NODE_KINDS: readonly NodeKind[] = [
  'repository',
  'service',
  'library',
  'datastore',
  'external_api',
  'container_image',
  'pipeline',
  'deployment',
  'environment',
  'infrastructure',
  'iac_module',
  'migration',
  'test_suite',
  'document',
  'owner',
  'unspecified',
]

export const NODE_KIND_META: Record<NodeKind, NodeKindMeta> = {
  unspecified: { label: 'Unknown', plural: 'unknown nodes', icon: CircleHelpIcon },
  repository: { label: 'Repository', plural: 'repositories', icon: FolderGitIcon },
  service: { label: 'Service', plural: 'services', icon: ServerIcon },
  library: { label: 'Library', plural: 'libraries', icon: PackageIcon },
  datastore: { label: 'Datastore', plural: 'datastores', icon: DatabaseIcon },
  infrastructure: { label: 'Infrastructure', plural: 'infrastructure parts', icon: CloudIcon },
  pipeline: { label: 'Pipeline', plural: 'pipelines', icon: WorkflowIcon },
  environment: { label: 'Environment', plural: 'environments', icon: LayersIcon },
  deployment: { label: 'Deployment', plural: 'deployments', icon: RocketIcon },
  container_image: { label: 'Container image', plural: 'container images', icon: ContainerIcon },
  iac_module: { label: 'IaC module', plural: 'IaC modules', icon: FileCodeIcon },
  test_suite: { label: 'Test suite', plural: 'test suites', icon: FlaskConicalIcon },
  document: { label: 'Document', plural: 'documents', icon: FileTextIcon },
  external_api: { label: 'External API', plural: 'external APIs', icon: GlobeIcon },
  owner: { label: 'Owner', plural: 'owners', icon: UserIcon },
  migration: { label: 'Migration', plural: 'migrations', icon: DatabaseZapIcon },
}

/**
 * The overview panel lists the parts of a codebase in these plain-language
 * buckets, after Floorplan's "Systems" list. Kinds not named fall to "Other".
 */
export const SYSTEM_GROUPS: readonly { label: string; hint: string; kinds: readonly NodeKind[] }[] = [
  { label: 'Apps and services', hint: 'what runs', kinds: ['service'] },
  { label: 'Shared code', hint: 'packages the apps build on', kinds: ['library'] },
  { label: 'Data', hint: 'where state lives and how it changes', kinds: ['datastore', 'migration'] },
  { label: 'Outside services', hint: 'what the code calls out to', kinds: ['external_api'] },
  {
    label: 'Delivery',
    hint: 'how it is built and shipped',
    kinds: ['pipeline', 'container_image', 'deployment', 'environment', 'infrastructure', 'iac_module'],
  },
  { label: 'Quality and docs', hint: 'tests and written knowledge', kinds: ['test_suite', 'document'] },
]

export interface RelationStyle {
  label: string
  /** SVG dash pattern; solid when unset. */
  dash?: string
  /** Drawn in the brand colour. Governance only. */
  accent?: boolean
}

/** Structure solid, infrastructure dashed, validation dotted, governance accent. */
export const RELATION_STYLE: Record<RelationCategory, RelationStyle> = {
  unspecified: { label: 'Other' },
  ownership: { label: 'Ownership' },
  structure: { label: 'Structure' },
  source: { label: 'Source' },
  execution: { label: 'Execution' },
  infrastructure: { label: 'Infrastructure', dash: '6 4' },
  validation: { label: 'Validation', dash: '1.5 3.5' },
  governance: { label: 'Governance', accent: true },
  outcome: { label: 'Outcome' },
}

/** `DEPENDS_ON` → `depends on`. */
export function relationLabel(relation: string) {
  return relation.toLowerCase().replaceAll('_', ' ')
}

/** "1 hop" / "3 hops" */
export function hopsLabel(distance: number) {
  return count(distance, 'hop', 'hops')
}
