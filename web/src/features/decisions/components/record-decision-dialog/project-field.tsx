import { useQuery } from '@tanstack/react-query'

import { ComboboxField } from '@/components/shared/form'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Skeleton } from '@/components/ui/skeleton'
import { projectPickerQuery } from '@/features/proposals/api/queries'

interface ProjectFieldProps {
  value: string
  onChange: (projectId: string) => void
}

/** Optional project for a project-scoped decision. */
export function ProjectField({ value, onChange }: ProjectFieldProps) {
  const projects = useQuery(projectPickerQuery())

  return (
    <Field>
      <FieldLabel htmlFor="decision-project">Project</FieldLabel>
      {projects.isPending ? (
        <Skeleton className="h-8 w-full rounded-md" />
      ) : projects.isError || projects.data.length === 0 ? (
        <FieldDescription>No projects to pick from.</FieldDescription>
      ) : (
        <ComboboxField
          id="decision-project"
          value={value}
          onValueChange={onChange}
          options={projects.data.map((project) => ({ value: project.id, label: project.repoFullName }))}
          placeholder="Any project"
          searchPlaceholder="Search projects"
          emptyText="No project matches."
        />
      )}
    </Field>
  )
}
