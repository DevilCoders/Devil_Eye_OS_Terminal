//! Backend-independent renderer frame, damage, geometry, and glyph contracts.
//!
//! The terminal model owns cells and cursor state. A renderer receives an
//! immutable frame and may not mutate or retain its borrowed cell storage.

use std::error::Error;
use std::fmt;

use crate::screen::{Cell, CursorPosition, TerminalScreen};
use crate::TerminalSize;

/// Default upper bound on separately tracked damage rectangles.
pub const DEFAULT_MAX_DAMAGE_RECTS: usize = 128;

/// A rectangle in terminal-cell coordinates, with an exclusive bottom/right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRect {
    top: u16,
    left: u16,
    rows: u16,
    columns: u16,
}

impl CellRect {
    pub const fn new(
        top: u16,
        left: u16,
        rows: u16,
        columns: u16,
    ) -> Result<Self, RenderContractError> {
        if rows == 0 || columns == 0 {
            return Err(RenderContractError::ZeroRectangleExtent);
        }
        Ok(Self {
            top,
            left,
            rows,
            columns,
        })
    }

    pub const fn top(self) -> u16 {
        self.top
    }
    pub const fn left(self) -> u16 {
        self.left
    }
    pub const fn rows(self) -> u16 {
        self.rows
    }
    pub const fn columns(self) -> u16 {
        self.columns
    }

    const fn bottom_exclusive(self) -> u32 {
        self.top as u32 + self.rows as u32
    }

    const fn right_exclusive(self) -> u32 {
        self.left as u32 + self.columns as u32
    }

    fn is_within(self, size: TerminalSize) -> bool {
        self.bottom_exclusive() <= u32::from(size.rows())
            && self.right_exclusive() <= u32::from(size.columns())
    }

    fn touches_or_overlaps(self, other: Self) -> bool {
        u32::from(self.top) <= other.bottom_exclusive()
            && u32::from(other.top) <= self.bottom_exclusive()
            && u32::from(self.left) <= other.right_exclusive()
            && u32::from(other.left) <= self.right_exclusive()
    }

    fn union(self, other: Self) -> Self {
        let top = self.top.min(other.top);
        let left = self.left.min(other.left);
        let bottom = self.bottom_exclusive().max(other.bottom_exclusive());
        let right = self.right_exclusive().max(other.right_exclusive());
        Self {
            top,
            left,
            rows: (bottom - u32::from(top)) as u16,
            columns: (right - u32::from(left)) as u16,
        }
    }
}

/// Errors from constructing renderer-facing geometry or frame metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderContractError {
    ZeroDamageLimit,
    ZeroRectangleExtent,
    RectangleOutOfBounds,
    FrameSizeMismatch,
    SelectionOutOfBounds,
    InvalidGlyphMetrics,
    InvalidScaleFactor,
}

impl fmt::Display for RenderContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroDamageLimit => "damage rectangle limit must be non-zero",
            Self::ZeroRectangleExtent => "rectangle rows and columns must be non-zero",
            Self::RectangleOutOfBounds => "rectangle exceeds terminal cell bounds",
            Self::FrameSizeMismatch => "damage tracker dimensions do not match terminal frame",
            Self::SelectionOutOfBounds => "selection endpoint exceeds terminal cell bounds",
            Self::InvalidGlyphMetrics => {
                "glyph metrics must be finite, positive, and internally consistent"
            }
            Self::InvalidScaleFactor => "scale factor must be finite and greater than zero",
        };
        formatter.write_str(message)
    }
}

impl Error for RenderContractError {}

/// Bounded dirty-region accumulator for one terminal size.
///
/// Overlapping or edge-adjacent rectangles are coalesced. If the configured
/// rectangle budget is exhausted, the tracker degrades to one full-frame
/// invalidation rather than allocating without bound or dropping damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageTracker {
    size: TerminalSize,
    max_rectangles: usize,
    full: bool,
    rectangles: Vec<CellRect>,
}

