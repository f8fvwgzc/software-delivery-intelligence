import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Tag, type TagTone } from '@/components/system/tag'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { cdQueries } from '../../api/queries'

type Presence = 'set' | 'missing' | 'checking' | 'unknown'

const PRESENCE: Record<Presence, { label: string; tone: TagTone }> = {
  set: { label: 'Set', tone: 'success' },
  missing: { label: 'Missing', tone: 'warning' },
  checking: { label: 'Checking', tone: 'neutral' },
  unknown: { label: 'Not visible', tone: 'neutral' },
}

/**
 * The secret names the generated files reference, each checked against the
 * workspace's secrets. Only an admin may read those (CONTRACT.md §22), so
 * only an admin's browser asks, and only here, once the files are shown;
 * anyone else sees the names without a verdict.
 */
export function RequiredSecrets({ names }: { names: readonly string[] }) {
  const { currentWorkspace } = useAuth()
  const isAdmin = hasRole(currentWorkspace.myRole, 'admin')
  const secrets = useQuery({ ...cdQueries.secrets(currentWorkspace.id), enabled: isAdmin && names.length > 0 })
  const existing = new Set(secrets.data?.map((secret) => secret.name))
  const presenceOf = (name: string): Presence =>
    !isAdmin || secrets.isError ? 'unknown' : secrets.isPending ? 'checking' : existing.has(name) ? 'set' : 'missing'

  if (names.length === 0) {
    return <p className="text-xs text-muted-foreground">These files reference no secrets.</p>
  }

  return (
    <div className="flex flex-col gap-2">
      <ul className="flex flex-col gap-1.5">
        {names.map((name) => {
          const presence = PRESENCE[presenceOf(name)]
          return (
            <li key={name} className="flex min-w-0 items-center justify-between gap-2">
              <code className="truncate font-mono text-xs" title={name}>
                {name}
              </code>
              <Tag tone={presence.tone} dot={presence.tone !== 'neutral'}>
                {presence.label}
              </Tag>
            </li>
          )
        })}
      </ul>
      <p className="text-xs text-muted-foreground">
        {isAdmin ? (
          <>
            {secrets.isError ? 'The workspace secrets could not be read. ' : null}
            <Link to={routes.secrets()} className="text-foreground underline underline-offset-4 decoration-border hover:decoration-foreground">
              Manage workspace secrets
            </Link>
          </>
        ) : (
          'Only admins can see which are set; an admin stores them under Secrets.'
        )}
      </p>
    </div>
  )
}
