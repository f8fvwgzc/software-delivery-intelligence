import { AlertCircleIcon, ExternalLinkIcon } from 'lucide-react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'

import type { GithubAccount } from '../../api/types'
import { initials } from '../../lib/labels'
import { ScopeList } from './scope-list'
import { TokenMissing } from './token-missing'

export { AccountCardSkeleton } from './skeleton'

/** Who the token belongs to, what it may do, or how to configure one. */
export function AccountCard({ account }: { account: GithubAccount }) {
  if (!account.tokenConfigured) {
    return (
      <Card size="sm">
        <CardContent>
          <TokenMissing />
        </CardContent>
      </Card>
    )
  }

  return (
    <Card size="sm">
      <CardHeader className="flex items-center gap-3">
        <Avatar size="lg">
          <AvatarImage src={account.avatarUrl} alt="" />
          <AvatarFallback>{initials(account.login)}</AvatarFallback>
        </Avatar>
        <div className="flex min-w-0 flex-col">
          <CardTitle className="truncate font-mono text-sm">{account.login}</CardTitle>
          <CardDescription className="truncate text-xs">{account.name || 'GitHub account'}</CardDescription>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3">
        {account.error ? (
          <Alert variant="destructive">
            <AlertCircleIcon />
            <AlertTitle>GitHub rejected the token</AlertTitle>
            <AlertDescription>{account.error}</AlertDescription>
          </Alert>
        ) : null}

        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Scopes</span>
          <ScopeList scopes={account.scopes} />
        </div>

        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Repositories visible</span>
          <span className="tabular-nums">
            {account.privateRepos.toLocaleString()} private · {account.publicRepos.toLocaleString()} public
          </span>
        </div>
      </CardContent>

      <CardFooter>
        <Button asChild variant="ghost" size="sm" className="-ml-2">
          <a href={account.htmlUrl} target="_blank" rel="noreferrer">
            Open profile on GitHub
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
      </CardFooter>
    </Card>
  )
}
