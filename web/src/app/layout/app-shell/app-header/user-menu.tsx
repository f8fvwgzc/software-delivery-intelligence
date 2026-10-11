import { LogOutIcon } from 'lucide-react'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useLogout } from '@/features/auth/api/mutations'
import { useAuth } from '@/features/auth/lib/auth-context'
import { initials } from '@/features/auth/lib/roles'

/** The signed-in GitHub account and the way out. */
export function UserMenu() {
  const { user } = useAuth()
  const logout = useLogout()

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Signed in as ${user.login}. Account menu`}
          loading={logout.isPending}
          className="rounded-full"
        >
          <Avatar size="sm" className="size-6">
            <AvatarImage src={user.avatarUrl} alt="" />
            <AvatarFallback className="text-2xs">{initials(user.login)}</AvatarFallback>
          </Avatar>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={6} className="min-w-52">
        <DropdownMenuLabel className="flex flex-col gap-0.5 font-normal">
          <span className="truncate text-sm font-medium text-foreground">{user.name || user.login}</span>
          <span className="truncate font-mono text-xs text-muted-foreground">
            {user.login}
            {user.githubHost !== 'github.com' ? ` · ${user.githubHost}` : ''}
          </span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem onSelect={() => logout.mutate()} disabled={logout.isPending}>
            <LogOutIcon />
            Sign out
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
