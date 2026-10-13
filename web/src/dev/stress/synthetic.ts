/*
 * Synthetic data for the stress page, generated at runtime from a seeded
 * PRNG so every run sees the same shapes without a fixture file. Nothing
 * here is imported by production code.
 */
import type { RepoTree } from '@/features/code/api/queries'
import type { DiagramSpec } from '@/features/diagrams/api/queries'
import type { Project } from '@/features/projects/api/queries'
import type { Finding, Review } from '@/features/reviews/api/types'
import type { components } from '@/lib/api/schema'

type ExecutionEvent = components['schemas']['ExecutionEvent']
type EventType = components['schemas']['EventType']
type FindingSeverity = components['schemas']['FindingSeverity']

/** Mulberry32: small, fast, deterministic. */
export function seeded(seed: number) {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const pick = <T>(random: () => number, items: readonly T[]): T => items[Math.floor(random() * items.length)]

const STACKS = ['typescript', 'react', 'rust', 'postgres', 'docker', 'kubernetes', 'terraform', 'python', 'go', 'redis', 'nginx']
const WORDS = ['gateway', 'ledger', 'billing', 'search', 'ingest', 'reports', 'identity', 'edge', 'catalog', 'notify', 'audit', 'planner']
const OWNERS = ['acme', 'northwind', 'contoso', 'globex', 'initech']
const START = Date.UTC(2026, 0, 1)

function isoAt(random: () => number, spreadDays = 300) {
  return new Date(START + random() * spreadDays * 86_400_000).toISOString()
}

/** `n` connected projects across a handful of owners, every status and mode represented. */
export function syntheticProjects(n: number, seed = 1): Project[] {
  const random = seeded(seed)
  const statuses: Project['status'][] = ['ready', 'ready', 'ready', 'scanning', 'pending', 'failed']
  return Array.from({ length: n }, (_, index) => {
    const name = `${pick(random, WORDS)}-${pick(random, WORDS)}-${index + 1}`
    const owner = pick(random, OWNERS)
    const status = pick(random, statuses)
    const scanned = status === 'ready' || status === 'failed'
    const stack = Array.from(new Set(Array.from({ length: 1 + Math.floor(random() * 6) }, () => pick(random, STACKS))))
    return {
      id: `prj-${index + 1}`,
      name,
      repoFullName: `${owner}/${name}`,
      repoUrl: `https://example.invalid/${owner}/${name}`,
      description: `Synthetic project ${index + 1}`,
      defaultBranch: 'main',
      isPrivate: random() < 0.6,
      status,
      mode: random() < 0.2 ? 'execute' : 'observe',
      stack,
      primaryLanguage: stack[0] ?? '',
      maturityScore: scanned ? Math.floor(random() * 101) : 0,
      openQuestions: scanned ? Math.floor(random() * 6) : 0,
      hasConfig: random() < 0.3,
      tldr: scanned ? `A ${stack.join(', ')} service that ${pick(random, WORDS)}s the ${pick(random, WORDS)}.` : '',
      lastScanId: scanned ? `scan-${index + 1}` : null,
      lastScannedAt: scanned ? isoAt(random) : null,
      createdAt: isoAt(random, 30),
    }
  })
}

const EXTENSIONS = ['ts', 'tsx', 'rs', 'py', 'go', 'md', 'json', 'yaml', 'toml', 'sql', 'css', 'html']
const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  ts: 'typescript',
  tsx: 'tsx',
  rs: 'rust',
  py: 'python',
  go: 'go',
  md: 'markdown',
  json: 'json',
  yaml: 'yaml',
  toml: 'toml',
  sql: 'sql',
  css: 'css',
  html: 'html',
}
const DIRS = ['src', 'crates', 'services', 'web', 'infra', 'docs', 'tests', 'scripts', 'packages']

/**
 * A repository tree of `n` entries: roughly a tenth are directories, files
 * sit up to six levels deep, and every ancestor directory exists as its own
 * entry (as GitHub's recursive tree returns them).
 */
