//! Descriptors passed by native programs over Unix-domain sockets: the native
//! half of `SCM_RIGHTS` (known-issues `A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS`).
//!
//! A native program's descriptor table lives in its C library, not in the
//! kernel. The kernel sees a descriptor as the pair the library keeps for it:
//! a type -- [`fd_handle_type`], the codes a spawn's `fd_map` uses -- and a
//! handle the process holds (its `ipc_handles`). So a native send names the
//! descriptors it carries as such pairs ([`NativeRight`]), and a native
//! receive hands back pairs for the library to put in its table.
//!
//! In between, a descriptor travels exactly as a Linux program's does: as a
//! [`Passed`] reference in the message's [`Bundle`], charged to the sender's
//! user while in flight, dropped if never received. A native receiver can
//! therefore take what a Linux sender sent and the reverse, for the kinds the
//! native C library has a type for -- files, pipes, the console, eventfds and
//! Unix sockets. Anything else that arrives (an epoll, a memfd...) has no
//! native form and is dropped, which the receive reports as Linux reports a
//! descriptor dropped for want of room (`MSG_CTRUNC`).
//!
//! [`fd_handle_type`]: crate::proc::spawn::fd_handle_type

use super::passed::{self, Bundle, Passed};
use crate::error::{KernelError, KernelResult};
use crate::proc::linux_fd::{FdEntry, HandleKind};
use crate::proc::pcb::{self, ProcessId};
use crate::proc::spawn::fd_handle_type;
use alloc::vec::Vec;

/// One descriptor as a native program names it: the handle, its type (an
/// [`fd_handle_type`] code) and the open file description's status flags
/// (`O_APPEND`, `O_NONBLOCK`...), which travel with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRight {
    /// The kernel handle, one the sending process holds.
    pub handle: u64,
    /// Its type: an [`fd_handle_type`] code.
    pub kind: u8,
    /// The open file description's status flags.
    pub status_flags: u32,
}

/// The kind of descriptor a native type code names, if it can travel.
#[must_use]
pub const fn handle_kind_of(native: u8) -> Option<HandleKind> {
    match native {
        fd_handle_type::FILE => Some(HandleKind::File),
        fd_handle_type::PIPE => Some(HandleKind::Pipe),
        fd_handle_type::CONSOLE => Some(HandleKind::Console),
        fd_handle_type::EVENTFD => Some(HandleKind::EventFd),
        fd_handle_type::UNIX_SOCKET => Some(HandleKind::UnixSocket),
        // The kernel's own TCP/UDP sockets, stream-socket endpoints and
        // ptys have no form a Linux receiver could take either; none
        // travels.
        _ => None,
    }
}

/// The native type code for a descriptor of `kind`, if the native C library
/// has one.
#[must_use]
pub const fn native_type_of(kind: HandleKind) -> Option<u8> {
    match kind {
        HandleKind::File => Some(fd_handle_type::FILE),
        HandleKind::Pipe => Some(fd_handle_type::PIPE),
        HandleKind::Console => Some(fd_handle_type::CONSOLE),
        HandleKind::EventFd => Some(fd_handle_type::EVENTFD),
        HandleKind::UnixSocket => Some(fd_handle_type::UNIX_SOCKET),
        _ => None,
    }
}

/// Take a reference to each descriptor `rights` names -- each one process
/// `pid` holds -- for a send to carry, charged to `uid` until it is received
/// or dropped: the [`Bundle`] a native send passes on, `None` for none.
///
/// All or nothing: every reference taken goes back if any is refused.
///
/// # Errors
///
/// - `InvalidArgument` for more than [`passed::MAX_PER_MESSAGE`].
/// - `NotSupported` for a type no descriptor of which can travel.
/// - `InvalidHandle` for a handle `pid` does not hold, or whose object has
///   ended.
/// - `TooManyReferences` when `uid` -- not root -- already has more
///   descriptors in flight than `pid`'s `RLIMIT_NOFILE` (Linux's
///   `ETOOMANYREFS`).
pub fn take(pid: ProcessId, uid: u32, rights: &[NativeRight]) -> KernelResult<Option<Bundle>> {
    if rights.is_empty() {
        return Ok(None);
    }
    if rights.len() > passed::MAX_PER_MESSAGE {
        return Err(KernelError::InvalidArgument);
    }
    let mut taken = Vec::new();
    taken
        .try_reserve(rights.len())
        .map_err(|_| KernelError::OutOfMemory)?;
    let mut refused = None;
    for right in rights {
        match take_one(pid, right) {
            Ok(p) => taken.push(p),
            Err(e) => {
                refused = Some(e);
                break;
            }
        }
    }
    let nofile = pcb::get_rlimit(pid, 7).map_or(u64::MAX, |(cur, _)| cur);
    if refused.is_none() && uid != 0 && u64::from(passed::in_flight_for(uid)) > nofile {
        refused = Some(KernelError::TooManyReferences);
    }
    if let Some(e) = refused {
        // What was taken goes back.
        drop(taken);
        passed::drain();
        return Err(e);
    }
    for p in &mut taken {
        p.charge(uid);
    }
    Ok(Bundle::new(taken))
}

