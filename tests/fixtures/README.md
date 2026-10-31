# Test Fixtures

Fixtures are immutable, deterministic test inputs and expected outputs.

## Rules

- Prefer plain text or small binary fixtures with a short explanation of their origin and expected behavior.
- Never store secrets, credentials, personal data, production logs, or host-specific paths.
- Keep one fixture focused on one behavior and use descriptive filenames.
- Reference fixtures by paths relative to the test crate or repository root; never assume a developer's home directory.
- When a fixture represents malformed or adversarial input, document the boundary condition it is intended to exercise.
- Generated fuzz corpora and large binary corpora belong in CI artifacts or dedicated storage, not in the source repository.

No product protocol fixtures are checked in yet; add them with the corresponding terminal parser or PTY implementation and its tests.
