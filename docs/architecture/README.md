# Architecture Documentation

This directory contains architecture decision records and subsystem design documents.

Required design boundaries:
- Rust owns the desktop application, lifecycle, orchestration, GUI, and renderer integration.
- C owns the independent DE-Shell implementation.
- The terminal emulator must host external compatible shells; it is not coupled to DE-Shell.
- PTY/process sessions are isolated per terminal tab.
- OS-specific behavior is hidden behind platform adapters.
- Rust/C interfaces are explicit, versioned, and ownership-safe.

Architecture changes must record the decision, alternatives, compatibility impact, and test implications. See the [engineering roadmap](../engineering/ROADMAP.md) and [workflow](../engineering/WORKFLOW.md).
