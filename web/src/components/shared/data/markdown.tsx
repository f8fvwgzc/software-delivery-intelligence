import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { cn } from '@/lib/utils'

interface MarkdownProps {
  children: string
  className?: string
}

/** Renders generated documents and assistant replies. */
export function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={cn('prose-sdi', className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  )
}
