/**
 * The deterministic client testkit (D119): a fake transport + fake worker +
 * virtual clock composed into one driveable client (`createClientHarness`).
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
export {
  createClientHarness,
  type ClientHarness,
  type ClientHarnessOptions,
} from './clientHarness'
export {
  createFakeTransport,
  messageUpdatedFrame,
  transportRow,
  type FakeTransport,
  type TransportRow,
} from './fakeTransport'
export {
  createWorkerKit,
  LoopbackReplicaWorker,
  realHandleResponder,
  type WorkerKit,
  type WorkerResponder,
} from './fakeWorker'
export { createVirtualClock, type VirtualClock } from './virtualClock'
export { loadRealHandleFactory } from './wasmHandle'