/// One reference to what `right` names, if `pid` holds it.
fn take_one(pid: ProcessId, right: &NativeRight) -> KernelResult<Passed> {
    let kind = handle_kind_of(right.kind).ok_or(KernelError::NotSupported)?;
    if let Some(resource) = kind.resource_type()
        && !pcb::owns_ipc_handle(pid, resource, right.handle)
    {
        return Err(KernelError::InvalidHandle);
    }
    Passed::take(FdEntry::of(kind, right.handle, right.status_flags))
}

/// What a native receive landed: the descriptors, in the order they were
/// sent, and whether any was dropped -- no room for it, or no native form.
#[derive(Debug, Default)]
pub struct Landed {
    /// The descriptors the receiver now holds, as it names them.
    pub rights: Vec<NativeRight>,
    /// Some were dropped (`MSG_CTRUNC`).
    pub truncated: bool,
}

/// Land in process `pid` what `bundle` carries, at most `room` of them, and
/// `publish` the result to the program: each becomes a reference `pid`
/// holds, among its handles, unless it held the object already (one
/// reference per object: the one brought is dropped, and the handle it
/// already had is reported). What does not fit, has no native form, or
/// cannot be recorded is dropped, and the result says so.
///
/// If `publish` fails -- the program's memory went away under the receive --
/// nothing lands: a program that was never told a handle has no way to close
/// it, so each reference goes back as though there had been no room for it,
/// and `publish`'s error is returned. Every reference not landed is released
/// before this returns, either way.
///
/// # Errors
///
/// Whatever `publish` returns.
pub fn land(
    pid: ProcessId,
    bundle: Bundle,
    room: usize,
    publish: impl FnOnce(&Landed) -> KernelResult<()>,
) -> KernelResult<Landed> {
    let mut landed = Landed::default();
    // The references recorded as `pid`'s, kept until `publish` says the
    // program knows of them.
    let mut fresh = Vec::new();
    let room = room.min(passed::MAX_PER_MESSAGE);
    if landed.rights.try_reserve(room).is_err() || fresh.try_reserve(room).is_err() {
        // No memory for the lists: the receive delivers the data, and the
        // descriptors go as though there were no room for them.
        landed.truncated = true;
        drop(bundle);
        passed::drain();
        return publish(&landed).map(|()| landed);
    }
    for p in bundle.into_passed() {
        let entry = p.entry();
        let native = native_type_of(entry.kind);
        let Some(kind) = native.filter(|_| landed.rights.len() < room) else {
            landed.truncated = true;
            drop(p);
            continue;
        };
        match pcb::native_install_passed(pid, entry.kind, entry.raw_handle) {
            // The receiver's own reference stands for it; this one goes.
            Ok(true) => drop(p),
            Ok(false) => fresh.push(p),
            Err(_) => {
                landed.truncated = true;
                drop(p);
                continue;
            }
        }
        landed.rights.push(NativeRight {
            handle: entry.raw_handle,
            kind,
            status_flags: entry.status_flags,
        });
    }
    let published = publish(&landed);
    for p in fresh {
        let entry = p.entry();
        if published.is_err() && pcb::native_uninstall_passed(pid, entry.kind, entry.raw_handle) {
            drop(p);
        } else {
            // The reference is the receiver's now -- or was, and a close of
            // the handle it was told of has released it already.
            let _ = p.land();
        }
    }
    passed::drain();
    published.map(|()| landed)
}

