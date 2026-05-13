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

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    // -----------------------------------------------------------------------
    // Struct layout
    // -----------------------------------------------------------------------

    #[test]
    fn utsname_len_is_65() {
        assert_eq!(UTSNAME_LEN, 65);
    }

    #[test]
    fn utsname_struct_size() {
        // 5 fields * 65 bytes each = 325 bytes.
        assert_eq!(mem::size_of::<Utsname>(), 5 * 65);
    }

    // -----------------------------------------------------------------------
    // uname — success case
    // -----------------------------------------------------------------------

    #[test]
    fn uname_returns_zero_on_success() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        let ret = uname(&mut uts as *mut Utsname);
        assert_eq!(ret, 0);
    }

    #[test]
    fn uname_fills_sysname() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);
        assert_eq!(&uts.sysname[..8], b"CustomOS");
        // Null terminated after the string.
        assert_eq!(uts.sysname[8], 0);
    }

    #[test]
    fn uname_fills_nodename() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);
        assert_eq!(&uts.nodename[..9], b"localhost");
        assert_eq!(uts.nodename[9], 0);
    }

    #[test]
    fn uname_fills_release() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);
        assert_eq!(&uts.release[..5], b"0.1.0");
        assert_eq!(uts.release[5], 0);
    }

    #[test]
    fn uname_fills_version() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);
        assert_eq!(&uts.version[..6], b"#1 SMP");
        assert_eq!(uts.version[6], 0);
    }

    #[test]
    fn uname_fills_machine() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);
        assert_eq!(&uts.machine[..6], b"x86_64");
        assert_eq!(uts.machine[6], 0);
    }

    // -----------------------------------------------------------------------
    // uname — null pointer
    // -----------------------------------------------------------------------

    #[test]
    fn uname_null_returns_negative_one() {
        let ret = uname(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // fill_field — null termination
    // -----------------------------------------------------------------------

    #[test]
    fn fill_field_null_terminates_short_string() {
        let mut field = [0xFFu8; UTSNAME_LEN];
        fill_field(&mut field, b"abc");
        assert_eq!(&field[..3], b"abc");
        assert_eq!(field[3], 0); // null terminator
    }

    #[test]
    fn fill_field_null_terminates_empty_string() {
        let mut field = [0xFFu8; UTSNAME_LEN];
        fill_field(&mut field, b"");
        assert_eq!(field[0], 0); // immediately null-terminated
    }

    #[test]
    fn fill_field_truncates_long_string() {
        // String longer than UTSNAME_LEN - 1 (64 chars max).
        let long = [b'X'; 128];
        let mut field = [0u8; UTSNAME_LEN];
        fill_field(&mut field, &long);
        // First 64 bytes should be 'X'.
        for byte in &field[..64] {
            assert_eq!(*byte, b'X');
        }
        // Byte 64 (index UTSNAME_LEN - 1) must be null terminator.
        assert_eq!(field[64], 0);
    }

    #[test]
    fn fill_field_exact_max_length() {
        // Exactly 64 bytes — fills completely and null-terminates at [64].
        let exact = [b'A'; 64];
        let mut field = [0u8; UTSNAME_LEN];
        fill_field(&mut field, &exact);
        for byte in &field[..64] {
            assert_eq!(*byte, b'A');
        }
        assert_eq!(field[64], 0);
    }

    #[test]
    fn fill_field_preserves_content() {
        let mut field = [0u8; UTSNAME_LEN];
        fill_field(&mut field, b"hello");
        assert_eq!(&field[..5], b"hello");
        assert_eq!(field[5], 0);
    }

    // -----------------------------------------------------------------------
    // uname — fields remain independent (no overlap)
    // -----------------------------------------------------------------------

    #[test]
    fn uname_fields_do_not_overlap() {
        let mut uts = unsafe { mem::zeroed::<Utsname>() };
        uname(&mut uts as *mut Utsname);

        // Each field should start with the correct string and not
        // bleed into adjacent fields.
        assert!(uts.sysname[0] == b'C');  // "CustomOS"
        assert!(uts.nodename[0] == b'l'); // "localhost"
        assert!(uts.release[0] == b'0');  // "0.1.0"
        assert!(uts.version[0] == b'#');  // "#1 SMP"
        assert!(uts.machine[0] == b'x');  // "x86_64"
    }

    // -----------------------------------------------------------------------
    // uname — zeroes padding after null terminator
    // -----------------------------------------------------------------------

    #[test]
    fn uname_zeroes_entire_struct_first() {
        // Pre-fill with garbage, then call uname.
        let mut uts: Utsname = unsafe { mem::zeroed() };
        // Fill with 0xFF.
        let ptr = &mut uts as *mut Utsname as *mut u8;
        unsafe { core::ptr::write_bytes(ptr, 0xFF, mem::size_of::<Utsname>()); }

        uname(&mut uts as *mut Utsname);

        // After "CustomOS" (8 bytes) + null at [8], bytes [9..65] should
        // be zero because uname zeroes the struct before filling.
        for &byte in &uts.sysname[9..] {
            assert_eq!(byte, 0, "sysname should be zeroed after the null terminator");
        }
    }
}
