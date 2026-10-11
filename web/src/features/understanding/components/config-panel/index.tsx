import { AlertTriangleIcon, ExternalLinkIcon } from 'lucide-react'

import { KeyValue } from '@/components/shared/data/key-value'
import type { Project, ProjectConfig } from '@/features/projects/api/queries'

import { configItems } from '../../lib/config'
import { sourceFileUrl } from '../../lib/github'
import { ConfigExample } from './config-example'

interface ConfigPanelProps {
  config: ProjectConfig
  project: Pick<Project, 'repoUrl' | 'defaultBranch'>
}

/**
 * What `.sdi.yml` declares. Present: the configured facts, with any parse
 * problems in warning text. Absent: a copyable minimal example.
 */
export function ConfigPanel({ config, project }: ConfigPanelProps) {
  if (!config.present) {
    return <ConfigExample />
  }

  const href = config.path ? sourceFileUrl(project, config.path) : null
  const items = configItems(config).map((item) => ({
    label: item.label,
    value: item.value ?? <span className="text-muted-foreground">Not set</span>,
  }))

  return (
    <div className="flex flex-col gap-3">
      {config.errors.length > 0 ? (
        <div role="status" className="flex flex-col gap-1 rounded-lg bg-warning/5 p-3 text-xs text-warning ring-1 ring-warning/20">
          <span className="inline-flex items-center gap-1.5 font-medium">
            <AlertTriangleIcon className="size-3.5" aria-hidden />
            The file has problems and is ignored until they are fixed
          </span>
          <ul className="flex list-disc flex-col gap-0.5 pl-5">
            {config.errors.map((error) => (
              <li key={error}>{error}</li>
            ))}
          </ul>
        </div>
      ) : null}

      <KeyValue items={items} />

      <p className="text-xs text-muted-foreground">
        Configured values answer the matching open questions automatically.
        {href ? (
          <>
            {' '}
            <a
              href={href}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 rounded-sm font-mono outline-none hover:text-foreground hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              {config.path}
              <ExternalLinkIcon className="size-2.5" aria-hidden />
            </a>
          </>
        ) : null}
      </p>
    </div>
  )
}

export { ConfigPanelSkeleton } from './config-skeleton'
