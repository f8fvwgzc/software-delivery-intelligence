import { ExternalLinkIcon } from 'lucide-react'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import type { Workspace } from '@/features/auth/api/types'

import { initials } from '../../lib/labels'

/** GitHub's page for an installation, where the repository selection is changed. */
export function installationSettingsUrl(workspace: Pick<Workspace, 'githubHost' | 'slug' | 'installationId'>) {
  const host = workspace.githubHost || 'github.com'
  return `https://${host}/organizations/${workspace.slug}/settings/installations/${workspace.installationId ?? ''}`
}

const SELECTION_LABEL: Record<string, string> = {
  all: 'All repositories of the organisation',
  selected: 'Only the repositories picked on GitHub',
}

/** An organisation workspace: the installation, what it grants, and where to change that. */
export function InstallationCard({ workspace }: { workspace: Workspace }) {
  const selection = workspace.repositorySelection ?? ''

  return (
    <Card size="sm">
      <CardHeader className="flex items-center gap-3">
        <Avatar size="lg" className="rounded-md">
          <AvatarImage src={workspace.avatarUrl} alt="" className="rounded-md" />
          <AvatarFallback className="rounded-md">{initials(workspace.slug)}</AvatarFallback>
        </Avatar>
        <div className="flex min-w-0 flex-col">
          <CardTitle className="truncate text-sm">{workspace.name}</CardTitle>
          <CardDescription className="truncate font-mono text-xs">{workspace.slug}</CardDescription>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">GitHub App installation</span>
          <span className="font-mono text-xs tabular-nums">
            {workspace.installationId ?? '—'}
            {workspace.githubHost && workspace.githubHost !== 'github.com' ? ` · ${workspace.githubHost}` : ''}
          </span>
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Repository selection</span>
          <span>{SELECTION_LABEL[selection] ?? 'Unknown'}</span>
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">In this workspace</span>
          <span className="tabular-nums">
            {workspace.projectCount.toLocaleString()} {workspace.projectCount === 1 ? 'project' : 'projects'} ·{' '}
            {workspace.memberCount.toLocaleString()} {workspace.memberCount === 1 ? 'member' : 'members'}
          </span>
        </div>
      </CardContent>

      <CardFooter>
        <Button asChild variant="ghost" size="sm" className="-ml-2">
          <a href={installationSettingsUrl(workspace)} target="_blank" rel="noreferrer">
            Change on GitHub
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
      </CardFooter>
    </Card>
  )
}
