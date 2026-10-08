import type { components } from '@/lib/api/schema'

type RiskLevel = components['schemas']['RiskLevel']
type PolicyDecision = components['schemas']['PolicyDecision']

/** Group order on the page, safest first. */
export const RISK_ORDER: RiskLevel[] = ['safe', 'medium', 'high', 'blocked']

export const RISK: Record<RiskLevel, { label: string; description: string; dot: string }> = {
  unspecified: { label: 'Unclassified', description: 'Actions without a risk rating.', dot: 'bg-muted-foreground/50' },
  safe: { label: 'Safe', description: 'Read-only or local. Cannot change anything shared.', dot: 'bg-success' },
  medium: { label: 'Medium', description: 'Creates disposable resources or runs code in isolation.', dot: 'bg-warning' },
  high: { label: 'High', description: 'Touches shared or production systems.', dot: 'bg-destructive' },
  blocked: { label: 'Blocked', description: 'Never runs through the platform.', dot: 'bg-foreground' },
}

export const POLICY_DECISION: Record<PolicyDecision, { label: string; description: string }> = {
  unspecified: { label: 'Unset', description: 'No decision recorded.' },
  auto: { label: 'Auto', description: 'Runs without asking.' },
  approval: { label: 'Approval', description: 'Waits for a human before running.' },
  required: { label: 'Required', description: 'Always needs a human. Cannot be relaxed.' },
  blocked: { label: 'Blocked', description: 'Refused outright.' },
}

/** Decisions an operator may set on an editable policy. */
export const EDITABLE_DECISIONS: PolicyDecision[] = ['auto', 'approval', 'required']

/** CONTRACT.md §5.3 */
export const PERMISSIONS = [
  'repository:read',
  'container:build',
  'container:run',
  'environment:write',
  'database:write',
  'cloud:write',
  'deploy:staging',
  'deploy:production',
  'secrets:write',
] as const

export const IDENTITY_KIND: Record<string, string> = {
  simulated: 'Dry run',
  github: 'GitHub',
  cloud: 'Cloud',
}

export function isExpired(expiresAt?: string | null, now = Date.now()) {
  if (!expiresAt) return false
  const time = Date.parse(expiresAt)
  return !Number.isNaN(time) && time <= now
}
