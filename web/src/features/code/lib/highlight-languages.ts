/**
 * The analyzer's lowercase language ids (crates/analyzer/src/lang.rs) that
 * shiki can tokenize, mapped to shiki's grammar name. Anything missing here
 * renders as plain text. Kept apart from the highlighter so the decision
 * costs nothing: the grammar chunks only load for a listed language.
 */
const SHIKI_LANGUAGE = {
  asciidoc: 'asciidoc',
  astro: 'astro',
  batch: 'bat',
  c: 'c',
  clojure: 'clojure',
  cmake: 'cmake',
  cpp: 'cpp',
  csharp: 'csharp',
  css: 'css',
  csv: 'csv',
  dart: 'dart',
  dockerfile: 'dockerfile',
  elixir: 'elixir',
  elm: 'elm',
  erlang: 'erlang',
  fsharp: 'fsharp',
  go: 'go',
  graphql: 'graphql',
  groovy: 'groovy',
  haskell: 'haskell',
  hcl: 'hcl',
  html: 'html',
  ini: 'ini',
  java: 'java',
  javascript: 'javascript',
  json: 'json',
  jsx: 'jsx',
  julia: 'julia',
  kotlin: 'kotlin',
  latex: 'latex',
  less: 'less',
  lua: 'lua',
  makefile: 'makefile',
  markdown: 'markdown',
  mdx: 'mdx',
  nim: 'nim',
  'objective-c': 'objective-c',
  'objective-cpp': 'objective-cpp',
  ocaml: 'ocaml',
  perl: 'perl',
  php: 'php',
  powershell: 'powershell',
  protobuf: 'proto',
  python: 'python',
  r: 'r',
  rst: 'rst',
  ruby: 'ruby',
  rust: 'rust',
  sass: 'sass',
  scala: 'scala',
  scss: 'scss',
  shell: 'shellscript',
  sql: 'sql',
  stylus: 'stylus',
  svelte: 'svelte',
  swift: 'swift',
  toml: 'toml',
  tsx: 'tsx',
  typescript: 'typescript',
  vb: 'vb',
  vue: 'vue',
  xml: 'xml',
  yaml: 'yaml',
  zig: 'zig',
} as const

export type ShikiLanguage = (typeof SHIKI_LANGUAGE)[keyof typeof SHIKI_LANGUAGE]

/** Files above this size render plain: tokenizing them would stall the main thread. */
export const HIGHLIGHT_MAX_BYTES = 200 * 1024

/** The shiki grammar for an analyzer language id, or null when the file stays plain. */
export function shikiLanguage(language: string): ShikiLanguage | null {
  return Object.hasOwn(SHIKI_LANGUAGE, language) ? SHIKI_LANGUAGE[language as keyof typeof SHIKI_LANGUAGE] : null
}
