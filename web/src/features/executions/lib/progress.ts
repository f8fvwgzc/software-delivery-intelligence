import type { components } from '@/lib/api/schema'

type Step = components['schemas']['Step']

export interface StepProgress {
  total: number
  succeeded: number
  failed: number
  blocked: number
  running: number
  pending: number
}

/** Counts behind "3/7 steps" and the segmented bar. */
export function stepProgress(steps: Step[]): StepProgress {
  const progress: StepProgress = { total: steps.length, succeeded: 0, failed: 0, blocked: 0, running: 0, pending: 0 }
  for (const step of steps) {
    switch (step.status) {
      case 'succeeded':
        progress.succeeded += 1
        break
      case 'failed':
        progress.failed += 1
        break
      case 'blocked':
        progress.blocked += 1
        break
      case 'running':
      case 'verifying':
        progress.running += 1
        break
      default:
        progress.pending += 1
    }
  }
  return progress
}
