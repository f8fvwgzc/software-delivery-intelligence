import { ChevronRightIcon } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar'
import { cn } from '@/lib/utils'

import { isNavItemActive, type NavSection as NavSectionData } from './navigation'

/** Ref of the current page's row: the sidebar scrolls on short viewports, and the active row must be in view. */
const revealRow = (row: HTMLAnchorElement | null) => row?.scrollIntoView({ block: 'nearest' })

interface NavSectionProps {
  section: NavSectionData
  pathname: string
  /** The row style of the sidebar, shared with the plain items. */
  rowClassName: string
}

/**
 * Several pages under one row. Closed by default, so the list stays one
 * calm column that fits the viewport; it opens on a click, and by itself
 * whenever one of its pages is the current one, so the active row is never
 * hidden behind a closed section.
 */
export function NavSection({ section, pathname, rowClassName }: NavSectionProps) {
  const containsCurrent = section.items.some((item) => isNavItemActive(item, pathname))
  const [open, setOpen] = useState(containsCurrent)
  // Arriving on one of its pages (a link, the palette, the address bar) opens it; leaving does not close it.
  const [wasCurrent, setWasCurrent] = useState(containsCurrent)
  if (containsCurrent !== wasCurrent) {
    setWasCurrent(containsCurrent)
    if (containsCurrent) setOpen(true)
  }

  return (
    <SidebarMenuItem>
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger asChild>
          <SidebarMenuButton className={rowClassName}>
            <section.icon />
            <span>{section.label}</span>
            <ChevronRightIcon
              aria-hidden
              className={cn('ml-auto size-3.5! transition-transform duration-(--motion-fast) motion-reduce:transition-none', open && 'rotate-90')}
            />
          </SidebarMenuButton>
        </CollapsibleTrigger>
        <CollapsibleContent>
          {/* The hairline at the left ties the pages to the row they belong to. */}
          <ul className="mt-0.5 ml-[1.0625rem] flex flex-col gap-0.5 border-l border-sidebar-border pl-2" aria-label={section.label}>
            {section.items.map((item) => {
              const active = isNavItemActive(item, pathname)
              return (
                <SidebarMenuItem key={item.to}>
                  <SidebarMenuButton asChild isActive={active} tooltip={item.label} className={rowClassName}>
                    <Link to={item.to} ref={active ? revealRow : undefined}>
                      <item.icon />
                      <span>{item.label}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              )
            })}
          </ul>
        </CollapsibleContent>
      </Collapsible>
    </SidebarMenuItem>
  )
}
