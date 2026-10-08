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

/** The seven kinds a proposal can be about, in the order they are offered. */
export const PROPOSAL_KINDS = [
  'deployment',
  'infrastructure',
  'ci_cd',
  'testing',
  'security',
  'documentation',
  'dependency',
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

/** Risk per policy action type (CONTRACT §5.2). Unknown types read as medium. */
export type RiskLevel = 'safe' | 'medium' | 'high' | 'blocked'

const ACTION_RISK: Record<string, RiskLevel> = {
  'repo.read': 'safe',
  'repo.analyze': 'safe',
  'tests.run_local': 'safe',
  'docs.generate': 'safe',
  'deps.install': 'medium',
  'container.build': 'medium',
  'container.run': 'medium',
  'env.create_temporary': 'medium',
  'db.create_temporary': 'medium',
  'cloud.create_dev_resource': 'medium',
  'cloud.modify_resource': 'high',
  'deploy.staging': 'high',
  'deploy.production': 'high',
  'db.migrate_production': 'high',
  'backup.restore_production': 'high',
  'resource.delete': 'high',
  'firewall.modify': 'high',
  'secrets.rotate': 'high',
  'db.destroy_production': 'blocked',
}

export function riskOf(actionType: string): RiskLevel {
  return ACTION_RISK[actionType] ?? 'medium'
}

export const RISK_TONE: Record<RiskLevel, StatusTone> = {
  safe: 'success',
  medium: 'warning',
  high: 'destructive',
  blocked: 'destructive',
}

export const RISK_LABEL: Record<RiskLevel, string> = {
  safe: 'Safe',
  medium: 'Medium risk',
  high: 'High risk',
  blocked: 'Blocked',
}
