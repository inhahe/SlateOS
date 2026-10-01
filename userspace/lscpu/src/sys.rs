//! What upstream asks the kernel and the processor directly: `uname`, the
//! raw `sched_getaffinity` system call, `personality`, and `cpuid`.
//!
//! On SlateOS, whose Rust target reports `target_os = "linux"`, these are
//! the SlateOS C library's; on the Windows host the unit tests run on, each
//! gives the answer of a machine that has none of them.

/// `uname().machine`.
///
/// # Errors
///
/// `uname` failed -- upstream's `error: uname failed`.
pub fn machine() -> std::io::Result<Vec<u8>> {
    imp::machine()
}

/// `get_max_number_of_cpus()`: the size of the kernel's CPU mask in bits,
/// as the raw `sched_getaffinity` returns it in bytes -- asked with ever
/// larger buffers while it says `EINVAL` -- or a negative number when it
/// fails otherwise.
#[must_use]
pub fn max_number_of_cpus() -> i32 {
    imp::max_number_of_cpus()
}

/// `personality(PER_LINUX32)` succeeding (and being undone): this aarch64
/// kernel runs 32-bit programs.
#[must_use]
pub fn has_aarch32() -> bool {
    imp::has_aarch32()
}

/// `read_hypervisor_cpuid()`'s signature: `cpuid` leaf `0x40000000`'s
/// vendor string, twelve bytes, or `None` where there is no `cpuid`.
#[must_use]
#[allow(
    clippy::unnecessary_wraps,
    reason = "`None` on every architecture but x86-64"
)]
pub fn hypervisor_signature() -> Option<[u8; 12]> {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: `cpuid` exists on every x86-64 processor and only writes
        // the four registers the intrinsic returns.
        #[allow(
            unused_unsafe,
            reason = "safe in newer toolchains, unsafe in older ones"
        )]
        let r = unsafe { core::arch::x86_64::__cpuid_count(0x4000_0000, 0) };
        let mut id = [0u8; 12];
        id[..4].copy_from_slice(&r.ebx.to_le_bytes());
        id[4..8].copy_from_slice(&r.ecx.to_le_bytes());
        id[8..].copy_from_slice(&r.edx.to_le_bytes());
        Some(id)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

#[cfg(unix)]
mod imp {
    use std::ffi::{c_char, c_int, c_long};
    use std::io;

    /// `_UTSNAME_LENGTH`, glibc's and the SlateOS C library's alike.
    const UTSNAME_LEN: usize = 65;

    /// `struct utsname`, six fields as Linux has them.
    #[repr(C)]
    struct Utsname {
        sysname: [c_char; UTSNAME_LEN],
        nodename: [c_char; UTSNAME_LEN],
        release: [c_char; UTSNAME_LEN],
        version: [c_char; UTSNAME_LEN],
        machine: [c_char; UTSNAME_LEN],
        domainname: [c_char; UTSNAME_LEN],
    }

    /// `EINVAL`.
    const EINVAL: i32 = 22;

    /// `SYS_sched_getaffinity` on this architecture.
    #[cfg(target_arch = "x86_64")]
    const SYS_SCHED_GETAFFINITY: c_long = 204;
    #[cfg(target_arch = "aarch64")]
    const SYS_SCHED_GETAFFINITY: c_long = 123;

    mod ffi {
        use std::ffi::{c_int, c_long};

        unsafe extern "C" {
            pub fn uname(buf: *mut super::Utsname) -> c_int;
            pub fn syscall(number: c_long, ...) -> c_long;
            #[cfg(target_arch = "aarch64")]
            pub fn personality(persona: std::ffi::c_ulong) -> c_int;
        }
    }

    pub fn machine() -> io::Result<Vec<u8>> {
        let mut buf = Utsname {
            sysname: [0; UTSNAME_LEN],
            nodename: [0; UTSNAME_LEN],
            release: [0; UTSNAME_LEN],
            version: [0; UTSNAME_LEN],
            machine: [0; UTSNAME_LEN],
            domainname: [0; UTSNAME_LEN],
        };
        // SAFETY: `buf` is a `struct utsname` of the size the C library
        // fills, owned here and valid for the whole call.
        let rc = unsafe { ffi::uname(&raw mut buf) };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        let bytes: Vec<u8> = buf.machine.iter().map(|&c| c as u8).collect();
        Ok(crate::cstr::c_str(&bytes).to_vec())
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn max_number_of_cpus() -> i32 {
        let mut cpus: usize = 2048;
        loop {
            let setsize = crate::cpuset::alloc_size(cpus);
            let mut set = vec![0u64; setsize / 8];
            // SAFETY: the raw system call writes at most `setsize` bytes to
            // `set`, which holds exactly that many and outlives the call.
            let n = unsafe {
                ffi::syscall(
                    SYS_SCHED_GETAFFINITY,
                    0 as c_long,
                    c_long::try_from(setsize).unwrap_or(c_long::MAX),
                    set.as_mut_ptr(),
                )
            };
            if n < 0
                && io::Error::last_os_error().raw_os_error() == Some(EINVAL)
                && cpus < 1024 * 1024
            {
                cpus = cpus.saturating_mul(2);
                continue;
            }
            // `int n = syscall(...)`, `return n * 8`.
            return (n as c_int).wrapping_mul(8);
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn max_number_of_cpus() -> i32 {
        -1
    }

    #[cfg(target_arch = "aarch64")]
    pub fn has_aarch32() -> bool {
        /// `PER_LINUX32`.
        const PER_LINUX32: std::ffi::c_ulong = 0x0008;
        // SAFETY: `personality` takes a value and touches no memory of ours.
        let pers = unsafe { ffi::personality(PER_LINUX32) };
        if pers == -1 {
            return false;
        }
        // SAFETY: as above; this restores the persona just replaced.
        unsafe { ffi::personality(pers as std::ffi::c_ulong) };
        true
    }

    #[cfg(not(target_arch = "aarch64"))]
    pub fn has_aarch32() -> bool {
        false
    }
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the same signatures as the unix module's, whose calls can fail"
)]
mod imp {
    pub fn machine() -> std::io::Result<Vec<u8>> {
        Ok(std::env::consts::ARCH.as_bytes().to_vec())
    }

    pub fn max_number_of_cpus() -> i32 {
        -1
    }

    pub fn has_aarch32() -> bool {
        false
    }
}
