# Public Headers

This directory is reserved for public C headers and stable cross-language contracts.

Rules:
- Keep exported declarations minimal and versioned.
- Document ownership, lifetimes, error conventions, and ABI compatibility.
- Do not expose private implementation structures.
- Rust/C boundary changes require contract tests and review.
