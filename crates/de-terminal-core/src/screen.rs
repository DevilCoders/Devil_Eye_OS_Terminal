//! Bounded terminal cell and screen-grid model.
//!
//! The grid stores whole grapheme clusters in fixed-size inline buffers. The
//! upstream input/parser layer supplies grapheme boundaries and display width;
//! the grid enforces storage, cell-pair, indexing, and allocation invariants.

use std::error::Error;
use std::fmt;

use crate::TerminalSize;

/// Maximum UTF-8 byte length stored for one grapheme cluster.
pub const MAX_GRAPHEME_BYTES: usize = 64;

/// Display width assigned to a grapheme by the Unicode input/parser layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphemeWidth {
    Single,
    Wide,
}

/// A complete UTF-8 grapheme cluster stored inline in one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grapheme {
    bytes: [u8; MAX_GRAPHEME_BYTES],
    length: u8,
}

impl Grapheme {
    pub fn new(value: &str) -> Result<Self, ScreenGridError> {
        if value.is_empty() {
            return Err(ScreenGridError::EmptyGrapheme);
        }
        if value.len() > MAX_GRAPHEME_BYTES {
            return Err(ScreenGridError::GraphemeTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(ScreenGridError::ControlCharacterInGrapheme);
        }

        let mut bytes = [0u8; MAX_GRAPHEME_BYTES];
        bytes[..value.len()].copy_from_slice(value.as_bytes());

        Ok(Self {
            bytes,
            length: value.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.length)])
            .expect("Grapheme stores validated UTF-8")
    }

    pub const fn byte_length(self) -> usize {
        self.length as usize
    }
}

/// Terminal foreground/background color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb {
        red: u8,
        green: u8,
        blue: u8,
    },
}

/// Compact, composable terminal text-style flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StyleFlags(u16);

impl StyleFlags {
    pub const BOLD: Self = Self(1u16 << 0u32);
    pub const DIM: Self = Self(1u16 << 1u32);
    pub const ITALIC: Self = Self(1u16 << 2u32);
    pub const UNDERLINE: Self = Self(1u16 << 3u32);
    pub const BLINK: Self = Self(1u16 << 4u32);
    pub const INVERSE: Self = Self(1u16 << 5u32);
    pub const HIDDEN: Self = Self(1u16 << 6u32);
    pub const STRIKETHROUGH: Self = Self(1u16 << 7u32);

    pub const fn empty() -> Self {
        Self(0u16)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }

    pub const fn with(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }

    pub const fn without(self, flag: Self) -> Self {
        Self(self.0 & !flag.0)
    }
}

/// Attributes applied to a cell's grapheme or empty background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttributes {
    foreground: Color,
    background: Color,
    styles: StyleFlags,
}

impl CellAttributes {
    pub const fn new(foreground: Color, background: Color, styles: StyleFlags) -> Self {
        Self {
            foreground,
            background,
            styles,
        }
    }

    pub const fn foreground(self) -> Color {
        self.foreground
    }

    pub const fn background(self) -> Color {
        self.background
    }

    pub const fn styles(self) -> StyleFlags {
        self.styles
    }
}

/// Logical content stored in a terminal cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellContent {
    #[default]
    Empty,
    Grapheme {
        value: Grapheme,
        width: GraphemeWidth,
    },
    /// Trailing cell occupied by the wide grapheme immediately to its left.
    Continuation,
}

/// One character cell with terminal attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    content: CellContent,
    attributes: CellAttributes,
}

impl Cell {
    pub const fn content(self) -> CellContent {
        self.content
    }

    pub const fn attributes(self) -> CellAttributes {
        self.attributes
    }

    pub const fn is_empty(self) -> bool {
        matches!(self.content, CellContent::Empty)
    }

    pub const fn is_continuation(self) -> bool {
        matches!(self.content, CellContent::Continuation)
    }
}

/// Failures while constructing or mutating a bounded screen grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenGridError {
    ZeroCellLimit,
    CapacityExceeded,
    DimensionOverflow,
    AllocationFailed,
    OutOfBounds,
    EmptyGrapheme,
    GraphemeTooLong,
    ControlCharacterInGrapheme,
    WideGraphemeAtRightEdge,
    InvalidScrollRegion,
}

impl fmt::Display for ScreenGridError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroCellLimit => "screen cell limit must be non-zero",
            Self::CapacityExceeded => "screen dimensions exceed the configured cell limit",
            Self::DimensionOverflow => "screen dimensions overflow addressable memory",
            Self::AllocationFailed => "screen cell allocation failed",
            Self::OutOfBounds => "screen row or column is out of bounds",
            Self::EmptyGrapheme => "grapheme must not be empty",
            Self::GraphemeTooLong => "grapheme exceeds the inline byte limit",
            Self::ControlCharacterInGrapheme => "grapheme contains a control character",
            Self::WideGraphemeAtRightEdge => "wide grapheme cannot start at the final column",
            Self::InvalidScrollRegion => "scroll region bounds are invalid for the screen",
        };

        formatter.write_str(message)
    }
}

impl Error for ScreenGridError {}

/// A fixed-size, row-major terminal screen with a hard cell-count limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenGrid {
    size: TerminalSize,
    cells: Vec<Cell>,
    max_cells: usize,
}

