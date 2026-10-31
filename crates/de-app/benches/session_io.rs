//! Linux PTY/session benchmarks for startup, input latency, and throughput.
//!
//! Run with:
//!   cargo bench -p de-app --bench session_io
//!
//! Environment overrides match the terminal-core harness:
//!   DE_BENCH_WARMUP_MS, DE_BENCH_SAMPLE_MS, DE_BENCH_SAMPLES

#![cfg(target_os = "linux")]

use std::fs;
use std::hint::black_box;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use de_app::lifecycle::ApplicationService;
use de_app::service_interfaces::{
    ProcessRead, SessionKind, SessionManagerService, SessionSpec, TerminalSession,
};
use de_app::{LinuxRuntimeProcess, LinuxRuntimeService, ManagedSession, SessionManager};
use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

fn setting(name: &str, default: u64, minimum: u64, maximum: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (minimum..=maximum).contains(value))
        .unwrap_or(default)
}

fn metadata() {
    println!("# benchmark_schema=1");
    println!("# os={}", std::env::consts::OS);
    println!("# arch={}", std::env::consts::ARCH);
    println!(
        "# rustc={}",
        Command::new("rustc")
            .arg("--version")
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned())
    );
    println!(
        "# commit={}",
        std::env::var("GITHUB_SHA").unwrap_or_else(|_| "local-unrecorded".to_owned())
    );
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_else(|| "unavailable".to_owned());
    println!("# cpu={cpu}");
    println!("name,record,sample,iterations,sample_mean_ns_per_op,p95_of_sample_means_ns_per_op,throughput_units_per_second");
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn benchmark(name: &str, units_per_op: u64, mut operation: impl FnMut()) {
    let warmup = Duration::from_millis(setting("DE_BENCH_WARMUP_MS", 500, 0, 60_000));
    let sample_duration = Duration::from_millis(setting("DE_BENCH_SAMPLE_MS", 200, 10, 60_000));
    let samples = setting("DE_BENCH_SAMPLES", 12, 3, 100) as usize;

    let warmup_start = Instant::now();
    while warmup_start.elapsed() < warmup {
        operation();
    }

    let mut per_sample = Vec::with_capacity(samples);
    let mut total_iterations = 0u64;
    let mut total_elapsed = Duration::ZERO;
    for sample in 0..samples {
        let started = Instant::now();
        let mut iterations = 0u64;
        while started.elapsed() < sample_duration {
            operation();
            iterations = iterations.saturating_add(1);
        }
        let elapsed = started.elapsed();
        let ns_per_op = elapsed.as_nanos() as f64 / iterations.max(1) as f64;
        per_sample.push(ns_per_op);
        total_iterations = total_iterations.saturating_add(iterations);
        total_elapsed += elapsed;
        println!(
            "{name},sample,{sample},{iterations},{ns_per_op:.2},,{:.2}",
            units_per_op as f64 * iterations as f64 / elapsed.as_secs_f64()
        );
    }
    per_sample.sort_by(f64::total_cmp);
    let median = percentile(&per_sample, 0.50);
    let p95 = percentile(&per_sample, 0.95);
    println!(
        "{name},summary,0,{total_iterations},{median:.2},{p95:.2},{:.2}",
        units_per_op as f64 * total_iterations as f64 / total_elapsed.as_secs_f64()
    );
    eprintln!(
        "SUMMARY {name}: samples={samples} iterations={total_iterations} median_ns/op={median:.2} p95_ns/op={p95:.2}"
    );
}

fn initialized_manager() -> SessionManager<LinuxRuntimeService> {
    let mut manager = SessionManager::new(LinuxRuntimeService::new());
    manager
        .initialize()
        .expect("initialize Linux session manager");
    manager
}

fn terminal_size() -> TerminalSize {
    TerminalSize::new(80, 24).expect("valid benchmark PTY size")
}

fn session_spec(program: ProgramSpec) -> SessionSpec {
    SessionSpec::new(SessionKind::ExternalProgram, program, terminal_size())
}

