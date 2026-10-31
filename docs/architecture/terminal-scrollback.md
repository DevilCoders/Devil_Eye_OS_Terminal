# Bounded Terminal Scrollback

**Task:** DE-005-04 (#140)  
**Target:** Linux x86_64  
**Implementation:** `TerminalScreen` in `de-terminal-core`

## Ownership and API

The terminal model owns retained scrollback rows. The visible `ScreenGrid` remains a fixed-size row-major viewport consumed by the existing immutable renderer frame contract. Scrollback is not mixed into the visible frame, and it does not change cursor coordinates or visible-grid dimensions.

- `TerminalScreen::new(size, max_cells)` keeps the existing low-memory behavior and creates a screen with scrollback disabled.
- `TerminalScreen::new_with_scrollback(size, max_cells, max_scrollback_cells)` enables a bounded history budget.
- `scrollback_rows()` iterates retained rows oldest-to-newest as immutable cell slices.
- `scrollback_row_count()` and `scrollback_cell_count()` report retained capacity usage.
- `max_scrollback_cells()` reports the configured payload-cell limit.
- `clear_scrollback()` clears and zeroes retained history without changing the visible screen.

The API does not expose mutable history cells. Each retained row preserves the complete `Cell` values, including grapheme attributes and wide-grapheme continuation cells.

## Capture semantics

- An upward scroll of the full-screen scrolling region appends the rows displaced from the top of the visible grid.
- Line feed at the bottom of the full-screen region uses the same capture path.
- Partial-region upward scrolling does not enter scrollback.
- Downward scrolling never appends history.
- A scroll count larger than the screen height retains only the newest rows that can fit in the configured history budget.
- Oldest retained rows are evicted first when new rows arrive.

## Memory bound

The budget is expressed in cell slots, not a number of lines. Effective row capacity is:

`max_rows = max_scrollback_cells / current_columns`

Any remainder smaller than one complete row is unused. A budget smaller than one row therefore retains no rows. The ring buffer allocates exactly the effective number of cell slots during construction using fallible reservation. Scroll operations then copy rows into preallocated storage and perform no heap allocation. If the requested history capacity cannot be allocated, construction returns `ScreenGridError::AllocationFailed`; no partially initialized screen is returned.

This budget excludes allocator metadata and the visible screen grid, which is separately bounded by `max_cells`.

## Resize behavior

Rows are retained at the column width active when they entered history. No implicit reflow or truncation is performed.

- A row-count-only resize preserves history.
- A column-count change clears history, because rows of different widths cannot be represented as one consistent viewport without a reflow policy.
- The visible grid resize remains atomic and preserves its existing top-left intersection semantics.

A future viewport/history integration may introduce an explicit reflow operation with dedicated Unicode and wide-cell tests. It must not silently change this contract.

## Failure and reset behavior

- Zero capacity disables history without changing scroll behavior.
- `clear_scrollback()` zeroes retained cell storage and resets the history indices; it does not change the visible grid.
- Invalid scroll regions continue to return `InvalidScrollRegion`.
- Scrollback capture itself does not allocate. Invalid internal row ranges return `OutOfBounds` before changing history; visible-grid scrolling remains bounded and allocation-free.
- No history operation launches processes, reads files, or invokes external services.

## Verification

Unit tests cover chronological retention, oldest-row eviction, hard cell limits, partial-region and downward-scroll exclusion, line-feed capture, wide-grapheme pairs, zero/sub-row capacity, explicit clear, and resize behavior. The renderer still receives only the visible screen grid; scrollback rendering and large-history viewport navigation are separate integration work.
