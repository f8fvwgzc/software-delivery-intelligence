import { Breadcrumbs } from './breadcrumbs'
import { CommandMenu } from './command-menu'
import { ThemeToggle } from './theme-toggle'

/** The sidebar has no toggle button; ⌘B and the rail on its edge collapse it. */
export function AppHeader() {
  return (
    <header className="sticky top-0 z-20 flex h-12 shrink-0 items-center gap-2 border-b bg-background/80 px-3 backdrop-blur-sm md:px-4">
      <Breadcrumbs />
      <div className="ml-auto flex items-center gap-1">
        <CommandMenu />
        <ThemeToggle />
      </div>
    </header>
  )
}
