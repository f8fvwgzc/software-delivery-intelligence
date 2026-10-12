import { SearchIcon } from 'lucide-react'
import { lazy, Suspense, useEffect, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'

/**
 * The palette (cmdk and its list) is its own chunk, loaded on intent: hover
 * or focus on the trigger, or the first ⌘K. The header renders without it.
 */
const loadPalette = () => import('./command-palette')
const CommandPalette = lazy(loadPalette)

/** ⌘K navigation. Pages only; project-level search lives on the Search page. */
export function CommandMenu() {
  const [open, setOpen] = useState(false)
  const [wanted, setWanted] = useState(false)

  const warm = () => {
    setWanted(true)
    void loadPalette()
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'k' && (event.metaKey || event.ctrlKey)) {
        event.preventDefault()
        setWanted(true)
        setOpen((value) => !value)
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  return (
    <>
      <Button
        variant="outline"
        size="sm"
        className="hidden h-7 w-48 justify-start gap-2 border-border bg-muted/40 font-normal text-muted-foreground shadow-none hover:bg-muted md:inline-flex"
        onMouseEnter={warm}
        onFocus={warm}
        onClick={() => setOpen(true)}
      >
        <SearchIcon data-icon="inline-start" />
        <span className="flex-1 text-left text-xs">Search pages…</span>
        <Kbd className="text-2xs">⌘K</Kbd>
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground md:hidden"
        aria-label="Go to page"
        onMouseEnter={warm}
        onFocus={warm}
        onClick={() => setOpen(true)}
      >
        <SearchIcon />
      </Button>
      {wanted || open ? (
        <Suspense fallback={null}>
          <CommandPalette open={open} onOpenChange={setOpen} />
        </Suspense>
      ) : null}
    </>
  )
}
