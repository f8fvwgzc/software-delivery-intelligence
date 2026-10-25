import { BoxesIcon } from 'lucide-react'

import { Frame } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'
import { count } from '@/lib/intl'

import type { Application } from '../../api/types'
import { groupByKind } from '../../lib/resources'
import { ResourceGroup } from './resource-group'

/** Above this many resources only the kinds that differ or are unwell start open. */
const OPEN_ALL_BELOW = 60

/**
 * What the application manages, live or desired, grouped by kind in the
 * order a sync applies them. Each resource has its sync and health, its
 * wave and hook, and its diff, read once for all of them when the first is
 * opened.
 */
export function ResourcesFrame({ application }: { application: Application }) {
  const groups = groupByKind(application.resources)
  const total = application.resources.length
  const outOfSync = groups.reduce((sum, group) => sum + group.outOfSync, 0)

  return (
    <Frame
      title="Resources"
      description="By kind, in the order a sync applies them."
      padding="none"
      meta={
        total > 0 ? (
          <>
            <Tag variant="outline" className="tabular-nums">
              {count(total, 'resource', 'resources')}
            </Tag>
            {outOfSync > 0 ? (
              <Tag tone="warning" className="tabular-nums">
                {outOfSync} out of sync
              </Tag>
            ) : null}
          </>
        ) : undefined
      }
    >
      {total === 0 ? (
        <PanelEmpty
          icon={BoxesIcon}
          title={application.reconciledAt ? 'Nothing to manage' : 'Not compared yet'}
          description={
            application.reconciledAt
              ? 'The source rendered no resources at this revision. Check the path and the type, then refresh.'
              : 'The first comparison renders the source and reads the cluster. Refresh to run it now.'
          }
        />
      ) : (
        groups.map((group) => (
          <ResourceGroup
            key={group.kind}
            applicationId={application.id}
            group={group}
            defaultOpen={total < OPEN_ALL_BELOW || group.outOfSync > 0 || group.unhealthy > 0}
          />
        ))
      )}
    </Frame>
  )
}