impl ScreenGrid {
    /// Allocates a screen only if its dimensions fit within max_cells.
    pub fn new(size: TerminalSize, max_cells: usize) -> Result<Self, ScreenGridError> {
        if max_cells == 0usize {
            return Err(ScreenGridError::ZeroCellLimit);
        }

        let count = cell_count(size)?;
        if count > max_cells {
            return Err(ScreenGridError::CapacityExceeded);
        }

        Ok(Self {
            size,
            cells: allocate_cells(count)?,
            max_cells,
        })
    }

    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    pub const fn max_cells(&self) -> usize {
        self.max_cells
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Returns a cell if the row and column are within the current dimensions.
    pub fn get_cell(&self, row: u16, column: u16) -> Option<&Cell> {
        self.index(row, column).map(|index| &self.cells[index])
    }

    /// Returns an immutable slice containing one complete row.
    pub fn row_cells(&self, row: u16) -> Option<&[Cell]> {
        if row >= self.size.rows() {
            return None;
        }

        let columns = usize::from(self.size.columns());
        let start = usize::from(row) * columns;
        Some(&self.cells[start..start + columns])
    }

    /// Places one complete grapheme and maintains wide-cell continuation pairs.
    pub fn set_grapheme(
        &mut self,
        row: u16,
        column: u16,
        value: &str,
        width: GraphemeWidth,
        attributes: CellAttributes,
    ) -> Result<(), ScreenGridError> {
        let index = self
            .index(row, column)
            .ok_or(ScreenGridError::OutOfBounds)?;
        let grapheme = Grapheme::new(value)?;

        if width == GraphemeWidth::Wide && column + 1u16 >= self.size.columns() {
            return Err(ScreenGridError::WideGraphemeAtRightEdge);
        }

        self.clear_cell_pair(index);
        if width == GraphemeWidth::Wide {
            self.clear_cell_pair(index + 1usize);
        }

        self.cells[index] = Cell {
            content: CellContent::Grapheme {
                value: grapheme,
                width,
            },
            attributes,
        };

        if width == GraphemeWidth::Wide {
            self.cells[index + 1usize] = Cell {
                content: CellContent::Continuation,
                attributes,
            };
        }

        Ok(())
    }

    /// Clears a cell and the other half of a wide-grapheme pair, if present.
    pub fn set_empty(&mut self, row: u16, column: u16) -> Result<(), ScreenGridError> {
        let index = self
            .index(row, column)
            .ok_or(ScreenGridError::OutOfBounds)?;
        self.clear_cell_pair(index);
        Ok(())
    }

    /// Resizes atomically, preserving the top-left intersection of the old grid.
    ///
    /// If the new right edge cuts through a wide grapheme, that grapheme is
    /// cleared rather than leaving an orphaned leading or continuation cell.
    pub fn resize(&mut self, new_size: TerminalSize) -> Result<(), ScreenGridError> {
        let new_count = cell_count(new_size)?;
        if new_count > self.max_cells {
            return Err(ScreenGridError::CapacityExceeded);
        }

        let mut new_cells = allocate_cells(new_count)?;
        let copy_rows = self.size.rows().min(new_size.rows());
        let copy_columns = self.size.columns().min(new_size.columns());
        let old_columns = usize::from(self.size.columns());
        let new_columns = usize::from(new_size.columns());

        for row in 0..usize::from(copy_rows) {
            for column in 0..usize::from(copy_columns) {
                let old_index = row * old_columns + column;
                let new_index = row * new_columns + column;
                new_cells[new_index] = self.cells[old_index];
            }

            if new_size.columns() < self.size.columns() {
                let edge_index = row * new_columns + new_columns - 1usize;
                if matches!(
                    new_cells[edge_index].content,
                    CellContent::Grapheme {
                        width: GraphemeWidth::Wide,
                        ..
                    }
                ) {
                    new_cells[edge_index] = Cell::default();
                }
            }
        }

        self.cells = new_cells;
        self.size = new_size;
        Ok(())
    }

    fn index(&self, row: u16, column: u16) -> Option<usize> {
        if row >= self.size.rows() || column >= self.size.columns() {
            return None;
        }

        Some(usize::from(row) * usize::from(self.size.columns()) + usize::from(column))
    }

    fn scroll_rows(
        &mut self,
        top: u16,
        bottom: u16,
        count: u16,
        downward: bool,
    ) -> Result<(), ScreenGridError> {
        if top > bottom || bottom >= self.size.rows() {
            return Err(ScreenGridError::InvalidScrollRegion);
        }
        if count == 0u16 {
            return Ok(());
        }

        let columns = usize::from(self.size.columns());
        let top_index = usize::from(top) * columns;
        let end_index = (usize::from(bottom) + 1usize) * columns;
        let region_rows = usize::from(bottom - top) + 1usize;
        let shift_rows = usize::from(count).min(region_rows);
        let shift_cells = shift_rows * columns;

        if shift_rows == region_rows {
            self.cells[top_index..end_index].fill(Cell::default());
        } else if downward {
            self.cells
                .copy_within(top_index..end_index - shift_cells, top_index + shift_cells);
            self.cells[top_index..top_index + shift_cells].fill(Cell::default());
        } else {
            self.cells
                .copy_within(top_index + shift_cells..end_index, top_index);
            self.cells[end_index - shift_cells..end_index].fill(Cell::default());
        }

        Ok(())
    }

    fn clear_cell_pair(&mut self, index: usize) {
        let columns = usize::from(self.size.columns());
        let column = index % columns;

        match self.cells[index].content {
            CellContent::Grapheme {
                width: GraphemeWidth::Wide,
                ..
            } if column + 1usize < columns => {
                self.cells[index + 1usize] = Cell::default();
            }
            CellContent::Continuation if column > 0usize => {
                let previous = index - 1usize;
                if matches!(
                    self.cells[previous].content,
                    CellContent::Grapheme {
                        width: GraphemeWidth::Wide,
                        ..
                    }
                ) {
                    self.cells[previous] = Cell::default();
                }
            }
            _ => {}
        }

        self.cells[index] = Cell::default();
    }
}

fn cell_count(size: TerminalSize) -> Result<usize, ScreenGridError> {
    usize::from(size.columns())
        .checked_mul(usize::from(size.rows()))
        .ok_or(ScreenGridError::DimensionOverflow)
}

fn allocate_cells(count: usize) -> Result<Vec<Cell>, ScreenGridError> {
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(count)
        .map_err(|_| ScreenGridError::AllocationFailed)?;
    cells.resize(count, Cell::default());
    Ok(cells)
}

/// Zero-based terminal cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPosition {
    row: u16,
    column: u16,
}

