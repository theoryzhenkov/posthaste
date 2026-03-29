---
scope: L0
summary: "UniFFI boundary decisions, GRDB callback model, type projection"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: README
  - path: spec/L0-jmap
dependents:
  - path: spec/L0-sync
  - path: spec/L0-ui
  - path: spec/L1-sync
  - path: spec/L1-ui
---

# Bridge domain -- L0

## Why UniFFI

Mozilla's UniFFI generates Swift bindings from Rust interface definitions. It supports async functions, callback interfaces, and complex type projection out of the box. Firefox and Signal both use it in production, which gives confidence in its maturity and maintenance trajectory.

The alternatives are weaker for this use case. Manual C FFI via cbindgen is lower-level and error-prone, requiring hand-written bridging code for every type. swift-bridge is less mature and has a smaller user base.

## The GRDB callback model

This is the central architectural decision for how Rust and Swift interact.

Rust owns all protocol and business logic. Swift owns the local cache (GRDB/SQLite) for reactive UI. The bridge connects them via a `CacheWriter` callback interface: Rust calls methods on a Swift-implemented object to write data into GRDB. SwiftUI then reads from GRDB via `ValueObservation`, which triggers view updates reactively whenever the underlying data changes.

Rust never reads from the cache for protocol decisions. It maintains its own in-memory state for sync orchestration. This means the cache is a downstream projection of protocol state, not a shared mutable store.

This pattern is borrowed from iNPUTmice/jmap-mua's architecture. It decouples protocol logic from UI completely. The sync engine can be tested in pure Rust with a mock `CacheWriter`. The UI can be tested with a pre-populated GRDB database and no network dependency.

## Type projection rules

External crate types never cross the FFI boundary. The `Email` type from `jmap-client`, the `Message` type from `mail-parser`, and `ammonia`'s sanitized output all stay on the Rust side. The bridge defines flat, FFI-safe record types with explicit conversion functions at the boundary.

Concrete rules: dates become `i64` timestamps. `HashMap<Id, bool>` becomes `Vec<String>`. HTML sanitization happens in Rust, so Swift receives clean HTML strings ready for rendering. No complex nested types cross the boundary.

## Async model

Rust `tokio` async functions are exposed as Swift `async` functions via UniFFI's async support. Long-running operations like sync and send are cancellable from the Swift side through structured concurrency.

## Callback interfaces (Rust calls Swift)

- `CacheWriter` -- write mailboxes, emails, threads, and deletions into GRDB
- `PushEventHandler` -- notify Swift when push events arrive (optional, used for UI badge updates)

## Exposed objects (Swift calls Rust)

- `MailClient` -- top-level entry point: connect, sync, search, compose
- `SearchEngine` -- query parsing and execution
- `ComposeSession` -- draft creation, editing, sending

## Error handling

Errors cross the boundary as typed UniFFI enums. Rust panics are caught at the FFI boundary and converted to error values. They never propagate into Swift. No error is silently swallowed; every failure path produces an explicit error that Swift must handle.

## Invariants

The bridge is the only Rust-Swift boundary in the application. No other FFI mechanisms are used. Rust owns all mutable protocol state; Swift receives immutable snapshots through `CacheWriter` calls. The bridge itself contains no business logic. It is a pure translation layer: type conversion, async bridging, and error mapping.

Callback interfaces are limited to `CacheWriter` and `PushEventHandler`. Adding new callback interfaces requires updating this spec.
