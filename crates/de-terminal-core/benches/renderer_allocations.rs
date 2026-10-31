//! Isolated renderer allocation probes.
//!
//! Run with:
//!   cargo bench -p de-terminal-core --bench renderer_allocations
//!
//! This executable performs allocation measurements only. It is separate from
//! terminal_core so allocator instrumentation cannot perturb timed workloads.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use de_terminal_core::render::{
    CellRect, DamageTracker, Frame, GlyphAtlas, GlyphBitmap, GlyphMetrics, RenderPalette, Renderer,
    SoftwareRenderer,
};
use de_terminal_core::screen::{CellAttributes, GraphemeWidth, TerminalScreen};
use de_terminal_core::TerminalSize;

static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

// SAFETY: this benchmark-only wrapper forwards pointers/layouts unchanged to
// System and records only successful allocation sizes.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the requested layout is forwarded unchanged to System.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: pointer and its original layout came from this same System allocator.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: pointer and its original layout came from this same System allocator.
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        resized
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn report_allocations(name: &str, operation: impl FnOnce()) {
    let count_before = ALLOCATION_COUNT.load(Ordering::Relaxed);
    let bytes_before = ALLOCATED_BYTES.load(Ordering::Relaxed);
    operation();
    let count = ALLOCATION_COUNT.load(Ordering::Relaxed) - count_before;
    let bytes = ALLOCATED_BYTES.load(Ordering::Relaxed) - bytes_before;
    println!("# allocations_{name}_count={count}");
    println!("# allocations_{name}_bytes={bytes}");
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

fn main() {
    if cfg!(debug_assertions) {
        return;
    }

    let size = TerminalSize::new(80, 24).expect("benchmark dimensions");
    let mut screen =
        TerminalScreen::new(size, usize::from(size.columns()) * usize::from(size.rows()))
            .expect("allocate benchmark screen");
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
                .expect("populate benchmark screen");
        }
    }

    let mut damage = DamageTracker::new(size, 128).expect("damage tracker");
    let mut renderer = make_renderer(size);
    renderer.set_cursor_visible(false);
    let initial = Frame::new(&screen, &damage, 0).expect("prime frame");
    renderer.render(initial, None).expect("prime renderer");
    damage.clear();

    damage.mark_all();
    let full_frame = Frame::new(&screen, &damage, 1).expect("full frame");
    report_allocations("render_full_80x24", || {
        renderer
            .render(full_frame, None)
            .expect("full render probe");
    });
    damage.clear();

    let dirty = CellRect::new(size.rows() / 2, 0, 1, size.columns()).expect("dirty row");
    damage.mark_rect(dirty).expect("mark dirty row");
    let dirty_frame = Frame::new(&screen, &damage, 2).expect("dirty frame");
    report_allocations("render_dirty_row_80x24", || {
        renderer
            .render(dirty_frame, None)
            .expect("dirty render probe");
    });
}
