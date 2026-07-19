// The closed provider vocabularies are wire-borne: re-served from `gen/`
// (ts-rs output), never restated here.

import type { ProviderHint } from '@/gen'

/** Compatibility alias: the UI historically names the wire's `ProviderHint`
 *  set `ProviderKind`. */
export type ProviderKind = ProviderHint

/** Redacted secret status returned by the API — never the actual value.
 *  Wire shape, re-served from `gen/`. */