impl DamageTracker {
    pub fn new(size: TerminalSize, max_rectangles: usize) -> Result<Self, RenderContractError> {
        if max_rectangles == 0 {
            return Err(RenderContractError::ZeroDamageLimit);
        }
        Ok(Self {
            size,
            max_rectangles,
            full: true,
            rectangles: Vec::new(),
        })
    }

    pub const fn size(&self) -> TerminalSize {
        self.size
    }
    pub const fn is_full(&self) -> bool {
        self.full
    }
    pub fn rectangles(&self) -> &[CellRect] {
        &self.rectangles
    }

    pub fn mark_cell(&mut self, row: u16, column: u16) -> Result<(), RenderContractError> {
        self.mark_rect(CellRect::new(row, column, 1, 1)?)
    }

    pub fn mark_rect(&mut self, rectangle: CellRect) -> Result<(), RenderContractError> {
        if !rectangle.is_within(self.size) {
            return Err(RenderContractError::RectangleOutOfBounds);
        }
        if self.full {
            return Ok(());
        }

        let mut merged = rectangle;
        let mut index = 0usize;
        while index < self.rectangles.len() {
            if merged.touches_or_overlaps(self.rectangles[index]) {
                merged = merged.union(self.rectangles.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }

        if self.rectangles.len() >= self.max_rectangles {
            self.mark_all();
            return Ok(());
        }

        self.rectangles.push(merged);
        Ok(())
    }

    pub fn mark_all(&mut self) {
        self.full = true;
        self.rectangles.clear();
    }

    /// Clears damage only after the renderer has successfully consumed the frame.
    pub fn clear(&mut self) {
        self.full = false;
        self.rectangles.clear();
    }

    /// A resize invalidates the complete new viewport.
    pub fn resize(&mut self, size: TerminalSize) {
        self.size = size;
        self.mark_all();
    }
}

/// Validated metrics for a terminal cell's font rasterization box.
///
/// Values are logical pixels before the renderer's scale factor. Descent is
/// represented as a positive magnitude below the baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
    advance_width: f32,
    ascent: f32,
    descent: f32,
    line_height: f32,
}

impl GlyphMetrics {
    pub fn new(
        advance_width: f32,
        ascent: f32,
        descent: f32,
        line_height: f32,
    ) -> Result<Self, RenderContractError> {
        let values = [advance_width, ascent, descent, line_height];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || line_height < ascent + descent
        {
            return Err(RenderContractError::InvalidGlyphMetrics);
        }
        Ok(Self {
            advance_width,
            ascent,
            descent,
            line_height,
        })
    }

    pub const fn advance_width(self) -> f32 {
        self.advance_width
    }
    pub const fn ascent(self) -> f32 {
        self.ascent
    }
    pub const fn descent(self) -> f32 {
        self.descent
    }
    pub const fn line_height(self) -> f32 {
        self.line_height
    }
}

/// Positive logical-to-physical pixel scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactor(f32);

impl ScaleFactor {
    pub fn new(value: f32) -> Result<Self, RenderContractError> {
        if !value.is_finite() || value <= 0.0 {
            return Err(RenderContractError::InvalidScaleFactor);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Zero-based cell coordinate used by a terminal selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellPosition {
    row: u16,
    column: u16,
}

impl CellPosition {
    pub const fn new(row: u16, column: u16) -> Self {
        Self { row, column }
    }

    pub const fn row(self) -> u16 {
        self.row
    }

    pub const fn column(self) -> u16 {
        self.column
    }

    fn is_within(self, size: TerminalSize) -> bool {
        self.row < size.rows() && self.column < size.columns()
    }

    fn linear_index(self, size: TerminalSize) -> u32 {
        u32::from(self.row) * u32::from(size.columns()) + u32::from(self.column)
    }
}

/// Terminal selection semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Select the inclusive row-major range between anchor and extent.
    Linear,
    /// Select the inclusive rectangle bounded by anchor and extent.
    Rectangular,
}

/// Immutable selection range carried by a render frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    anchor: CellPosition,
    extent: CellPosition,
    mode: SelectionMode,
}

impl Selection {
    pub fn new(
        anchor: CellPosition,
        extent: CellPosition,
        mode: SelectionMode,
        size: TerminalSize,
    ) -> Result<Self, RenderContractError> {
        if !anchor.is_within(size) || !extent.is_within(size) {
            return Err(RenderContractError::SelectionOutOfBounds);
        }
        Ok(Self {\n            anchor,\n            extent,\n            mode,\n        })
    }

