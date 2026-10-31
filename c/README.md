# C Subsystems

This directory owns the independent DE-Shell C implementation and any narrowly scoped C support code.

Rules:
- DE-Shell is a shell implementation, not the terminal emulator.
- Keep public interfaces in the corresponding `include/` area.
- Compile with strict warnings and test parser/execution boundaries.
- Keep OS-specific behavior behind explicit adapters.
- NASM is not placed here by default; assembly is permitted only for measured, justified low-level routines.
