import { useNavigate } from 'react-router'

import { visibleNavigation } from '@/app/layout/app-shell/app-sidebar/navigation'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { useAuth } from '@/features/auth/lib/auth-context'

interface CommandPaletteProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** The lazy half of the ⌘K menu: the dialog, the input and the page list. */
export default function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  const groups = visibleNavigation(currentWorkspace.myRole)

  const go = (to: string) => {
    onOpenChange(false)
    navigate(to)
  }

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} title="Go to" description="Jump to a page">
      <CommandInput placeholder="Type a page name…" />
      <CommandList>
        <CommandEmpty>No pages found.</CommandEmpty>
        {groups.map((group) => (
          <CommandGroup key={group.label} heading={group.label}>
            {group.items.map((item) => (
              <CommandItem key={item.to} value={`${group.label} ${item.label}`} onSelect={() => go(item.to)}>
                <item.icon />
                {item.label}
              </CommandItem>
            ))}
          </CommandGroup>
        ))}
      </CommandList>
    </CommandDialog>
  )
}