fn wait_for_marker(
    session: &mut ManagedSession<LinuxRuntimeProcess>,
    marker: &[u8],
    buffer: &mut [u8],
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut received = Vec::new();
    while !received
        .windows(marker.len())
        .any(|window| window == marker)
    {
        match session
            .read_output(buffer)
            .expect("read PTY startup marker")
        {
            ProcessRead::Data(count) => {
                received.extend_from_slice(&buffer[..count.get()]);
            }
            ProcessRead::WouldBlock => {
                assert!(Instant::now() < deadline, "PTY child did not become ready");
                thread::yield_now();
            }
            ProcessRead::Eof => {
                panic!("PTY closed before readiness marker");
            }
        }
    }
    let marker_end = received
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("readiness marker was observed")
        + marker.len();
    assert_eq!(
        marker_end,
        received.len(),
        "unexpected PTY bytes followed the readiness marker"
    );
}

fn round_trip(
    session: &mut ManagedSession<LinuxRuntimeProcess>,
    payload: &[u8],
    buffer: &mut [u8],
) {
    session.send_input(payload).expect("queue PTY input");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut received = 0usize;

    // Drain PTY output while flushing input. Waiting for the entire input
    // queue to empty before reading can deadlock when the child blocks on a
    // full PTY output buffer and therefore stops consuming input.
    while session.pending_input_bytes() > 0 || received < payload.len() {
        let mut progressed = false;
        if session.pending_input_bytes() > 0 {
            progressed |= session.flush_input().expect("flush PTY input") > 0;
        }

        match session.read_output(buffer).expect("read PTY echo") {
            ProcessRead::Data(count) => {
                let count = count.get();
                let end = received
                    .checked_add(count)
                    .filter(|end| *end <= payload.len())
                    .expect("PTY produced more bytes than the submitted payload");
                let expected = &payload[received..end];
                assert_eq!(
                    expected,
                    &buffer[..count],
                    "PTY loopback changed payload bytes"
                );
                received = end;
                progressed = true;
            }
            ProcessRead::WouldBlock => {}
            ProcessRead::Eof => panic!("PTY reached EOF during loopback"),
        }

        if !progressed {
            assert!(Instant::now() < deadline, "PTY loopback timed out");
            thread::yield_now();
        }
    }
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("benchmark target skipped in debug/test profile; use cargo bench");
        return;
    }
    metadata();
    let size = terminal_size();

    benchmark("session_manager_init_shutdown", 1, || {
        let mut manager = initialized_manager();
        manager.shutdown().expect("shutdown session manager");
        black_box(manager);
    });

    let mut manager = initialized_manager();
    let short_program = ProgramSpec::new("/usr/bin/true").expect("true executable");
    let short_spec = session_spec(short_program);
    benchmark("pty_session_spawn_close", 1, || {
        let id = manager
            .create_session(short_spec.clone())
            .expect("spawn short-lived PTY process");
        manager
            .close_session(id)
            .expect("close short-lived PTY process");
        black_box(id);
    });

    let loopback_program = ProgramSpec::new("/bin/sh")
        .expect("shell executable")
        .with_arg("-c")
        .with_arg("stty raw -echo; printf DE_READY_MARKER; exec /bin/cat");
    let loopback_id = manager
        .create_session(session_spec(loopback_program))
        .expect("spawn PTY loopback process");
    let mut buffer = vec![0u8; 16 * 1024];
    wait_for_marker(
        manager
            .session_mut(loopback_id)
            .expect("loopback session exists"),
        b"DE_READY_MARKER",
        &mut buffer,
    );

    benchmark("pty_input_round_trip_1_byte", 1, || {
        let session = manager
            .session_mut(loopback_id)
            .expect("loopback session exists");
        round_trip(session, black_box(b"x"), &mut buffer);
    });

    let payload = vec![b'z'; 32 * 1024];
    benchmark("pty_throughput_32kib", payload.len() as u64, || {
        let session = manager
            .session_mut(loopback_id)
            .expect("loopback session exists");
        round_trip(session, black_box(&payload), &mut buffer);
    });

    manager
        .close_session(loopback_id)
        .expect("close PTY loopback process");
    manager.shutdown().expect("shutdown Linux session manager");
    black_box(size);
}
