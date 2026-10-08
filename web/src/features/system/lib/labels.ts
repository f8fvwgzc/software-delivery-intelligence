import type { StatusTone } from '@/components/shared/data'

import type { ServiceStatus, SystemStatus } from '../api/types'

/** Health check result → display label and StatusBadge tone. */
export const SERVICE_STATUS: Record<ServiceStatus, { label: string; tone: StatusTone }> = {
  serving: { label: 'Serving', tone: 'success' },
  not_serving: { label: 'Not serving', tone: 'warning' },
  unreachable: { label: 'Unreachable', tone: 'destructive' },
}

/** Display names for the three backing services (CONTRACT.md §2). */
export const SERVICE_NAME: Record<string, string> = {
  knowledge: 'Knowledge',
  intelligence: 'Intelligence',
  execution: 'Execution',
}

export function serviceName(name: string) {
  return SERVICE_NAME[name] ?? name
}

/** 6px status dot fill per tone. */
export const DOT_CLASS: Record<StatusTone, string> = {
  neutral: 'bg-muted-foreground/50',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

export interface OverallHealth {
  tone: StatusTone
  label: string
  down: number
  total: number
}

/** Collapses the per-service checks into one dot and one line. */
export function overallHealth(status: SystemStatus): OverallHealth {
  const total = status.services.length
  const down = status.services.filter((service) => service.status !== 'serving').length

  if (total === 0) return { tone: 'neutral', label: 'No services reported', down, total }
  if (down === 0) return { tone: 'success', label: 'All services up', down, total }
  return {
    tone: down === total ? 'destructive' : 'warning',
    label: `${down} ${down === 1 ? 'service' : 'services'} down`,
    down,
    total,
  }
}

/** Gateway `status` field → tone. */
export function gatewayTone(status: string): StatusTone {
  return status === 'ok' ? 'success' : 'warning'
}
