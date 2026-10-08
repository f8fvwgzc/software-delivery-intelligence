import { ChevronDownIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

interface DependsOnSelectProps {
  /** Keys of the other steps in the plan. */
  options: string[]
  value: string[]
  onChange: (next: string[]) => void
}

/** Multi-select of step keys, rendered as a checkbox list in a popover. */
export function DependsOnSelect({ options, value, onChange }: DependsOnSelectProps) {
  const label = value.length === 0 ? 'None' : value.join(', ')

  const toggle = (key: string, checked: boolean) => {
    onChange(checked ? [...value, key] : value.filter((item) => item !== key))
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="w-full justify-between font-mono text-xs font-normal">
          <span className="truncate">{label}</span>
          <ChevronDownIcon data-icon="inline-end" className="text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-56 p-2">
        {options.length === 0 ? (
          <p className="px-1 py-1 text-xs text-muted-foreground">Add another step first.</p>
        ) : (
          <div className="flex flex-col gap-1">
            {options.map((key) => {
              const id = `depends-${key}`
              return (
                <Label key={key} htmlFor={id} className="flex h-7 cursor-pointer items-center gap-2 rounded-md px-1.5 font-mono text-xs font-normal hover:bg-muted">
                  <Checkbox id={id} checked={value.includes(key)} onCheckedChange={(checked) => toggle(key, checked === true)} />
                  {key}
                </Label>
              )
            })}
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}