export function syntheticTree(n: number, seed = 2): RepoTree {
  const random = seeded(seed)
  const entries: RepoTree['entries'] = []
  const seen = new Set<string>()
  const directories: string[] = ['']

  const ensureDir = (path: string) => {
    if (path === '' || seen.has(path)) return
    const slash = path.lastIndexOf('/')
    const parent = slash === -1 ? '' : path.slice(0, slash)
    if (parent) ensureDir(parent)
    seen.add(path)
    entries.push({ path, name: path.slice(path.lastIndexOf('/') + 1), isDir: true, size: 0, language: '' })
    directories.push(path)
  }

  // Grow a directory skeleton first, then fill it with files.
  const dirCount = Math.max(1, Math.floor(n / 12))
  for (let index = 0; index < dirCount; index++) {
    const parent = pick(random, directories)
    const depth = parent === '' ? 0 : parent.split('/').length
    const name = depth === 0 ? `${pick(random, DIRS)}-${index}` : `${pick(random, WORDS)}${index % 7}`
    if (depth >= 6) continue
    ensureDir(parent ? `${parent}/${name}` : name)
  }
  let fileIndex = 0
  while (entries.length < n) {
    const dir = pick(random, directories)
    const extension = pick(random, EXTENSIONS)
    const path = `${dir ? `${dir}/` : ''}${pick(random, WORDS)}_${fileIndex++}.${extension}`
    if (seen.has(path)) continue
    seen.add(path)
    entries.push({
      path,
      name: path.slice(path.lastIndexOf('/') + 1),
      isDir: false,
      size: Math.floor(random() * 40_000),
      language: LANGUAGE_BY_EXTENSION[extension] ?? '',
    })
  }
  return { projectId: 'stress-project', entries, commitSha: 'deadbeefcafe0123456789abcdef0123456789ab', truncated: false }
}

const EVENT_TYPES: EventType[] = [
  'state_changed',
  'policy_checked',
  'permission_checked',
  'step_started',
  'log',
  'log',
  'log',
  'step_succeeded',
  'checkpoint',
  'approval_requested',
  'approval_decided',
]

/** `n` execution events in sequence order; about a third are adapter `log` lines. */
export function syntheticEvents(n: number, executionId = 'stress-execution', seed = 3): ExecutionEvent[] {
  const random = seeded(seed)
  const steps = ['read', 'analyze', 'build', 'tests', 'package', 'deploy']
  let at = START
  return Array.from({ length: n }, (_, index) => {
    at += Math.floor(random() * 900)
    const type = pick(random, EVENT_TYPES)
    const stepKey = pick(random, steps)
    const long = random() < 0.05
    const message =
      type === 'log'
        ? `${pick(random, ['compiling', 'linking', 'fetching', 'running', 'checking'])} ${pick(random, WORDS)} ${index}${long ? ' '.padEnd(1) + 'with a long line of output that wraps across the width of the panel to exercise measured row heights in the windowed list' : ''}`
        : `${type.replaceAll('_', ' ')} for ${stepKey}`
    return {
      executionId,
      seq: index + 1,
      at: new Date(at).toISOString(),
      type,
      status: 'executing',
      stepKey,
      message,
      data: {},
    }
  })
}

const SEVERITIES: FindingSeverity[] = ['critical', 'high', 'high', 'medium', 'medium', 'medium', 'low', 'low', 'info']
const CATEGORIES = ['secrets', 'security', 'dependencies', 'docker', 'ci', 'iac', 'tests', 'docs', 'correctness']

