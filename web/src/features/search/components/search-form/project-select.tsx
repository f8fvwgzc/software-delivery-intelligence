import { useQuery } from '@tanstack/react-query'

import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { projectPickerQuery } from '@/features/proposals/api/queries'

const ALL = 'all'

interface ProjectSelectProps {
  /** '' means every project. */
  value: string
  onChange: (projectId: string) => void
}

/** Scope the search to one project, or all of them. */
export function ProjectSelect({ value, onChange }: ProjectSelectProps) {
  const projects = useQuery(projectPickerQuery())
  const items = projects.data ?? []

  return (
    <Select value={value || ALL} onValueChange={(next) => onChange(next === ALL ? '' : next)}>
      <SelectTrigger aria-label="Project" className="sm:w-56">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem value={ALL}>All projects</SelectItem>
          {items.map((project) => (
            <SelectItem key={project.id} value={project.id}>
              {project.repoFullName}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
