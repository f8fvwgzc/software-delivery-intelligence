import { ChevronRightIcon } from 'lucide-react'
import { useId, useState } from 'react'

import { Row } from '@/components/system/row'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import type { ResourceStatus } from '../../api/types'
import { apiVersion, resourceName } from '../../lib/resources'
import { HealthTag, SyncTag } from '../status-tags'
import { ResourceDiff } from './resource-diff'

interface ResourceRowProps {
  applicationId: string
  resource: ResourceStatus
}

/** One managed resource: where it is, how it compares, how healthy, its wave and hook, and its diff on demand. */
export function ResourceRow({ applicationId, resource }: ResourceRowProps) {
  const id = useId()
  const [open, setOpen] = useState(false)
  const name = resourceName(resource)

  return (
    <li>
      <Row
        size="md"
        mono
        title={<span title={name}>{name}</span>}
        meta={
          <>
            <span className="font-mono">{apiVersion(resource)}</span>
            <span className="tabular-nums">wave {resource.syncWave}</span>
            {resource.hook ? <Tag variant="outline">Hook</Tag> : null}
            {resource.healthMessage ? (
              <span className="min-w-0 truncate" title={resource.healthMessage}>
                {resource.healthMessage}
              </span>
            ) : null}
          </>
        }
        trailing={
          <>
            <SyncTag status={resource.syncStatus} />
            {/* Missing from the cluster is one fact: the sync tag says it, the health tag would only repeat it. */}
            {resource.syncStatus === 'missing' && resource.healthStatus === 'missing' ? null : (
              <HealthTag status={resource.healthStatus} message={resource.healthMessage} />
            )}
            <Button
              variant="ghost"
              size="xs"
              onClick={() => setOpen((current) => !current)}
              aria-expanded={open}
              aria-controls={open ? `${id}-diff` : undefined}
              aria-label={`${open ? 'Hide' : 'Show'} the diff of ${resource.kind} ${name}`}
            >
              <ChevronRightIcon data-icon="inline-start" className={cn('transition-transform duration-(--motion-fast)', open && 'rotate-90')} />
              Diff
            </Button>
          </>
        }
      />
      {open ? <ResourceDiff applicationId={applicationId} resource={resource} id={`${id}-diff`} /> : null}
    </li>
  )
}
