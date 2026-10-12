import type { ComponentProps } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

/** A code block scrolls sideways, so it must be reachable from the keyboard (WCAG 2.1.1). */
function Pre(props: ComponentProps<'pre'>) {
  return <pre tabIndex={0} {...props} />
}

const COMPONENTS = { pre: Pre }

/** Lazy half of `Markdown`: the only module that imports the engine. */
export default function MarkdownEngine({ children }: { children: string }) {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
      {children}
    </ReactMarkdown>
  )
}
