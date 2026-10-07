//! Pages that fault, for the tests of the functions that read past a
//! string's end within its page: `string.rs`'s and `wchar.rs`'s scanners,
//! which read whole aligned 16-byte blocks.
//!
//! [`Guarded`] is a readable, writable page between two that may not be
//! touched.  A string set against an edge of it, with the inaccessible page
//! beyond, shows by not crashing that a function read nothing outside its
//! string's page; one that does faults, and takes the test process with it --
//! a failure nobody can miss.  Two deliberate over-reads were tried against
//! these tests when they were written (2026-10-06), and both crashed them.
//!
//! Test-only, and only on the hosts the tests run on: Windows (`VirtualAlloc`)
//! and Linux (`mmap`).

/// The host's page size, which the faulting pages are made of.  Every
/// x86-64 Windows and Linux maps 4 KiB pages.
pub(crate) const HOST_PAGE: usize = 4096;

#[cfg(windows)]
mod host {
    use super::HOST_PAGE;
    use core::ffi::c_void;

    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const MEM_RELEASE: u32 = 0x8000;
    const PAGE_NOACCESS: u32 = 0x01;
    const PAGE_READWRITE: u32 = 0x04;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    }

    /// Three pages of address space, the middle one alone usable.
    pub(super) fn map() -> *mut u8 {
        // SAFETY: fresh address space reserved, then its middle page
        // committed.
        unsafe {
            let region = VirtualAlloc(
                core::ptr::null_mut(),
                3 * HOST_PAGE,
                MEM_RESERVE,
                PAGE_NOACCESS,
            );
            assert!(!region.is_null(), "VirtualAlloc reserve");
            let middle = region.cast::<u8>().add(HOST_PAGE).cast();
            let committed = VirtualAlloc(middle, HOST_PAGE, MEM_COMMIT, PAGE_READWRITE);
            assert!(!committed.is_null(), "VirtualAlloc commit");
            region.cast()
        }
    }

    pub(super) fn unmap(region: *mut u8) {
        // SAFETY: what `map` reserved, released whole (size 0, as
        // MEM_RELEASE requires).
        let released = unsafe { VirtualFree(region.cast(), 0, MEM_RELEASE) };
        assert_ne!(released, 0, "VirtualFree");
    }
}

#[cfg(target_os = "linux")]
mod host {
    use super::HOST_PAGE;
    use core::ffi::c_void;

    const PROT_NONE: i32 = 0;
    const PROT_READ: i32 = 1;
    const PROT_WRITE: i32 = 2;
    const MAP_PRIVATE: i32 = 0x02;
    const MAP_ANONYMOUS: i32 = 0x20;

    unsafe extern "C" {
        fn mmap(
            address: *mut c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut c_void;
        fn mprotect(address: *mut c_void, len: usize, prot: i32) -> i32;
        fn munmap(address: *mut c_void, len: usize) -> i32;
    }

    /// Three pages of address space, the middle one alone usable.
    pub(super) fn map() -> *mut u8 {
        // SAFETY: a fresh mapping, then its middle page opened.
        unsafe {
            let region = mmap(
                core::ptr::null_mut(),
                3 * HOST_PAGE,
                PROT_NONE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            );
            assert_ne!(region as isize, -1, "mmap");
            let middle = region.cast::<u8>().add(HOST_PAGE).cast();
            assert_eq!(
                mprotect(middle, HOST_PAGE, PROT_READ | PROT_WRITE),
                0,
                "mprotect"
            );
            region.cast()
        }
    }

    pub(super) fn unmap(region: *mut u8) {
        // SAFETY: what `map` mapped, unmapped whole.
        let unmapped = unsafe { munmap(region.cast(), 3 * HOST_PAGE) };
        assert_eq!(unmapped, 0, "munmap");
    }
}

/// Three pages, of which only the middle one may be touched.
pub(crate) struct Guarded {
    region: *mut u8,
}

impl Guarded {
    pub(crate) fn new() -> Self {
        Self {
            region: host::map(),
        }
    }

    /// The usable page's first byte.
    pub(crate) fn start(&self) -> *mut u8 {
        self.region.wrapping_add(HOST_PAGE)
    }

    /// One past its last.
    pub(crate) fn end(&self) -> *mut u8 {
        self.region.wrapping_add(2 * HOST_PAGE)
    }

    /// `bytes` written to end where the page does: where they start.
    pub(crate) fn at_end(&self, bytes: &[u8]) -> *mut u8 {
        assert!(bytes.len() <= HOST_PAGE, "more than a page");
        let start = self.end().wrapping_sub(bytes.len());
        // SAFETY: at most a page of bytes, into the usable page.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), start, bytes.len()) };
        start
    }

    /// `bytes` written at the page's start.
    pub(crate) fn at_start(&self, bytes: &[u8]) -> *mut u8 {
        assert!(bytes.len() <= HOST_PAGE, "more than a page");
        // SAFETY: at most a page of bytes, into the usable page.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.start(), bytes.len()) };
        self.start()
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        host::unmap(self.region);
    }
}
