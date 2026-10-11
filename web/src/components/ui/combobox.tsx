import * as React from "react"
import { cn } from "cn"
import { ChevronDownIcon } from "lucide-react"

import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"

/*
 * Searchable single select, composed from Popover + Command the way the
 * shadcn combobox recipe does it. `modal` defaults to true so the list keeps
 * focus and pointer events when it opens from inside a Dialog.
 */

function Combobox({
  modal = true,
  ...props
}: React.ComponentProps<typeof Popover>) {
  return <Popover data-slot="combobox" modal={modal} {...props} />
}

function ComboboxTrigger({
  className,
  size = "default",
  children,
  ...props
}: React.ComponentProps<"button"> & {
  size?: "sm" | "default"
}) {
  return (
    <PopoverTrigger asChild>
      <button
        type="button"
        role="combobox"
        aria-haspopup="listbox"
        data-slot="combobox-trigger"
        data-size={size}
        className={cn(
          "flex w-full min-w-0 items-center justify-between gap-1.5 rounded-md border border-input bg-transparent py-1 pr-2 pl-2.5 text-left text-sm whitespace-nowrap transition-colors outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 data-placeholder:text-muted-foreground data-[size=default]:h-8 data-[size=sm]:h-7 data-[size=sm]:text-xs dark:bg-input/30 dark:hover:bg-input/50 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
          className
        )}
        {...props}
      >
        {children}
        <ChevronDownIcon
          aria-hidden="true"
          className="pointer-events-none size-4 text-muted-foreground"
        />
      </button>
    </PopoverTrigger>
  )
}

function ComboboxValue({
  className,
  ...props
}: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="combobox-value"
      className={cn("flex min-w-0 flex-1 items-center gap-1.5 truncate", className)}
      {...props}
    />
  )
}

function ComboboxContent({
  className,
  align = "start",
  children,
  shouldFilter,
  filter,
  ...props
}: React.ComponentProps<typeof PopoverContent> &
  Pick<React.ComponentProps<typeof Command>, "shouldFilter" | "filter">) {
  return (
    <PopoverContent
      data-slot="combobox-content"
      align={align}
      className={cn(
        "w-auto min-w-[max(var(--radix-popover-trigger-width),14rem)] max-w-[min(24rem,calc(100vw-2rem))] gap-0 p-0",
        className
      )}
      {...props}
    >
      <Command shouldFilter={shouldFilter} filter={filter} className="rounded-lg!">
        {children}
      </Command>
    </PopoverContent>
  )
}

function ComboboxInput(props: React.ComponentProps<typeof CommandInput>) {
  return <CommandInput data-slot="combobox-input" {...props} />
}

function ComboboxList({
  className,
  ...props
}: React.ComponentProps<typeof CommandList>) {
  return (
    <CommandList
      data-slot="combobox-list"
      className={cn("max-h-64", className)}
      {...props}
    />
  )
}

function ComboboxEmpty({
  className,
  ...props
}: React.ComponentProps<typeof CommandEmpty>) {
  return (
    <CommandEmpty
      data-slot="combobox-empty"
      className={cn("py-4 text-xs text-muted-foreground", className)}
      {...props}
    />
  )
}

function ComboboxGroup(props: React.ComponentProps<typeof CommandGroup>) {
  return <CommandGroup data-slot="combobox-group" {...props} />
}

function ComboboxItem({
  className,
  selected = false,
  ...props
}: React.ComponentProps<typeof CommandItem> & {
  /** Shows the check mark; CommandItem reads it from `data-checked`. */
  selected?: boolean
}) {
  return (
    <CommandItem
      data-slot="combobox-item"
      data-checked={selected ? "true" : undefined}
      className={cn("rounded-md py-1 pl-2 pr-2 text-sm", className)}
      {...props}
    />
  )
}

export {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxTrigger,
  ComboboxValue,
}
