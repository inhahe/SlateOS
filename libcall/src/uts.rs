//! `uname(2)` through the C library: the system's name, its host's, its
//! release and version, the machine, and the NIS domain.
//!
//! The struct is glibc's on Linux and SlateOS's C library's, which keeps the
//! same layout (`posix/src/utsname.rs`): six fields of 65 bytes, each a
//! NUL-terminated string.

/// `_UTSNAME_LENGTH`: each field's size, its NUL included.
pub const UTSNAME_LENGTH: usize = 65;

/// `struct utsname`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Uname {
    sysname: [u8; UTSNAME_LENGTH],
    nodename: [u8; UTSNAME_LENGTH],
    release: [u8; UTSNAME_LENGTH],
    version: [u8; UTSNAME_LENGTH],
    machine: [u8; UTSNAME_LENGTH],
    domainname: [u8; UTSNAME_LENGTH],
}

/// A field up to its NUL -- the whole field if the library wrote none.
fn field(f: &[u8; UTSNAME_LENGTH]) -> &[u8] {
    let end = f.iter().position(|&b| b == 0).unwrap_or(f.len());
    f.get(..end).unwrap_or_default()
}

impl Uname {
    /// `sysname`.
    #[must_use]
    pub fn sysname(&self) -> &[u8] {
        field(&self.sysname)
    }
    /// `nodename`.
    #[must_use]
    pub fn nodename(&self) -> &[u8] {
        field(&self.nodename)
    }
    /// `release`.
    #[must_use]
    pub fn release(&self) -> &[u8] {
        field(&self.release)
    }
    /// `version`.
    #[must_use]
    pub fn version(&self) -> &[u8] {
        field(&self.version)
    }
    /// `machine`.
    #[must_use]
    pub fn machine(&self) -> &[u8] {
        field(&self.machine)
    }
    /// `domainname` (`__domainname`).
    #[must_use]
    pub fn domainname(&self) -> &[u8] {
        field(&self.domainname)
    }
}

#[cfg(unix)]
mod sys {
    use super::Uname;
    unsafe extern "C" {
        pub fn uname(buf: *mut Uname) -> i32;
    }
}

/// `uname (&buf)`.
///
/// # Errors
///
/// The `errno` it set -- `EFAULT` only, which a buffer of our own cannot
/// cause; [`ENOSYS`](crate::ENOSYS) on a host with no C library of ours.
pub fn uname() -> Result<Uname, i32> {
    let mut buf = Uname {
        sysname: [0; UTSNAME_LENGTH],
        nodename: [0; UTSNAME_LENGTH],
        release: [0; UTSNAME_LENGTH],
        version: [0; UTSNAME_LENGTH],
        machine: [0; UTSNAME_LENGTH],
        domainname: [0; UTSNAME_LENGTH],
    };
    #[cfg(unix)]
    {
        // SAFETY: `buf` is a `struct utsname` of the C library's layout, ours
        // and writable; the call fills it and keeps no pointer to it.
        if unsafe { sys::uname(&raw mut buf) } != 0 {
            return Err(crate::last_errno());
        }
        Ok(buf)
    }
    #[cfg(not(unix))]
    {
        let _ = &mut buf;
        Err(crate::ENOSYS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_ends_at_its_nul_or_its_end() {
        let mut f = [b'x'; UTSNAME_LENGTH];
        assert_eq!(field(&f).len(), UTSNAME_LENGTH);
        f[5] = 0;
        assert_eq!(field(&f), b"xxxxx");
    }

    /// The layout is the C library's: the one SlateOS's keeps.
    #[test]
    fn the_struct_is_six_fields_of_65_bytes() {
        assert_eq!(core::mem::size_of::<Uname>(), 6 * 65);
    }

    #[cfg(unix)]
    #[test]
    fn the_running_system_has_a_name() {
        let u = uname().unwrap_or_else(|e| panic!("uname: {e}"));
        assert!(!u.sysname().is_empty());
        assert!(!u.machine().is_empty());
    }
}
