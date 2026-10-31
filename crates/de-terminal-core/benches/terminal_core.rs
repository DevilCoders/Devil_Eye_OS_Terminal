//! Dependency-free, reproducible microbenchmarks for terminal-core hot paths.
//!
//! Run with:
//!   cargo bench -p de-terminal-core --bench terminal_core
//!
//! Environment overrides:
//!   DE_BENCH_WARMUP_MS (default 500)
//!   DE_BENCH_SAMPLE_MS (default 200)
//!   DE_BENCH_SAMPLES (default 12)

use std::fs;
use std::hint::black_box;
use std::mem::size_of;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use de_terminal_core::parser::VtParser;
use de_terminal_core::render::{
    CellRect, DamageTracker, Frame, GlyphAtlas, GlyphBitmap, GlyphMetrics, RenderPalette, Renderer,
    SoftwareRenderer,
};
use de_terminal_core::screen::{CellAttributes, GraphemeWidth, TerminalScreen};
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
    println!("# family={}", std::env::consts::FAMILY);
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
        units_per_op as f64 * 1_000_000_000.0 / median
    );
    eprintln!(
        "SUMMARY {name}: samples={samples} iterations={total_iterations} median_ns/op={median:.2} p95_ns/op={p95:.2}"
    );
}

fn make_screen(size: TerminalSize) -> TerminalScreen {
    let mut screen =
        TerminalScreen::new(size, usize::from(size.columns()) * usize::from(size.rows()))
            .expect("allocate bounded terminal screen");
    let attributes = CellAttributes::default();
    for row in 0..size.rows() {
        for column in 0..size.columns() {
            let glyph = if (row + column) % 11 == 0 { "界" } else { "x" };
            let width = if glyph == "界" {
                GraphemeWidth::Wide
            } else {
                GraphemeWidth::Single
            };
            if width == GraphemeWidth::Wide && column + 1 >= size.columns() {
                continue;
            }
            screen
                .set_grapheme(row, column, glyph, width, attributes)
                .expect("populate representative terminal screen");
        }
    }
    screen
}

fn make_renderer(size: TerminalSize) -> SoftwareRenderer {
    let bitmap = GlyphBitmap::new(8, 16, vec![255; 8 * 16]).expect("glyph bitmap");
    let atlas = GlyphAtlas::new(bitmap);
    SoftwareRenderer::new(
        size,
        GlyphMetrics::new(8.0, 12.0, 4.0, 16.0).expect("glyph metrics"),
        RenderPalette::default(),
        atlas,
        16 * 1024 * 1024,
    )
    .expect("software renderer")
}

fn render_benchmarks(size: TerminalSize, label: &str) {
    let screen = make_screen(size);
    let mut damage = DamageTracker::new(size, 128).expect("damage tracker");
    let mut renderer = make_renderer(size);
    let full_rect = CellRect::new(0, 0, size.rows(), size.columns()).expect("full rectangle");

    // Prime the renderer, then measure only subsequent frame rendering.
    let frame = Frame::new(&screen, &damage, 0).expect("frame");
    renderer.render(frame, None).expect("prime renderer");
    damage.clear();
    println!(
        "# renderer_surface_{label}_bytes={}",
        renderer.surface().pixels().len()
    );

    benchmark(
        &format!("render_full_{label}"),
        u64::from(size.rows()) * u64::from(size.columns()),
        || {
            damage.mark_all();
            let frame = Frame::new(&screen, &damage, 1).expect("full frame");
            renderer.render(frame, None).expect("full render");
            damage.clear();
            black_box(renderer.surface().pixels().len());
        },
    );

    let dirty = CellRect::new(size.rows() / 2, 0, 1, size.columns()).expect("dirty row");
    benchmark(
        &format!("render_dirty_row_{label}"),
        u64::from(size.columns()),
        || {
            damage.mark_rect(dirty).expect("mark dirty row");
            let frame = Frame::new(&screen, &damage, 2).expect("dirty frame");
            renderer.render(frame, None).expect("dirty render");
            damage.clear();
            black_box(renderer.surface().pixels().len());
        },
    );
    black_box(full_rect);
}

fn main() {
    metadata();

    let parser_payload = b"hello terminal \x1b[38;2;120;180;240mcolor\x1b[0m \xe7\x95\x8c\r\n";
    let mut parser = VtParser::new();
    benchmark("parser_mixed_stream", parser_payload.len() as u64, || {
        let events = parser
            .process(black_box(parser_payload))
            .expect("bounded parser input");
        black_box(events);
    });

    let input_payload = b"typed-input-0123456789";
    let mut input_parser = VtParser::new();
    benchmark("input_parse_proxy", input_payload.len() as u64, || {
        let events = input_parser
            .process(black_box(input_payload))
            .expect("bounded input");
        black_box(events);
    });

    let size = TerminalSize::new(80, 24).expect("benchmark size");
    let mut screen = make_screen(size);
    benchmark("screen_update_80x24", 80 * 24, || {
        for row in 0..size.rows() {
            for column in 0..size.columns() {
                screen
                    .set_grapheme(
                        row,
                        column,
                        "x",
                        GraphemeWidth::Single,
                        CellAttributes::default(),
                    )
                    .expect("update terminal cell");
            }
        }
        black_box(screen.grid().cells());
    });

    benchmark("screen_scroll_80x24", 80 * 24, || {
        screen.scroll_up(1).expect("scroll screen");
        black_box(screen.grid().cells());
    });

    let screen_bytes = screen.grid().cells().len() * size_of::<de_terminal_core::screen::Cell>();
    println!("# memory_screen_80x24_bytes={screen_bytes}");
    println!(
        "# memory_screen_80x24_bytes_per_cell={}",
        size_of::<de_terminal_core::screen::Cell>()
    );

    render_benchmarks(size, "80x24");
    render_benchmarks(
        TerminalSize::new(160, 48).expect("medium benchmark size"),
        "160x48",
    );
    render_benchmarks(
        TerminalSize::new(240, 80).expect("large benchmark size"),
        "240x80",
    );

    // Yield once so very short local runs do not monopolize a single core
    // while stdout is flushed by the harness.
    thread::yield_now();
}
