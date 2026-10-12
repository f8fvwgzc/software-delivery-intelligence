import { CheckIcon, ChevronsUpDownIcon, PlusIcon } from 'lucide-react'

import { StatusBadge } from '@/components/shared/data'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar'
import { Spinner } from '@/components/ui/spinner'
import { useInstallApp } from '@/features/auth/api/mutations'
import { useAuth } from '@/features/auth/lib/auth-context'
import { initials, KIND_LABEL, ROLE_LABEL, ROLE_TONE } from '@/features/auth/lib/roles'

/**
 * Top of the sidebar: the workspace every request is scoped to, and the
 * menu to change it. Personal first, then every organisation that installed
 * the app, then the way to add one.
 */
export function WorkspaceSwitcher() {
  const { workspaces, currentWorkspace, setWorkspace } = useAuth()
  const install = useInstallApp()

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              aria-label={`Workspace: ${currentWorkspace.name}. Switch workspace`}
              className="h-8 rounded-md px-1.5 data-open:bg-sidebar-accent"
            >
              <Avatar size="sm" className="size-5 rounded-[4px]">
                <AvatarImage src={currentWorkspace.avatarUrl} alt="" className="rounded-[4px]" />
                <AvatarFallback className="rounded-[4px] text-2xs">{initials(currentWorkspace.name)}</AvatarFallback>
              </Avatar>
              <span className="flex min-w-0 flex-1 items-baseline gap-1.5 leading-tight">
                <span className="truncate text-sm font-medium text-sidebar-foreground">{currentWorkspace.name}</span>
                <span className="hidden truncate text-2xs text-muted-foreground xl:inline">{KIND_LABEL[currentWorkspace.kind]}</span>
              </span>
              <ChevronsUpDownIcon className="ml-auto size-3.5 text-muted-foreground" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" sideOffset={4} className="w-(--radix-dropdown-menu-trigger-width) min-w-56">
            <DropdownMenuLabel>Workspaces</DropdownMenuLabel>
            <DropdownMenuGroup>
              {workspaces.map((workspace) => {
                const current = workspace.id === currentWorkspace.id
                return (
                  <DropdownMenuItem
                    key={workspace.id}
                    onSelect={() => setWorkspace(workspace.id)}
                    aria-current={current ? 'true' : undefined}
                    className="gap-2"
                  >
                    <Avatar size="sm" className="size-5 rounded-sm">
                      <AvatarImage src={workspace.avatarUrl} alt="" className="rounded-sm" />
                      <AvatarFallback className="rounded-sm text-2xs">{initials(workspace.name)}</AvatarFallback>
                    </Avatar>
                    <span className="flex min-w-0 flex-1 flex-col leading-tight">
                      <span className="truncate">{workspace.name}</span>
                      <span className="truncate text-2xs text-muted-foreground">{KIND_LABEL[workspace.kind]}</span>
                    </span>
                    <StatusBadge tone={ROLE_TONE[workspace.myRole]} dot={false} className="h-5 px-1.5 text-2xs">
                      {ROLE_LABEL[workspace.myRole]}
                    </StatusBadge>
                    {current ? <CheckIcon className="size-3.5 text-muted-foreground" aria-hidden /> : null}
                  </DropdownMenuItem>
                )
              })}
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem
                onSelect={(event) => {
                  event.preventDefault()
                  install.mutate()
                }}
                disabled={install.isPending}
                className="gap-2 text-muted-foreground"
              >
                {install.isPending ? <Spinner className="size-4" /> : <PlusIcon />}
                Install on an organisation
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
