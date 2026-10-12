import { useSuspenseQuery } from '@tanstack/react-query'
import { KeyRoundIcon, LockIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'
import { cdQueries } from '@/features/delivery-cd/api/queries'
import type { SecretInfo } from '@/features/delivery-cd/api/types'
import { count } from '@/lib/intl'

import { SecretRow } from './secret-row'

interface SecretsListProps {
  workspaceId: string
  onReplace: (secret: SecretInfo) => void
  onDelete: (name: string) => void
  /** The "Set secret" trigger, for the empty state. */
  emptyAction?: ReactNode
}

/** The workspace's secrets by name, for an admin (the only role the API answers). A value is never part of what it returns. */
export function SecretsList({ workspaceId, onReplace, onDelete, emptyAction }: SecretsListProps) {
  const { data: secrets } = useSuspenseQuery(cdQueries.secrets(workspaceId))

  if (secrets.length === 0) {
    return (
      <PanelEmpty
        icon={KeyRoundIcon}
        title="No secrets yet"
        description="A Jenkins token, an Azure DevOps access token, a webhook signing key or registry credentials: store it once, then refer to it by name in an environment or a registry."
        action={emptyAction}
      />
    )
  }

  return (
    <>
      <RowList aria-label="Workspace secrets">
        {secrets.map((secret) => (
          <li key={secret.name}>
            <SecretRow secret={secret} onReplace={onReplace} onDelete={onDelete} />
          </li>
        ))}
      </RowList>
      <FrameFooter>
        <LockIcon className="size-3 shrink-0" aria-hidden />
        <span>
          {count(secrets.length, 'secret', 'secrets')}. Values are encrypted at rest and never shown again; every use by a deployer is
          audited as <span className="font-mono">secret.resolve</span>.
        </span>
      </FrameFooter>
    </>
  )
}
