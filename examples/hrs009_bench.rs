//! HRS-009 benchmark harness: identical Hermes-shaped dataset + identical
//! workload through the engine-neutral shim, for BOTH backends
//! (`--features turso` build runs the turso arm; default build runs stock).
//!
//! Measurement discipline (HRS-009 brief): workload seed/sizes are FIXED
//! here and committed with the results; no tuning after seeing numbers.
//!
//! Run: cargo run --release --features turso --example hrs009_bench -- \
//!        --backend turso --out benchmarks/results/hrs009-turso.json
//!      cargo run --release --example hrs009_bench -- \
//!        --backend stock --out benchmarks/results/hrs009-stock.json
//!
//! Dataset: N sessions (default 5000), each with 2–10 transcript messages,
//! a few KB of text per session (deterministic xorshift PRNG, seed 0x009).
//! Workload: 10_000 ops, 70% reads (point reads by session id, message
//! page reads), 30% writes (ensure + append + periodic usage deltas), a
//! PASSIVE checkpoint every 500 writes (SQLite WAL pressure; turso no-ops
//! its own path), per-op latencies collected into p50/p95/p99.

use std::time::Instant;

use hermes_rs::persistence::{AppendMessage, ModelUsageDelta, OpenOptions, Patience, SessionStore};

#[cfg(feature = "turso")]
use hermes_rs::persistence::TursoBackend;

use hermes_rs::persistence::StockSqliteBackend;

/// Deterministic PRNG (xorshift64*), seed fixed for reproducibility.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

const SEED: u64 = 0x0090_0900_0000_0009;

fn session_id(i: u64) -> String {
    format!("20261015_bench_{i:08}")
}

/// A few KB of transcript-ish text per message pair.
fn transcript_text(rng: &mut Rng, i: u64) -> String {
    let words = 200 + rng.below(300) as usize;
    let mut s = String::with_capacity(words * 7);
    for w in 0..words {
        s.push_str(&format!(
            "tok{} ",
            (i.wrapping_mul(31) + w as u64 * 7) % 10_000
        ));
    }
    s
}

fn build_dataset(path: &std::path::Path, sessions: u64) -> std::time::Duration {
    let t0 = Instant::now();
    let (store, _open) = StockSqliteBackend::open(OpenOptions::read_write(path))
        .expect("dataset scaffold via stock backend");
    let mut rng = Rng::new(SEED);
    let mut i = 0u64;
    while i < sessions {
        let sid = session_id(i);
        let msgs = 2 + rng.below(9);
        store
            .write(Patience::TRANSCRIPT, &mut |tx| {
                tx.ensure_session(&sid, "cli", Some("glm-5.3-flash"))?;
                for m in 0..msgs {
                    tx.append_message(
                        &sid,
                        &AppendMessage {
                            role: if m % 2 == 0 { "user" } else { "assistant" }.into(),
                            content: Some(transcript_text(&mut rng, i * 10 + m)),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(())
            })
            .expect("seed session");
        i += 1;
    }
    let _ = store.checkpoint_passive();
    drop(store);
    t0.elapsed()
}

#[derive(Clone)]
struct Percentiles {
    p50: f64,
    p95: f64,
    p99: f64,
    mean: f64,
    max: f64,
}

fn percentiles(mut v: Vec<f64>) -> Percentiles {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    let at = |q: f64| -> f64 {
        let idx = (((n as f64) - 1.0) * q).round() as usize;
        v[idx]
    };
    Percentiles {
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        mean: v.iter().sum::<f64>() / n as f64,
        max: v[n - 1],
    }
}

fn vm_hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0)
}

fn storage_bytes(path: &std::path::Path) -> u64 {
    let mut total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        if let Ok(m) = std::fs::metadata(PathBuf::from(format!("{}{suffix}", path.display()))) {
            total += m.len();
        }
    }
    total
}

use std::path::PathBuf;

struct WorkloadResult {
    wall: std::time::Duration,
    read_lat: Vec<f64>,
    write_lat: Vec<f64>,
    reads: u64,
    writes: u64,
    errors: Vec<String>,
    peak_rss_kb: u64,
}

