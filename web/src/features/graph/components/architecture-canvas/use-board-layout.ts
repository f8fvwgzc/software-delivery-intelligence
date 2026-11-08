import { useEffect, useState } from 'react'

import type { Board } from '../../lib/board'
import { layoutBoard, type BoardLayout } from './elk-layout'

/**
 * The ELK layout of a board. ELK is asynchronous, so the previous layout
 * stays on screen until the next one lands (no blank frame between boards),
 * and a result for a board that has since been replaced is dropped.
 */
export function useBoardLayout(board: Board): BoardLayout | null {
  const [layout, setLayout] = useState<BoardLayout | null>(null)

  useEffect(() => {
    let current = true
    void layoutBoard(board).then((next) => {
      if (current) setLayout(next)
    })
    return () => {
      current = false
    }
  }, [board])

  return layout
}
