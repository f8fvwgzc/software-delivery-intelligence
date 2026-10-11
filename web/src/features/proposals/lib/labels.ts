import {
  BookOpenIcon,
  ClipboardCheckIcon,
  FileTextIcon,
  GitBranchIcon,
  HistoryIcon,
  LayersIcon,
  MessageSquareIcon,
  ScaleIcon,
  type LucideIcon,
} from 'lucide-react'

import type { StatusTone } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

export type ProposalKind = components['schemas']['ProposalKind']
export type ProposalStatus = components['schemas']['ProposalStatus']
export type EvidenceKind = components['schemas']['EvidenceKind']
export type MessageRole = components['schemas']['MessageRole']
export type ScoreBreakdown = components['schemas']['ScoreBreakdown']
export type ScoreWeights = components['schemas']['ScoreWeights']

/** The eight kinds a proposal can be about, in the order they are offered. */
export const PROPOSAL_KINDS = [
  'deployment',
  'infrastructure',
  'ci_cd',
  'testing',
  'security',
  'documentation',
  'dependency',
  'modernization',
] as const satisfies readonly ProposalKind[]

export const PROPOSAL_KIND_LABEL: Record<ProposalKind, string> = {
  unspecified: 'Unspecified',
  deployment: 'Deployment',
  infrastructure: 'Infrastructure',
  ci_cd: 'CI/CD',
  testing: 'Testing',
  security: 'Security',
  documentation: 'Documentation',
  dependency: 'Dependencies',
  modernization: 'Modernization',
}

/** True for a string that names one of the offered kinds (e.g. from `?kind=`). */
export function isProposalKind(value: string | null | undefined): value is (typeof PROPOSAL_KINDS)[number] {
  return PROPOSAL_KINDS.includes(value as (typeof PROPOSAL_KINDS)[number])
}

export const PROPOSAL_STATUS: Record<ProposalStatus, { label: string; tone: StatusTone; live?: boolean }> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  proposed: { label: 'Proposed', tone: 'info' },
  negotiating: { label: 'Negotiating', tone: 'brand', live: true },
  accepted: { label: 'Accepted', tone: 'success' },
  rejected: { label: 'Rejected', tone: 'destructive' },
  superseded: { label: 'Superseded', tone: 'neutral' },
}

/** Terminal states: no negotiation, no accept or reject. */
export function isProposalClosed(status: ProposalStatus) {
  return status === 'accepted' || status === 'rejected' || status === 'superseded'
}

export const EVIDENCE_KIND: Record<EvidenceKind, { label: string; icon: LucideIcon }> = {
  unspecified: { label: 'Evidence', icon: FileTextIcon },
  fact: { label: 'Fact', icon: ClipboardCheckIcon },
  pattern: { label: 'Pattern', icon: LayersIcon },
  decision: { label: 'Decision', icon: ScaleIcon },
  delivery_history: { label: 'Delivery history', icon: HistoryIcon },
  similar_project: { label: 'Similar project', icon: GitBranchIcon },
  policy: { label: 'Policy', icon: BookOpenIcon },
  answer: { label: 'Answer', icon: MessageSquareIcon },
}

/** Signals of the ranking formula (CONTRACT §6), in weight order. */
export const SCORE_SIGNALS = [
  { key: 'semanticSimilarity', label: 'Semantic similarity' },
  { key: 'policy', label: 'Policy agreement' },
  { key: 'success', label: 'Success rate' },
  { key: 'architectureFit', label: 'Architecture fit' },
  { key: 'recency', label: 'Recency' },
  { key: 'cost', label: 'Cost' },
  { key: 'security', label: 'Security' },
] as const satisfies readonly { key: keyof ScoreWeights; label: string }[]

export const MESSAGE_ROLE_LABEL: Record<MessageRole, string> = {
  unspecified: 'Note',
  user: 'You',
  assistant: 'Assistant',
  system: 'System',
}