fn run_workload(path: &std::path::Path, backend: &str, ops: u64) -> WorkloadResult {
    let mut errors = Vec::new();
    // SessionStore is not dyn-compatible (generic methods); the example runs
    // ONE backend per invocation, so each arm owns its concrete store.
    match backend {
        "turso" => {
            #[cfg(feature = "turso")]
            {
                let (s, _o) =
                    TursoBackend::open(OpenOptions::read_write(path)).expect("turso open");
                return run_workload_on(backend, s, path, ops, &mut errors);
            }
            #[cfg(not(feature = "turso"))]
            {
                let _ = backend;
                panic!("--backend turso requires --features turso");
            }
        }
        _ => {
            let (s, _o) =
                StockSqliteBackend::open(OpenOptions::read_write(path)).expect("stock open");
            run_workload_on(backend, s, path, ops, &mut errors)
        }
    }
}

fn run_workload_on<S: SessionStore>(
    backend: &str,
    store: S,
    path: &std::path::Path,
    ops: u64,
    errors: &mut Vec<String>,
) -> WorkloadResult {
    let _ = path;
    let _ = backend;
    let store = store;

    let mut rng = Rng::new(SEED ^ 0xFFFF);
    let mut read_lat = Vec::new();
    let mut write_lat = Vec::new();
    let mut reads = 0u64;
    let mut writes = 0u64;
    let t0 = Instant::now();
    for op in 0..ops {
        let sid = session_id(rng.below(5000));
        let is_read = op % 10 < 7; // 70/30 fixed pattern from the committed seed
        let start = Instant::now();
        if is_read {
            let r = store.read(&mut |tx| {
                let s = tx.get_session(&sid)?;
                let m = tx.get_messages(&sid, false, false, Some(50), 0)?;
                Ok((s.map(|x| x.message_count), m.len()))
            });
            match r {
                Ok(_) => reads += 1,
                Err(e) => errors.push(format!("read op {op}: {e}")),
            }
            read_lat.push(start.elapsed().as_secs_f64());
        } else {
            let r = store.write(Patience::WRITE, &mut |tx| {
                tx.ensure_session(&sid, "cli", Some("glm-5.3-flash"))?;
                tx.append_message(
                    &sid,
                    &AppendMessage {
                        role: "user".into(),
                        content: Some(format!("bench append op {op}")),
                        ..Default::default()
                    },
                )?;
                if op % 25 == 0 {
                    tx.record_model_usage(
                        &sid,
                        &ModelUsageDelta {
                            model: "glm-5.3-flash".into(),
                            billing_provider: "custom".into(),
                            billing_base_url: "http://127.0.0.1:20128/v1".into(),
                            billing_mode: "gateway".into(),
                            api_call_count: 1,
                            input_tokens: 100,
                            output_tokens: 50,
                            ..Default::default()
                        },
                    )?;
                }
                Ok(())
            });
            match r {
                Ok(_) => writes += 1,
                Err(e) => errors.push(format!("write op {op}: {e}")),
            }
            write_lat.push(start.elapsed().as_secs_f64());
            // WAL/checkpoint pressure every 500 writes (SQLite path; turso
            // no-ops its own checkpoint implementation).
            if writes % 500 == 0 {
                let _ = store.checkpoint_passive();
            }
        }
        if errors.len() > 20 {
            errors.push("... stopping: >20 errors".into());
            break;
        }
    }
    let wall = t0.elapsed();
    let peak_rss_kb = vm_hwm_kb();
    drop(store);
    WorkloadResult {
        wall,
        read_lat,
        write_lat,
        reads,
        writes,
        errors: std::mem::take(errors),
        peak_rss_kb,
    }
}

fn env_json() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1).map(|v| v.trim().to_string()))
        })
        .unwrap_or_else(|| "unknown".into());
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    format!(
        r#"{{"cpu_model":"{cpu}","cores":{cores},"kernel":"{kernel}","rustc":"{}"}}"#,
        rustc_version()
    )
}

