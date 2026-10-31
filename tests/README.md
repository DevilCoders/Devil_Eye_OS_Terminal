# Test Organization

Tests are organized by the boundary they validate:

- `unit/` — isolated Rust and C component behavior.
- `integration/` — cross-component and Rust/C boundary behavior.
- `e2e/` — complete terminal workflows and process/session behavior.
- `fixtures/` — deterministic input/output fixtures, terminal transcripts, and test data.
- `fuzz/` — parser, escape-sequence, and input-state fuzz targets.

Tests must be deterministic where practical, isolate process resources, and include cleanup/failure-path coverage. A test directory is not a substitute for tests that exercise real behavior.
