import { useQuery } from '@tanstack/react-query'

import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
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
        <Skeleton className="h-8 w-full rounded-lg" />
      ) : projects.isError || projects.data.length === 0 ? (
        <FieldDescription>No projects to pick from.</FieldDescription>
      ) : (
        <Select value={value} onValueChange={onChange}>
          <SelectTrigger id="decision-project" className="w-full">
            <SelectValue placeholder="Any project" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {projects.data.map((project) => (
                <SelectItem key={project.id} value={project.id}>
                  {project.repoFullName}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      )}
    </Field>
  )
}