/** A proposed review of a pull request touching `files` files, with a handful of findings per file. */
export function syntheticReview(files: number, seed = 4): Review {
  const random = seeded(seed)
  const findings: Finding[] = []
  for (let file = 0; file < files; file++) {
    const path = `src/${pick(random, DIRS)}/${pick(random, WORDS)}_${file}.${pick(random, ['ts', 'rs', 'py'])}`
    const perFile = 2 + Math.floor(random() * 6)
    for (let item = 0; item < perFile; item++) {
      const line = 1 + Math.floor(random() * 400)
      const category = pick(random, CATEGORIES)
      const finding: Finding = {
        id: `finding-${file}-${item}`,
        reviewId: 'stress-review',
        path,
        line,
        endLine: line + Math.floor(random() * 4),
        category,
        rule: `${category}/${pick(random, WORDS)}-${item}`,
        severity: pick(random, SEVERITIES),
        title: `${pick(random, ['Unpinned', 'Unchecked', 'Hard-coded', 'Missing', 'Unused'])} ${pick(random, WORDS)} in ${path.slice(path.lastIndexOf('/') + 1)}`,
        detail: `The analyzer saw \`${pick(random, WORDS)}\` at line ${line}. **Fix:** ${pick(random, ['pin the version', 'add a check', 'read it from the environment', 'add a test', 'remove it'])}.`,
        suggestion: random() < 0.5 ? `${pick(random, WORDS)}(${pick(random, WORDS)}, { safe: true })` : null,
        suggestionSource: null,
        canDraftFix: false,
        fixExecutionId: null,
        decision: random() < 0.15 ? 'accepted' : 'pending',
        decidedBy: null,
        decidedAt: null,
        decisionNote: null,
      }
      // The analyzer wrote the suggestions that exist; the model can be asked for the rest, except about a secret.
      finding.suggestionSource = finding.suggestion ? 'rule' : null
      finding.canDraftFix = !finding.suggestion && category !== 'secrets'
      findings.push(finding)
    }
  }
  return {
    id: 'stress-review',
    projectId: 'prj-1',
    projectName: 'stress project',
    prNumber: 4242,
    prTitle: `Refactor ${files} files across the platform`,
    prAuthor: 'octocat',
    headSha: 'cafebabe0123456789abcdef0123456789abcdef',
    status: 'proposed',
    reasoner: 'none',
    requestedBy: 'octocat',
    filesReviewed: files,
    findings,
    findingsTotal: findings.length,
    findingsAccepted: findings.filter((finding) => finding.decision === 'accepted').length,
    steps: [
      { key: 'context', label: 'Context', status: 'done', detail: `${files} files`, durationMs: 1200 },
      { key: 'analyze', label: 'Analyze', status: 'done', detail: '9 analyzers', durationMs: 8400 },
      { key: 'findings', label: 'Findings', status: 'done', detail: `${findings.length} findings`, durationMs: 300 },
      { key: 'summary', label: 'Summary', status: 'done', detail: '', durationMs: 2100 },
    ],
    summary: '## Walkthrough\n\nA synthetic review.',
    createdAt: new Date(START).toISOString(),
    updatedAt: new Date(START + 60_000).toISOString(),
    publishedUrl: null,
    error: null,
    fixExecutionId: null,
    mergeExecutionId: null,
    fixesReady: findings.filter((finding) => finding.decision === 'accepted' && finding.suggestion).length,
  }
}

const NODE_KINDS = ['service', 'module', 'job', 'ui', 'datastore', 'external', 'queue', 'infrastructure', 'actor']
const EDGE_STYLES = ['call', 'data', 'async', 'returns']

/** An architecture spec with `nodes` nodes in four groups and `edges` edges between them, labels on half. */
export function syntheticDiagram(nodes: number, edges: number, seed = 5): DiagramSpec {
  const random = seeded(seed)
  const groups = ['edge', 'core', 'data', 'ops'].map((id, index) => ({ id, label: `${id} tier`, description: `Group ${index + 1}` }))
  const specNodes = Array.from({ length: nodes }, (_, index) => {
    const kind = index === 0 ? 'actor' : pick(random, NODE_KINDS)
    return {
      id: `n${index}`,
      kind,
      label: `${pick(random, WORDS)} ${kind} ${index}`,
      groupId: kind === 'actor' || kind === 'external' ? undefined : groups[index % groups.length].id,
      path: kind === 'datastore' || kind === 'external' ? undefined : `src/${pick(random, WORDS)}/${pick(random, WORDS)}_${index}.rs`,
      caption: kind === 'datastore' ? 'postgres 16' : kind === 'external' ? 'vendor' : undefined,
    }
  })
  const specEdges = []
  const seen = new Set<string>()
  while (specEdges.length < edges) {
    const source = Math.floor(random() * nodes)
    const target = Math.floor(random() * nodes)
    if (source === target) continue
    const key = `${source}->${target}`
    if (seen.has(key)) continue
    seen.add(key)
    specEdges.push({
      id: `e${specEdges.length}`,
      source: `n${source}`,
      target: `n${target}`,
      style: pick(random, EDGE_STYLES),
      label: random() < 0.5 ? pick(random, ['reads', 'writes', 'calls', 'publishes', 'validates']) : undefined,
      dashed: random() < 0.2,
      evidencePath: `src/${pick(random, WORDS)}.rs`,
    })
  }
  return { direction: 'TB', groups, nodes: specNodes, edges: specEdges }
}

/** About `bytes` of plausible source text: short lines, a few very long ones. */
export function syntheticFile(bytes: number, seed = 6): string {
  const random = seeded(seed)
  const lines: string[] = []
  let size = 0
  let index = 0
  while (size < bytes) {
    const line =
      random() < 0.01
        ? `const blob_${index} = "${'x'.repeat(2_000)}"`
        : random() < 0.1
          ? ''
          : `${'  '.repeat(Math.floor(random() * 4))}${pick(random, ['const', 'let', 'return', 'await', 'export function', 'if ('])} ${pick(random, WORDS)}_${index} = ${pick(random, WORDS)}(${Math.floor(random() * 1000)}) // ${pick(random, WORDS)}`
    lines.push(line)
    size += line.length + 1
    index++
  }
  return lines.join('\n')
}
