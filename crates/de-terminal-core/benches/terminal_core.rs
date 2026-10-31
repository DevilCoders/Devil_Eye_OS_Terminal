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
use std::mem::size_of_val;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use de_terminal_core::parser::VtParser;
use de_terminal_core::render::{
    CellPosition, CellRect, DamageTracker, Frame, GlyphAtlas, GlyphBitmap, GlyphMetrics,
    RenderPalette, Renderer, ScaleFactor, Selection, SelectionMode, SoftwareRenderer,
};
use de_terminal_core::screen::{CellAttributes, Color, GraphemeWidth, StyleFlags, TerminalScreen};
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
        "# source_commit={}",
        std::env::var("GITHUB_HEAD_SHA")
            .or_else(|_| std::env::var("GITHUB_SHA"))
            .unwrap_or_else(|_| "local-unrecorded".to_owned())
    );
    println!(
        "# workflow_commit={}",
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

fn make_ascii_screen(size: TerminalSize) -> TerminalScreen {
    let mut screen =
        TerminalScreen::new(size, usize::from(size.columns()) * usize::from(size.rows()))
            .expect("allocate cursor benchmark screen");
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
                .expect("populate ASCII cursor benchmark screen");
        }
    }
    screen
}

fn make_styled_screen(size: TerminalSize) -> TerminalScreen {
    let mut screen =
        TerminalScreen::new(size, usize::from(size.columns()) * usize::from(size.rows()))
            .expect("allocate styled benchmark screen");

    for row in 0..size.rows() {
        let mut column = 0;
        while column < size.columns() {
            if screen
                .get_cell(row, column)
                .is_some_and(|cell| cell.is_continuation())
            {
                column += 1;
                continue;
            }

            let wide = (usize::from(row) * 7 + usize::from(column) * 3) % 17 == 0
                && column + 1 < size.columns();
            let (glyph, width) = if wide {
                ("界", GraphemeWidth::Wide)
            } else {
                ("x", GraphemeWidth::Single)
            };
            let variant = (row + column) % 8;
            let styles = match variant {
                0 => StyleFlags::BOLD.with(StyleFlags::UNDERLINE),
                1 => StyleFlags::DIM.with(StyleFlags::ITALIC),
                2 => StyleFlags::INVERSE,
                3 => StyleFlags::STRIKETHROUGH,
                4 => StyleFlags::BLINK,
                5 => StyleFlags::HIDDEN,
                _ => StyleFlags::empty(),
            };
            let foreground = if variant % 2 == 0 {
                Color::Indexed(((row + column) % 256) as u8)
            } else {
                Color::Rgb {
                    red: (row.wrapping_mul(11) % 256) as u8,
                    green: (column.wrapping_mul(7) % 256) as u8,
                    blue: ((row + column).wrapping_mul(5) % 256) as u8,
                }
            };
            let background = if variant % 3 == 0 {
                Color::Indexed(((row * 3 + column * 5) % 256) as u8)
            } else {
                Color::Default
            };
            screen
                .set_grapheme(
                    row,
                    column,
                    glyph,
                    width,
                    CellAttributes::new(foreground, background, styles),
                )
                .expect("populate styled benchmark screen");
            column += if wide { 2 } else { 1 };
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

fn render_background_benchmarks(size: TerminalSize, label: &str) {
    let screen = TerminalScreen::new(size, usize::from(size.columns()) * usize::from(size.rows()))
        .expect("allocate empty background benchmark screen");
    let mut damage = DamageTracker::new(size, 128).expect("background damage tracker");
    let mut renderer = make_renderer(size);
    renderer.set_cursor_visible(false);

    let initial = Frame::new(&screen, &damage, 20).expect("background prime frame");
    renderer
        .render(initial, None)
        .expect("prime background-only renderer");
    damage.clear();

    benchmark(
        &format!("render_background_only_{label}"),
        u64::from(size.rows()) * u64::from(size.columns()),
        || {
            damage.mark_all();
            let frame = Frame::new(&screen, &damage, 21).expect("background-only frame");
            renderer
                .render(frame, None)
                .expect("background-only render");
            damage.clear();
            black_box(renderer.surface().pixels().len());
        },
    );
}

fn render_feature_benchmarks(size: TerminalSize, label: &str) {
    let screen = make_styled_screen(size);
    let mut damage = DamageTracker::new(size, 128).expect("styled damage tracker");
    let mut renderer = make_renderer(size);
    renderer.set_cursor_visible(false);

    let selection = Selection::new(
        CellPosition::new(0, 0),
        CellPosition::new(size.rows() - 1, size.columns() / 2),
        SelectionMode::Linear,
        size,
    )
    .expect("representative linear selection");

    benchmark(
        &format!("render_color_style_churn_{label}"),
        u64::from(size.rows()) * u64::from(size.columns()),
        || {
            damage.mark_all();
            let frame = Frame::new(&screen, &damage, 3).expect("styled frame");
            renderer.render(frame, None).expect("styled render");
            damage.clear();
            black_box(renderer.surface().pixels().len());
        },
    );

    benchmark(
        &format!("render_selection_{label}"),
        u64::from(size.rows()) * u64::from(size.columns()),
        || {
            damage.mark_all();
            let frame = Frame::new_with_selection(&screen, &damage, 4, Some(selection))
                .expect("selected frame");
            renderer.render(frame, None).expect("selection render");
            damage.clear();
            black_box(renderer.surface().pixels().len());
        },
    );
}

fn render_cursor_and_resize_benchmarks(size: TerminalSize) {
    // Keep the cursor workload single-width so each one-cell damage mark
    // exactly covers the cursor's pixel span, including at wraparound.
    let mut screen = make_ascii_screen(size);
    let mut damage = DamageTracker::new(size, 128).expect("cursor damage tracker");
    let mut renderer = make_renderer(size);
    screen.set_cursor(0, 0);
    let initial = Frame::new(&screen, &damage, 0).expect("initial cursor frame");
    renderer
        .render(initial, None)
        .expect("prime cursor renderer");
    damage.clear();

    let mut cursor_column = 0u16;
    benchmark("render_cursor_move_80x24", 2, || {
        let previous = cursor_column;
        cursor_column = (cursor_column + 1) % size.columns();
        screen.set_cursor(0, cursor_column);
        damage.mark_cell(0, previous).expect("mark old cursor");
        damage.mark_cell(0, cursor_column).expect("mark new cursor");
        let frame = Frame::new(&screen, &damage, 5).expect("cursor frame");
        renderer.render(frame, None).expect("cursor render");
        damage.clear();
        black_box(renderer.surface().pixels().len());
    });

    let larger = TerminalSize::new(size.columns() * 2, size.rows() * 2)
        .expect("larger resize benchmark dimensions");
    let scale = ScaleFactor::new(1.0).expect("unit scale");
    benchmark("renderer_resize_round_trip_80x24_160x48", 2, || {
        renderer.resize(larger, scale).expect("resize larger");
        renderer.resize(size, scale).expect("resize original");
        black_box(renderer.surface().pixels().len());
    });
}

fn scrollback_benchmarks(size: TerminalSize, label: &str) {
    const HISTORY_ROWS: usize = 256;
    let columns = usize::from(size.columns());
    let rows = usize::from(size.rows());
    let visible_cells = columns * rows;
    let history_cells = columns * HISTORY_ROWS;
    let mut screen = TerminalScreen::new_with_scrollback(size, visible_cells, history_cells)
        .expect("allocate bounded scrollback benchmark screen");

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
                .expect("populate scrollback benchmark screen");
        }
    }

    // Fill the ring before timing so the measured path includes steady-state
    // oldest-row eviction rather than one-time capacity growth.
    for _ in 0..HISTORY_ROWS {
        screen.scroll_up(1).expect("prime full scrollback ring");
    }
    assert_eq!(screen.scrollback_row_count(), HISTORY_ROWS);
    assert_eq!(screen.scrollback_cell_count(), history_cells);
    println!("# memory_scrollback_{label}_capacity_cells={history_cells}");
    println!(
        "# memory_scrollback_{label}_capacity_bytes={}",
        history_cells * std::mem::size_of::<de_terminal_core::screen::Cell>()
    );

    benchmark(
        &format!("scrollback_append_full_ring_{label}"),
        u64::from(size.columns()),
        || {
            screen
                .scroll_up(1)
                .expect("append and evict scrollback row");
            black_box(screen.scrollback_row_count());
        },
    );

    benchmark(
        &format!("scrollback_iterate_{HISTORY_ROWS}_rows_{label}"),
        (columns * HISTORY_ROWS) as u64,
        || {
            let retained_cells = screen
                .scrollback_rows()
                .map(<[de_terminal_core::screen::Cell]>::len)
                .sum::<usize>();
            black_box(retained_cells);
        },
    );
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("benchmark target skipped in debug/test profile; use cargo bench");
        return;
    }
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

    let screen_bytes = size_of_val(screen.grid().cells());
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

    render_background_benchmarks(size, "80x24");
    render_background_benchmarks(
        TerminalSize::new(160, 48).expect("medium background size"),
        "160x48",
    );
    render_background_benchmarks(
        TerminalSize::new(240, 80).expect("large background size"),
        "240x80",
    );
    render_feature_benchmarks(size, "80x24");
    render_feature_benchmarks(
        TerminalSize::new(160, 48).expect("medium styled size"),
        "160x48",
    );
    render_feature_benchmarks(
        TerminalSize::new(240, 80).expect("large styled size"),
        "240x80",
    );
    render_cursor_and_resize_benchmarks(size);
    scrollback_benchmarks(size, "80x24");
    scrollback_benchmarks(
        TerminalSize::new(240, 80).expect("large scrollback benchmark size"),
        "240x80",
    );

    // Yield once so very short local runs do not monopolize a single core
    // while stdout is flushed by the harness.
    thread::yield_now();
}
