import {
  BracesIcon,
  ContainerIcon,
  FileCode2Icon,
  FileCogIcon,
  FileIcon,
  FileTextIcon,
  ServerIcon,
  type LucideIcon,
} from 'lucide-react'

/** One generated file as the viewer shows it; the gateway's `Artifact` fits as is. */
export interface ViewerFile {
  /** Repository-relative path, e.g. `.github/workflows/deploy.yml`. */
  path: string
  /** Full file content. */
  content: string
  /** `dockerfile`, `yaml`, `hcl`, `bicep`, `json`, `markdown`, `nginx` or `text`. */
  language: string
  /** One sentence: why this file exists. */
  purpose?: string
  /** True when the repository already has a file at this path. */
  replacesExisting?: boolean
}

/** Icon per generator language (CONTRACT.md §14); the extension decides for `text`. */
const LANGUAGE_ICON: Record<string, LucideIcon> = {
  dockerfile: ContainerIcon,
  yaml: FileCogIcon,
  hcl: FileCode2Icon,
  bicep: FileCode2Icon,
  json: BracesIcon,
  markdown: FileTextIcon,
  nginx: ServerIcon,
  groovy: FileCode2Icon,
}

const EXTENSION_ICON: Record<string, LucideIcon> = {
  yml: FileCogIcon,
  yaml: FileCogIcon,
  tf: FileCode2Icon,
  bicep: FileCode2Icon,
  json: BracesIcon,
  md: FileTextIcon,
  conf: ServerIcon,
  toml: FileCogIcon,
  env: FileCogIcon,
  example: FileCogIcon,
}

export function fileIcon(file: Pick<ViewerFile, 'language' | 'path'>): LucideIcon {
  const byLanguage = LANGUAGE_ICON[file.language.toLowerCase()]
  if (byLanguage) return byLanguage
  const name = baseName(file.path)
  if (/^dockerfile/i.test(name) || /\.dockerfile$/i.test(name)) return ContainerIcon
  if (/^jenkinsfile/i.test(name)) return FileCode2Icon
  const extension = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1).toLowerCase() : ''
  return EXTENSION_ICON[extension] ?? FileIcon
}

export function baseName(path: string) {
  const index = path.lastIndexOf('/')
  return index === -1 ? path : path.slice(index + 1)
}

export function directoryOf(path: string) {
  const index = path.lastIndexOf('/')
  return index === -1 ? '' : path.slice(0, index)
}

export interface FileGroup<T extends ViewerFile = ViewerFile> {
  /** Directory, '' for the repository root. */
  directory: string
  files: T[]
}

/** Files grouped by directory: root first, then directories in path order; files by name. */
export function groupByDirectory<T extends ViewerFile>(files: readonly T[]): FileGroup<T>[] {
  const groups = new Map<string, T[]>()
  for (const file of files) {
    const directory = directoryOf(file.path)
    const list = groups.get(directory)
    if (list) list.push(file)
    else groups.set(directory, [file])
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : a.localeCompare(b)))
    .map(([directory, list]) => ({
      directory,
      files: list.toSorted((a, b) => baseName(a.path).localeCompare(baseName(b.path))),
    }))
}

export function splitLines(content: string) {
  const lines = content.split('\n')
  // A trailing newline is a line terminator, not an empty last line.
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop()
  return lines
}

function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  anchor.rel = 'noopener'
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
  // Give the download a tick to start before the URL is revoked.
  setTimeout(() => URL.revokeObjectURL(url), 1_000)
}

/** Downloads one file as text, named after its basename. */
export function downloadFile(file: Pick<ViewerFile, 'path' | 'content'>) {
  saveBlob(new Blob([file.content], { type: 'text/plain;charset=utf-8' }), baseName(file.path))
}

/** Zips every file at its repository path. `fflate` is loaded on demand. */
export async function downloadFilesZip(files: readonly Pick<ViewerFile, 'path' | 'content'>[], filename: string) {
  const { strToU8, zipSync } = await import('fflate')
  const entries: Record<string, Uint8Array> = {}
  for (const file of files) entries[file.path] = strToU8(file.content)
  const bytes = zipSync(entries, { level: 6 })
  saveBlob(new Blob([bytes as BlobPart], { type: 'application/zip' }), filename)
}
