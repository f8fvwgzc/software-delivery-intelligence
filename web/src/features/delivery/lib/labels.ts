import type { StatusTone } from '@/components/shared/data'

import type { DeploymentStatus, RunStatus } from './types'

export interface StatusMeta {
  label: string
  tone: StatusTone
  /** In-flight states pulse their dot. */
  live?: boolean
}

export const RUN_STATUS: Record<RunStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  queued: { label: 'Queued', tone: 'neutral' },
  in_progress: { label: 'In progress', tone: 'info', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  cancelled: { label: 'Cancelled', tone: 'neutral' },
  skipped: { label: 'Skipped', tone: 'neutral' },
}

export const DEPLOYMENT_STATUS: Record<DeploymentStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  pending: { label: 'Pending', tone: 'neutral' },
  in_progress: { label: 'In progress', tone: 'info', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  inactive: { label: 'Inactive', tone: 'neutral' },
}

/** 6px dot fill per tone; mirrors the StatusBadge dot so both read the same. */
export const TONE_DOT_CLASS: Record<StatusTone, string> = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

const PROVIDER_LABELS: Record<string, string> = {
  github_actions: 'GitHub Actions',
  gitlab_ci: 'GitLab CI',
  circleci: 'CircleCI',
  azure_pipelines: 'Azure Pipelines',
}

const SOURCE_LABELS: Record<string, string> = {
  github_deployment: 'GitHub deployment',
  workflow_run: 'Workflow run',
}

/** `snake_case_value` → `Snake case value`, for enum-like strings the contract leaves open. */
export function humanize(value: string) {
  const spaced = value.replaceAll('_', ' ').trim()
  return spaced ? spaced.charAt(0).toUpperCase() + spaced.slice(1) : value
}

export const providerLabel = (provider: string) => PROVIDER_LABELS[provider] ?? humanize(provider)
export const sourceLabel = (source: string) => SOURCE_LABELS[source] ?? humanize(source)
