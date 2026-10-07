//! The file mapping against stand-ins: memory from the host's allocator, a
//! file of the test's bytes, and a log of what was asked.

extern crate std;

use super::*;
use crate::fcntl::{O_RDONLY, O_RDWR, O_WRONLY};
use crate::mman::{MAP_FIXED, MAP_SHARED, PROT_EXEC, PROT_NONE};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::RefCell;
use std::string::String;
use std::vec::Vec;
use std::{format, vec};

/// The page the stand-in's memory is aligned to.
const PAGE: usize = 16384;

/// A file, the memory it is mapped into, and a kernel, in the test.
struct Fake {
    /// The file's bytes.
    file: Vec<u8>,
    /// The most bytes one read gives.
    chunk: usize,
    /// Errors the reads answer first, one each.
    read_errors: RefCell<Vec<i32>>,
    /// The descriptor is a directory.
    directory: bool,
    /// The anonymous mapping fails with this.
    anonymous_errno: Option<i32>,
    /// `mprotect` fails with this.
    protect_errno: Option<i32>,
    /// The memory given out, and not yet unmapped.
    memory: RefCell<Option<(*mut u8, usize)>>,
    /// What was asked, in order.
    log: RefCell<Vec<String>>,
}

impl Fake {
    fn file(bytes: &[u8]) -> Self {
        Self {
            file: bytes.to_vec(),
            chunk: usize::MAX,
            read_errors: RefCell::new(Vec::new()),
            directory: false,
            anonymous_errno: None,
            protect_errno: None,
            memory: RefCell::new(None),
            log: RefCell::new(Vec::new()),
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        if let Some((p, len)) = self.memory.borrow_mut().take() {
            // SAFETY: allocated by `anonymous` with this layout.
            unsafe { dealloc(p, Layout::from_size_align(len, PAGE).unwrap()) };
        }
    }
}

impl Calls for Fake {
    fn anonymous(&self, _addr: *mut c_void, length: SizeT, flags: i32) -> *mut c_void {
        self.log
            .borrow_mut()
            .push(format!("anonymous {length} {:#x}", flags));
        if let Some(e) = self.anonymous_errno {
            errno::set_errno(e);
            return MAP_FAILED;
        }
        // Whole pages, as the kernel maps them.
        let whole = length.next_multiple_of(PAGE);
        let layout = Layout::from_size_align(whole, PAGE).unwrap();
        // SAFETY: `whole` is not zero (mmap refused a zero length already).
        let p = unsafe { alloc_zeroed(layout) };
        assert!(!p.is_null());
        *self.memory.borrow_mut() = Some((p, whole));
        p.cast()
    }

    fn read_at(&self, _fd: Fd, buf: *mut u8, count: SizeT, offset: OffT) -> SsizeT {
        self.log
            .borrow_mut()
            .push(format!("read {count} at {offset}"));
        if let Some(e) = self.read_errors.borrow_mut().pop() {
            errno::set_errno(e);
            return -1;
        }
        let at = usize::try_from(offset).unwrap();
        let rest = self.file.get(at..).unwrap_or(&[]);
        let n = rest.len().min(count).min(self.chunk);
        // SAFETY: the caller's buffer holds `count` bytes.
        unsafe { core::ptr::copy_nonoverlapping(rest.as_ptr(), buf, n) };
        SsizeT::try_from(n).unwrap()
    }

    fn protect(&self, _p: *mut c_void, _length: SizeT, prot: i32) -> i32 {
        self.log.borrow_mut().push(format!("protect {prot}"));
        match self.protect_errno {
            Some(e) => {
                errno::set_errno(e);
                -1
            }
            None => 0,
        }
    }

    fn unmap(&self, p: *mut c_void, length: SizeT) -> i32 {
        self.log.borrow_mut().push(format!("unmap {length}"));
        let (mine, whole) = self.memory.borrow_mut().take().unwrap();
        assert_eq!(mine.cast::<c_void>(), p);
        assert_eq!(whole, length.next_multiple_of(PAGE));
        // SAFETY: allocated by `anonymous` with this layout.
        unsafe { dealloc(mine, Layout::from_size_align(whole, PAGE).unwrap()) };
        0
    }

