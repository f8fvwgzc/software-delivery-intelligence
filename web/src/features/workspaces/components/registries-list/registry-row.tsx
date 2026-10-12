import { ContainerIcon } from 'lucide-react'

import { Row } from '@/components/system/row'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Registry } from '@/features/delivery-cd/api/types'
import { registryPath } from '@/features/delivery-cd/lib/labels'
import { registryAuthLabel, registryKind } from '@/features/delivery-cd/lib/registries'

interface RegistryRowProps {
  registry: Registry
  canEdit: boolean
  onEdit: (registry: Registry) => void
  onDelete: (registry: Registry) => void
}

/** One registry: where it is, how it is signed in to, and whether releases go there by default. */
export function RegistryRow({ registry, canEdit, onEdit, onDelete }: RegistryRowProps) {
  const secrets = [registry.usernameSecret, registry.passwordSecret].filter(Boolean)
  const deleteButton = (
    <Button
      variant="ghost"
      size="xs"
      className="text-destructive hover:text-destructive"
      onClick={() => onDelete(registry)}
      disabled={registry.isDefault}
      aria-label={`Delete ${registry.name}`}
    >
      Delete
    </Button>
  )

  return (
    <Row
      leading={<ContainerIcon className="text-muted-foreground" aria-hidden />}
      title={
        <span className="inline-flex min-w-0 items-center gap-2 align-bottom">
          <span className="truncate font-medium">{registry.name}</span>
          {registry.isDefault ? <Tag tone="brand">Default</Tag> : null}
        </span>
      }
      meta={
        <>
          <span className="font-mono text-foreground/80">{registryPath(registry)}</span>
          <span>{registryKind(registry.kind).label}</span>
          <span>{registryAuthLabel(registry.auth)}</span>
          {registry.auth === 'secret' && secrets.length > 0 ? <span className="font-mono">{secrets.join(', ')}</span> : null}
        </>
      }
      trailing={
        canEdit ? (
          <>
            <Button variant="ghost" size="xs" onClick={() => onEdit(registry)} aria-label={`Edit ${registry.name}`}>
              Edit
            </Button>
            {registry.isDefault ? (
              <Tooltip>
                {/* A disabled button fires no pointer events; the wrapper carries the tooltip. */}
                <TooltipTrigger asChild>
                  <span tabIndex={0} className="inline-flex rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                    {deleteButton}
                  </span>
                </TooltipTrigger>
                <TooltipContent>Make another registry the default first.</TooltipContent>
              </Tooltip>
            ) : (
              deleteButton
            )}
          </>
        ) : undefined
      }
    />
  )
}
