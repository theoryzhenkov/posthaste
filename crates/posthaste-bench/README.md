# posthaste-bench

Profiling and regression-gating harness for PostHaste's **offline** store hot
paths. Every workload drives `posthaste-store` through its public domain ports
against synthetic, deterministic fixtures — no network, no external services.

This crate is a workspace member but is **kept out of the default build**
(`default-members` in the root `Cargo.toml`) and out of the main `ci.yml` Rust
job, because it pulls profiling-only dependencies and a Valgrind-backed bench
harness. It is exercised by the `profile.yml` workflow instead.

## Operations under profile

Defined once in [`src/workloads.rs`](src/workloads.rs) and reused by all tools:

| Operation    | Store path exercised                                  |
| ------------ | ----------------------------------------------------- |
| `ingest`     | `apply_sync_batch` — write a batch of N messages      |
| `list_inbox` | `list_message_page` — first inbox page, newest first  |
| `search`     | `query_message_page_by_rule` — parsed smart-mailbox   |
| `mutate`     | `set_keywords` — toggle a keyword on one message       |

## Techniques

Four complementary, CI-friendly techniques (no `perf`/root required):

| Tool            | Answers                       | Artifact                          |
| --------------- | ----------------------------- | --------------------------------- |
| `pprof`         | where CPU time goes           | `<op>.cpu.flamegraph.svg` + `.pb` |
| `dhat`          | where allocations/memory go   | `<op>.dhat-heap.json`             |
| `criterion`     | wall-clock timing + trend     | `target/criterion/**` (HTML/JSON) |
| `iai-callgrind` | deterministic regression gate | Callgrind instruction counts      |

`perf`/`cargo-flamegraph` is available as a local-only deep-dive recipe.

## Running locally

```sh
# CPU flamegraphs + pprof protobufs + dhat heap profiles for every operation.
just lab profile                 # -> target/profile/<timestamp>/
just lab profile --out /tmp/prof # custom output dir

# Criterion wall-clock benchmarks (HTML + JSON under target/criterion).
just lab bench

# Deterministic regression gate. Requires valgrind and a matching runner:
#   cargo install iai-callgrind-runner --version 0.16.1
just lab bench-gate

# Local-only perf flamegraph (requires perf + `cargo install flamegraph`).
just lab flamegraph-perf
```

The `pprof` flamegraph SVG is the headline artifact: open it in a browser and
click frames to zoom. The `.pprof.pb` opens in [pprof](https://github.com/google/pprof)
or [speedscope](https://www.speedscope.app/); the `dhat-heap.json` opens in the
[DHAT viewer](https://nnethercote.github.io/dh_view/dh_view.html).

## CI (`profile.yml`)

- **regression-gate** (pull requests touching store/domain/bench): runs the
  iai-callgrind gate, comparing the PR head against its base via saved baselines.
  Instruction counts are deterministic, so this is reliable on noisy shared
  runners. Fails the build when total instructions (`Ir`) regress > 5%.
- **artifacts** (push to `main`, weekly schedule, manual dispatch): produces
  flamegraphs, heap profiles, and Criterion reports on a Linux + macOS matrix
  ("different machines") and uploads them.

## Lab integration

Two suites in `tools/lab/suites.toml` (tagged `profile`, not `lab-smoke`) run
the workloads and Criterion benches through the lab runner, which captures the
emitted `POSTHASTE_LAB_ARTIFACT_PATH=` markers into the run manifest:

```sh
just lab verify --tag profile --run-root target/lab/profile
```

## Scope and next steps

Phase 1 covers `posthaste-store` offline operations only. Live IMAP/JMAP network
operations are intentionally deferred (non-reproducible across machines). Future
phases can add `posthaste-engine` workloads (against the existing
`MockJmapGateway`) and `posthaste-domain` parsing/conversion paths by adding to
`workloads.rs` and registering new benches.