    fn is_directory(&self, _fd: Fd) -> bool {
        self.directory
    }
}

/// A descriptor of `kind`, open with `flags`; closed when dropped.
struct TestFd(Fd);

impl TestFd {
    fn new(kind: HandleKind, flags: i32) -> Self {
        Self(fdtable::alloc_fd_with_flags(kind, 0x6d6d, flags).expect("fd table full"))
    }
}

impl Drop for TestFd {
    fn drop(&mut self) {
        let _ = fdtable::close_fd(self.0);
    }
}

/// Map `length` of `fake`'s file from `offset`: the bytes, or the errno.
fn mapped(
    fake: &Fake,
    fd: &TestFd,
    length: usize,
    prot: i32,
    flags: i32,
    offset: OffT,
) -> Result<Vec<u8>, i32> {
    errno::set_errno(0);
    let p = map_with(
        fake,
        core::ptr::null_mut(),
        length,
        prot,
        flags,
        fd.0,
        offset,
    );
    if p == MAP_FAILED {
        return Err(errno::get_errno());
    }
    // SAFETY: the stand-in's `length` bytes.
    Ok(unsafe { core::slice::from_raw_parts(p.cast::<u8>(), length) }.to_vec())
}

#[test]
fn a_private_mapping_is_the_file_then_zeros() {
    let fake = Fake::file(b"0123456789");
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    let bytes = mapped(&fake, &fd, PAGE, PROT_READ, MAP_PRIVATE, 0).unwrap();
    assert_eq!(bytes.get(..10), Some(&b"0123456789"[..]));
    assert!(bytes.get(10..).is_some_and(|t| t.iter().all(|&b| b == 0)));
    assert_eq!(
        fake.log(),
        [
            format!("anonymous {PAGE} {:#x}", MAP_PRIVATE | MAP_ANONYMOUS),
            format!("read {PAGE} at 0"),
            format!("read {} at 10", PAGE - 10),
            format!("protect {PROT_READ}"),
        ]
    );
}

#[test]
fn an_offset_maps_from_there() {
    let file: Vec<u8> = (0..20_000u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let fake = Fake::file(&file);
    let fd = TestFd::new(HandleKind::File, O_RDWR);
    let bytes = mapped(&fake, &fd, PAGE, PROT_READ | PROT_EXEC, MAP_PRIVATE, 16384).unwrap();
    assert_eq!(bytes.get(..20_000 - 16384), file.get(16384..));
    assert!(
        bytes
            .get(20_000 - 16384..)
            .is_some_and(|t| t.iter().all(|&b| b == 0))
    );
}

/// Linux maps whole pages: past `length`, to the end of its last page, the
/// file's bytes are there too where the file has them, and zeros after.
#[test]
fn the_last_page_is_mapped_whole_as_linux_maps_it() {
    let file: Vec<u8> = (0..1000u32)
        .map(|i| u8::try_from(i % 200 + 1).unwrap())
        .collect();
    let fake = Fake::file(&file);
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    errno::set_errno(0);
    let p = map_with(
        &fake,
        core::ptr::null_mut(),
        100,
        PROT_READ,
        MAP_PRIVATE,
        fd.0,
        0,
    );
    assert_ne!(p, MAP_FAILED);
    // SAFETY: the stand-in gave a whole page.
    let page = unsafe { core::slice::from_raw_parts(p.cast::<u8>(), PAGE) };
    assert_eq!(page.get(..1000), Some(&file[..]));
    assert!(page.get(1000..).is_some_and(|t| t.iter().all(|&b| b == 0)));
    assert_eq!(fake.log().get(1), Some(&format!("read {PAGE} at 0")));
}

#[test]
fn short_reads_and_interruptions_are_read_through() {
    let mut fake = Fake::file(b"abcdefghij");
    fake.chunk = 3;
    fake.read_errors = RefCell::new(vec![errno::EINTR]);
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    let bytes = mapped(&fake, &fd, PAGE, PROT_READ, MAP_PRIVATE, 0).unwrap();
    assert_eq!(bytes.get(..10), Some(&b"abcdefghij"[..]));
}

#[test]
fn a_writable_private_mapping_keeps_the_protection_it_was_made_with() {
    let fake = Fake::file(b"x");
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    assert!(mapped(&fake, &fd, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE, 0).is_ok());
    assert!(!fake.log().iter().any(|l| l.starts_with("protect")));
}

#[test]
fn placement_flags_are_the_callers() {
    let fake = Fake::file(b"x");
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    assert!(mapped(&fake, &fd, PAGE, PROT_READ, MAP_PRIVATE | MAP_FIXED, 0).is_ok());
    assert_eq!(
        fake.log().first(),
        Some(&format!(
            "anonymous {PAGE} {:#x}",
            MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS
        ))
    );
}

#[test]
fn a_failed_read_or_protect_unmaps_and_reports_it() {
    let fake = Fake::file(b"0123456789");
    fake.read_errors.borrow_mut().push(errno::EIO);
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    assert_eq!(
        mapped(&fake, &fd, PAGE, PROT_READ, MAP_PRIVATE, 0),
        Err(errno::EIO)
    );
    assert_eq!(fake.log().last(), Some(&format!("unmap {PAGE}")));

    let mut fake = Fake::file(b"0123456789");
    fake.protect_errno = Some(errno::ENOMEM);
    assert_eq!(
        mapped(&fake, &fd, PAGE, PROT_NONE, MAP_PRIVATE, 0),
        Err(errno::ENOMEM)
    );
    assert_eq!(fake.log().last(), Some(&format!("unmap {PAGE}")));

    let mut fake = Fake::file(b"0123456789");
    fake.anonymous_errno = Some(errno::ENOMEM);
    assert_eq!(
        mapped(&fake, &fd, PAGE, PROT_READ, MAP_PRIVATE, 0),
        Err(errno::ENOMEM)
    );
    assert_eq!(fake.log().len(), 1, "nothing read when there is no memory");
}

#[test]
fn a_shared_read_only_mapping_is_the_file_too() {
    let fake = Fake::file(b"shared");
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    let bytes = mapped(&fake, &fd, PAGE, PROT_READ, MAP_SHARED, 0).unwrap();
    assert_eq!(bytes.get(..6), Some(&b"shared"[..]));
}

#[test]
fn what_cannot_be_mapped_is_refused_in_linuxs_order() {
    let rd = TestFd::new(HandleKind::File, O_RDONLY);
    let wr = TestFd::new(HandleKind::File, O_WRONLY);
    let rw = TestFd::new(HandleKind::File, O_RDWR);
    let pipe_rd = TestFd::new(HandleKind::Pipe, O_RDONLY);
    let pipe_wr = TestFd::new(HandleKind::Pipe, O_WRONLY);
    let shared_rw = PROT_READ | PROT_WRITE;
    let cases: [(&TestFd, i32, i32, OffT, i32); 9] = [
        // offset + length past the largest file; a negative offset
        (
            &rd,
            PROT_READ,
            MAP_PRIVATE,
            i64::MAX - 100,
            errno::EOVERFLOW,
        ),
        (&rd, PROT_READ, MAP_PRIVATE, -16384, errno::EOVERFLOW),
        // shared and writable, not open for writing -- before "not readable"
        (&rd, shared_rw, MAP_SHARED, 0, errno::EACCES),
        // not open for reading, private or shared
        (&wr, PROT_READ, MAP_PRIVATE, 0, errno::EACCES),
        (&wr, shared_rw, MAP_SHARED, 0, errno::EACCES),
        // a pipe's write end is unreadable before it is unmappable
        (&pipe_wr, PROT_READ, MAP_PRIVATE, 0, errno::EACCES),
        (&pipe_rd, PROT_READ, MAP_PRIVATE, 0, errno::ENODEV),
        // growing protections are no file's
        (
            &rd,
            PROT_READ | PROT_GROWSDOWN,
            MAP_PRIVATE,
            0,
            errno::EINVAL,
        ),
        // shared and writable on a writable descriptor: this library's
        (&rw, shared_rw, MAP_SHARED, 0, errno::ENODEV),
    ];
    for (fd, prot, flags, offset, want) in cases {
        let fake = Fake::file(b"x");
        assert_eq!(
            mapped(&fake, fd, PAGE, prot, flags, offset),
            Err(want),
            "prot {prot:#x} flags {flags:#x} offset {offset}"
        );
        assert!(fake.log().is_empty(), "nothing asked of the kernel");
    }
    let mut directory = Fake::file(b"");
    directory.directory = true;
    assert_eq!(
        mapped(&directory, &rd, PAGE, PROT_READ, MAP_PRIVATE, 0),
        Err(errno::ENODEV)
    );
    let closed = TestFd::new(HandleKind::File, O_RDONLY);
    let n = closed.0;
    drop(closed);
    errno::set_errno(0);
    let p = map_with(
        &Fake::file(b""),
        core::ptr::null_mut(),
        PAGE,
        PROT_READ,
        MAP_PRIVATE,
        n,
        0,
    );
    assert_eq!((p, errno::get_errno()), (MAP_FAILED, errno::EBADF));
}

/// `mmap` itself sends a file mapping here: a pipe is refused as one, before
/// anything reaches the kernel -- where, until 2026-10-06, the descriptor
/// was dropped and anonymous memory came back.
#[test]
fn mmap_maps_a_file_rather_than_anonymous_memory() {
    let pipe = TestFd::new(HandleKind::Pipe, O_RDONLY);
    errno::set_errno(0);
    let p = crate::mman::mmap(
        core::ptr::null_mut(),
        PAGE,
        PROT_READ,
        MAP_PRIVATE,
        pipe.0,
        0,
    );
    assert_eq!((p, errno::get_errno()), (MAP_FAILED, errno::ENODEV));
}
