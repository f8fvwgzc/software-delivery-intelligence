import type { StatusTone } from '@/components/shared/data'

/** GitHub's view of a member, as the sync records it. */
export const GITHUB_ROLE_LABEL: Record<string, string> = {
  owner: 'Org owner',
  member: 'Member',
  outside_collaborator: 'Outside collaborator',
}

export function githubRoleLabel(role: string) {
  return GITHUB_ROLE_LABEL[role] ?? role
}

/** Audit actions are dotted keys; the first segment says which part of the product acted. */
const ACTION_TONE: Record<string, StatusTone> = {
  data: 'warning',
  identity: 'brand',
  policy: 'brand',
  approval: 'success',
  execution: 'info',
  proposal: 'info',
  decision: 'info',
  project: 'neutral',
  workspace: 'warning',
  member: 'brand',
}

export function actionTone(action: string): StatusTone {
  const prefix = action.split('.', 1)[0] ?? ''
  return ACTION_TONE[prefix] ?? 'neutral'
}

/** Quick filters offered above the audit table; typing any other prefix works too. */
export const ACTION_PREFIXES = [
  { value: '', label: 'All' },
  { value: 'project.', label: 'Projects' },
  { value: 'proposal.', label: 'Proposals' },
  { value: 'decision.', label: 'Decisions' },
  { value: 'execution.', label: 'Executions' },
  { value: 'approval.', label: 'Approvals' },
  { value: 'policy.', label: 'Policies' },
  { value: 'identity.', label: 'Identities' },
  { value: 'data.', label: 'Data' },
] as const

export const CODE_CACHE_OPTIONS = [
  { value: 'none', label: 'Never stored', description: 'Files are read into memory for analysis and dropped.' },
  { value: 'session', label: 'Per commit, with a TTL', description: 'Cached for the code browser and diagrams; purged after the TTL or on disconnect.' },
  { value: 'persistent', label: 'Persistent', description: 'Kept until purged or the project is disconnected.' },
] as const

export type CodeCacheMode = (typeof CODE_CACHE_OPTIONS)[number]['value']

export const REASONER_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'claude', label: 'Claude' },
] as const

export type ReasonerMode = (typeof REASONER_OPTIONS)[number]['value']

/** `DataControls.codeToModel` (CONTRACT.md §19, §23): whether source lines may reach the model so it can write a fix. */
export const CODE_TO_MODEL_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'changed_lines', label: 'Lines a finding points at' },
] as const

export type CodeToModelMode = (typeof CODE_TO_MODEL_OPTIONS)[number]['value']

/** The stored mode; anything else (an identity service from before §23 answers nothing) reads as off, the default. */
export function codeToModelMode(value: string | null | undefined): CodeToModelMode {
  return value === 'changed_lines' ? 'changed_lines' : 'off'
}

export interface StorageRow {
  what: string
  where: string
  retention: string
  control: string
}

/** CONTRACT.md §19, verbatim in substance; the product shows the same table TRUST.md points at. */
export const STORAGE_ROWS: StorageRow[] = [
  {
    what: 'Facts, graph, understanding, maturity, dependencies, pipeline and deployment metadata',
    where: 'knowledge schema, per workspace',
    retention: 'Until the project is disconnected',
    control: 'No (this is the product)',
  },
  {
    what: 'Semantic chunks and embeddings',
    where: 'knowledge.chunks',
    retention: 'Until disconnect',
    control: 'Minimal chunks keeps excerpts of 400 characters at most',
  },
  {
    what: 'Repository tree',
    where: 'knowledge.repo_trees',
    retention: 'Latest commit only',
    control: 'No (needed for the code browser)',
  },
  {
    what: 'File contents (code browser, diagram inputs)',
    where: 'knowledge.repo_files',
    retention: 'Code cache: never / per commit with a TTL (default 24 h, purged on disconnect) / persistent',
    control: 'Yes',
  },
  {
    what: 'Generated artifacts, proposals, decisions, documents, diagrams',
    where: 'intelligence schema',
    retention: 'Until deleted',
    control: 'n/a',
  },
  {
    what: 'Execution logs and outputs',
    where: 'execution schema',
    retention: 'Until deleted; working directories removed after each run',
    control: 'n/a',
  },
  {
    what: 'LLM requests',
    where: 'None stored; sent only with the reasoner on and the workspace’s own key; structured context, never raw files',
    retention: '—',
    control: 'Yes (off by default)',
  },
  {
    what: 'Source lines sent to the model to draft a fix',
    where:
      'None stored; sent only with the reasoner on and “Source lines to the model” set: the lines one finding points at and at most 20 either side, 120 at most. Never a whole file, never a finding about a secret; a line that looks like a secret is replaced by <redacted>',
    retention: 'The drafted fix lives with the review; the request is not stored',
    control: 'Yes (off by default)',
  },
  {
    what: 'Audit entries',
    where: 'identity.audit',
    retention: 'Audit retention days (default 365)',
    control: 'No',
  },
  {
    what: 'GitHub tokens',
    where: 'Installation tokens minted per request, cached in memory up to 55 min; OAuth tokens encrypted at rest',
    retention: 'Session lifetime',
    control: '—',
  },
]
