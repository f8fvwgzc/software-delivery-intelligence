import type { StatusTone } from './status-badge'
import type { components } from '@/lib/api/schema'

export type RiskLevel = components['schemas']['RiskLevel']

/**
 * One vocabulary for risk across every feature: the label the operator
 * reads, the badge tone and the 6px dot fill. Features used to carry their
 * own copies of this map; the dot and label are the signal, so they must
 * read the same wherever a step, policy or approval is shown.
 */
export const RISK: Record<RiskLevel, { label: string; tone: StatusTone; dot: string }> = {
  unspecified: { label: 'Unknown risk', tone: 'neutral', dot: 'bg-muted-foreground/50' },
  safe: { label: 'Safe', tone: 'success', dot: 'bg-success' },
  medium: { label: 'Medium risk', tone: 'warning', dot: 'bg-warning' },
  high: { label: 'High risk', tone: 'destructive', dot: 'bg-destructive' },
  blocked: { label: 'Blocked', tone: 'destructive', dot: 'bg-foreground' },
}

/** Risk per policy action type (CONTRACT §5.2). Unknown types read as medium. */
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
  'repo.checkout': 'safe',
  'env.destroy_temporary': 'safe',
  'repo.open_pull_request': 'medium',
}

/** Risk of a plan step that only carries its action type (proposal and pattern plans). */
export function riskOf(actionType: string): RiskLevel {
  return ACTION_RISK[actionType] ?? 'medium'
}