fn rustc_version() -> String {
    // Read from the build script environment is not available to examples;
    // shell out is forbidden — record via the compiler-built constant.
    option_env!("RUSTC_VERSION")
        .unwrap_or("unknown (see bench README env block)")
        .to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut backend = "stock".to_string();
    let mut out = String::from("benchmarks/results/hrs009.json");
    let mut data_dir = std::env::temp_dir();
    let mut sessions: u64 = 5000;
    let mut ops: u64 = 10_000;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" => {
                backend = args[i + 1].clone();
                i += 1;
            }
            "--out" => {
                out = args[i + 1].clone();
                i += 1;
            }
            "--data-dir" => {
                data_dir = PathBuf::from(args[i + 1].clone());
                i += 1;
            }
            "--sessions" => {
                sessions = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--ops" => {
                ops = args[i + 1].parse().unwrap();
                i += 1;
            }
            other => panic!("unknown arg {other}"),
        }
        i += 1;
    }
    std::fs::create_dir_all(std::path::Path::new(&out).parent().expect("out parent")).unwrap();

    data_dir.push(format!("hrs009-{backend}-data"));
    std::fs::create_dir_all(&data_dir).unwrap();
    let db_path = data_dir.join("state.db");

    let seed_elapsed = if db_path.exists() {
        std::time::Duration::ZERO
    } else {
        println!(
            "seeding {} sessions (stock writer, shared dataset shape)...",
            sessions
        );
        build_dataset(&db_path, sessions)
    };
    let dataset_bytes = storage_bytes(&db_path);

    println!(
        "running {backend} workload: {ops} ops on {}",
        db_path.display()
    );
    let result = run_workload(&db_path, &backend, ops);

    let read_p = percentiles(result.read_lat.clone());
    let write_p = percentiles(result.write_lat.clone());
    let total_ops = (result.reads + result.writes) as f64;
    let throughput = total_ops / result.wall.as_secs_f64();

    let record = format!(
        r#"{{"task":"HRS-009","backend":"{backend}","seed":{SEED},
  "dataset": {{"sessions": {sessions},"seed_wall_seconds": {:?},"storage_bytes": {dataset_bytes},
              "generator": "xorshift64* seed {SEED}; 2-10 messages/session; 200-500 words/message"}},
  "workload": {{"ops": {ops},"reads": {reads},"writes": {writes},"read_write_mix":"70/30 fixed by seed",
                "pattern": "point reads by session id + message page reads; ensure+append writes; usage delta every 25th write; passive checkpoint every 500 writes"}},
  "results": {{
    "wall_seconds": {:?},
    "throughput_ops_per_s": {throughput:.1},
    "read_latency": {{"p50_ms": {:.3},"p95_ms": {:.3},"p99_ms": {:.3},"mean_ms": {:.3},"max_ms": {:.3},"n": {reads}}},
    "write_latency": {{"p50_ms": {:.3},"p95_ms": {:.3},"p99_ms": {:.3},"mean_ms": {:.3},"max_ms": {:.3},"n": {writes}}},
    "peak_rss_kb": {rss},
    "errors": {nerr}
  }},
  "environment": {env},
  "caveats": "single process per engine; benchmark host and load at run time are recorded in benchmarks/README.md; no tuning pass was made after first results"
}}"#,
        seed_elapsed,
        result.wall.as_secs_f64(),
        read_p.p50 * 1000.0,
        read_p.p95 * 1000.0,
        read_p.p99 * 1000.0,
        read_p.mean * 1000.0,
        read_p.max * 1000.0,
        write_p.p50 * 1000.0,
        write_p.p95 * 1000.0,
        write_p.p99 * 1000.0,
        write_p.mean * 1000.0,
        write_p.max * 1000.0,
        reads = result.reads,
        writes = result.writes,
        rss = result.peak_rss_kb,
        nerr = result.errors.len(),
        env = env_json(),
    );

    std::fs::write(&out, record).expect("write result json");
    println!(
        "{backend}: wall {:?}, {throughput:.1} ops/s, read p95 {:.2}ms, write p95 {:.2}ms, rss {}KB, errors {}",
        result.wall,
        read_p.p95 * 1000.0,
        write_p.p95 * 1000.0,
        result.peak_rss_kb,
        result.errors.len()
    );
    if !result.errors.is_empty() {
        println!(
            "first errors: {:?}",
            &result.errors[..result.errors.len().min(3)]
        );
    }
}
