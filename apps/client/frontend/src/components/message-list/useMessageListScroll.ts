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
  const nodeRef = useRef<HTMLDivElement | null>(null)
  const resizeObserverRef = useRef<ResizeObserver | null>(null)
  const restoredViewKeyRef = useRef<string | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)

  useEffect(() => {
    restoredViewKeyRef.current = null
  }, [currentViewKey])

  useEffect(() => {
    const node = nodeRef.current
    if (!node || restoredViewKeyRef.current === currentViewKey) {
      return
    }
    const savedOffset = scrollOffsetByView.get(currentViewKey) ?? 0
    restoredViewKeyRef.current = currentViewKey
    node.scrollTop = savedOffset
    const frame = requestAnimationFrame(() => setScrollTop(savedOffset))
    return () => cancelAnimationFrame(frame)
  }, [currentViewKey, messageCount])

  // Callback ref: measure + observe whenever the scroll container mounts, so
  // the viewport height is never stuck at 0. A `[]` effect raced the container
  // mount (e.g. when it appears after a no-mailbox first render) and left the
  // list rendering only the fallback ~8 rows regardless of window size.
  const scrollContainerRef = useCallback((node: HTMLDivElement | null) => {
    resizeObserverRef.current?.disconnect()
    nodeRef.current = node
    if (!node) {
      resizeObserverRef.current = null
      return
    }
    setViewportHeight(node.clientHeight)
    const observer = new ResizeObserver(() =>
      setViewportHeight(node.clientHeight),
    )
    observer.observe(node)
    resizeObserverRef.current = observer
  }, [])

  const handleScroll = useCallback(() => {
    const node = nodeRef.current
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
  }, [
    currentViewKey,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isSearchBlocked,
  ])

  useEffect(() => {
    const node = nodeRef.current
    if (isSearchBlocked) {
      return
    }
    if (!node || !hasNextPage || isFetchingNextPage) {
      return
    }
    if (node.scrollHeight <= node.clientHeight + ROW_HEIGHT * 4) {
      fetchNextPage()
    }
  }, [
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    messageCount,
    isSearchBlocked,
  ])

  return { handleScroll, scrollContainerRef, scrollTop, viewportHeight }
}
