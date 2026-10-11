import type { ReactNode } from 'react'

import type { ProjectConfig } from '@/features/projects/api/queries'

/** The minimal `.sdi.yml` from CONTRACT.md §12, offered when a repository has none. */
export const CONFIG_EXAMPLE = `version: 1
deploy:
  target: kubernetes          # vm | azure | aws | gcp | kubernetes | fly | render | none
  environments: [staging, production]
  region: westeurope
budget:
  monthly_usd: 20
test:
  command: "cargo test"
  health_path: "/health"
  port: 8080
scan:
  exclude: ["docs/legacy/**", "vendor/**"]
`

export interface ConfigItem {
  label: string
  /** Null when the file does not set the key. */
  value: ReactNode | null
}

function formatUsd(value: number) {
  return `$${value.toLocaleString(undefined, { maximumFractionDigits: 2 })} / month`
}

/** The configured facts in display order; a null value means "not set". */
export function configItems(config: ProjectConfig): ConfigItem[] {
  return [
    { label: 'Deploy target', value: config.deployTarget ?? null },
    { label: 'Environments', value: config.environments.length > 0 ? config.environments.join(', ') : null },
    { label: 'Region', value: config.region ?? null },
    { label: 'Budget', value: config.budgetMonthlyUsd != null ? formatUsd(config.budgetMonthlyUsd) : null },
    { label: 'Test command', value: config.testCommand ?? null },
    { label: 'Health path', value: config.healthPath ?? null },
    { label: 'Port', value: config.port != null ? String(config.port) : null },
    { label: 'Excludes', value: config.scanExclude.length > 0 ? config.scanExclude.join(', ') : null },
  ]
}
