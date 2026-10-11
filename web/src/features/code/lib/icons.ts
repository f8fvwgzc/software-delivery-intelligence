import {
  ContainerIcon,
  FileCodeIcon,
  FileCogIcon,
  FileIcon,
  FileImageIcon,
  FileJsonIcon,
  FileLockIcon,
  FileTerminalIcon,
  FileTextIcon,
  FolderIcon,
  FolderOpenIcon,
} from 'lucide-react'
import type { ComponentType } from 'react'

export type EntryIcon = ComponentType<{ className?: string; 'aria-hidden'?: boolean | 'true' }>

const BY_EXTENSION: Record<string, EntryIcon> = {
  ts: FileCodeIcon,
  tsx: FileCodeIcon,
  js: FileCodeIcon,
  jsx: FileCodeIcon,
  mjs: FileCodeIcon,
  cjs: FileCodeIcon,
  rs: FileCodeIcon,
  py: FileCodeIcon,
  go: FileCodeIcon,
  md: FileTextIcon,
  markdown: FileTextIcon,
  txt: FileTextIcon,
  json: FileJsonIcon,
  yaml: FileJsonIcon,
  yml: FileJsonIcon,
  toml: FileJsonIcon,
  sh: FileTerminalIcon,
  bash: FileTerminalIcon,
  zsh: FileTerminalIcon,
  lock: FileLockIcon,
  png: FileImageIcon,
  jpg: FileImageIcon,
  jpeg: FileImageIcon,
  gif: FileImageIcon,
  svg: FileImageIcon,
  webp: FileImageIcon,
  ico: FileImageIcon,
}

const BY_NAME: Record<string, EntryIcon> = {
  dockerfile: ContainerIcon,
  'docker-compose.yml': ContainerIcon,
  'docker-compose.yaml': ContainerIcon,
  'compose.yml': ContainerIcon,
  'compose.yaml': ContainerIcon,
  makefile: FileCogIcon,
  '.gitignore': FileCogIcon,
  '.dockerignore': FileCogIcon,
  '.editorconfig': FileCogIcon,
  '.sdi.yml': FileCogIcon,
}

interface IconEntry {
  name: string
  isDir: boolean
  language?: string
}

/** Lucide glyph for a tree entry: folder state for directories, a few file kinds by name and extension. */
export function entryIcon(entry: IconEntry, open = false): EntryIcon {
  if (entry.isDir) return open ? FolderOpenIcon : FolderIcon
  const lower = entry.name.toLowerCase()
  const byName = BY_NAME[lower]
  if (byName) return byName
  if (entry.language === 'dockerfile') return ContainerIcon
  if (entry.language === 'shell') return FileTerminalIcon
  const dot = lower.lastIndexOf('.')
  if (dot === -1) return FileIcon
  return BY_EXTENSION[lower.slice(dot + 1)] ?? FileIcon
}