    pub const fn anchor(self) -> CellPosition {
        self.anchor
    }

    pub const fn extent(self) -> CellPosition {
        self.extent
    }

    pub const fn mode(self) -> SelectionMode {
        self.mode
    }

    fn is_within(self, size: TerminalSize) -> bool {
        self.anchor.is_within(size) && self.extent.is_within(size)
    }

    pub fn contains(self, row: u16, column: u16, size: TerminalSize) -> bool {
        let position = CellPosition::new(row, column);
        if !position.is_within(size) || !self.is_within(size) {
            return false;
        }
        match self.mode {
            SelectionMode::Linear => {
                let anchor = self.anchor.linear_index(size);
                let extent = self.extent.linear_index(size);
                let current = position.linear_index(size);
                current >= anchor.min(extent) && current <= anchor.max(extent)
            }
            SelectionMode::Rectangular => {
                row >= self.anchor.row.min(self.extent.row)
                    && row <= self.anchor.row.max(self.extent.row)
                    && column >= self.anchor.column.min(self.extent.column)
                    && column <= self.anchor.column.max(self.extent.column)
            }
        }
    }
}

/// Immutable borrowed snapshot passed to a renderer.
///
/// Coordinates are zero-based cells, row-major. The renderer must not retain
/// the cell slice after the render call returns. The caller increments
/// generation whenever the terminal frame changes.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    size: TerminalSize,
    cursor: CursorPosition,
    cells: &'a [Cell],
    damage: &'a DamageTracker,
    selection: Option<Selection>,
    generation: u64,
}

impl<'a> Frame<'a> {
    pub fn new(
        screen: &'a TerminalScreen,
        damage: &'a DamageTracker,
        generation: u64,
    ) -> Result<Self, RenderContractError> {
        Self::new_with_selection(screen, damage, generation, None)
    }

    pub fn new_with_selection(
        screen: &'a TerminalScreen,
        damage: &'a DamageTracker,
        generation: u64,
        selection: Option<Selection>,
    ) -> Result<Self, RenderContractError> {
        if screen.size() != damage.size() {
            return Err(RenderContractError::FrameSizeMismatch);
        }
        if selection.is_some_and(|value| !value.is_within(screen.size())) {
            return Err(RenderContractError::SelectionOutOfBounds);
        }
        Ok(Self {
            size: screen.size(),
            cursor: screen.cursor(),
            cells: screen.grid().cells(),
            damage,
            selection,
            generation,
        })
    }

    pub const fn size(self) -> TerminalSize {
        self.size
    }
    pub const fn cursor(self) -> CursorPosition {
        self.cursor
    }
    pub const fn cells(self) -> &'a [Cell] {
        self.cells
    }
    pub const fn damage(self) -> &'a DamageTracker {
        self.damage
    }
    pub const fn selection(self) -> Option<Selection> {
        self.selection
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub fn row_cells(self, row: u16) -> Option<&'a [Cell]> {
        if row >= self.size.rows() {
            return None;
        }
        let columns = usize::from(self.size.columns());
        let start = usize::from(row) * columns;
        Some(&self.cells[start..start + columns])
    }
}

/// Backend-independent rendering boundary.
///
/// Implementations own their graphics resources. They borrow frame data only
/// during `render`, honor cell-space damage/clipping, and report failures
/// without mutating terminal state or terminating child processes.
pub trait Renderer {
    type Error: Error;