impl CursorPosition {
    pub const fn row(self) -> u16 {
        self.row
    }

    pub const fn column(self) -> u16 {
        self.column
    }
}

/// Inclusive top and bottom rows of the active scrolling region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollRegion {
    top: u16,
    bottom: u16,
}

impl ScrollRegion {
    pub const fn top(self) -> u16 {
        self.top
    }

    pub const fn bottom(self) -> u16 {
        self.bottom
    }

    const fn full(size: TerminalSize) -> Self {
        Self {
            top: 0u16,
            bottom: size.rows() - 1u16,
        }
    }
}

/// Range affected by a terminal erase operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseMode {
    /// Erase from the cursor through the end of the selected row or display.
    ToEnd,
    /// Erase from the beginning through the cursor, inclusive.
    ToBeginning,
    /// Erase the complete selected row or display.
    All,
}

/// Bounded ring buffer of complete rows displaced by full-screen scrolling.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScrollbackBuffer {
    cells: Vec<Cell>,
    max_cells: usize,
    columns: u16,
    max_rows: usize,
    row_count: usize,
    start_row: usize,
}

impl ScrollbackBuffer {
    fn new(columns: u16, max_cells: usize) -> Result<Self, ScreenGridError> {
        let columns_usize = usize::from(columns);
        let max_rows = max_cells / columns_usize;
        let capacity_cells = max_rows
            .checked_mul(columns_usize)
            .ok_or(ScreenGridError::DimensionOverflow)?;
        Ok(Self {
            cells: allocate_cells(capacity_cells)?,
            max_cells,
            columns,
            max_rows,
            row_count: 0usize,
            start_row: 0usize,
        })
    }

    fn cell_count(&self) -> usize {
        self.row_count * usize::from(self.columns)
    }

    fn clear(&mut self) {
        self.cells.fill(Cell::default());
        self.row_count = 0usize;
        self.start_row = 0usize;
    }

    fn row(&self, chronological_index: usize) -> Option<&[Cell]> {
        if chronological_index >= self.row_count || self.max_rows == 0usize {
            return None;
        }
        let slot = (self.start_row + chronological_index) % self.max_rows;
        let columns = usize::from(self.columns);
        let start = slot * columns;
        Some(&self.cells[start..start + columns])
    }

    fn push_row(&mut self, source: &[Cell]) {
        let slot = if self.row_count < self.max_rows {
            let slot = (self.start_row + self.row_count) % self.max_rows;
            self.row_count += 1usize;
            slot
        } else {
            let slot = self.start_row;
            self.start_row = (self.start_row + 1usize) % self.max_rows;
            slot
        };
        let columns = usize::from(self.columns);
        let start = slot * columns;
        self.cells[start..start + columns].copy_from_slice(source);
    }

    fn append_from_grid(
        &mut self,
        grid: &ScreenGrid,
        first_row: u16,
        count: u16,
    ) -> Result<(), ScreenGridError> {
        if count == 0u16 || self.max_rows == 0usize {
            return Ok(());
        }
        let end_row = first_row
            .checked_add(count)
            .ok_or(ScreenGridError::DimensionOverflow)?;
        if end_row > grid.size().rows() {
            return Err(ScreenGridError::OutOfBounds);
        }

        // If more rows scroll out than history can retain, only the newest
        // rows can survive the operation. Avoid copying rows that are evicted
        // within the same scroll operation.
        let rows_to_capture = usize::from(count).min(self.max_rows);
        let first_offset = usize::from(count) - rows_to_capture;
        let first_captured = first_row
            .checked_add(
                u16::try_from(first_offset).map_err(|_| ScreenGridError::DimensionOverflow)?,
            )
            .ok_or(ScreenGridError::DimensionOverflow)?;
        for row_index in first_captured..end_row {
            let source = grid
                .row_cells(row_index)
                .ok_or(ScreenGridError::OutOfBounds)?;
            self.push_row(source);
        }
        Ok(())
    }
}

/// Screen grid, cursor, scrolling-region state, and optional bounded scrollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalScreen {
    grid: ScreenGrid,
    cursor: CursorPosition,
    scroll_region: ScrollRegion,
    scrollback: ScrollbackBuffer,
}

impl TerminalScreen {
    /// Creates a screen with scrollback disabled for compatibility and minimal memory use.
    pub fn new(size: TerminalSize, max_cells: usize) -> Result<Self, ScreenGridError> {
        Self::new_with_scrollback(size, max_cells, 0usize)
    }

    /// Creates a screen with a hard scrollback payload limit expressed in cells.
    ///
    /// Rows are retained at their original column width. Changing the screen's
    /// column count clears history rather than silently reflowing old rows.
    /// The effective row capacity is `max_scrollback_cells / columns`; any
    /// remainder is unused. Cell storage is preallocated at construction.
    pub fn new_with_scrollback(
        size: TerminalSize,
        max_cells: usize,
        max_scrollback_cells: usize,
    ) -> Result<Self, ScreenGridError> {
        Ok(Self {
            grid: ScreenGrid::new(size, max_cells)?,
            cursor: CursorPosition::default(),
            scroll_region: ScrollRegion::full(size),
            scrollback: ScrollbackBuffer::new(size.columns(), max_scrollback_cells)?,
        })
    }

    pub const fn size(&self) -> TerminalSize {
        self.grid.size()
    }

