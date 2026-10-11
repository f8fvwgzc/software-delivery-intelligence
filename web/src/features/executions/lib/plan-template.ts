import type { components } from '@/lib/api/schema'

type PlanStep = components['schemas']['PlanStep']

/** CONTRACT.md §5.3: the permissions each action type needs by default. */
export const DEFAULT_PERMISSIONS: Record<string, string[]> = {
  'repo.read': ['repository:read'],
  'repo.analyze': ['repository:read'],
  'tests.run_local': [],
  'docs.generate': ['repository:read'],
  'deps.install': [],
  'container.build': ['container:build'],
  'container.run': ['container:run'],
  'env.create_temporary': ['environment:write'],
  'db.create_temporary': ['database:write'],
  'cloud.create_dev_resource': ['cloud:write'],
  'cloud.modify_resource': ['cloud:write'],
  'deploy.staging': ['deploy:staging'],
  'deploy.production': ['deploy:production'],
  'db.migrate_production': ['database:write'],
  'backup.restore_production': ['database:write'],
  'resource.delete': ['cloud:write'],
  'firewall.modify': ['cloud:write'],
  'secrets.rotate': ['secrets:write'],
  'db.destroy_production': ['database:write'],
  'repo.checkout': ['repository:read'],
  'env.destroy_temporary': ['environment:write'],
  'repo.open_pull_request': ['repository:write'],
}

/** What the editor holds per step; the rest of `PlanStep` is derived on submit. */
export interface DraftStep {
  key: string
  title: string
  actionType: string
  adapter: string
  dependsOn: string[]
}

/**
 * A realistic rehearsal plan. The final step needs `container:run`, which the
 * seeded simulated identity does not hold, so a dry run with this template
 * walks through an approval and a `PERMISSION_DENIED` stop.
 */
export const PLAN_TEMPLATE: DraftStep[] = [
  { key: 'read', title: 'Read repository', actionType: 'repo.read', adapter: 'simulated', dependsOn: [] },
  { key: 'analyze', title: 'Analyze repository', actionType: 'repo.analyze', adapter: 'simulated', dependsOn: ['read'] },
  { key: 'deps', title: 'Install dependencies', actionType: 'deps.install', adapter: 'simulated', dependsOn: ['analyze'] },
  { key: 'build', title: 'Build container image', actionType: 'container.build', adapter: 'simulated', dependsOn: ['deps'] },
  { key: 'tests', title: 'Run tests locally', actionType: 'tests.run_local', adapter: 'simulated', dependsOn: ['deps'] },
  { key: 'run', title: 'Run container', actionType: 'container.run', adapter: 'simulated', dependsOn: ['build', 'tests'] },
]

export function toPlanStep(draft: DraftStep): PlanStep {
  return {
    key: draft.key.trim(),
    title: draft.title.trim(),
    description: draft.title.trim(),
    actionType: draft.actionType,
    adapter: draft.adapter,
    dependsOn: draft.dependsOn,
    params: {},
    requiredPermissions: DEFAULT_PERMISSIONS[draft.actionType] ?? [],
    verification: 'The adapter reports success and the step output is non-empty.',
  }
}

/** Returns a message per invalid step index, or an empty map when the plan is valid. */
export function validatePlan(steps: DraftStep[]): Map<number, string> {
  const errors = new Map<number, string>()
  const keys = steps.map((step) => step.key.trim())
  steps.forEach((step, index) => {
    const key = step.key.trim()
    if (!key) {
      errors.set(index, 'Key is required.')
      return
    }
    if (!/^[a-z0-9_-]+$/i.test(key)) {
      errors.set(index, 'Key may only use letters, digits, - and _.')
      return
    }
    if (keys.indexOf(key) !== index) {
      errors.set(index, `Key "${key}" is used twice.`)
      return
    }
    if (!step.title.trim()) {
      errors.set(index, 'Title is required.')
      return
    }
    if (step.dependsOn.includes(key)) {
      errors.set(index, 'A step cannot depend on itself.')
      return
    }
    const missing = step.dependsOn.find((dependency) => !keys.includes(dependency))
    if (missing) errors.set(index, `Depends on unknown step "${missing}".`)
  })
  return errors
}
