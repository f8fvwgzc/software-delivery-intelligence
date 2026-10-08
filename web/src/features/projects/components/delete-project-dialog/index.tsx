import { Trash2Icon } from 'lucide-react'
import type { ReactNode } from 'react'
import { useState } from 'react'

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { useDeleteProject } from '../../api/mutations'
import type { Project } from '../../api/queries'

interface DeleteProjectDialogProps {
  project: Pick<Project, 'id' | 'name' | 'repoFullName'>
  /** The button that opens the dialog. Must be a single element that takes a ref. */
  trigger: ReactNode
  /** Shown on hover of an icon-only trigger. */
  tooltip?: string
  onDeleted?: () => void
}

/** Disconnects a project. Everything known about it goes with it, so the verb is explicit. */
export function DeleteProjectDialog({ project, trigger, tooltip, onDeleted }: DeleteProjectDialogProps) {
  const [open, setOpen] = useState(false)
  const deleteProject = useDeleteProject(project)

  const handleConfirm = () => {
    deleteProject.mutate(undefined, {
      onSuccess: () => {
        setOpen(false)
        onDeleted?.()
      },
    })
  }

  // Both triggers merge their props onto the one button, the Radix way.
  const dialogTrigger = <AlertDialogTrigger asChild>{trigger}</AlertDialogTrigger>

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      {tooltip ? (
        <Tooltip>
          <TooltipTrigger asChild>{dialogTrigger}</TooltipTrigger>
          <TooltipContent>{tooltip}</TooltipContent>
        </Tooltip>
      ) : (
        dialogTrigger
      )}
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-destructive/10 text-destructive">
            <Trash2Icon />
          </AlertDialogMedia>
          <AlertDialogTitle>Disconnect {project.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            Facts, graph, questions, proposals and execution history for{' '}
            <span className="font-mono text-foreground">{project.repoFullName}</span> are deleted. The
            repository on GitHub is not touched.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleteProject.isPending}>Keep project</AlertDialogCancel>
          <Button variant="destructive" onClick={handleConfirm} loading={deleteProject.isPending}>
            Delete project
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
