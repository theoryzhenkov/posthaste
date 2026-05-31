export const LOG_EVENTS = {
  apiRequestCompleted: 'api.request.completed',
  apiRequestFailed: 'api.request.failed',
  apiRequestStarted: 'api.request.started',
  daemonEventMalformed: 'daemon.event.malformed',
  daemonEventStreamError: 'daemon.event.stream_error',
  frontendConsoleOutput: 'frontend.console.output',
  frontendErrorUncaught: 'frontend.error.uncaught',
  frontendErrorUnhandledRejection: 'frontend.error.unhandled_rejection',
  resourceFetchError: 'resource.fetch.error',
} as const

export type LogEvent = (typeof LOG_EVENTS)[keyof typeof LOG_EVENTS]
