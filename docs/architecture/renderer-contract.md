# Renderer Abstraction and Frame Contract

**Scope:** DE-009-01. This contract defines the backend-independent boundary between the terminal model and a renderer. It does not select a GUI toolkit, GPU API, font library, or concrete rendering backend.

## Ownership

- `de-terminal-core` owns terminal cells, cursor state, terminal modes, and parser state.
- The application/session layer owns frame generations and coordinates damage invalidation with terminal mutations.
- A renderer owns its graphics device, surfaces, pipelines, textures, font caches, and other backend resources.
- A renderer receives immutable borrowed frame data. It must not mutate terminal state or retain borrowed cell references after `render` returns.
- Renderer failures are returned to the application. They do not terminate PTY sessions or child processes.

## Coordinate spaces

The public frame contract uses three distinct coordinate spaces:

1. **Cell space:** zero-based `(row, column)`, row-major. `CellRect` stores `top`, `left`, `rows`, and `columns`; its bottom and right edges are exclusive.
2. **Logical pixel space:** derived from cell coordinates and validated `GlyphMetrics`; values are independent of monitor DPI.
3. **Physical pixel space:** logical pixel values multiplied by a validated `ScaleFactor`.

Terminal model coordinates are always cell-space values. A backend must not write physical pixel dimensions back into the terminal model.

## Frame snapshot

A `Frame` contains:

- Terminal dimensions.
- Cursor position.
- Immutable row-major cell slice.
- Damage tracker snapshot/reference.
- Caller-provided monotonically increasing generation.

The cell slice length must equal `columns × rows`. Each row is contiguous. The renderer must use the frame's dimensions and must not assume that a previous frame had the same size.

Generation increments whenever the application publishes a changed frame. A renderer may use it to reject stale asynchronous work; it must not infer that a frame is unchanged solely because the generation is equal unless the application guarantees that invariant.

## Cell and glyph semantics

- One cell contains one complete grapheme cluster or is empty/continuation storage for a wide grapheme.
- A wide grapheme occupies its leading cell plus a continuation cell; render it once at the leading cell and reserve the continuation cell's horizontal advance.
- The terminal engine supplies grapheme boundaries and display width. A renderer must not independently reflow terminal cells or reinterpret terminal display width.
- Font shaping may combine code points inside a grapheme cluster, but shaping must not change the terminal's cell-grid advance or move subsequent cells.
- Combining marks and emoji sequences are rendered as part of their owning grapheme cluster.
- If the selected font lacks a glyph, the backend may use a documented fallback glyph/font; it must preserve the cell's advance and must not mutate terminal text.
- Cursor is a cell-space position. Cursor shape/blink policy is applied by the renderer or application layer, not by changing cell contents.

## Glyph metrics and scale

`GlyphMetrics` stores positive finite logical-pixel values:

- `advance_width`: horizontal cell advance.
- `ascent`: baseline-to-top distance.
- `descent`: positive baseline-to-bottom magnitude.
- `line_height`: vertical cell advance, at least ascent plus descent.

`ScaleFactor` is finite and greater than zero. A monitor scale change requires the application to resize/reconfigure the renderer and publish a new frame generation. Terminal rows/columns change only when the logical cell grid changes, not merely because the physical DPI changed.

## Damage tracking

`DamageTracker` is bounded by a configured maximum rectangle count.

- New trackers begin with full-frame invalidation.
- `mark_cell` and `mark_rect` add cell-space damage after bounds validation.
- Overlapping and edge-adjacent rectangles are coalesced.
- When the rectangle budget is exceeded, the tracker degrades to full-frame invalidation. It never drops damage or grows without bound.
- A terminal resize changes tracker dimensions and invalidates the full new frame.
- The application clears damage only after the renderer successfully consumes the corresponding frame. A failed render retains damage for retry.
- Damage describes regions that may have changed; it is not a guarantee that every cell inside a rectangle differs.

A backend may further clip dirty regions to a viewport or scissor rectangle. Clipping is an optimization only; it must not suppress later invalidation for cells outside the current clip.

## Resize semantics

On terminal grid resize:

1. The terminal model applies the new dimensions atomically.
2. The application updates damage-tracker dimensions; the entire new frame becomes dirty.
3. The renderer receives the new size and current scale factor.
4. The application propagates the corresponding PTY window size through the session runtime.
5. The next rendered frame uses the new cell dimensions and generation.

The application serializes resize operations for a session. If renderer resize fails, the application reports the error and may recreate renderer resources. If PTY resize fails, the session manager reconciles the effective dimensions; the renderer must not silently claim the PTY adopted the requested size.

## Render backend contract

The `Renderer` trait provides:

- `resize(size, scale_factor)`: update the backend viewport and cell/pixel mapping.
- `render(frame, clip)`: consume one immutable frame, honoring its damage and optional cell-space clip.

Implementations must:

- Avoid blocking the GUI/event loop on process I/O or unbounded GPU waits.
- Respect the borrowed frame lifetime.
- Preserve cell ordering and wide-grapheme occupancy.
- Apply clipping without changing terminal model state.
- Return a typed backend error for resource/device/surface failures.
- Avoid retaining pointers/references to the frame after the call.
- Be safe to recreate after device loss without destroying child-process sessions.

## Verification

The contract module tests:

- Initial full-frame invalidation and incremental cell damage.
- Rectangle bounds and zero-extent rejection.
- Adjacent/overlapping damage coalescing.
- Bounded damage fallback to full-frame invalidation.
- Full invalidation after resize.
- Immutable row-major frame snapshots and cursor/dimension exposure.
- Frame/damage dimension mismatch rejection.
- Glyph metric and scale-factor validation.

Concrete GPU/font rendering tests belong to the selected backend implementation task; this contract does not claim a renderer exists.


## Selection and software reference renderer

The frame may carry an optional immutable selection:

- **Linear selection** includes both endpoints and every row-major cell between them.
- **Rectangular selection** includes the inclusive rectangle between anchor and extent.
- Both endpoints are validated against the frame dimensions. A frame rejects stale/out-of-bounds selections.
- The application marks the old and new selection regions dirty when selection changes.

The initial concrete implementation is `SoftwareRenderer`, a deterministic CPU reference backend. It consumes a caller-provided `GlyphAtlas` of alpha bitmaps, so font discovery/rasterization remains a separate concern and can later feed the same frame contract from a GPU backend.

The software renderer implements:

- RGBA surface allocation with checked dimensions, a hard pixel budget, and fallible reservation.
- Default, 16-color, 256-color, and true-color resolution.
- Cell background/foreground, inverse, dim, bold, italic, underline, strikethrough, hidden, and blink visibility.
- Single- and double-cell grapheme placement, with continuation-cell handling.
- Selection and block-cursor palette overrides.
- Cell-space clipping, bounded damage-region redraw, and full invalidation after resize/cursor/blink visibility changes.
- Deterministic pixel output for headless golden tests.

The glyph atlas is an input to this pipeline, not a built-in font. A concrete font provider must rasterize complete grapheme clusters into atlas entries and supply a fallback glyph. Missing atlas entries use the explicit fallback and preserve the terminal cell advance.

The CPU backend is the correctness/reference implementation and test surface. It does not claim to provide a window, GPU acceleration, font discovery, or a production desktop UI.
