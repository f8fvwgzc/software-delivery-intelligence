import {
  BoxesIcon,
  CodeIcon,
  FileTextIcon,
  LayersIcon,
  MessagesSquareIcon,
  ScaleIcon,
  ServerCogIcon,
  WorkflowIcon,
  type LucideIcon,
} from 'lucide-react'

import type { components } from '@/lib/api/schema'

export type KnowledgeKind = components['schemas']['KnowledgeKind']

/** The kinds a user can narrow to, in toggle order. */
export const SEARCH_KINDS = [
  'code',
  'iac',
  'ci_cd',
  'architecture',
  'document',
  'pattern',
  'decision',
  'conversation',
] as const satisfies readonly KnowledgeKind[]

export const KNOWLEDGE_KIND: Record<KnowledgeKind, { label: string; icon: LucideIcon }> = {
  unspecified: { label: 'Other', icon: FileTextIcon },
  code: { label: 'Code', icon: CodeIcon },
  iac: { label: 'IaC', icon: ServerCogIcon },
  document: { label: 'Document', icon: FileTextIcon },
  ci_cd: { label: 'CI/CD', icon: WorkflowIcon },
  architecture: { label: 'Architecture', icon: BoxesIcon },
  pattern: { label: 'Pattern', icon: LayersIcon },
  decision: { label: 'Decision', icon: ScaleIcon },
  conversation: { label: 'Conversation', icon: MessagesSquareIcon },
}

/** Queries shorter than this are not sent. */
export const MIN_QUERY_LENGTH = 2
