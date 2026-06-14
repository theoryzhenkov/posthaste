import { useCallback, useEffect, useRef, useState } from 'react'

import { ROW_HEIGHT, scrollOffsetByView } from './model'

export function useMessageListScroll({
  currentViewKey,
  fetchNextPage,
  hasNextPage,
  isFetchingNextPage,
  isSearchBlocked,
  messageCount,
}: {
  currentViewKey: string
  fetchNextPage: () => void
  hasNextPage: boolean
  isFetchingNextPage: boolean
  isSearchBlocked: boolean
  messageCount: number
}) {
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const restoredViewKeyRef = useRef<string | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)

  useEffect(() => {
    restoredViewKeyRef.current = null
  }, [currentViewKey])

  useEffect(() => {
    const node = scrollContainerRef.current
    if (!node || restoredViewKeyRef.current === currentViewKey) {
      return
    }
    const savedOffset = scrollOffsetByView.get(currentViewKey) ?? 0
    restoredViewKeyRef.current = currentViewKey
    node.scrollTop = savedOffset
    const frame = requestAnimationFrame(() => setScrollTop(savedOffset))
    return () => cancelAnimationFrame(frame)
  }, [currentViewKey, messageCount])

  useEffect(() => {
    const node = scrollContainerRef.current
    if (!node) {
      return
    }

    const updateViewportHeight = () => setViewportHeight(node.clientHeight)
    updateViewportHeight()

    const resizeObserver = new ResizeObserver(updateViewportHeight)
    resizeObserver.observe(node)
    return () => resizeObserver.disconnect()
  }, [])

  const handleScroll = useCallback(() => {
    const node = scrollContainerRef.current
    if (!node) {
      return
    }
    setScrollTop(node.scrollTop)
    scrollOffsetByView.set(currentViewKey, node.scrollTop)
    if (isSearchBlocked) {
      return
    }
    const distanceToEnd = node.scrollHeight - node.scrollTop - node.clientHeight
    if (distanceToEnd < ROW_HEIGHT * 20 && hasNextPage && !isFetchingNextPage) {
      fetchNextPage()
    }
  }, [currentViewKey, fetchNextPage, hasNextPage, isFetchingNextPage, isSearchBlocked])

  useEffect(() => {
    const node = scrollContainerRef.current
    if (isSearchBlocked) {
      return
    }
    if (!node || !hasNextPage || isFetchingNextPage) {
      return
    }
    if (node.scrollHeight <= node.clientHeight + ROW_HEIGHT * 4) {
      fetchNextPage()
    }
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, messageCount, isSearchBlocked])

  return { handleScroll, scrollContainerRef, scrollTop, viewportHeight }
}