    pub const fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    pub const fn scroll_region(&self) -> ScrollRegion {
        self.scroll_region
    }

    /// Returns a read-only grid view; dimension changes must go through TerminalScreen::resize.
    pub const fn grid(&self) -> &ScreenGrid {
        &self.grid
    }

    /// Returns retained rows from oldest to newest. Each row has the width
    /// that was active when it entered history; no implicit reflow is applied.
    pub fn scrollback_rows(&self) -> impl Iterator<Item = &[Cell]> {
        let mut index = 0usize;
        std::iter::from_fn(move || {
            let row = self.scrollback.row(index)?;
            index += 1usize;
            Some(row)
        })
    }

    /// Returns the number of retained rows.
    pub fn scrollback_row_count(&self) -> usize {
        self.scrollback.rows.len()
    }

    /// Returns the number of retained cell slots.
    pub fn scrollback_cell_count(&self) -> usize {
        self.scrollback.cell_count()
    }

    /// Returns the configured maximum retained cell slots.
    pub const fn max_scrollback_cells(&self) -> usize {
        self.scrollback.max_cells
    }

    /// Drops all retained history without changing the visible screen.
    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
    }

    /// Writes one grapheme without exposing mutable access to grid dimensions.
    pub fn set_grapheme(
        &mut self,
        row: u16,
        column: u16,
        value: &str,
        width: GraphemeWidth,
        attributes: CellAttributes,
    ) -> Result<(), ScreenGridError> {
        self.grid
            .set_grapheme(row, column, value, width, attributes)
    }

    /// Clears one cell while preserving wide-grapheme pair invariants.
    pub fn set_empty(&mut self, row: u16, column: u16) -> Result<(), ScreenGridError> {
        self.grid.set_empty(row, column)
    }

    pub fn get_cell(&self, row: u16, column: u16) -> Option<&Cell> {
        self.grid.get_cell(row, column)
    }

    /// Sets the cursor, clamping both coordinates to the current screen bounds.
    pub fn set_cursor(&mut self, row: u16, column: u16) {
        self.cursor.row = row.min(self.size().rows() - 1u16);
        self.cursor.column = column.min(self.size().columns() - 1u16);
    }

    pub fn move_cursor_up(&mut self, count: u16) {
        self.cursor.row = self.cursor.row.saturating_sub(count);
    }

    pub fn move_cursor_down(&mut self, count: u16) {
        self.cursor.row = self
            .cursor
            .row
            .saturating_add(count)
            .min(self.size().rows() - 1u16);
    }

    pub fn move_cursor_left(&mut self, count: u16) {
        self.cursor.column = self.cursor.column.saturating_sub(count);
    }

    pub fn move_cursor_right(&mut self, count: u16) {
        self.cursor.column = self
            .cursor
            .column
            .saturating_add(count)
            .min(self.size().columns() - 1u16);
    }

    /// Implements carriage return: move to column zero without changing row.
    pub fn carriage_return(&mut self) {
        self.cursor.column = 0u16;
    }

    /// Implements line feed, scrolling the active region when the cursor is at its bottom.
    pub fn line_feed(&mut self) -> Result<(), ScreenGridError> {
        if self.cursor.row == self.scroll_region.bottom {
            self.scroll_up(1u16)?;
        } else if self.cursor.row < self.size().rows() - 1u16 {
            self.cursor.row += 1u16;
        }
        Ok(())
    }

    /// Sets an inclusive scrolling region. The top must not exceed the bottom.
    /// A one-row region is valid and scrolls by clearing that row on line feed.
    pub fn set_scroll_region(&mut self, top: u16, bottom: u16) -> Result<(), ScreenGridError> {
        if top > bottom || bottom >= self.size().rows() {
            return Err(ScreenGridError::InvalidScrollRegion);
        }
        self.scroll_region = ScrollRegion { top, bottom };
        Ok(())
    }

    pub fn reset_scroll_region(&mut self) {
        self.scroll_region = ScrollRegion::full(self.size());
    }

    pub fn scroll_up(&mut self, count: u16) -> Result<(), ScreenGridError> {
        if count > 0u16
            && self.scroll_region.top == 0u16
            && self.scroll_region.bottom == self.size().rows() - 1u16
        {
            self.scrollback
                .append_from_grid(&self.grid, 0u16, count.min(self.size().rows()))?;
        }
        self.grid.scroll_rows(
            self.scroll_region.top,
            self.scroll_region.bottom,
            count,
            false,
        )
    }

    pub fn scroll_down(&mut self, count: u16) -> Result<(), ScreenGridError> {
        self.grid.scroll_rows(
            self.scroll_region.top,
            self.scroll_region.bottom,
            count,
            true,
        )
    }

    /// Erases cells on the cursor row without moving the cursor.
    pub fn erase_line(&mut self, mode: EraseMode) -> Result<(), ScreenGridError> {
        let columns = self.size().columns();
        let (start, end) = match mode {
            EraseMode::ToEnd => (self.cursor.column, columns - 1u16),
            EraseMode::ToBeginning => (0u16, self.cursor.column),
            EraseMode::All => (0u16, columns - 1u16),
        };

        for column in start..=end {
            self.grid.set_empty(self.cursor.row, column)?;
        }
        Ok(())
    }

    /// Erases the display range selected by mode without moving the cursor.
    pub fn erase_display(&mut self, mode: EraseMode) -> Result<(), ScreenGridError> {
        let size = self.size();
        let start = match mode {
            EraseMode::ToEnd => (self.cursor.row, self.cursor.column),
            EraseMode::ToBeginning => (0u16, 0u16),
            EraseMode::All => (0u16, 0u16),
        };
        let end = match mode {
            EraseMode::ToEnd => (size.rows() - 1u16, size.columns() - 1u16),
            EraseMode::ToBeginning => (self.cursor.row, self.cursor.column),
            EraseMode::All => (size.rows() - 1u16, size.columns() - 1u16),
        };

        for row in start.0..=end.0 {
            let first_column = if row == start.0 { start.1 } else { 0u16 };
            let last_column = if row == end.0 {
                end.1
            } else {
                size.columns() - 1u16
            };
            for column in first_column..=last_column {
                self.grid.set_empty(row, column)?;
            }
        }
        Ok(())
    }

    /// Resizes the grid, clamps the cursor, and preserves/clamps the scrolling region.
    ///
    /// If the old region collapses after shrinking, the active region resets to the full screen.
    pub fn resize(&mut self, size: TerminalSize) -> Result<(), ScreenGridError> {
        let resized_scrollback = if self.scrollback.columns != size.columns() {
            Some(ScrollbackBuffer::new(
                size.columns(),
                self.scrollback.max_cells,
            )?)
        } else {
            None
        };
        self.grid.resize(size)?;
        if let Some(scrollback) = resized_scrollback {
            self.scrollback = scrollback;
        }
        self.cursor.row = self.cursor.row.min(size.rows() - 1u16);
        self.cursor.column = self.cursor.column.min(size.columns() - 1u16);

        let top = self.scroll_region.top.min(size.rows() - 1u16);
        let bottom = self.scroll_region.bottom.min(size.rows() - 1u16);
        self.scroll_region = if top > bottom {
            ScrollRegion::full(size)
        } else {
            ScrollRegion { top, bottom }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CellAttributes, CellContent, Color, CursorPosition, EraseMode, Grapheme, GraphemeWidth,
        ScreenGrid, ScreenGridError, ScrollRegion, StyleFlags, TerminalScreen, MAX_GRAPHEME_BYTES,
    };
    use crate::TerminalSize;

    fn size(columns: u16, rows: u16) -> TerminalSize {
        TerminalSize::new(columns, rows).expect("non-zero terminal dimensions")
    }

    #[test]
    fn cursor_movement_is_clamped_to_screen_bounds() {
        let mut screen = TerminalScreen::new(size(5u16, 3u16), 15usize).expect("screen");
        screen.set_cursor(99u16, 99u16);
        assert_eq!(
            screen.cursor(),
            CursorPosition {
                row: 2u16,
                column: 4u16
            }
        );

        screen.move_cursor_down(u16::MAX);
        screen.move_cursor_right(u16::MAX);
        assert_eq!(
            screen.cursor(),
            CursorPosition {
                row: 2u16,
                column: 4u16
            }
        );

        screen.move_cursor_up(u16::MAX);
        screen.move_cursor_left(u16::MAX);
        assert_eq!(screen.cursor(), CursorPosition::default());
    }

    #[test]
    fn carriage_return_and_line_feed_update_cursor() {
        let mut screen = TerminalScreen::new(size(5u16, 3u16), 15usize).expect("screen");
        screen.set_cursor(1u16, 4u16);
        screen.carriage_return();
        assert_eq!(
            screen.cursor(),
            CursorPosition {
                row: 1u16,
                column: 0u16
            }
        );
        screen.line_feed().expect("line feed");
        assert_eq!(
            screen.cursor(),
            CursorPosition {
                row: 2u16,
                column: 0u16
            }
        );
        screen.line_feed().expect("line feed at bottom");
        assert_eq!(screen.cursor().row(), 2u16);
    }

    #[test]
    fn line_feed_scrolls_only_the_active_region() {
        let mut screen = TerminalScreen::new(size(2u16, 4u16), 8usize).expect("screen");
        for (row, value) in [(0u16, "A"), (1u16, "B"), (2u16, "C"), (3u16, "D")] {
            screen
                .set_grapheme(
                    row,
                    0u16,
                    value,
                    GraphemeWidth::Single,
                    CellAttributes::default(),
                )
                .expect("write row marker");
        }
        screen.set_scroll_region(1u16, 2u16).expect("scroll region");
        screen.set_cursor(2u16, 0u16);
        screen.line_feed().expect("scroll at region bottom");

        let row_text = |row| match screen.grid().get_cell(row, 0u16).expect("cell").content() {
            CellContent::Grapheme { value, .. } => value.as_str().to_owned(),
            CellContent::Empty => String::new(),
            CellContent::Continuation => panic!("unexpected continuation"),
        };
        assert_eq!(row_text(0u16), "A");
        assert_eq!(row_text(1u16), "C");
        assert_eq!(row_text(2u16), "");
        assert_eq!(row_text(3u16), "D");
        assert_eq!(screen.cursor().row(), 2u16);
    }

    #[test]
    fn full_screen_scroll_retains_complete_rows_in_chronological_order() {
        let mut screen = TerminalScreen::new_with_scrollback(size(3u16, 2u16), 6usize, 6usize)
            .expect("screen with scrollback");
        for (row, value) in [(0u16, "A"), (1u16, "B")] {
            screen
                .set_grapheme(
                    row,
                    0u16,
                    value,
                    GraphemeWidth::Single,
                    CellAttributes::default(),
                )
                .expect("write row marker");
        }

        screen.scroll_up(1u16).expect("scroll full screen");

        let history: Vec<_> = screen
            .scrollback_rows()
            .map(|row| match row[0].content() {
                CellContent::Grapheme { value, .. } => value.as_str().to_owned(),
                other => panic!("unexpected retained cell: {other:?}"),
            })
            .collect();
        assert_eq!(history, vec!["A".to_owned()]);
        assert_eq!(screen.scrollback_cell_count(), 3usize);
        assert_eq!(
            screen
                .grid()
                .get_cell(0u16, 0u16)
                .expect("visible top")
                .content(),
            CellContent::Grapheme {
                value: Grapheme::new("B").expect("grapheme"),
                width: GraphemeWidth::Single,
            }
        );
    }

    #[test]
    fn scrollback_evicts_oldest_rows_and_never_exceeds_cell_limit() {
        let mut screen = TerminalScreen::new_with_scrollback(size(2u16, 2u16), 4usize, 4usize)
            .expect("screen with bounded scrollback");
        for (row, value) in [(0u16, "A"), (1u16, "B")] {
            screen
                .set_grapheme(
                    row,
                    0u16,
                    value,
                    GraphemeWidth::Single,
                    CellAttributes::default(),
                )
                .expect("write row marker");
        }

        screen.scroll_up(2u16).expect("scroll two rows");
        assert_eq!(screen.scrollback_row_count(), 2usize);
        assert_eq!(screen.scrollback_cell_count(), 4usize);

        screen
            .set_grapheme(
                1u16,
                0u16,
                "C",
                GraphemeWidth::Single,
                CellAttributes::default(),
            )
            .expect("write next row");
        screen.scroll_up(1u16).expect("scroll next row");

        let history: Vec<_> = screen
            .scrollback_rows()
            .map(|row| match row[0].content() {
                CellContent::Grapheme { value, .. } => value.as_str().to_owned(),
                CellContent::Empty => String::new(),
                CellContent::Continuation => panic!("unexpected continuation"),
            })
            .collect();
        assert_eq!(history, vec!["B".to_owned(), String::new()]);
        assert!(screen.scrollback_cell_count() <= screen.max_scrollback_cells());
    }

    #[test]
    fn partial_region_and_downward_scroll_do_not_append_history() {
        let mut screen = TerminalScreen::new_with_scrollback(size(2u16, 4u16), 8usize, 8usize)
            .expect("screen with scrollback");
        for (row, value) in [(0u16, "A"), (1u16, "B"), (2u16, "C"), (3u16, "D")] {
            screen
                .set_grapheme(
                    row,
                    0u16,
                    value,
                    GraphemeWidth::Single,
                    CellAttributes::default(),
                )
                .expect("write row marker");
        }

        screen
            .set_scroll_region(1u16, 2u16)
            .expect("partial region");
        screen.scroll_up(1u16).expect("partial scroll");
        screen.scroll_down(1u16).expect("partial scroll down");
        assert_eq!(screen.scrollback_row_count(), 0usize);

        screen.reset_scroll_region();
        screen.scroll_down(1u16).expect("full-screen scroll down");
        assert_eq!(screen.scrollback_row_count(), 0usize);
    }

    #[test]
    fn line_feed_at_full_screen_bottom_retains_top_row() {
        let mut screen = TerminalScreen::new_with_scrollback(size(2u16, 2u16), 4usize, 4usize)
            .expect("screen with scrollback");
        screen
            .set_grapheme(
                0u16,
                0u16,
                "A",
                GraphemeWidth::Single,
                CellAttributes::default(),
            )
            .expect("write first row");
        screen.set_cursor(1u16, 0u16);
        screen.line_feed().expect("line feed at bottom");
        assert_eq!(screen.scrollback_row_count(), 1usize);
        assert!(matches!(
            screen.scrollback_rows().next().expect("history row")[0].content(),
            CellContent::Grapheme { value, .. } if value.as_str() == "A"
        ));
    }

    #[test]
    fn wide_grapheme_pairs_are_preserved_in_scrollback_rows() {
        let mut screen = TerminalScreen::new_with_scrollback(size(3u16, 2u16), 6usize, 6usize)
            .expect("screen with scrollback");
        screen
            .set_grapheme(
                0u16,
                0u16,
                "界",
                GraphemeWidth::Wide,
                CellAttributes::default(),
            )
            .expect("write wide grapheme");
        screen.scroll_up(1u16).expect("scroll full screen");
        let row = screen.scrollback_rows().next().expect("retained row");
        assert!(matches!(
            row[0].content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Wide,
                ..
            }
        ));
        assert!(matches!(row[1].content(), CellContent::Continuation));
    }

    #[test]
    fn scrollback_is_disabled_at_zero_capacity_and_clears_explicitly() {
        let mut disabled =
            TerminalScreen::new(size(2u16, 2u16), 4usize).expect("screen without scrollback");
        disabled.scroll_up(1u16).expect("scroll without history");
        assert_eq!(disabled.scrollback_row_count(), 0usize);
        assert_eq!(disabled.max_scrollback_cells(), 0usize);

        let mut enabled = TerminalScreen::new_with_scrollback(size(2u16, 2u16), 4usize, 4usize)
            .expect("screen with history");
        enabled
            .set_grapheme(
                0u16,
                0u16,
                "A",
                GraphemeWidth::Single,
                CellAttributes::default(),
            )
            .expect("write row");
        enabled.scroll_up(1u16).expect("scroll");
        let visible_before_clear = enabled.grid().cells().to_vec();
        enabled.clear_scrollback();
        assert_eq!(enabled.scrollback_row_count(), 0usize);
        assert_eq!(enabled.scrollback_cell_count(), 0usize);
        assert_eq!(enabled.grid().cells(), visible_before_clear);
    }

    #[test]
    fn column_resize_clears_scrollback_without_reflow_but_row_resize_preserves_it() {
        let mut screen = TerminalScreen::new_with_scrollback(size(2u16, 2u16), 8usize, 8usize)
            .expect("screen with scrollback");
        screen
            .set_grapheme(
                0u16,
                0u16,
                "A",
                GraphemeWidth::Single,
                CellAttributes::default(),
            )
            .expect("write row");
        screen.scroll_up(1u16).expect("scroll");
        assert_eq!(screen.scrollback_row_count(), 1usize);

        screen.resize(size(2u16, 3u16)).expect("resize row count");
        assert_eq!(screen.scrollback_row_count(), 1usize);

        screen.resize(size(3u16, 3u16)).expect("resize columns");
        assert_eq!(screen.scrollback_row_count(), 0usize);
    }

    #[test]
    fn rejects_unallocatable_scrollback_capacity_without_panicking() {
        assert_eq!(
            TerminalScreen::new_with_scrollback(size(1u16, 1u16), 1usize, usize::MAX)
                .err(),
            Some(ScreenGridError::AllocationFailed)
        );
    }

    #[test]
    fn scrollback_capacity_smaller_than_one_row_retains_nothing() {
        let mut screen = TerminalScreen::new_with_scrollback(size(4u16, 2u16), 8usize, 3usize)
            .expect("screen with sub-row history capacity");
        screen.scroll_up(1u16).expect("scroll");
        assert_eq!(screen.scrollback_row_count(), 0usize);
        assert_eq!(screen.scrollback_cell_count(), 0usize);
    }

    #[test]
    fn scroll_down_moves_rows_and_clears_new_top_rows() {
        let mut screen = TerminalScreen::new(size(2u16, 4u16), 8usize).expect("screen");
        for (row, value) in [(1u16, "B"), (2u16, "C")] {
            screen
                .set_grapheme(
                    row,
                    0u16,
                    value,
                    GraphemeWidth::Single,
                    CellAttributes::default(),
                )
                .expect("write row marker");
        }
        screen.set_scroll_region(1u16, 2u16).expect("scroll region");
        screen.scroll_down(1u16).expect("scroll down");
        assert!(screen
            .grid()
            .get_cell(1u16, 0u16)
            .expect("new top")
            .is_empty());
        match screen
            .grid()
            .get_cell(2u16, 0u16)
            .expect("moved row")
            .content()
        {
            CellContent::Grapheme { value, .. } => assert_eq!(value.as_str(), "B"),
            other => panic!("unexpected cell after scroll: {other:?}"),
        }
    }

    #[test]
    fn erase_line_and_display_modes_clear_exact_ranges() {
        let mut screen = TerminalScreen::new(size(4u16, 3u16), 12usize).expect("screen");
        for row in 0u16..3u16 {
            for column in 0u16..4u16 {
                screen
                    .set_grapheme(
                        row,
                        column,
                        "x",
                        GraphemeWidth::Single,
                        CellAttributes::default(),
                    )
                    .expect("write cell");
            }
        }
        screen.set_cursor(1u16, 2u16);
        screen
            .erase_line(EraseMode::ToBeginning)
            .expect("erase line prefix");
        assert!(screen.grid().get_cell(1u16, 0u16).expect("cell").is_empty());
        assert!(screen.grid().get_cell(1u16, 2u16).expect("cell").is_empty());
        assert!(!screen.grid().get_cell(1u16, 3u16).expect("cell").is_empty());

        screen
            .erase_display(EraseMode::ToEnd)
            .expect("erase display suffix");
        assert!(!screen
            .grid()
            .get_cell(0u16, 3u16)
            .expect("above cursor")
            .is_empty());
        assert!(screen
            .grid()
            .get_cell(1u16, 3u16)
            .expect("cursor row suffix")
            .is_empty());
        assert!(screen
            .grid()
            .get_cell(2u16, 0u16)
            .expect("below cursor")
            .is_empty());

        screen
            .erase_display(EraseMode::All)
            .expect("erase full display");
        assert!(screen.grid().cells().iter().all(|cell| cell.is_empty()));
    }

    #[test]
    fn invalid_scroll_region_is_rejected_without_changing_existing_region() {
        let mut screen = TerminalScreen::new(size(4u16, 3u16), 12usize).expect("screen");
        let original = screen.scroll_region();
        assert_eq!(
            screen.set_scroll_region(2u16, 1u16),
            Err(ScreenGridError::InvalidScrollRegion)
        );
        assert_eq!(
            screen.set_scroll_region(0u16, 3u16),
            Err(ScreenGridError::InvalidScrollRegion)
        );
        assert_eq!(screen.scroll_region(), original);
    }

    #[test]
    fn resize_clamps_cursor_and_preserves_valid_scroll_region() {
        let mut screen = TerminalScreen::new(size(5u16, 5u16), 25usize).expect("screen");
        screen.set_cursor(4u16, 4u16);
        screen.set_scroll_region(1u16, 4u16).expect("scroll region");
        screen.resize(size(3u16, 3u16)).expect("shrink screen");
        assert_eq!(
            screen.cursor(),
            CursorPosition {
                row: 2u16,
                column: 2u16
            }
        );
        assert_eq!(
            screen.scroll_region(),
            ScrollRegion {
                top: 1u16,
                bottom: 2u16
            }
        );
    }

    #[test]
    fn allocates_bounded_row_major_empty_grid() {
        let grid = ScreenGrid::new(size(4u16, 2u16), 8usize).expect("grid allocation");

        assert_eq!(grid.cell_count(), 8usize);
        assert_eq!(grid.size(), size(4u16, 2u16));
        assert!(grid.cells().iter().all(|cell| cell.is_empty()));
        assert_eq!(grid.row_cells(1u16).expect("second row").len(), 4usize);
        assert!(grid.row_cells(2u16).is_none());
    }

    #[test]
    fn rejects_zero_limit_and_excessive_dimensions() {
        assert_eq!(
            ScreenGrid::new(size(2u16, 2u16), 0usize),
            Err(ScreenGridError::ZeroCellLimit)
        );
        assert_eq!(
            ScreenGrid::new(size(4u16, 2u16), 7usize),
            Err(ScreenGridError::CapacityExceeded)
        );
    }

    #[test]
    fn stores_combining_sequence_as_one_grapheme() {
        let value = "e\u{301}";
        let grapheme = Grapheme::new(value).expect("valid combining grapheme");
        let mut grid = ScreenGrid::new(size(4u16, 1u16), 4usize).expect("grid");
        grid.set_grapheme(
            0u16,
            0u16,
            value,
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("store combining grapheme");

        assert_eq!(grapheme.as_str(), value);
        assert_eq!(grapheme.byte_length(), value.len());
        assert!(matches!(
            grid.get_cell(0u16, 0u16).expect("cell").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Single,
                ..
            }
        ));
        if let CellContent::Grapheme { value: stored, .. } =
            grid.get_cell(0u16, 0u16).expect("cell").content()
        {
            assert_eq!(stored.as_str(), value);
        } else {
            panic!("expected grapheme cell");
        }
    }

    #[test]
    fn wide_grapheme_occupies_leading_and_continuation_cells() {
        let mut grid = ScreenGrid::new(size(4u16, 1u16), 4usize).expect("grid");
        let attributes = CellAttributes::new(
            Color::Indexed(2u8),
            Color::Rgb {
                red: 10u8,
                green: 20u8,
                blue: 30u8,
            },
            StyleFlags::empty().with(StyleFlags::BOLD),
        );

        grid.set_grapheme(0u16, 1u16, "界", GraphemeWidth::Wide, attributes)
            .expect("wide grapheme");

        assert!(matches!(
            grid.get_cell(0u16, 1u16).expect("leading").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Wide,
                ..
            }
        ));
        assert!(grid
            .get_cell(0u16, 2u16)
            .expect("continuation")
            .is_continuation());
        assert_eq!(
            grid.get_cell(0u16, 2u16)
                .expect("continuation")
                .attributes(),
            attributes
        );
        assert!(attributes.styles().contains(StyleFlags::BOLD));
    }

    #[test]
    fn overwriting_continuation_clears_previous_wide_grapheme() {
        let mut grid = ScreenGrid::new(size(4u16, 1u16), 4usize).expect("grid");
        grid.set_grapheme(
            0u16,
            1u16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.set_grapheme(
            0u16,
            2u16,
            "x",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("overwrite continuation");

        assert!(grid.get_cell(0u16, 1u16).expect("old lead").is_empty());
        assert!(matches!(
            grid.get_cell(0u16, 2u16).expect("new cell").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Single,
                ..
            }
        ));
    }

    #[test]
    fn rejects_wide_grapheme_at_right_edge_without_mutation() {
        let mut grid = ScreenGrid::new(size(2u16, 1u16), 2usize).expect("grid");

        assert_eq!(
            grid.set_grapheme(
                0u16,
                1u16,
                "界",
                GraphemeWidth::Wide,
                CellAttributes::default()
            ),
            Err(ScreenGridError::WideGraphemeAtRightEdge)
        );
        assert!(grid.get_cell(0u16, 1u16).expect("edge").is_empty());
    }

    #[test]
    fn clearing_leading_cell_clears_wide_continuation() {
        let mut grid = ScreenGrid::new(size(3u16, 1u16), 3usize).expect("grid");
        grid.set_grapheme(
            0u16,
            0u16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.set_empty(0u16, 0u16).expect("clear wide grapheme");

        assert!(grid.get_cell(0u16, 0u16).expect("lead").is_empty());
        assert!(grid.get_cell(0u16, 1u16).expect("continuation").is_empty());
    }

    #[test]
    fn resize_preserves_intersection_and_clears_cut_wide_cluster() {
        let mut grid = ScreenGrid::new(size(4u16, 2u16), 12usize).expect("grid");
        grid.set_grapheme(
            0u16,
            0u16,
            "A",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("single grapheme");
        grid.set_grapheme(
            0u16,
            2u16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.resize(size(3u16, 2u16)).expect("shrink width");

        assert_eq!(grid.size(), size(3u16, 2u16));
        assert!(matches!(
            grid.get_cell(0u16, 0u16).expect("preserved").content(),
            CellContent::Grapheme { .. }
        ));
        assert!(grid.get_cell(0u16, 2u16).expect("cut wide lead").is_empty());

        grid.resize(size(4u16, 3u16)).expect("grow screen");
        assert_eq!(grid.size(), size(4u16, 3u16));
        assert!(grid.get_cell(2u16, 3u16).expect("new cell").is_empty());
    }

    #[test]
    fn failed_resize_is_atomic_when_cell_limit_would_be_exceeded() {
        let mut grid = ScreenGrid::new(size(2u16, 2u16), 4usize).expect("grid");
        grid.set_grapheme(
            0u16,
            0u16,
            "x",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("store grapheme");
        let original = grid.clone();

        assert_eq!(
            grid.resize(size(3u16, 2u16)),
            Err(ScreenGridError::CapacityExceeded)
        );
        assert_eq!(grid, original);
    }

    #[test]
    fn rejects_invalid_grapheme_values_and_out_of_bounds_access() {
        let mut grid = ScreenGrid::new(size(2u16, 2u16), 4usize).expect("grid");

        assert_eq!(Grapheme::new(""), Err(ScreenGridError::EmptyGrapheme));
        assert_eq!(
            Grapheme::new("\u{7}"),
            Err(ScreenGridError::ControlCharacterInGrapheme)
        );
        assert_eq!(
            Grapheme::new(&"x".repeat(MAX_GRAPHEME_BYTES + 1usize)),
            Err(ScreenGridError::GraphemeTooLong)
        );
        assert_eq!(
            grid.set_grapheme(
                2u16,
                0u16,
                "x",
                GraphemeWidth::Single,
                CellAttributes::default()
            ),
            Err(ScreenGridError::OutOfBounds)
        );
        assert!(grid.get_cell(0u16, 2u16).is_none());
    }
}
