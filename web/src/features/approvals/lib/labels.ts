import type { StatusTone } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

type ApprovalStatus = components['schemas']['ApprovalStatus']
type RiskLevel = components['schemas']['RiskLevel']
type PolicyDecision = components['schemas']['PolicyDecision']

export const APPROVAL_STATUS: Record<ApprovalStatus, { label: string; tone: StatusTone }> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  pending: { label: 'Pending', tone: 'warning' },
  approved: { label: 'Approved', tone: 'success' },
  rejected: { label: 'Rejected', tone: 'destructive' },
  expired: { label: 'Expired', tone: 'neutral' },
}

export const RISK: Record<RiskLevel, { label: string; dot: string }> = {
  unspecified: { label: 'Unknown risk', dot: 'bg-muted-foreground/50' },
  safe: { label: 'Safe', dot: 'bg-success' },
  medium: { label: 'Medium risk', dot: 'bg-warning' },
  high: { label: 'High risk', dot: 'bg-destructive' },
  blocked: { label: 'Blocked', dot: 'bg-foreground' },
}

export const POLICY_DECISION: Record<PolicyDecision, string> = {
  unspecified: 'No policy',
  auto: 'Runs automatically',
  approval: 'Needs approval',
  required: 'Approval required',
  blocked: 'Blocked by policy',
}
