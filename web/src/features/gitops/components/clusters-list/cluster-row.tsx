import { ServerIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { Row } from '@/components/system/row'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { count } from '@/lib/intl'

import type { Cluster } from '../../api/types'
import { clusterStatus } from '../../lib/labels'

interface ClusterRowProps {
  cluster: Cluster
  canEdit: boolean
  onEdit: (cluster: Cluster) => void
  onDelete: (cluster: Cluster) => void
}

/** One cluster: whether it answered, its API server and version, what deploys to it, and the secret it reads (by name). */
export function ClusterRow({ cluster, canEdit, onEdit, onDelete }: ClusterRowProps) {
  const status = clusterStatus(cluster.status)

  return (
    <Row
      leading={<ServerIcon className="text-muted-foreground" aria-hidden />}
      title={
        <span className="inline-flex min-w-0 items-center gap-2 align-bottom">
          <span className="truncate font-medium">{cluster.name}</span>
          <Tag tone={status.tone} dot title={status.hint}>
            {status.label}
          </Tag>
        </span>
      }
      meta={
        <>
          {cluster.server ? <span className="font-mono text-foreground/80">{cluster.server}</span> : <span>Server not read yet</span>}
          {cluster.serverVersion ? <span className="font-mono">{cluster.serverVersion}</span> : null}
          <span className="tabular-nums">{count(cluster.applicationCount, 'application', 'applications')}</span>
          <span>
            checked <RelativeTime value={cluster.checkedAt} fallback="never" className="tabular-nums" />
          </span>
          <span>
            secret <span className="font-mono">{cluster.kubeconfigSecret}</span>
          </span>
          {cluster.context ? (
            <span>
              context <span className="font-mono">{cluster.context}</span>
            </span>
          ) : null}
          <span>
            {cluster.namespaces.length > 0 ? (
              <>
                only <span className="font-mono">{cluster.namespaces.join(', ')}</span>
              </>
            ) : (
              'every namespace'
            )}
          </span>
          {cluster.message ? <span className="basis-full text-destructive">{cluster.message}</span> : null}
        </>
      }
      trailing={
        canEdit ? (
          <>
            <Button variant="ghost" size="xs" onClick={() => onEdit(cluster)} aria-label={`Edit ${cluster.name}`}>
              Edit
            </Button>
            <Button
              variant="ghost"
              size="xs"
              className="text-destructive hover:text-destructive"
              onClick={() => onDelete(cluster)}
              aria-label={`Delete ${cluster.name}`}
            >
              Delete
            </Button>
          </>
        ) : undefined
      }
    />
  )
}
