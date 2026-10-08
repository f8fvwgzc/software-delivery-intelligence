import { highlight } from '../../lib/highlight'

/** Text with the query's words marked. Segments are rendered as text nodes only. */
export function HighlightedText({ text, query }: { text: string; query: string }) {
  const segments = highlight(text, query)
  return (
    <>
      {segments.map((segment, index) =>
        segment.match ? (
          <mark key={index} className="rounded-sm bg-brand/15 text-foreground">
            {segment.text}
          </mark>
        ) : (
          <span key={index}>{segment.text}</span>
        ),
      )}
    </>
  )
}
