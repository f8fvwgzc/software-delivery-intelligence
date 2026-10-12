import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

/** Lazy half of `Markdown`: the only module that imports the engine. */
export default function MarkdownEngine({ children }: { children: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
}
