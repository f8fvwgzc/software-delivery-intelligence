import { useSuspenseQuery } from '@tanstack/react-query'
import { ContainerIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'
import { cdQueries } from '@/features/delivery-cd/api/queries'
import type { Registry } from '@/features/delivery-cd/api/types'

import { RegistryRow } from './registry-row'

interface RegistriesListProps {
  canEdit: boolean
  onEdit: (registry: Registry) => void
  onDelete: (registry: Registry) => void
  /** The "Add registry" trigger, for the empty state. */
  emptyAction?: ReactNode
}

/** The workspace's registries, the default first. */
export function RegistriesList({ canEdit, onEdit, onDelete, emptyAction }: RegistriesListProps) {
  const { data } = useSuspenseQuery(cdQueries.registries())
  const registries = data.toSorted((a, b) => Number(b.isDefault) - Number(a.isDefault) || a.name.localeCompare(b.name))

  if (registries.length === 0) {
    return (
      <PanelEmpty
        icon={ContainerIcon}
        title="No registries"
        description="A registry is where built images are pushed and where a deployment verifies them. GitHub Container Registry needs no credentials; add it, or the one your images already live in."
        action={emptyAction}
      />
    )
  }

  return (
    <RowList aria-label="Registries">
      {registries.map((registry) => (
        <li key={registry.id}>
          <RegistryRow registry={registry} canEdit={canEdit} onEdit={onEdit} onDelete={onDelete} />
        </li>
      ))}
    </RowList>
  )
}
