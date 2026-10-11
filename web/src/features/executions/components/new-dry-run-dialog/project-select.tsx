import { useQuery } from '@tanstack/react-query'

import { ComboboxField } from '@/components/shared/form'

import { executionQueries } from '../../api/queries'

interface ProjectSelectProps {
  value: string
  onChange: (projectId: string) => void
  /** Locks the control when the dialog is opened from inside a project. */
  disabled?: boolean
  invalid?: boolean
  id?: string
  /** Id of the hint or error that describes the control. */
  describedBy?: string
}

export function ProjectSelect({ value, onChange, disabled, invalid, id, describedBy }: ProjectSelectProps) {
  const { data: projects, isPending, isError } = useQuery(executionQueries.projectOptions())

  const placeholder = isPending ? 'Loading projects' : isError ? 'Projects unavailable' : 'Choose a project'
  const options = (projects ?? []).map((project) => ({
    value: project.id,
    label: project.name,
    description: project.repoFullName,
  }))

  return (
    <ComboboxField
      id={id}
      value={value}
      onValueChange={onChange}
      options={options}
      disabled={disabled || isError}
      loading={isPending}
      aria-invalid={invalid}
      aria-describedby={describedBy}
      placeholder={placeholder}
      searchPlaceholder="Search projects"
      emptyText={projects?.length === 0 ? 'No projects connected yet.' : 'No project matches.'}
    />
  )
}
