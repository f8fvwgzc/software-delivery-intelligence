import { useQuery } from '@tanstack/react-query'

import { ComboboxField } from '@/components/shared/form'
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

  const options = [
    { value: ALL, label: 'All projects' },
    ...items.map((project) => ({ value: project.id, label: project.repoFullName })),
  ]

  return (
    <ComboboxField
      aria-label="Project"
      className="sm:w-56"
      value={value || ALL}
      onValueChange={(next) => onChange(next === ALL ? '' : next)}
      options={options}
      searchPlaceholder="Search projects"
      emptyText="No project matches."
    />
  )
}
