import type { StatusTone } from '@/components/shared/data/status-badge'

import type { ProjectMode, ProjectStatus } from '../api/queries'

interface StatusLabel {
  label: string
  tone: StatusTone
  /** The status dot pulses: something is happening right now. */
  live?: boolean
}

export const PROJECT_STATUS: Record<ProjectStatus, StatusLabel> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  pending: { label: 'Pending', tone: 'neutral' },
  scanning: { label: 'Scanning', tone: 'info', live: true },
  ready: { label: 'Ready', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
}

export const PROJECT_MODE: Record<ProjectMode, StatusLabel> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  observe: { label: 'Observe', tone: 'neutral' },
  execute: { label: 'Execute', tone: 'warning' },
}
