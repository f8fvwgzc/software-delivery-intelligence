import { useQuery } from '@tanstack/react-query'

import { ComboboxField } from '@/components/shared/form'
import { Skeleton } from '@/components/ui/skeleton'

import { projectPickerQuery } from '../../api/queries'

interface ProjectSelectProps {
  id?: string
  value: string
  onChange: (projectId: string) => void
}

/** Picker over every connected project. Loads inline; never suspends the dialog. */
export function ProjectSelect({ id, value, onChange }: ProjectSelectProps) {
  const projects = useQuery(projectPickerQuery())

  if (projects.isPending) return <Skeleton className="h-8 w-full rounded-md" />

  if (projects.isError) {
    return (
      <p className="text-xs text-destructive" role="alert">
        Projects could not be loaded.{' '}
        <button type="button" className="underline underline-offset-4" onClick={() => projects.refetch()}>
          Try again
        </button>
      </p>
    )
  }

  if (projects.data.length === 0) {
    return <p className="text-xs text-muted-foreground">Connect a repository first.</p>
  }

  return (
    <ComboboxField
      id={id}
      value={value}
      onValueChange={onChange}
      options={projects.data.map((project) => ({ value: project.id, label: project.repoFullName }))}
      placeholder="Choose a project"
      searchPlaceholder="Search projects"
      emptyText="No project matches."
    />
  )
}
