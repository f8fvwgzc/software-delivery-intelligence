import { SidebarTrigger } from '@/components/ui/sidebar'

import { Breadcrumbs } from './breadcrumbs'
import { CommandMenu } from './command-menu'
import { ThemeToggle } from './theme-toggle'
import { UserMenu } from './user-menu'

/** 40px: breadcrumb at the left, search, theme and user at the right. ⌘B and the rail collapse the sidebar. */
export function AppHeader() {
  return (
    <header className="sticky top-0 z-20 flex h-(--header-h) shrink-0 items-center gap-2 border-b bg-background/85 px-3 backdrop-blur-sm md:px-5">
      <SidebarTrigger className="-ml-1 text-muted-foreground md:hidden" aria-label="Open navigation" />
      <Breadcrumbs />
      <div className="ml-auto flex items-center gap-0.5">
        <CommandMenu />
        <ThemeToggle />
        <UserMenu />
      </div>
    </header>
  )
}