    fn resize(&mut self, size: TerminalSize, scale_factor: ScaleFactor) -> Result<(), Self::Error>;

    fn render(&mut self, frame: Frame<'_>, clip: Option<CellRect>) -> Result<(), Self::Error>;
}

mod software;
pub use software::{
    GlyphAtlas, GlyphBitmap, PixelSurface, RenderPalette, Rgba, SoftwareRenderError,
    SoftwareRenderer,
};

#[cfg(test)]
mod tests {
    use super::{CellRect, DamageTracker, Frame, GlyphMetrics, RenderContractError, ScaleFactor};
    use crate::screen::{CellAttributes, Color, GraphemeWidth, TerminalScreen};
    use crate::TerminalSize;

    fn size() -> TerminalSize {
        TerminalSize::new(80, 24).expect("valid size")
    }

    #[test]
    fn damage_tracker_starts_full_and_clear_enables_incremental_damage() {
        let mut damage = DamageTracker::new(size(), 4).expect("tracker");
        assert!(damage.is_full());
        assert!(damage.rectangles().is_empty());

        damage.clear();
        damage.mark_cell(2, 3).expect("mark cell");
        assert!(!damage.is_full());
        assert_eq!(damage.rectangles(), &[CellRect::new(2, 3, 1, 1).unwrap()]);
    }

    #[test]
    fn damage_rectangles_coalesce_when_adjacent_or_overlapping() {
        let mut damage = DamageTracker::new(size(), 8).expect("tracker");
        damage.clear();
        damage
            .mark_rect(CellRect::new(1, 1, 1, 4).unwrap())
            .unwrap();
        damage
            .mark_rect(CellRect::new(1, 5, 1, 3).unwrap())
            .unwrap();
        damage
            .mark_rect(CellRect::new(1, 3, 2, 2).unwrap())
            .unwrap();

        assert_eq!(damage.rectangles(), &[CellRect::new(1, 1, 2, 7).unwrap()]);
    }

    #[test]
    fn damage_budget_falls_back_to_full_frame_invalidation() {
        let mut damage = DamageTracker::new(size(), 1).expect("tracker");
        damage.clear();
        damage.mark_cell(0, 0).unwrap();
        damage.mark_cell(2, 2).unwrap();

        assert!(damage.is_full());
        assert!(damage.rectangles().is_empty());
    }

    #[test]
    fn resize_invalidates_entire_new_frame() {
        let mut damage = DamageTracker::new(size(), 4).expect("tracker");
        damage.clear();
        damage.mark_cell(0, 0).unwrap();
        let resized = TerminalSize::new(132, 43).expect("resized");
        damage.resize(resized);

        assert_eq!(damage.size(), resized);
        assert!(damage.is_full());
        assert!(damage.rectangles().is_empty());
    }

    #[test]
    fn rejects_out_of_bounds_damage_and_zero_extent() {
        let mut damage = DamageTracker::new(size(), 4).expect("tracker");
        damage.clear();
        assert_eq!(
            CellRect::new(0, 0, 0, 1),
            Err(RenderContractError::ZeroRectangleExtent)
        );
        assert_eq!(
            damage.mark_cell(24, 0),
            Err(RenderContractError::RectangleOutOfBounds)
        );
        assert_eq!(
            damage.mark_rect(CellRect::new(23, 79, 1, 2).unwrap()),
            Err(RenderContractError::RectangleOutOfBounds)
        );
    }

