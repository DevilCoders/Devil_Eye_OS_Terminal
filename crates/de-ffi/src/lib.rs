//! Fixed-layout Rust types shared with the DE-Shell C ABI.
//!
//! This crate defines only ABI-safe values and compatibility rules. It does
//! not expose Rust-owned strings, vectors, references, trait objects, or
//! allocator-owned buffers across the C boundary. Safe wrappers and calls
//! into the native library are introduced by the dependent FFI task.

/// Current Rust/C interface version.
pub const ABI_VERSION: DeAbiVersion = DeAbiVersion {
    major: 0,
    minor: 1,
    patch: 0,
};

/// Version tuple shared with C through a fixed C-compatible memory layout.
///
/// The layout is three consecutive 16-bit unsigned integers. Its size is six
/// bytes and its alignment is two bytes on supported targets.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl DeAbiVersion {
    /// Returns whether a library version can serve this requested version.
    ///
    /// Major versions must match and the library minor version must be at
    /// least the requested minor version. Patch differences are compatible.
    pub const fn is_compatible_with(self, library: Self) -> bool {
        self.major == library.major && self.minor <= library.minor
    }
}

/// A fixed-width status value returned by the C ABI.
///
/// This is a transparent integer wrapper rather than a Rust enum so unknown
/// future C status values remain representable without creating an invalid
/// Rust enum discriminant.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeStatus(i32);

impl DeStatus {
    pub const OK: Self = Self(0);
    pub const INVALID_ARGUMENT: Self = Self(1);
    pub const INTERNAL_ERROR: Self = Self(2);
    pub const BUSY: Self = Self(3);
    pub const PARSE_ERROR: Self = Self(4);
    pub const RESOURCE_LIMIT: Self = Self(5);
    pub const UNSUPPORTED_OPERATION: Self = Self(6);
    pub const EXECUTION_UNAVAILABLE: Self = Self(7);
    pub const EXTERNAL_EXECUTION_FAILED: Self = Self(8);
    pub const ABI_MISMATCH: Self = Self(9);

    /// Wraps a raw status returned by a C function, including unknown values.
    pub const fn from_raw(value: i32) -> Self {
        Self(value)
    }

    /// Returns the exact signed 32-bit status value.
    pub const fn as_raw(self) -> i32 {
        self.0
    }

    /// Returns whether the status represents success.
    pub const fn is_ok(self) -> bool {
        self.0 == Self::OK.0
    }
}

#[cfg(test)]
mod tests {
    use super::{DeAbiVersion, DeStatus, ABI_VERSION};
    use std::mem::{align_of, offset_of, size_of};

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
    fn abi_version_has_fixed_c_layout() {
        assert_eq!(size_of::<DeAbiVersion>(), 6);
        assert_eq!(align_of::<DeAbiVersion>(), 2);
        assert_eq!(offset_of!(DeAbiVersion, major), 0);
        assert_eq!(offset_of!(DeAbiVersion, minor), 2);
        assert_eq!(offset_of!(DeAbiVersion, patch), 4);
    }

    #[test]
    fn abi_compatibility_accepts_older_minor_and_patch_versions() {
        let library = ABI_VERSION;
        assert!(DeAbiVersion {
            major: 0,
            minor: 0,
            patch: 99,
        }
        .is_compatible_with(library));
        assert!(DeAbiVersion {
            major: 0,
            minor: 1,
            patch: 99,
        }
        .is_compatible_with(library));
    }

    #[test]
    fn abi_compatibility_rejects_major_or_future_minor_versions() {
        assert!(!DeAbiVersion {
            major: 1,
            minor: 0,
            patch: 0,
        }
        .is_compatible_with(ABI_VERSION));
        assert!(!DeAbiVersion {
            major: 0,
            minor: 2,
            patch: 0,
        }
        .is_compatible_with(ABI_VERSION));
    }

    #[test]
    fn status_has_fixed_i32_layout_and_stable_values() {
        assert_eq!(size_of::<DeStatus>(), 4);
        assert_eq!(align_of::<DeStatus>(), 4);
        assert_eq!(DeStatus::OK.as_raw(), 0);
        assert_eq!(DeStatus::INVALID_ARGUMENT.as_raw(), 1);
        assert_eq!(DeStatus::INTERNAL_ERROR.as_raw(), 2);
        assert_eq!(DeStatus::BUSY.as_raw(), 3);
        assert_eq!(DeStatus::PARSE_ERROR.as_raw(), 4);
        assert_eq!(DeStatus::RESOURCE_LIMIT.as_raw(), 5);
        assert_eq!(DeStatus::UNSUPPORTED_OPERATION.as_raw(), 6);
        assert_eq!(DeStatus::EXECUTION_UNAVAILABLE.as_raw(), 7);
        assert_eq!(DeStatus::EXTERNAL_EXECUTION_FAILED.as_raw(), 8);
        assert_eq!(DeStatus::ABI_MISMATCH.as_raw(), 9);
        assert!(DeStatus::OK.is_ok());
        assert!(!DeStatus::INTERNAL_ERROR.is_ok());
    }

    #[test]
    fn unknown_status_values_remain_representable() {
        let unknown = DeStatus::from_raw(0x1234);
        assert_eq!(unknown.as_raw(), 0x1234);
        assert!(!unknown.is_ok());
    }
}
