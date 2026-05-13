//! POSIX system identification.
//!
//! Implements `uname()` and `struct utsname` for system identification.
//! Programs use this to determine the OS name, version, and machine type.

/// Length of each utsname field (including null terminator).
///
/// POSIX requires at least 9 bytes.  We use 65 to match Linux.
const UTSNAME_LEN: usize = 65;

/// System identification structure.
///
/// Returned by `uname()`.  Each field is a null-terminated C string.
#[repr(C)]
pub struct Utsname {
    /// Operating system name.
    pub sysname: [u8; UTSNAME_LEN],
    /// Network node hostname.
    pub nodename: [u8; UTSNAME_LEN],
    /// Operating system release.
    pub release: [u8; UTSNAME_LEN],
    /// Operating system version.
    pub version: [u8; UTSNAME_LEN],
    /// Hardware type.
    pub machine: [u8; UTSNAME_LEN],
}

/// Copy a byte string into a fixed-size utsname field, null-terminated.
fn fill_field(field: &mut [u8; UTSNAME_LEN], src: &[u8]) {
    let mut i: usize = 0;
    let limit = if src.len() < UTSNAME_LEN.wrapping_sub(1) {
        src.len()
    } else {
        UTSNAME_LEN.wrapping_sub(1)
    };

    while i < limit {
        if let (Some(dst), Some(&b)) = (field.get_mut(i), src.get(i)) {
            *dst = b;
        }
        i = i.wrapping_add(1);
    }

    // Null-terminate.
    if let Some(slot) = field.get_mut(i) {
        *slot = 0;
    }
}

/// Get system identification.
///
/// Fills the `utsname` structure pointed to by `buf` with information
/// about the operating system and hardware.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn uname(buf: *mut Utsname) -> i32 {
    if buf.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }

    // SAFETY: Caller guarantees `buf` is valid for writing a Utsname.
    let uts = unsafe { &mut *buf };

    // Zero the entire struct first.
    uts.sysname = [0u8; UTSNAME_LEN];
    uts.nodename = [0u8; UTSNAME_LEN];
    uts.release = [0u8; UTSNAME_LEN];
    uts.version = [0u8; UTSNAME_LEN];
    uts.machine = [0u8; UTSNAME_LEN];

    fill_field(&mut uts.sysname, b"CustomOS");
    fill_field(&mut uts.nodename, b"localhost");
    fill_field(&mut uts.release, b"0.1.0");
    fill_field(&mut uts.version, b"#1 SMP");
    fill_field(&mut uts.machine, b"x86_64");

    0
}
