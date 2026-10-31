# ADR-0004: Immutable terminal frames and bounded renderer damage

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #51, #52

## Context

The terminal model must remain the sole owner of screen cells and parser state, while rendering backends may be CPU-based or GPU/window-backed. Direct mutation of terminal cells by a renderer would break ownership and make backend behavior nondeterministic.

## Decision

The terminal engine owns the screen grid, cursor, modes, scrollback, and damage state. A renderer consumes an immutable frame snapshot or immutable borrowed frame view with a documented lifetime. The renderer must not retain borrowed frame references beyond that lifetime or mutate terminal state.

Damage tracking is bounded. When the rectangle budget is exhausted, it degrades to full-frame invalidation rather than growing without limit or dropping updates. Resize invalidates the new viewport. A clipped render must not discard a pending full redraw for pixels/cells outside the clip.

The deterministic CPU software renderer is the reference implementation and headless test surface. Font rasterization is provided through a glyph atlas; GPU/window integration is a separate backend concern.

## Alternatives considered

1. **Renderer directly mutates the terminal grid:** rejected because it violates single ownership and complicates concurrency.
2. **Unbounded dirty-region list:** rejected because malformed workloads could cause unbounded memory growth.
3. **GPU-only implementation:** rejected because deterministic headless tests and non-GPU environments need a reference backend.
4. **Renderer owns font discovery:** rejected because font providers and rendering backends should be independently replaceable.

## Consequences

Immutable frames simplify testing and renderer replacement. The application must track frame lifetimes and issue redraws after state changes. The CPU reference backend prioritizes correctness, not production GPU performance.

## Affected interfaces and verification

- [Renderer Contract](../renderer-contract.md)
- [Terminal Screen State](../terminal-screen-state.md)
- [Cross-Component API Contracts](../api-contracts.md)

Verify immutable frame borrowing, damage coalescing and overflow fallback, clipped full redraw, wide-cell continuation redraw, Unicode graphemes, cursor/selection overlays, resize, and pixel-budget enforcement.
