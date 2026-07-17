/**
 * Render-error fault domain. A render-time throw in any descendant is caught
 * here and rendered as a recoverable fallback instead of unmounting the whole
 * window. Wrap independent regions (surfaces, the mail shell) so one crash
 * cannot blank the entire app.
 *
 */
import { Component, type ErrorInfo, type ReactNode } from 'react'

import { LOG_EVENTS } from '../logEvents'
import { uiLogger } from '../logger'
import { Button } from './ui/button'

interface ErrorBoundaryProps {
  /** Identifies which boundary caught the error, for logging. */
  label: string
  children: ReactNode
  /** Custom fallback; defaults to a centered "try again" panel. */
  fallback?: (error: Error, reset: () => void) => ReactNode
  /** When any entry changes, a caught error is cleared (e.g. on navigation). */
  resetKeys?: ReadonlyArray<unknown>
}

interface ErrorBoundaryState {
  error: Error | null
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    uiLogger.error(
      {
        event: LOG_EVENTS.frontendErrorUncaught,
        boundary: this.props.label,
        componentStack: info.componentStack ?? null,
      },
      `Render error caught by ${this.props.label} boundary: ${error.message}`,
    )
  }

  componentDidUpdate(previous: ErrorBoundaryProps): void {
    if (
      this.state.error !== null &&
      !sameResetKeys(previous.resetKeys, this.props.resetKeys)
    ) {
      this.setState({ error: null })
    }
  }

  private readonly reset = (): void => {
    this.setState({ error: null })
  }

  render(): ReactNode {
    const { error } = this.state
    if (error === null) {
      return this.props.children
    }
    if (this.props.fallback) {
      return this.props.fallback(error, this.reset)
    }
    return <DefaultErrorFallback error={error} reset={this.reset} />
  }
}

function sameResetKeys(
  a: ReadonlyArray<unknown> = [],
  b: ReadonlyArray<unknown> = [],
): boolean {
  return a.length === b.length && a.every((value, i) => Object.is(value, b[i]))
}

function DefaultErrorFallback({
  error,
  reset,
}: {
  error: Error
  reset: () => void
}) {
  return (
    <div className="flex h-full min-h-0 flex-col items-center justify-center gap-3 p-6 text-center">
      <p className="text-sm font-medium text-foreground">
        Something went wrong
      </p>
      <p className="max-w-md text-xs break-words text-muted-foreground">
        {error.message}
      </p>
      <Button variant="outline" size="sm" onClick={reset}>
        Try again
      </Button>
    </div>
  )
}
