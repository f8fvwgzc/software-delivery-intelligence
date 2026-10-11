import { createHighlighterCore, type HighlighterCore, type LanguageInput, type ThemedToken } from 'shiki/core'
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript'

import type { ShikiLanguage } from './highlight-languages'

/**
 * Loaded on demand (`import('./highlighter')`) so shiki's core and the
 * grammars stay out of the main bundle. One highlighter for the whole app;
 * each grammar is fetched once, the first time a file in that language opens.
 */

/** Both themes are emitted as CSS variables; `.code-token` in index.css picks one per theme. */
export const THEMES = { light: 'github-light', dark: 'github-dark' } as const

/**
 * github-dark paints comments in the same grey as github-light, which sits at
 * about 3:1 on the app's dark card; GitHub's own current dark comment grey
 * (#8b949e) reads at 4.9:1. Only that one colour is swapped.
 */
const COLOR_REPLACEMENTS = { 'github-dark': { '#6a737d': '#8b949e' } }

/** Lines longer than this are left unstyled rather than fed to the regex engine. */
const MAX_TOKENIZED_LINE_LENGTH = 4_000

const GRAMMARS = {
  asciidoc: () => import('shiki/langs/asciidoc.mjs'),
  astro: () => import('shiki/langs/astro.mjs'),
  bat: () => import('shiki/langs/bat.mjs'),
  c: () => import('shiki/langs/c.mjs'),
  clojure: () => import('shiki/langs/clojure.mjs'),
  cmake: () => import('shiki/langs/cmake.mjs'),
  cpp: () => import('shiki/langs/cpp.mjs'),
  csharp: () => import('shiki/langs/csharp.mjs'),
  css: () => import('shiki/langs/css.mjs'),
  csv: () => import('shiki/langs/csv.mjs'),
  dart: () => import('shiki/langs/dart.mjs'),
  dockerfile: () => import('shiki/langs/dockerfile.mjs'),
  elixir: () => import('shiki/langs/elixir.mjs'),
  elm: () => import('shiki/langs/elm.mjs'),
  erlang: () => import('shiki/langs/erlang.mjs'),
  fsharp: () => import('shiki/langs/fsharp.mjs'),
  go: () => import('shiki/langs/go.mjs'),
  graphql: () => import('shiki/langs/graphql.mjs'),
  groovy: () => import('shiki/langs/groovy.mjs'),
  haskell: () => import('shiki/langs/haskell.mjs'),
  hcl: () => import('shiki/langs/hcl.mjs'),
  html: () => import('shiki/langs/html.mjs'),
  ini: () => import('shiki/langs/ini.mjs'),
  java: () => import('shiki/langs/java.mjs'),
  javascript: () => import('shiki/langs/javascript.mjs'),
  json: () => import('shiki/langs/json.mjs'),
  jsx: () => import('shiki/langs/jsx.mjs'),
  julia: () => import('shiki/langs/julia.mjs'),
  kotlin: () => import('shiki/langs/kotlin.mjs'),
  latex: () => import('shiki/langs/latex.mjs'),
  less: () => import('shiki/langs/less.mjs'),
  lua: () => import('shiki/langs/lua.mjs'),
  makefile: () => import('shiki/langs/makefile.mjs'),
  markdown: () => import('shiki/langs/markdown.mjs'),
  mdx: () => import('shiki/langs/mdx.mjs'),
  nim: () => import('shiki/langs/nim.mjs'),
  'objective-c': () => import('shiki/langs/objective-c.mjs'),
  'objective-cpp': () => import('shiki/langs/objective-cpp.mjs'),
  ocaml: () => import('shiki/langs/ocaml.mjs'),
  perl: () => import('shiki/langs/perl.mjs'),
  php: () => import('shiki/langs/php.mjs'),
  powershell: () => import('shiki/langs/powershell.mjs'),
  proto: () => import('shiki/langs/proto.mjs'),
  python: () => import('shiki/langs/python.mjs'),
  r: () => import('shiki/langs/r.mjs'),
  rst: () => import('shiki/langs/rst.mjs'),
  ruby: () => import('shiki/langs/ruby.mjs'),
  rust: () => import('shiki/langs/rust.mjs'),
  sass: () => import('shiki/langs/sass.mjs'),
  scala: () => import('shiki/langs/scala.mjs'),
  scss: () => import('shiki/langs/scss.mjs'),
  shellscript: () => import('shiki/langs/shellscript.mjs'),
  sql: () => import('shiki/langs/sql.mjs'),
  stylus: () => import('shiki/langs/stylus.mjs'),
  svelte: () => import('shiki/langs/svelte.mjs'),
  swift: () => import('shiki/langs/swift.mjs'),
  toml: () => import('shiki/langs/toml.mjs'),
  tsx: () => import('shiki/langs/tsx.mjs'),
  typescript: () => import('shiki/langs/typescript.mjs'),
  vb: () => import('shiki/langs/vb.mjs'),
  vue: () => import('shiki/langs/vue.mjs'),
  xml: () => import('shiki/langs/xml.mjs'),
  yaml: () => import('shiki/langs/yaml.mjs'),
  zig: () => import('shiki/langs/zig.mjs'),
} satisfies Record<ShikiLanguage, LanguageInput>

let highlighter: Promise<HighlighterCore> | undefined
const grammars = new Map<ShikiLanguage, Promise<void>>()

function getHighlighter() {
  highlighter ??= createHighlighterCore({
    themes: [import('shiki/themes/github-light.mjs'), import('shiki/themes/github-dark.mjs')],
    langs: [],
    // Grammars with constructs the JS engine cannot translate degrade to partial styling instead of throwing.
    engine: createJavaScriptRegexEngine({ forgiving: true }),
  })
  return highlighter
}

function loadGrammar(core: HighlighterCore, lang: ShikiLanguage) {
  let loading = grammars.get(lang)
  if (!loading) {
    loading = core.loadLanguage(GRAMMARS[lang])
    grammars.set(lang, loading)
  }
  return loading
}

/**
 * Tokens for each line of `code`, each carrying `--shiki-light` / `--shiki-dark`
 * colour variables (and font-style ones where the theme sets them) in `htmlStyle`.
 */
export async function tokenize(code: string, lang: ShikiLanguage): Promise<ThemedToken[][]> {
  const core = await getHighlighter()
  await loadGrammar(core, lang)
  return core.codeToTokens(code, {
    lang,
    themes: THEMES,
    defaultColor: false,
    colorReplacements: COLOR_REPLACEMENTS,
    tokenizeMaxLineLength: MAX_TOKENIZED_LINE_LENGTH,
  }).tokens
}
