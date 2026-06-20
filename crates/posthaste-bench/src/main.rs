//! `posthaste-profile`: run each offline store workload under a CPU sampler
//! (pprof -> flamegraph SVG + pprof protobuf) and a heap profiler (dhat ->
//! `dhat-heap.json`), writing artifacts and emitting lab artifact markers.
//!
//! Artifacts land under `$POSTHASTE_LAB_RUN_DIR/artifacts/profile` when invoked
//! by the lab runner (which only records paths inside the run dir), or under
//! `--out <dir>` / `target/profile/<timestamp>` otherwise.

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use posthaste_bench::workloads;

// dhat hooks the global allocator. With no active `dhat::Profiler` the overhead
// is negligible, so leaving it installed for the CPU phase is acceptable.
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Wall-clock budget per operation for CPU sampling.
const CPU_BUDGET: Duration = Duration::from_secs(3);
/// Sampling frequency (Hz) for the CPU profiler.
const CPU_FREQUENCY: i32 = 997;

fn main() -> Result<()> {
    let out = resolve_out_dir()?;
    fs::create_dir_all(&out).with_context(|| format!("create out dir {}", out.display()))?;
    eprintln!("posthaste-profile: writing artifacts to {}", out.display());

    let count = workloads::DEFAULT_MESSAGE_COUNT;

    // Run the cheap, reusable fixture once for read-heavy operations.
    let seeded = workloads::open_seeded(count);

    profile("ingest", &out, &mut || workloads::ingest(count))?;
    profile("list_inbox", &out, &mut || {
        let _ = workloads::list_inbox(&seeded);
    })?;
    profile("search", &out, &mut || {
        let _ = workloads::search(&seeded);
    })?;
    profile("fts_search", &out, &mut || {
        let _ = workloads::fts_search(&seeded);
    })?;
    profile("mutate", &out, &mut || workloads::mutate(&seeded, 1))?;

    let session = workloads::open_seeded(count);
    profile("session_loop", &out, &mut || {
        workloads::session_loop(&session, workloads::DEFAULT_SESSION_ROUNDS);
    })?;

    Ok(())
}

/// Profile one operation under both CPU and heap profilers.
fn profile(name: &str, out: &Path, op: &mut dyn FnMut()) -> Result<()> {
    profile_cpu(name, out, op)?;
    profile_heap(name, out, op)?;
    Ok(())
}

/// CPU sampling: repeatedly run `op` for a fixed budget, then write a flamegraph
/// SVG and a pprof protobuf profile.
fn profile_cpu(name: &str, out: &Path, op: &mut dyn FnMut()) -> Result<()> {
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(CPU_FREQUENCY)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .context("start cpu profiler")?;

    let deadline = Instant::now() + CPU_BUDGET;
    let mut iterations: u64 = 0;
    while Instant::now() < deadline {
        op();
        iterations += 1;
    }

    let report = guard.report().build().context("build cpu report")?;
    eprintln!("posthaste-profile: {name} cpu ran {iterations} iterations");

    let svg = out.join(format!("{name}.cpu.flamegraph.svg"));
    let file = File::create(&svg).with_context(|| format!("create {}", svg.display()))?;
    report.flamegraph(file).context("write flamegraph")?;
    emit_artifact(&svg)?;

    use pprof::protos::Message;
    let profile = report.pprof().context("encode pprof profile")?;
    let mut bytes = Vec::new();
    profile
        .write_to_vec(&mut bytes)
        .context("serialize pprof profile")?;
    let pb = out.join(format!("{name}.cpu.pprof.pb"));
    fs::write(&pb, &bytes).with_context(|| format!("write {}", pb.display()))?;
    emit_artifact(&pb)?;

    Ok(())
}

/// Heap profiling: run `op` once under dhat and write a `dhat-heap.json` viewable
/// in the online DHAT viewer.
fn profile_heap(name: &str, out: &Path, op: &mut dyn FnMut()) -> Result<()> {
    let path = out.join(format!("{name}.dhat-heap.json"));
    {
        let _profiler = dhat::Profiler::builder().file_name(&path).build();
        op();
    }
    emit_artifact(&path)?;
    Ok(())
}

/// Resolve where artifacts should be written, honouring the lab run dir and an
/// optional `--out <dir>` argument.
fn resolve_out_dir() -> Result<PathBuf> {
    if let Some(dir) = parse_out_arg() {
        return Ok(dir);
    }
    if let Ok(run_dir) = env::var("POSTHASTE_LAB_RUN_DIR") {
        return Ok(PathBuf::from(run_dir).join("artifacts").join("profile"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(PathBuf::from("target")
        .join("profile")
        .join(stamp.to_string()))
}

fn parse_out_arg() -> Option<PathBuf> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => return args.next().map(PathBuf::from),
            other => {
                if let Some(value) = other.strip_prefix("--out=") {
                    return Some(PathBuf::from(value));
                }
            }
        }
    }
    None
}

/// Print the lab artifact marker with an absolute path so the lab runner records
/// it (the lab only accepts paths that exist under the run dir).
fn emit_artifact(path: &Path) -> Result<()> {
    let absolute = path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))?;
    println!("POSTHASTE_LAB_ARTIFACT_PATH={}", absolute.display());
    Ok(())
}
