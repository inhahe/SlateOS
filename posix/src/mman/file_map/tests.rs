//! The file mapping against a stand-in kernel, which answers what the test
//! says and logs what it was asked.

extern crate std;

use super::*;
use crate::fcntl::{O_RDONLY, O_RDWR, O_WRONLY};
use crate::mman::{MAP_FIXED, MAP_SHARED, PROT_READ};
use std::cell::RefCell;
use std::format;
use std::string::String;
use std::vec::Vec;

/// The page a mapping is made of here.
const PAGE: usize = 16384;

/// The handle the test's descriptors hold.
const HANDLE: u64 = 0x6d6d;

/// A kernel, in the test.
struct Fake {
    /// What `SYS_MMAP_FILE` answers.
    answer: i64,
    /// The descriptor is a directory.
    directory: bool,
    /// What was asked, in order.
    log: RefCell<Vec<String>>,
}

impl Fake {
    fn answering(answer: i64) -> Self {
        Self {
            answer,
            directory: false,
            log: RefCell::new(Vec::new()),
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl Calls for Fake {
    fn map_file(
        &self,
        addr: *mut c_void,
        length: SizeT,
        prot: i32,
        flags: i32,
        handle: u64,
        offset: OffT,
    ) -> i64 {
        self.log.borrow_mut().push(format!(
            "map_file {:#x} {length} {prot:#x} {flags:#x} {handle:#x} {offset}",
            addr as usize
        ));
        self.answer
    }

    fn is_directory(&self, _fd: Fd) -> bool {
        self.directory
    }
}

/// A descriptor of `kind`, open with `flags`; closed when dropped.
struct TestFd(Fd);

impl TestFd {
    fn new(kind: HandleKind, flags: i32) -> Self {
        Self(fdtable::alloc_fd_with_flags(kind, HANDLE, flags).expect("fd table full"))
    }
}

impl Drop for TestFd {
    fn drop(&mut self) {
        let _ = fdtable::close_fd(self.0);
    }
}

/// Map `length` of `fd`'s file from `offset` with `fake`: the address, or
/// the errno.
fn mapped(
    fake: &Fake,
    fd: &TestFd,
    length: usize,
    prot: i32,
    flags: i32,
    offset: OffT,
) -> Result<usize, i32> {
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
    Ok(p as usize)
}

/// A mapping that passes this library's refusals is the kernel's, asked
/// with the caller's own address, length, protection, flags and offset, and
/// the descriptor's handle.
#[test]
fn a_file_mapping_is_the_kernels() {
    let fd = TestFd::new(HandleKind::File, O_RDONLY);
    let fake = Fake::answering(0x7000_0000);
    errno::set_errno(0);
    let p = map_with(
        &fake,
        0x4000_0000 as *mut c_void,
        3 * PAGE,
        PROT_READ,
        MAP_PRIVATE | MAP_FIXED,
        fd.0,
        2 * PAGE as OffT,
    );
    assert_eq!(p as usize, 0x7000_0000);
    assert_eq!(
        fake.log(),
        [format!(
            "map_file 0x40000000 {} {PROT_READ:#x} {:#x} {HANDLE:#x} {}",
            3 * PAGE,
            MAP_PRIVATE | MAP_FIXED,
            2 * PAGE
        )]
    );
}

/// A shared, writable mapping of a descriptor open for writing is the
/// kernel's to grant: until 2026-10-17 this library refused it itself.
#[test]
fn a_shared_writable_mapping_is_the_kernels_to_grant() {
    let rw = TestFd::new(HandleKind::File, O_RDWR);
    let fake = Fake::answering(0x7100_0000);
    let shared_rw = PROT_READ | PROT_WRITE;
    assert_eq!(
        mapped(&fake, &rw, PAGE, shared_rw, MAP_SHARED, 0),
        Ok(0x7100_0000)
    );
    assert_eq!(fake.log().len(), 1);
}

/// The kernel's errnos are passed on, but its `ENOSYS` -- a writable shared
/// mapping it cannot save yet, or no such call -- is `ENODEV`, which is
/// what Linux's `mmap` answers for what cannot be mapped.
#[test]
fn the_kernels_refusals_are_passed_on_and_enosys_is_enodev() {
    let rw = TestFd::new(HandleKind::File, O_RDWR);
    let shared_rw = PROT_READ | PROT_WRITE;
    for (answer, want) in [
        (-13, errno::EACCES),
        (-12, errno::ENOMEM),
        (-38, errno::ENODEV),
        (errno::native::NO_SUCH_SYSCALL, errno::ENODEV),
    ] {
        let fake = Fake::answering(answer);
        assert_eq!(
            mapped(&fake, &rw, PAGE, shared_rw, MAP_SHARED, 0),
            Err(want),
            "answer {answer}"
        );
    }
}

#[test]
fn what_cannot_be_mapped_is_refused_in_linuxs_order() {
    let rd = TestFd::new(HandleKind::File, O_RDONLY);
    let wr = TestFd::new(HandleKind::File, O_WRONLY);
    let pipe_rd = TestFd::new(HandleKind::Pipe, O_RDONLY);
    let pipe_wr = TestFd::new(HandleKind::Pipe, O_WRONLY);
    let shared_rw = PROT_READ | PROT_WRITE;
    let cases: [(&TestFd, i32, i32, OffT, i32); 8] = [
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
    ];
    for (fd, prot, flags, offset, want) in cases {
        let fake = Fake::answering(0x7000_0000);
        assert_eq!(
            mapped(&fake, fd, PAGE, prot, flags, offset),
            Err(want),
            "prot {prot:#x} flags {flags:#x} offset {offset}"
        );
        assert!(fake.log().is_empty(), "nothing asked of the kernel");
    }
    let mut directory = Fake::answering(0x7000_0000);
    directory.directory = true;
    assert_eq!(
        mapped(&directory, &rd, PAGE, PROT_READ, MAP_PRIVATE, 0),
        Err(errno::ENODEV)
    );
    assert!(directory.log().is_empty());
    let closed = TestFd::new(HandleKind::File, O_RDONLY);
    let n = closed.0;
    drop(closed);
    errno::set_errno(0);
    let p = map_with(
        &Fake::answering(0x7000_0000),
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
