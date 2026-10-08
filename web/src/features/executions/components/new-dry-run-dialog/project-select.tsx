import { useQuery } from '@tanstack/react-query'

import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

import { executionQueries } from '../../api/queries'

interface ProjectSelectProps {
  value: string
  onChange: (projectId: string) => void
  /** Locks the control when the dialog is opened from inside a project. */
  disabled?: boolean
  invalid?: boolean
  id?: string
}

export function ProjectSelect({ value, onChange, disabled, invalid, id }: ProjectSelectProps) {
  const { data: projects, isPending, isError } = useQuery(executionQueries.projectOptions())

  const placeholder = isPending ? 'Loading projects' : isError ? 'Projects unavailable' : 'Choose a project'

  return (
    <Select value={value} onValueChange={onChange} disabled={disabled || isPending || isError}>
      <SelectTrigger id={id} className="w-full" aria-invalid={invalid || undefined}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {projects?.map((project) => (
            <SelectItem key={project.id} value={project.id}>
              <span className="truncate">{project.name}</span>
              <span className="ml-1 font-mono text-2xs text-muted-foreground">{project.repoFullName}</span>
            </SelectItem>
          ))}
          {projects?.length === 0 && (
            <div className="px-2 py-1.5 text-xs text-muted-foreground">No projects connected yet.</div>
          )}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
