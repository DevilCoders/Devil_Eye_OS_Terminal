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

        let mut bytes = [0U8; MAX_GRAPHEME_BYTES];
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
    Rgb { red: u8, green: u8, blue: u8 },
}

/// Compact, composable terminal text-style flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StyleFlags(u16);

impl StyleFlags {
    pub const BOLD: Self = Self(1U16 << 0U32);
    pub const DIM: Self = Self(1U16 << 1U32);
    pub const ITALIC: Self = Self(1U16 << 2U32);
    pub const UNDERLINE: Self = Self(1U16 << 3U32);
    pub const BLINK: Self = Self(1U16 << 4U32);
    pub const INVERSE: Self = Self(1U16 << 5U32);
    pub const HIDDEN: Self = Self(1U16 << 6U32);
    pub const STRIKETHROUGH: Self = Self(1U16 << 7U32);

    pub const fn empty() -> Self {
        Self(0U16)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellContent {
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

impl Default for CellContent {
    fn default() -> Self {
        Self::Empty
    }
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
        if max_cells == 0Usize {
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
        let index = self.index(row, column).ok_or(ScreenGridError::OutOfBounds)?;
        let grapheme = Grapheme::new(value)?;

        if width == GraphemeWidth::Wide && column + 1U16 >= self.size.columns() {
            return Err(ScreenGridError::WideGraphemeAtRightEdge);
        }

        self.clear_cell_pair(index);
        if width == GraphemeWidth::Wide {
            self.clear_cell_pair(index + 1Usize);
        }

        self.cells[index] = Cell {
            content: CellContent::Grapheme {
                value: grapheme,
                width,
            },
            attributes,
        };

        if width == GraphemeWidth::Wide {
            self.cells[index + 1Usize] = Cell {
                content: CellContent::Continuation,
                attributes,
            };
        }

        Ok(())
    }

    /// Clears a cell and the other half of a wide-grapheme pair, if present.
    pub fn set_empty(&mut self, row: u16, column: u16) -> Result<(), ScreenGridError> {
        let index = self.index(row, column).ok_or(ScreenGridError::OutOfBounds)?;
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
                let edge_index = row * new_columns + new_columns - 1Usize;
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

        Some(
            usize::from(row) * usize::from(self.size.columns()) + usize::from(column),
        )
    }

    fn clear_cell_pair(&mut self, index: usize) {
        let columns = usize::from(self.size.columns());
        let column = index % columns;

        match self.cells[index].content {
            CellContent::Grapheme {
                width: GraphemeWidth::Wide,
                ..
            } if column + 1Usize < columns => {
                self.cells[index + 1Usize] = Cell::default();
            }
            CellContent::Continuation if column > 0Usize => {
                let previous = index - 1Usize;
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

#[cfg(test)]
mod tests {
    use super::{
        CellAttributes, CellContent, Color, Grapheme, GraphemeWidth, ScreenGrid,
        ScreenGridError, StyleFlags, MAX_GRAPHEME_BYTES,
    };
    use crate::TerminalSize;

    fn size(columns: u16, rows: u16) -> TerminalSize {
        TerminalSize::new(columns, rows).expect("non-zero terminal dimensions")
    }

    #[test]
    fn allocates_bounded_row_major_empty_grid() {
        let grid = ScreenGrid::new(size(4U16, 2U16), 8Usize).expect("grid allocation");

        assert_eq!(grid.cell_count(), 8Usize);
        assert_eq!(grid.size(), size(4U16, 2U16));
        assert!(grid.cells().iter().all(|cell| cell.is_empty()));
        assert_eq!(grid.row_cells(1U16).expect("second row").len(), 4Usize);
        assert!(grid.row_cells(2U16).is_none());
    }

    #[test]
    fn rejects_zero_limit_and_excessive_dimensions() {
        assert_eq!(
            ScreenGrid::new(size(2U16, 2U16), 0Usize),
            Err(ScreenGridError::ZeroCellLimit)
        );
        assert_eq!(
            ScreenGrid::new(size(4U16, 2U16), 7Usize),
            Err(ScreenGridError::CapacityExceeded)
        );
    }

    #[test]
    fn stores_combining_sequence_as_one_grapheme() {
        let value = "e\u{301}";
        let grapheme = Grapheme::new(value).expect("valid combining grapheme");
        let mut grid = ScreenGrid::new(size(4U16, 1U16), 4Usize).expect("grid");
        grid.set_grapheme(
            0U16,
            0U16,
            value,
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("store combining grapheme");

        assert_eq!(grapheme.as_str(), value);
        assert_eq!(grapheme.byte_length(), value.len());
        assert!(matches!(
            grid.get_cell(0U16, 0U16).expect("cell").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Single,
                ..
            }
        ));
        if let CellContent::Grapheme { value: stored, .. } =
            grid.get_cell(0U16, 0U16).expect("cell").content()
        {
            assert_eq!(stored.as_str(), value);
        } else {
            return Err("expected grapheme cell");
        }
    }

    #[test]
    fn wide_grapheme_occupies_leading_and_continuation_cells() {
        let mut grid = ScreenGrid::new(size(4U16, 1U16), 4Usize).expect("grid");
        let attributes = CellAttributes::new(
            Color::Indexed(2U8),
            Color::Rgb {
                red: 10U8,
                green: 20U8,
                blue: 30U8,
            },
            StyleFlags::empty().with(StyleFlags::BOLD),
        );

        grid.set_grapheme(0U16, 1U16, "界", GraphemeWidth::Wide, attributes)
            .expect("wide grapheme");

        assert!(matches!(
            grid.get_cell(0U16, 1U16).expect("leading").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Wide,
                ..
            }
        ));
        assert!(grid
            .get_cell(0U16, 2U16)
            .expect("continuation")
            .is_continuation());
        assert_eq!(
            grid.get_cell(0U16, 2U16).expect("continuation").attributes(),
            attributes
        );
        assert!(attributes.styles().contains(StyleFlags::BOLD));
    }

    #[test]
    fn overwriting_continuation_clears_previous_wide_grapheme() {
        let mut grid = ScreenGrid::new(size(4U16, 1U16), 4Usize).expect("grid");
        grid.set_grapheme(
            0U16,
            1U16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.set_grapheme(
            0U16,
            2U16,
            "x",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("overwrite continuation");

        assert!(grid.get_cell(0U16, 1U16).expect("old lead").is_empty());
        assert!(matches!(
            grid.get_cell(0U16, 2U16).expect("new cell").content(),
            CellContent::Grapheme {
                width: GraphemeWidth::Single,
                ..
            }
        ));
    }

    #[test]
    fn rejects_wide_grapheme_at_right_edge_without_mutation() {
        let mut grid = ScreenGrid::new(size(2U16, 1U16), 2Usize).expect("grid");

        assert_eq!(
            grid.set_grapheme(
                0U16,
                1U16,
                "界",
                GraphemeWidth::Wide,
                CellAttributes::default()
            ),
            Err(ScreenGridError::WideGraphemeAtRightEdge)
        );
        assert!(grid.get_cell(0U16, 1U16).expect("edge").is_empty());
    }

    #[test]
    fn clearing_leading_cell_clears_wide_continuation() {
        let mut grid = ScreenGrid::new(size(3U16, 1U16), 3Usize).expect("grid");
        grid.set_grapheme(
            0U16,
            0U16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.set_empty(0U16, 0U16).expect("clear wide grapheme");

        assert!(grid.get_cell(0U16, 0U16).expect("lead").is_empty());
        assert!(grid
            .get_cell(0U16, 1U16)
            .expect("continuation")
            .is_empty());
    }

    #[test]
    fn resize_preserves_intersection_and_clears_cut_wide_cluster() {
        let mut grid = ScreenGrid::new(size(4U16, 2U16), 8Usize).expect("grid");
        grid.set_grapheme(
            0U16,
            0U16,
            "A",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("single grapheme");
        grid.set_grapheme(
            0U16,
            2U16,
            "界",
            GraphemeWidth::Wide,
            CellAttributes::default(),
        )
        .expect("wide grapheme");

        grid.resize(size(3U16, 2U16)).expect("shrink width");

        assert_eq!(grid.size(), size(3U16, 2U16));
        assert!(matches!(
            grid.get_cell(0U16, 0U16).expect("preserved").content(),
            CellContent::Grapheme { .. }
        ));
        assert!(grid.get_cell(0U16, 2U16).expect("cut wide lead").is_empty());

        grid.resize(size(4U16, 3U16)).expect("grow screen");
        assert_eq!(grid.size(), size(4U16, 3U16));
        assert!(grid.get_cell(2U16, 3U16).expect("new cell").is_empty());
    }

    #[test]
    fn failed_resize_is_atomic_when_cell_limit_would_be_exceeded() {
        let mut grid = ScreenGrid::new(size(2U16, 2U16), 4Usize).expect("grid");
        grid.set_grapheme(
            0U16,
            0U16,
            "x",
            GraphemeWidth::Single,
            CellAttributes::default(),
        )
        .expect("store grapheme");
        let original = grid.clone();

        assert_eq!(
            grid.resize(size(3U16, 2U16)),
            Err(ScreenGridError::CapacityExceeded)
        );
        assert_eq!(grid, original);
    }

    #[test]
    fn rejects_invalid_grapheme_values_and_out_of_bounds_access() {
        let mut grid = ScreenGrid::new(size(2U16, 2U16), 4Usize).expect("grid");

        assert_eq!(
            Grapheme::new(""),
            Err(ScreenGridError::EmptyGrapheme)
        );
        assert_eq!(
            Grapheme::new("\u{7}"),
            Err(ScreenGridError::ControlCharacterInGrapheme)
        );
        assert_eq!(
            Grapheme::new(&"x".repeat(MAX_GRAPHEME_BYTES + 1Usize)),
            Err(ScreenGridError::GraphemeTooLong)
        );
        assert_eq!(
            grid.set_grapheme(
                2U16,
                0U16,
                "x",
                GraphemeWidth::Single,
                CellAttributes::default()
            ),
            Err(ScreenGridError::OutOfBounds)
        );
        assert!(grid.get_cell(0U16, 2U16).is_none());
    }
}
