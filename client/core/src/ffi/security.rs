//! The default security descriptor (Phase 1 §6.1).
//!
//! One descriptor for everything in Phase 1:
//!
//! ```text
//! O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;WD)
//! ```
//!
//! Full access for SYSTEM, Administrators, Everyone. **Converted once at
//! startup and held as bytes in Rust**; keeping it out of C++ keeps the adapter
//! thin.
//!
//! The conversion uses `ConvertStringSecurityDescriptorToSecurityDescriptorW`
//! from `advapi32`, declared here by hand rather than pulled in from a Windows
//! crate. That is deliberate: `space-client-core` must not gain a Windows crate
//! dependency, because the no-WinFsp conformance job (§11.2) depends on the
//! crate staying free of one. `advapi32` is a System32 DLL present on every
//! Windows install and unrelated to WinFsp.

use contracts::{ErrorCode, SpaceError};

/// The Phase 1 descriptor in SDDL form.
pub const DEFAULT_SDDL: &str = "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;WD)";

/// `SDDL_REVISION_1`.
#[cfg(windows)]
const SDDL_REVISION_1: u32 = 1;

#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        StringSecurityDescriptor: *const u16,
        StringSDRevision: u32,
        SecurityDescriptor: *mut *mut core::ffi::c_void,
        SecurityDescriptorSize: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GetLastError() -> u32;
}

/// Convert [`DEFAULT_SDDL`] to a self-relative binary security descriptor.
#[cfg(windows)]
pub fn default_descriptor() -> Result<Vec<u8>, SpaceError> {
    let sddl: Vec<u16> = DEFAULT_SDDL.encode_utf16().chain(std::iter::once(0)).collect();
    let mut psd: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut size: u32 = 0;

    // SAFETY: `sddl` is a NUL-terminated UTF-16 buffer that outlives the call;
    // `psd` and `size` are valid out-pointers. On success the callee allocates
    // with LocalAlloc and we free with LocalFree below.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut psd,
            &mut size,
        )
    };

    if ok == 0 || psd.is_null() {
        let err = unsafe { GetLastError() };
        return Err(SpaceError::new(
            ErrorCode::InternalError,
            format!("failed to convert the default security descriptor (GetLastError = {err})"),
        ));
    }

    // SAFETY: the callee guarantees `size` readable bytes at `psd` on success.
    let bytes = unsafe { std::slice::from_raw_parts(psd as *const u8, size as usize) }.to_vec();
    // SAFETY: `psd` came from LocalAlloc inside the conversion call.
    unsafe {
        LocalFree(psd);
    }

    if bytes.is_empty() {
        return Err(SpaceError::new(
            ErrorCode::InternalError,
            "the converted security descriptor is empty",
        ));
    }
    Ok(bytes)
}

/// Non-Windows builds have no SDDL converter.
///
/// The core is a Windows component; this arm exists only so the crate still
/// *compiles* elsewhere (which keeps `cargo check` useful on a non-Windows
/// developer machine). Calling it is an error rather than a silent empty
/// descriptor, because an empty descriptor would produce an `S:` that Explorer
/// can see but not open -- the §6.2 symptom -- and that must never be reachable
/// by accident.
#[cfg(not(windows))]
pub fn default_descriptor() -> Result<Vec<u8>, SpaceError> {
    Err(SpaceError::new(
        ErrorCode::InternalError,
        "the default security descriptor requires Windows",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_default_descriptor_converts() {
        let sd = default_descriptor().expect("SDDL conversion failed");
        assert!(!sd.is_empty());

        // A self-relative descriptor starts with Revision = 1, Sbz1 = 0, then a
        // little-endian control word. SE_SELF_RELATIVE is 0x8000.
        assert_eq!(sd[0], 1, "security descriptor revision");
        let control = u16::from_le_bytes([sd[2], sd[3]]);
        assert_ne!(control & 0x8000, 0, "descriptor must be self-relative");
        // SE_DACL_PRESENT is 0x0004; the SDDL above defines a DACL.
        assert_ne!(control & 0x0004, 0, "descriptor must carry a DACL");
        // SE_DACL_PROTECTED is 0x1000; the "P" flag in the SDDL sets it.
        assert_ne!(control & 0x1000, 0, "the P flag must protect the DACL");
    }

    #[test]
    fn conversion_is_deterministic() {
        // It is converted once at startup and held; if it were not stable, the
        // size reported to a caller could differ from the bytes later copied.
        let a = default_descriptor().unwrap();
        let b = default_descriptor().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn the_descriptor_is_small_enough_for_a_typical_query_buffer() {
        // Not a hard requirement, but a descriptor of surprising size would
        // mean the SDDL changed without anyone noticing.
        let sd = default_descriptor().unwrap();
        assert!(sd.len() < 1024, "unexpectedly large descriptor: {}", sd.len());
    }
}
