import { useQuery } from '@tanstack/react-query'

import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
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

  if (projects.isPending) return <Skeleton className="h-8 w-full rounded-lg" />

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
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger id={id} className="w-full">
        <SelectValue placeholder="Choose a project" />
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
  )
}
