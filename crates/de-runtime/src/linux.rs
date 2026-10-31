//! Linux-specific runtime resources.

// Process control requires narrowly scoped libc syscalls; keep unsafe code inside this adapter.
#[allow(unsafe_code)]
pub mod process;
pub mod pty;
