import { Tag } from '@/components/system/tag'

import type { SyncPolicy } from '../../api/types'
import { automationTags, healthStatus, syncStatus, type StatusMeta } from '../../lib/labels'

function StatusTag({ meta, className }: { meta: StatusMeta; className?: string }) {
  return (
    <Tag tone={meta.tone} dot live={meta.live} title={meta.hint} className={className}>
      {meta.label}
    </Tag>
  )
}

/** `synced`, `out_of_sync`, … as a tag with its dot. */
export function SyncTag({ status, className }: { status: string | null | undefined; className?: string }) {
  return <StatusTag meta={syncStatus(status)} className={className} />
}

/** `healthy`, `degraded`, … as a tag; `progressing` pulses. Renders nothing when the check said nothing. */
export function HealthTag({ status, message, className }: { status: string | null | undefined; message?: string | null; className?: string }) {
  if (!status) return null
  const meta = healthStatus(status)
  return <StatusTag meta={message ? { ...meta, hint: message } : meta} className={className} />
}

/**
 * Auto sync, Self heal and Prune as outline tags, or "Manual" in muted text:
 * whether anything reaches the cluster without an approval is read here.
 */
export function AutomationTags({ policy }: { policy: Pick<SyncPolicy, 'automated' | 'selfHeal' | 'prune'> }) {
  const tags = automationTags(policy)
  if (tags.length === 0) return <span className="text-xs text-muted-foreground">Manual</span>
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {tags.map((tag) => (
        <Tag key={tag} variant="outline">
          {tag}
        </Tag>
      ))}
    </span>
  )
}
