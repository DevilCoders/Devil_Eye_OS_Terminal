//! Platform-independent terminal model primitives.
//!
//! This crate owns terminal state invariants. PTY handling, escape-sequence
//! parsing, rendering, and GUI integration belong to separate components.

/// Default terminal dimensions used when no user preference is available.
pub const DEFAULT_COLUMNS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

/// A terminal's character-cell dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    columns: u16,
    rows: u16,
}

/// Why a terminal size could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSizeError {
    ZeroColumns,
    ZeroRows,
}

impl TerminalSize {
    /// Creates dimensions, rejecting zero-width or zero-height terminals.
    pub const fn new(columns: u16, rows: u16) -> Result<Self, TerminalSizeError> {
        if columns == 0 {
            return Err(TerminalSizeError::ZeroColumns);
        }
        if rows == 0 {
            return Err(TerminalSizeError::ZeroRows);
        }

        Ok(Self { columns, rows })
    }

    /// Number of character columns.
    pub const fn columns(self) -> u16 {
        self.columns
    }

    /// Number of character rows.
    pub const fn rows(self) -> u16 {
        self.rows
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalSize, TerminalSizeError, DEFAULT_COLUMNS, DEFAULT_ROWS};

    #[test]
    fn accepts_non_zero_dimensions() {
        let size = TerminalSize::new(120, 40).expect("valid dimensions");
        assert_eq!(size.columns(), 120);
        assert_eq!(size.rows(), 40);
    }

    #[test]
    fn rejects_zero_columns() {
        assert_eq!(TerminalSize::new(0, 24), Err(TerminalSizeError::ZeroColumns));
    }

    #[test]
    fn rejects_zero_rows() {
        assert_eq!(TerminalSize::new(80, 0), Err(TerminalSizeError::ZeroRows));
    }

    #[test]
    fn default_dimensions_are_80_by_24() {
        let size = TerminalSize::default();
        assert_eq!(size.columns(), DEFAULT_COLUMNS);
        assert_eq!(size.rows(), DEFAULT_ROWS);
    }
}