    #[test]
    fn frame_exposes_immutable_row_major_screen_snapshot() {
        let mut screen = TerminalScreen::new(size(), 80 * 24).expect("screen");
        screen
            .set_grapheme(
                0,
                0,
                "A",
                GraphemeWidth::Single,
                CellAttributes::new(Color::Default, Color::Default, Default::default()),
            )
            .expect("write cell");
        screen.set_cursor(2, 3);
        let damage = DamageTracker::new(size(), 16).expect("damage");
        let frame = Frame::new(&screen, &damage, 7).expect("frame");

        assert_eq!(frame.size(), size());
        assert_eq!(frame.cursor().row(), 2);
        assert_eq!(frame.cursor().column(), 3);
        assert_eq!(frame.generation(), 7);
        assert_eq!(frame.cells().len(), 80 * 24);
        assert_eq!(frame.row_cells(0).unwrap().len(), 80);
        assert!(frame.row_cells(24).is_none());
        assert_eq!(frame.cells()[0], *screen.get_cell(0, 0).unwrap());
    }

    #[test]
    fn frame_rejects_stale_damage_dimensions() {
        let screen = TerminalScreen::new(size(), 80 * 24).expect("screen");
        let resized = TerminalSize::new(100, 30).expect("resized");
        let damage = DamageTracker::new(resized, 16).expect("damage");

        assert_eq!(
            Frame::new(&screen, &damage, 0).map(|_| ()),
            Err(RenderContractError::FrameSizeMismatch)
        );
    }

    #[test]
    fn linear_selection_uses_inclusive_row_major_endpoints() {
        use super::{CellPosition, Selection, SelectionMode};

        let selection = Selection::new(
            CellPosition::new(1, 2),
            CellPosition::new(2, 1),
            SelectionMode::Linear,
            size(),
        )
        .expect("valid selection");

        assert!(!selection.contains(1, 1, size()));
        assert!(selection.contains(1, 2, size()));
        assert!(selection.contains(1, 79, size()));
        assert!(selection.contains(2, 0, size()));
        assert!(selection.contains(2, 1, size()));
        assert!(!selection.contains(2, 2, size()));
    }

    #[test]
    fn rectangular_selection_includes_the_bounded_rectangle() {
        use super::{CellPosition, Selection, SelectionMode};

        let selection = Selection::new(
            CellPosition::new(3, 7),
            CellPosition::new(1, 2),
            SelectionMode::Rectangular,
            size(),
        )
        .expect("valid selection");

        assert!(selection.contains(1, 2, size()));
        assert!(selection.contains(2, 5, size()));
        assert!(selection.contains(3, 7, size()));
        assert!(!selection.contains(0, 2, size()));
        assert!(!selection.contains(2, 8, size()));
    }

    #[test]
    fn selection_rejects_out_of_bounds_endpoints_and_frames() {
        use super::{CellPosition, Selection, SelectionMode};

        assert_eq!(
            Selection::new(
                CellPosition::new(24, 0),
                CellPosition::new(0, 0),
                SelectionMode::Linear,
                size(),
            ),
            Err(RenderContractError::SelectionOutOfBounds)
        );

        let screen = TerminalScreen::new(size(), 80 * 24).expect("screen");
        let damage = DamageTracker::new(size(), 8).expect("damage");
        let invalid = Selection {
            anchor: CellPosition::new(0, 0),
            extent: CellPosition::new(24, 0),
            mode: SelectionMode::Linear,
        };
        assert_eq!(
            Frame::new_with_selection(&screen, &damage, 1, Some(invalid)).map(|_| ()),
            Err(RenderContractError::SelectionOutOfBounds)
        );
    }

    #[test]
    fn validates_glyph_metrics_and_scale_factor() {
        let metrics = GlyphMetrics::new(8.0, 12.0, 4.0, 18.0).expect("metrics");
        assert_eq!(metrics.advance_width(), 8.0);
        assert!(GlyphMetrics::new(8.0, 12.0, 4.0, 15.0).is_err());
        assert!(GlyphMetrics::new(f32::NAN, 12.0, 4.0, 18.0).is_err());

        assert_eq!(ScaleFactor::new(1.5).unwrap().get(), 1.5);
        assert!(ScaleFactor::new(0.0).is_err());
        assert!(ScaleFactor::new(f32::INFINITY).is_err());
    }
}
