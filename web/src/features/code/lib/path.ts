/** Repository-relative path helpers. Paths never start or end with `/`. */

export function normalizePath(raw: string | undefined): string {
  if (!raw) return ''
  return raw
    .split('/')
    .filter((segment) => segment !== '' && segment !== '.')
    .join('/')
}

export function splitPath(path: string): string[] {
  return path ? path.split('/') : []
}

export function parentPath(path: string): string {
  const index = path.lastIndexOf('/')
  return index === -1 ? '' : path.slice(0, index)
}

export function baseName(path: string): string {
  const index = path.lastIndexOf('/')
  return index === -1 ? path : path.slice(index + 1)
}

/** `a/b/c` → `['a', 'a/b']`: every directory above the path, top down. */
export function ancestorPaths(path: string): string[] {
  const parts = splitPath(path)
  const out: string[] = []
  for (let index = 1; index < parts.length; index++) {
    out.push(parts.slice(0, index).join('/'))
  }
  return out
}

export function joinPath(dir: string, name: string): string {
  return dir ? `${dir}/${name}` : name
}

export function isReadme(name: string): boolean {
  return /^readme\.(md|markdown)$/i.test(name)
}
