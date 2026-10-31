# Terminal Screen State

This document defines the state semantics implemented by the platform-independent terminal screen model. It covers cursor state, scroll margins, line movement, erase ranges, and resize reconciliation. VT/ANSI byte parsing remains a separate parser responsibility.

## Coordinates and cursor movement

- Rows and columns are zero-based.
- The cursor always remains within the current screen dimensions.
- Absolute cursor placement clamps coordinates to the last valid row and column.
- Relative up/left movement saturates at zero.
- Relative down/right movement clamps at the last row/column.
- Carriage return moves to column zero and preserves the current row.
- Line feed advances one row without implicitly performing carriage return.

## Scrolling region

A scroll region has inclusive top and bottom row indices. The default region covers the entire screen. A requested region is rejected if its top is greater than its bottom or its bottom lies outside the current screen.

Scrolling up shifts rows toward the top and fills vacated rows at the bottom with empty cells. Scrolling down shifts rows toward the bottom and fills vacated rows at the top. A scroll count greater than the region height clears the region. Rows outside the active region are unchanged.

When line feed is issued at the bottom row of the active region, the region scrolls upward by one row and the cursor remains on the bottom row. Elsewhere, line feed advances the cursor unless it is already on the final screen row.

## Erase semantics

Erase operations do not move the cursor.

| Mode | Line erase | Display erase |
| --- | --- | --- |
| ToEnd | Cursor cell through end of current row | Cursor cell through bottom-right cell |
| ToBeginning | Start of current row through cursor cell, inclusive | Top-left cell through cursor cell, inclusive |
| All | Entire current row | Entire screen |

Cell erasure preserves wide-grapheme invariants: erasing either half of a wide cell clears the corresponding pair rather than leaving an orphan continuation or lead cell.

## Resize behavior

Resize first validates the requested dimensions and cell budget. On success:

- The screen grid is resized while preserving the overlapping cell rectangle.
- A wide grapheme cut by the new right edge is cleared rather than left malformed.
- Cursor row and column are clamped to the new bounds.
- The scroll region is clamped to the new last row when it remains valid.
- If the previous region no longer forms a valid range after clamping, it resets to the full new screen.

A failed resize leaves the previous grid, cursor, and scroll region unchanged.

## Ownership boundary

The screen wrapper owns the grid dimensions, cursor, and scroll region as one state object. Callers may inspect the grid and perform bounded cell edits through the wrapper, but cannot obtain a mutable reference to the underlying grid or resize it independently. This prevents cursor and margin state from becoming inconsistent with the grid.

## Verification

Unit tests cover cursor clamping and relative movement, carriage return and line feed, region-only scrolling, scroll-down behavior, invalid margin rejection, line/display erase ranges, wide-grapheme integrity, and resize preservation.
