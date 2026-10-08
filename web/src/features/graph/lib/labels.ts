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

export interface NodeKindMeta {
  label: string
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
  unspecified: { label: 'Unknown', icon: CircleHelpIcon },
  repository: { label: 'Repository', icon: FolderGitIcon },
  service: { label: 'Service', icon: ServerIcon },
  library: { label: 'Library', icon: PackageIcon },
  datastore: { label: 'Datastore', icon: DatabaseIcon },
  infrastructure: { label: 'Infrastructure', icon: CloudIcon },
  pipeline: { label: 'Pipeline', icon: WorkflowIcon },
  environment: { label: 'Environment', icon: LayersIcon },
  deployment: { label: 'Deployment', icon: RocketIcon },
  container_image: { label: 'Container image', icon: ContainerIcon },
  iac_module: { label: 'IaC module', icon: FileCodeIcon },
  test_suite: { label: 'Test suite', icon: FlaskConicalIcon },
  document: { label: 'Document', icon: FileTextIcon },
  external_api: { label: 'External API', icon: GlobeIcon },
  owner: { label: 'Owner', icon: UserIcon },
  migration: { label: 'Migration', icon: DatabaseZapIcon },
}

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
  return distance === 1 ? '1 hop' : `${distance} hops`
}
