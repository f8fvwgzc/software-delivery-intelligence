#!/usr/bin/env node
/*
 * Reads `dist/` after `vite build`, prints every JS chunk with its raw and
 * gzip size, and fails when the budget of CONTRACT.md §21 is exceeded:
 *
 *   initial JS (the entry plus every chunk it imports statically) ≤ 220 kB gzip
 *   any other chunk ≤ 120 kB gzip, except the lazily loaded engines
 *   (shiki, mermaid, React Flow, the editorial canvas, the markdown engine)
 *
 * Usage: node scripts/check-bundle.mjs [--dist dist] [--json]
 */
import { readdir, readFile } from 'node:fs/promises'
import path from 'node:path'
import { gzipSync } from 'node:zlib'

const INITIAL_BUDGET = 220 * 1024
const ROUTE_BUDGET = 120 * 1024
/**
 * Roots of the lazily loaded engines. Everything they import, statically or
 * dynamically (mermaid's diagram chunks, shiki's grammars), counts as engine
 * and is allowed over the route budget.
 */
const ENGINE_ROOTS = [/highlighter/i, /mermaid-editor/i, /architecture-canvas/i, /editorial-canvas/i, /markdown-engine/i, /motion-lib/i]

const args = process.argv.slice(2)
const distArg = args.indexOf('--dist')
const dist = path.resolve(distArg === -1 ? 'dist' : args[distArg + 1])
const asJson = args.includes('--json')

const kb = (bytes) => `${(bytes / 1024).toFixed(1)} kB`
const pad = (value, width, right = false) => (right ? String(value).padStart(width) : String(value).padEnd(width))

async function listJs(dir) {
  const entries = await readdir(dir, { withFileTypes: true, recursive: true })
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.js'))
    .map((entry) => path.join(entry.parentPath ?? entry.path, entry.name))
}

/** Dynamic import specifiers: `import("./x.js")`, also as a template literal. */
function dynamicImports(source) {
  const found = new Set()
  const pattern = /import\(\s*["'`]([^"'`]+\.js)["'`]\s*\)/g
  let match
  while ((match = pattern.exec(source)) !== null) found.add(match[1])
  return [...found]
}

/** Static import specifiers of a built ES module: `import ... from "./x.js"` and bare `import "./x.js"`. */
function staticImports(source) {
  const found = new Set()
  const pattern = /(?:^|[;\s])import\s*(?:[^'"]*?\s*from\s*)?["']([^"']+\.js)["']/g
  let match
  while ((match = pattern.exec(source)) !== null) found.add(match[1])
  // `export ... from` re-exports load eagerly too.
  const reexport = /export\s*\*?\s*(?:\{[^}]*\})?\s*from\s*["']([^"']+\.js)["']/g
  while ((match = reexport.exec(source)) !== null) found.add(match[1])
  return [...found]
}

async function main() {
  const html = await readFile(path.join(dist, 'index.html'), 'utf8')
  const entryMatch = /<script[^>]+type="module"[^>]+src="([^"]+\.js)"/.exec(html)
  if (!entryMatch) throw new Error('No module entry script in dist/index.html; run `vite build` first.')
  const entry = path.join(dist, entryMatch[1].replace(/^\//, ''))

  const files = await listJs(dist)
  const chunks = new Map()
  for (const file of files) {
    const source = await readFile(file)
    chunks.set(file, { file, raw: source.length, gzip: gzipSync(source, { level: 9 }).length, source: source.toString('utf8') })
  }

  // Initial = the entry and everything reachable through static imports.
  const initial = new Set()
  const queue = [entry]
  while (queue.length > 0) {
    const current = queue.pop()
    if (!chunks.has(current) || initial.has(current)) continue
    initial.add(current)
    for (const specifier of staticImports(chunks.get(current).source)) {
      queue.push(path.resolve(path.dirname(current), specifier))
    }
  }

  // Engine = reachable from an engine root through any import, minus the initial set.
  const engine = new Set()
  const engineQueue = files.filter((file) => ENGINE_ROOTS.some((pattern) => pattern.test(path.basename(file))))
  while (engineQueue.length > 0) {
    const current = engineQueue.pop()
    if (!chunks.has(current) || engine.has(current) || initial.has(current)) continue
    engine.add(current)
    const source = chunks.get(current).source
    for (const specifier of [...staticImports(source), ...dynamicImports(source)]) {
      engineQueue.push(path.resolve(path.dirname(current), specifier))
    }
  }

  const rows = [...chunks.values()]
    .map((chunk) => ({
      name: path.relative(dist, chunk.file),
      raw: chunk.raw,
      gzip: chunk.gzip,
      initial: initial.has(chunk.file),
      engine: engine.has(chunk.file),
    }))
    .sort((a, b) => Number(b.initial) - Number(a.initial) || b.gzip - a.gzip)

  const initialGzip = rows.filter((row) => row.initial).reduce((sum, row) => sum + row.gzip, 0)
  const failures = []
  if (initialGzip > INITIAL_BUDGET) failures.push(`initial JS ${kb(initialGzip)} gzip exceeds ${kb(INITIAL_BUDGET)}`)
  for (const row of rows) {
    if (!row.initial && !row.engine && row.gzip > ROUTE_BUDGET) failures.push(`${row.name} ${kb(row.gzip)} gzip exceeds ${kb(ROUTE_BUDGET)}`)
  }

  if (asJson) {
    console.log(JSON.stringify({ initialGzip, rows, failures }, null, 2))
  } else {
    console.log(`\n${pad('chunk', 46)} ${pad('raw', 11, true)} ${pad('gzip', 11, true)}  kind`)
    console.log('-'.repeat(78))
    for (const row of rows) {
      const kind = row.initial ? 'initial' : row.engine ? 'engine (lazy)' : 'route (lazy)'
      console.log(`${pad(row.name, 46)} ${pad(kb(row.raw), 11, true)} ${pad(kb(row.gzip), 11, true)}  ${kind}`)
    }
    console.log('-'.repeat(78))
    console.log(`initial JS: ${kb(initialGzip)} gzip of ${kb(INITIAL_BUDGET)} budget (${rows.filter((row) => row.initial).length} chunks)`)
    for (const failure of failures) console.error(`FAIL ${failure}`)
    if (failures.length === 0) console.log('bundle within budget')
  }
  process.exitCode = failures.length > 0 ? 1 : 0
}

main().catch((error) => {
  console.error(error.message)
  process.exitCode = 1
})
