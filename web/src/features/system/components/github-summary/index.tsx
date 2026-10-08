import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { KeyValue, StatusBadge } from '@/components/shared/data'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

import { Count } from '../count'

type GithubAccount = components['schemas']['GithubAccount']

/**
 * Compact account summary for the system page. The full card with the
 * token setup instructions lives on the Connect page.
 */
export function GithubSummary({ account }: { account: GithubAccount }) {
  if (!account.tokenConfigured) {
    return (
      <div className="flex flex-col gap-3">
        <KeyValue items={[{ label: 'Token', value: <StatusBadge tone="warning">Not configured</StatusBadge> }]} />
        <p className="text-xs text-muted-foreground">
          Set <code className="rounded-sm bg-muted px-1 py-0.5 text-2xs">GITHUB_TOKEN</code> for the knowledge service to
          list and connect repositories.
        </p>
        <Button asChild variant="outline" size="sm" className="w-fit">
          <Link to={routes.connect()}>Open connect page</Link>
        </Button>
      </div>
    )
  }

  const initials = account.login.slice(0, 2).toUpperCase()

  return (
    <KeyValue
      items={[
        {
          label: 'Account',
          value: (
            <span className="inline-flex items-center gap-2">
              <Avatar size="sm">
                <AvatarImage src={account.avatarUrl} alt="" />
                <AvatarFallback>{initials}</AvatarFallback>
              </Avatar>
              <a
                href={account.htmlUrl}
                target="_blank"
                rel="noreferrer"
                className="font-mono text-xs underline-offset-4 hover:underline"
              >
                {account.login}
              </a>
            </span>
          ),
        },
        { label: 'Name', value: account.name || '—' },
        {
          label: 'Token',
          value: account.error ? (
            <StatusBadge tone="destructive">Rejected</StatusBadge>
          ) : (
            <StatusBadge tone="success">Valid</StatusBadge>
          ),
        },
        {
          label: 'Scopes',
          value: (
            <span className="font-mono text-xs" title={account.scopes.join(', ')}>
              {account.scopes.length > 0 ? account.scopes.join(', ') : '—'}
            </span>
          ),
        },
        {
          label: 'Repositories',
          value: (
            <span className="tabular-nums">
              <Count value={account.privateRepos} /> private · <Count value={account.publicRepos} /> public
            </span>
          ),
        },
      ]}
    />
  )
}
