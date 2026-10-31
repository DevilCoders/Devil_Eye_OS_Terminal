//! Linux-specific runtime resources.

pub mod process;
pub mod pty;
pub mod runtime;

#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

#[cfg(test)]
static LINUX_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_resource_lock() -> MutexGuard<'static, ()> {
    LINUX_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
