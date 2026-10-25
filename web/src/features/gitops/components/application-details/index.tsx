import { KeyValue } from '@/components/shared/data/key-value'
import { RelativeTime } from '@/components/shared/data/relative-time'
import type { Project } from '@/features/projects/api/queries'

import type { Application } from '../../api/types'
import { sourceTypeLabel } from '../../lib/labels'
import { sourceRepoName } from '../../lib/repo'
import { RevisionLink } from '../revision-link'
import { AutomationTags } from '../status-tags'

const MONO = 'font-mono'

interface ApplicationDetailsProps {
  application: Application
  project?: Pick<Project, 'repoFullName'>
  repoUrl?: string
}

/** Where it deploys, what it renders, which commits, and when it syncs on its own: the settings as the service holds them. */
export function ApplicationDetails({ application, project, repoUrl }: ApplicationDetailsProps) {
  const { source, syncPolicy } = application
  const parameters = Object.entries(source.helmParameters)

  const items = [
    { label: 'Cluster', value: application.clusterName },
    { label: 'Namespace', value: application.namespace ? <span className={MONO}>{application.namespace}</span> : 'cluster default' },
    { label: 'Repository', value: <span className={MONO}>{sourceRepoName(source, project)}</span> },
    { label: 'Revision', value: source.revision ? <span className={MONO}>{source.revision}</span> : 'default branch' },
    { label: 'Path', value: <span className={MONO}>{source.path || '/'}</span> },
    { label: 'Type', value: sourceTypeLabel(source.type) },
    { label: 'Rendered', value: <RevisionLink revision={application.revision} repoUrl={repoUrl} fallback="not yet" /> },
    { label: 'Last synced', value: <RevisionLink revision={application.syncedRevision} repoUrl={repoUrl} fallback="never" /> },
    { label: 'Compared', value: <RelativeTime value={application.reconciledAt} fallback="not yet" /> },
    { label: 'Sync policy', value: <AutomationTags policy={syncPolicy} /> },
    { label: 'Create namespace', value: syncPolicy.createNamespace ? 'Yes' : 'No' },
  ]
  if (syncPolicy.automated) items.push({ label: 'Retries', value: <span className="tabular-nums">{syncPolicy.retryLimit || 3}</span> })
  if (source.type === 'directory' || source.recurse) items.push({ label: 'Subdirectories', value: source.recurse ? 'Included' : 'Not read' })
  if (source.helmReleaseName) items.push({ label: 'Release name', value: <span className={MONO}>{source.helmReleaseName}</span> })

  return (
    <div className="flex flex-col gap-3">
      <KeyValue items={items} />
      {source.kustomizeImages.length > 0 ? <ListBlock label="Image overrides" values={source.kustomizeImages} /> : null}
      {source.helmValueFiles.length > 0 ? <ListBlock label="Values files" values={source.helmValueFiles} /> : null}
      {parameters.length > 0 ? <ListBlock label="Parameters" values={parameters.map(([key, value]) => `${key}=${value}`)} /> : null}
    </div>
  )
}

function ListBlock({ label, values }: { label: string; values: readonly string[] }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <ul className="flex flex-col gap-0.5">
        {values.map((value) => (
          <li key={value} className="truncate font-mono text-xs" title={value}>
            {value}
          </li>
        ))}
      </ul>
    </div>
  )
}
