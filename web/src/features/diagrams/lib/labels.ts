import {
  ArrowRightLeftIcon,
  BoxIcon,
  ClockIcon,
  CloudIcon,
  DatabaseIcon,
  GlobeIcon,
  LayersIcon,
  LayoutIcon,
  ListTreeIcon,
  MailIcon,
  NetworkIcon,
  PackageIcon,
  RouteIcon,
  ServerIcon,
  ShapesIcon,
  SparklesIcon,
  TableIcon,
  UserIcon,
  type LucideIcon,
} from 'lucide-react'

import type { StatusTone } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

export type DiagramKind = Exclude<components['schemas']['DiagramKind'], 'unspecified'>

/** The nine kinds a user can ask for, with what each is built from (CONTRACT.md §16). */
export const DIAGRAM_KINDS: readonly DiagramKind[] = [
  'architecture',
  'high_level',
  'data_flow',
  'deployment',
  'dependency',
  'sequence',
  'module_map',
  'db_schema',
  'custom',
]

interface KindMeta {
  label: string
  description: string
  icon: LucideIcon
}

export const DIAGRAM_KIND: Record<DiagramKind, KindMeta> = {
  architecture: {
    label: 'Architecture',
    description: 'Services, libraries, datastores and external APIs, grouped by directory.',
    icon: NetworkIcon,
  },
  high_level: {
    label: 'High level',
    description: 'Actors, the product and external systems. At most nine nodes.',
    icon: LayersIcon,
  },
  data_flow: {
    label: 'Data flow',
    description: 'Datastores, queues and services with read, write and publish edges.',
    icon: ArrowRightLeftIcon,
  },
  deployment: {
    label: 'Deployment',
    description: 'Environments, images, infrastructure and the pipelines that release to them.',
    icon: CloudIcon,
  },
  dependency: {
    label: 'Dependency',
    description: 'Packages by ecosystem with their fan-in.',
    icon: PackageIcon,
  },
  sequence: {
    label: 'Sequence',
    description: 'The principal workflow from actor to entry point to datastore and back.',
    icon: RouteIcon,
  },
  module_map: {
    label: 'Module map',
    description: 'Directories and packages as a tree of the repository.',
    icon: ListTreeIcon,
  },
  db_schema: {
    label: 'Database schema',
    description: 'Tables and columns from migrations and ORM models found in the scan.',
    icon: TableIcon,
  },
  custom: {
    label: 'Custom',
    description: 'From a prompt, or Mermaid source pasted in to import.',
    icon: SparklesIcon,
  },
}

export function kindMeta(kind: string): KindMeta {
  return DIAGRAM_KIND[kind as DiagramKind] ?? { label: kind, description: '', icon: ShapesIcon }
}

/** `generated` | `edited` | `mermaid`. */
export const DIAGRAM_SOURCE: Record<string, { label: string; tone: StatusTone; description: string }> = {
  generated: { label: 'Generated', tone: 'neutral', description: 'Built from the knowledge graph; nothing edited.' },
  edited: { label: 'Edited', tone: 'info', description: 'Title or spec changed since generation.' },
  mermaid: { label: 'Mermaid', tone: 'warning', description: 'The Mermaid source is authoritative; the spec is stale.' },
}

export function sourceMeta(source: string) {
  return DIAGRAM_SOURCE[source] ?? { label: source, tone: 'neutral' as StatusTone, description: '' }
}

/** Node kind → lucide icon, for lists and tables. */
const NODE_ICON: Record<string, LucideIcon> = {
  actor: UserIcon,
  service: ServerIcon,
  module: PackageIcon,
  ui: LayoutIcon,
  datastore: DatabaseIcon,
  queue: MailIcon,
  external: GlobeIcon,
  infra: CloudIcon,
  job: ClockIcon,
}

export function nodeIcon(kind: string): LucideIcon {
  return NODE_ICON[kind] ?? BoxIcon
}

export const NODE_KIND_LABEL: Record<string, string> = {
  actor: 'Actor',
  service: 'Service',
  module: 'Module',
  ui: 'UI',
  datastore: 'Datastore',
  queue: 'Queue',
  external: 'External',
  infra: 'Infrastructure',
  job: 'Job',
}

/** How the editorial canvas draws a node (CONTRACT.md §17): the shape says the kind, the tint says the group. */
export type NodeShape = 'circle' | 'rect' | 'rounded' | 'cylinder' | 'stadium' | 'hexagon' | 'subroutine'

const NODE_SHAPE: Record<string, NodeShape> = {
  actor: 'circle',
  service: 'rect',
  module: 'rect',
  job: 'rect',
  ui: 'rounded',
  datastore: 'cylinder',
  external: 'stadium',
  queue: 'hexagon',
  infra: 'subroutine',
}

export function nodeShape(kind: string): NodeShape {
  return NODE_SHAPE[kind] ?? 'rect'
}

/** Legend wording per shape, in the order the legend lists them. */
export const NODE_SHAPE_LABEL: Record<NodeShape, string> = {
  circle: 'Actor',
  rect: 'Service / module / job',
  rounded: 'UI',
  cylinder: 'Datastore',
  stadium: 'External',
  hexagon: 'Queue',
  subroutine: 'Infrastructure',
}

export const NODE_SHAPES: readonly NodeShape[] = ['circle', 'rect', 'rounded', 'cylinder', 'stadium', 'hexagon', 'subroutine']

export type EdgeStyle = 'call' | 'data' | 'async' | 'returns' | 'contains'

export const EDGE_STYLES: readonly EdgeStyle[] = ['call', 'data', 'async', 'returns', 'contains']

interface EdgeStyleMeta {
  label: string
  /** Always dashed, regardless of `edge.dashed`. */
  dashed: boolean
}

/** Every style is drawn in the foreground at 60%; `returns` is dashed like an inferred edge. */
export const EDGE_STYLE: Record<EdgeStyle, EdgeStyleMeta> = {
  call: { label: 'Call', dashed: false },
  data: { label: 'Data', dashed: false },
  async: { label: 'Async', dashed: false },
  returns: { label: 'Returns', dashed: true },
  contains: { label: 'Contains', dashed: false },
}

export function edgeStyle(style: string | undefined): EdgeStyle {
  return EDGE_STYLES.includes(style as EdgeStyle) ? (style as EdgeStyle) : 'call'
}

export function shortSha(sha: string | null | undefined) {
  return sha ? sha.slice(0, 7) : ''
}

/** File name for exports: "view-engine-architecture.svg". */
export function diagramFileName(title: string, extension: 'svg' | 'png') {
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return `${slug || 'diagram'}.${extension}`
}
