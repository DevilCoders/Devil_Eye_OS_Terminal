//! Stable, C-compatible data types for the Rust/C boundary.
//!
//! This crate intentionally contains no C implementation and no unsafe
//! bindings yet. Exported functions and ownership rules will be introduced
//! alongside the corresponding C subsystem.

/// Current Rust/C interface version.
pub const ABI_VERSION: DeAbiVersion = DeAbiVersion {
    major: 0,
    minor: 1,
    patch: 0,
};

/// Version tuple shared with C through a C-compatible memory layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

/// Status values reserved for future C ABI function results.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeStatus {
    Ok = 0,
    InvalidArgument = 1,
    InternalError = 2,
}

#[cfg(test)]
mod tests {
    use super::{DeAbiVersion, DeStatus, ABI_VERSION};

    #[test]
    fn exposes_expected_abi_version() {
        assert_eq!(
            ABI_VERSION,
            DeAbiVersion {
                major: 0,
                minor: 1,
                patch: 0,
            }
        );
    }

    #[test]
    fn abi_version_has_c_compatible_six_byte_layout() {
        assert_eq!(std::mem::size_of::<DeAbiVersion>(), 6);
    }

    #[test]
    fn status_values_are_stable() {
        assert_eq!(DeStatus::Ok as i32, 0);
        assert_eq!(DeStatus::InvalidArgument as i32, 1);
        assert_eq!(DeStatus::InternalError as i32, 2);
    }
}