/// Self-test: a pipe's write end passes from one process to another over a
/// socket pair -- refused from a process that does not hold it, refused for a
/// type that cannot travel -- and lands as the receiver's own; a second copy
/// of a held object lands as the handle already held; a receive with no room
/// drops what it cannot take and says so, releasing the references.
pub fn self_test() -> KernelResult<()> {
    use super::pipe;
    use super::unix_socket::{self, Kind};
    use crate::cap::ResourceType;

    fn fail(what: &str) -> KernelResult<()> {
        crate::serial_println!("[native_rights]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    let sender = pcb::create("rights-sender", 0);
    let receiver = pcb::create("rights-receiver", 0);
    let (rd, wr) = pipe::create();
    pcb::register_ipc_handle(sender, ResourceType::Pipe, wr.raw());
    let (s1, s2) = unix_socket::pair(Kind::Stream)?;
    let right = NativeRight {
        handle: wr.raw(),
        kind: fd_handle_type::PIPE,
        status_flags: 0,
    };
    let result = (|| {
        // Refused: a handle the process does not hold; a type that cannot
        // travel. Nothing is taken either way.
        if take(receiver, 0, &[right]).map(|_| ()) != Err(KernelError::InvalidHandle) {
            return fail("a handle the sender does not hold was not InvalidHandle");
        }
        let tcp = NativeRight {
            kind: fd_handle_type::TCP_SOCKET,
            ..right
        };
        if take(sender, 0, &[tcp]).map(|_| ()) != Err(KernelError::NotSupported) {
            return fail("a kernel TCP socket was not NotSupported");
        }
        // Received, but the program could not be told: nothing lands, and
        // the publish's error is the receive's.
        let bundle = take(sender, 0, &[right])?;
        unix_socket::send_as(s1, b"o", None, bundle, false)?;
        let mut buf = [0u8; 4];
        let got = unix_socket::recv(s2, &mut buf, true, false)?;
        let Some(bundle) = got.rights else {
            return fail("the first message arrived without its descriptor");
        };
        let unpublished = land(receiver, bundle, 4, |_| Err(KernelError::InvalidAddress));
        if unpublished.map(|_| ()) != Err(KernelError::InvalidAddress)
            || pcb::owns_ipc_handle(receiver, ResourceType::Pipe, wr.raw())
        {
            return fail("a landing the program was not told of stayed landed");
        }
        // Sent, received, landed: the receiver holds the write end.
        let bundle = take(sender, 0, &[right])?;
        unix_socket::send_as(s1, b"p", None, bundle, false)?;
        let got = unix_socket::recv(s2, &mut buf, true, false)?;
        let Some(bundle) = got.rights else {
            return fail("the message arrived without its descriptor");
        };
        let landed = land(receiver, bundle, 4, |_| Ok(()))?;
        if landed.truncated
            || landed.rights != [right]
            || !pcb::owns_ipc_handle(receiver, ResourceType::Pipe, wr.raw())
        {
            return fail("the write end did not land as the receiver's own");
        }
        // Again, to a receiver that holds it now: the same handle, no
        // second reference.
        let bundle = take(sender, 0, &[right])?;
        unix_socket::send_as(s1, b"q", None, bundle, false)?;
        let got = unix_socket::recv(s2, &mut buf, true, false)?;
        let Some(bundle) = got.rights else {
            return fail("the second message arrived without its descriptor");
        };
        let again = land(receiver, bundle, 4, |_| Ok(()))?;
        if again.rights != [right] || again.truncated {
            return fail("a held object did not land as the handle already held");
        }
        // No room: dropped, said, released.
        let bundle = take(sender, 0, &[right])?;
        unix_socket::send_as(s1, b"r", None, bundle, false)?;
        let got = unix_socket::recv(s2, &mut buf, true, false)?;
        let Some(bundle) = got.rights else {
            return fail("the third message arrived without its descriptor");
        };
        let none = land(receiver, bundle, 0, |_| Ok(()))?;
        if !none.truncated || !none.rights.is_empty() {
            return fail("a receive with no room did not drop and say so");
        }
        Ok(())
    })();
    // Each process's exit-time release of what it holds: the write end goes
    // with the second of them.
    pcb::destroy(sender);
    pcb::destroy(receiver);
    unix_socket::close(s1);
    unix_socket::close(s2);
    passed::drain();
    // Every reference taken went back: with no writer left, the read end
    // sees the end of the pipe. A leaked one would leave it waiting.
    let mut byte = [0u8; 1];
    let ended = pipe::try_read(rd, &mut byte);
    pipe::close(rd);
    result?;
    if ended != Ok(0) {
        return fail("a reference to the write end leaked (the read end never saw it close)");
    }
    crate::serial_println!(
        "[native_rights]   a pipe end passed between processes, landing as the receiver's own; \
         unheld and untravelling refused, unpublished undone, held lands once, no room \
         dropped, nothing leaked: OK"
    );
    Ok(())
}
