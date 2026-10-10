//! Native socket handles: what a process holds for one of the kernel's own
//! TCP connections, TCP listeners or UDP sockets -- the objects behind the
//! native `SYS_TCP_*` and `SYS_UDP_*` calls (`syscall::handlers`) -- as
//! opposed to the slot the protocol module keeps each one in.
//!
//! Until 2026-10-02 the slot index was the handle, and three things were wrong
//! with that (`known-issues` `A-TCP-AND-UDP-SOCKETS-ARE-NOT-COUNTED-PER-PROCESS`):
//! - **possession:** slots number from 0, so any process with the `Socket`
//!   capability could name every socket by counting, and send on, read from
//!   or close another process's;
//! - **reuse:** a slot the protocol retired on its own and gave to a new
//!   connection was still named by the old number, which then reached the new
//!   connection;
//! - **counting:** a fork shared the one socket rather than holding a
//!   reference, so closing it in either process closed it for both, nothing
//!   closed it when a process died, and an exec could not drop it.
//!
//! Now a handle is an id from a counter that never repeats, kept here with the
//! kind and slot it names, that slot's generation when it was issued (what
//! `tcp::connect_tagged` and the other `*_tagged` calls report), and how many
//! processes hold it:
//! - each holder has it in its `ipc_handles` as `ResourceType::NativeSocket`,
//!   which every call checks before this table is asked (`require_ipc_handle`
//!   in `syscall::handlers`);
//! - an operation holds a pin on the slot while it runs ([`SlotPin`]), taken
//!   only while the slot still holds the generation the handle was issued
//!   for, so it can neither reach a successor nor see its slot given away
//!   mid-call;
//! - a fork, or a spawn that passes the socket on, adds a holder ([`dup`]); a
//!   close, an exit or an exec that drops it takes one away ([`release`]), and
//!   the last one closes the socket.
//!
//! The kernel's own users of `tcp` and `udp` -- the DNS resolver, the shell's
//! commands, the HTTP client -- keep using slots: they hand nothing to a
//! process.
//!
//! Lock order: [`HANDLES`] is never held while a protocol's table is taken.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::sync::Mutex;

/// What a native socket handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A TCP connection: a slot of `tcp`'s connection table.
    TcpConnection,
    /// A TCP listener: a slot of `tcp`'s listener table.
    TcpListener,
    /// A UDP socket: a slot of `udp`'s socket table.
    Udp,
}

impl Kind {
    /// Pin `slot` of this kind's table if it holds the object of
    /// `generation`.
    fn pin(self, slot: usize, generation: u32) -> bool {
        match self {
            Self::TcpConnection => super::tcp::pin_conn(slot, generation),
            Self::TcpListener => super::tcp::pin_listener(slot, generation),
            Self::Udp => super::udp::pin(slot, generation),
        }
    }

    /// Let go of a [`Self::pin`].
    fn unpin(self, slot: usize) {
        match self {
            Self::TcpConnection => super::tcp::unpin_conn(slot),
            Self::TcpListener => super::tcp::unpin_listener(slot),
            Self::Udp => super::udp::unpin(slot),
        }
    }
}

/// One issued handle.
#[derive(Debug, Clone, Copy)]
struct Entry {
    kind: Kind,
    slot: usize,
    /// The slot's generation when the handle was issued: the object it names.
    generation: u32,
    /// How many processes hold the handle.
    holders: u32,
}

/// Every issued handle that some process still holds.
static HANDLES: Mutex<BTreeMap<u64, Entry>> = Mutex::named(BTreeMap::new(), b"native_socket");

/// The next handle: from 1, so 0 names nothing, and never reused.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// An operation's hold on the slot a handle names ([`resolve`]): the slot is
/// not given to another object while this lives. The object in it may still
/// end meanwhile -- a reset, a close by another holder -- which the protocol
/// call then reports as it would any ended object.
#[must_use]
pub struct SlotPin {
    kind: Kind,
    slot: usize,
}

impl SlotPin {
    /// The protocol table slot the handle names.
    #[must_use]
    pub fn slot(&self) -> usize {
        self.slot
    }
}

impl Drop for SlotPin {
    fn drop(&mut self) {
        self.kind.unpin(self.slot);
    }
}

/// Issue a handle, held once, for the `kind` object of `generation` in
/// `slot` -- the pair a `*_tagged` create call returned.
#[must_use]
pub fn issue(kind: Kind, slot: usize, generation: u32) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    HANDLES.lock().insert(
        handle,
        Entry {
            kind,
            slot,
            generation,
            holders: 1,
        },
    );
    handle
}

/// Pin the slot `handle` names, if it is a `kind` handle and its object is
/// still in that slot; `None` otherwise -- a handle never issued or already
/// released, one of another kind, or one whose object has ended and whose
/// slot has gone to another. Possession is the caller's to check first.
pub fn resolve(handle: u64, kind: Kind) -> Option<SlotPin> {
    let entry = HANDLES
        .lock()
        .get(&handle)
        .copied()
        .filter(|e| e.kind == kind)?;
    // The protocol's table is taken after this table's lock is let go.
    entry
        .kind
        .pin(entry.slot, entry.generation)
        .then_some(SlotPin {
            kind,
            slot: entry.slot,
        })
}

/// What `handle` names, for a caller that takes more than one kind.
#[must_use]
pub fn kind_of(handle: u64) -> Option<Kind> {
    HANDLES.lock().get(&handle).map(|e| e.kind)
}

/// One more holder of `handle`: a fork, or a spawn passing the socket on.
/// Returns the handle, which the new holder shares.
///
/// # Errors
///
/// `InvalidHandle` for a handle not in the table.
pub fn dup(handle: u64) -> KernelResult<u64> {
    let mut handles = HANDLES.lock();
    let entry = handles.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    entry.holders = entry
        .holders
        .checked_add(1)
        .ok_or(KernelError::OutOfMemory)?;
    Ok(handle)
}

/// How the last holder's release ends a TCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// An orderly close: `FIN`, as `close(2)`.
    Close,
    /// A reset: `RST`, as `close(2)` with `SO_LINGER` of zero.
    Abort,
}

/// One holder fewer of `handle`. The last one ends its object -- if it is
/// still in its slot; one that ended on its own has nothing left to close.
/// Another holder's release leaves the object alone, as closing one of two
/// descriptors for a socket does.
///
/// # Errors
///
/// The protocol's close, when this was the last holder and the close failed.
pub fn release(handle: u64, ending: Ending) -> KernelResult<()> {
    let last = {
        let mut handles = HANDLES.lock();
        let Some(entry) = handles.get_mut(&handle) else {
            return Ok(());
        };
        if entry.holders > 1 {
            entry.holders = entry.holders.saturating_sub(1);
            return Ok(());
        }
        handles.remove(&handle)
    };
    let Some(entry) = last else {
        return Ok(());
    };
    // Pinned across the close, so the slot cannot change hands between the
    // check and the close.
    if !entry.kind.pin(entry.slot, entry.generation) {
        return Ok(());
    }
    let result = match (entry.kind, ending) {
        (Kind::TcpConnection, Ending::Close) => super::tcp::close(entry.slot),
        (Kind::TcpConnection, Ending::Abort) => super::tcp::abort(entry.slot),
        (Kind::TcpListener, _) => super::tcp::close_listener(entry.slot),
        (Kind::Udp, _) => {
            super::udp::close(entry.slot);
            Ok(())
        }
    };
    entry.kind.unpin(entry.slot);
    result
}

/// How many processes hold `handle` (0 for one not in the table). For tests.
fn holders(handle: u64) -> u32 {
    HANDLES.lock().get(&handle).map_or(0, |e| e.holders)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The table's rules, on real UDP sockets: a handle resolves to its slot and
/// only for its own kind; a second holder keeps the socket open through the
/// first's release and the last release closes it; a handle whose socket
/// ended does not reach the next socket given its slot; and a pinned slot is
/// not given away.
///
/// # Errors
///
/// `InternalError` naming the rule that failed; a bind's own error.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[native_socket]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    let ns = crate::netns::ROOT_NS;

    serial_println!("[native_socket] Running self-test...");

    // Resolve, kind, holders.
    let (slot, generation) = super::udp::bind_tagged(ns, 0)?;
    let h = issue(Kind::Udp, slot, generation);
    let resolved = resolve(h, Kind::Udp).map(|p| p.slot());
    let wrong_kind = resolve(h, Kind::TcpConnection).is_some();
    let duped = dup(h).is_ok() && holders(h) == 2;
    let first = release(h, Ending::Close).is_ok();
    let open_after_first = resolve(h, Kind::Udp).is_some();
    let second = release(h, Ending::Close).is_ok();
    let gone = resolve(h, Kind::Udp).is_none() && holders(h) == 0;
    let socket_closed = super::udp::local_port(slot).is_none();
    if resolved != Some(slot) {
        return fail("a handle does not resolve to its slot");
    }
    if wrong_kind {
        return fail("a UDP handle resolved as a TCP connection");
    }
    if !(duped && first && open_after_first) {
        return fail("a second holder did not keep the socket through the first's release");
    }
    if !(second && gone && socket_closed) {
        return fail("the last release did not close the socket");
    }

    // A handle whose socket ended does not reach the slot's next socket.
    let (slot, generation) = super::udp::bind_tagged(ns, 0)?;
    let stale = issue(Kind::Udp, slot, generation);
    super::udp::close(slot);
    let (slot2, generation2) = super::udp::bind_tagged(ns, 0)?;
    let fresh = issue(Kind::Udp, slot2, generation2);
    let stale_reaches = resolve(stale, Kind::Udp).is_some();
    let fresh_reaches = resolve(fresh, Kind::Udp).is_some();
    // The stale handle's release must not close the fresh socket.
    let stale_released = release(stale, Ending::Close).is_ok();
    let fresh_survives = resolve(fresh, Kind::Udp).is_some();
    let fresh_closed = release(fresh, Ending::Close).is_ok();
    if stale_reaches {
        return fail("a handle reached the socket given its slot after its own ended");
    }
    if !(fresh_reaches && stale_released && fresh_survives && fresh_closed) {
        return fail("a stale handle's release touched the slot's new socket");
    }

    // A pinned slot is not given to a new socket.
    let (slot, generation) = super::udp::bind_tagged(ns, 0)?;
    let h = issue(Kind::Udp, slot, generation);
    let Some(pin) = resolve(h, Kind::Udp) else {
        return fail("a live handle could not be pinned");
    };
    super::udp::close(slot);
    let (other, _) = super::udp::bind_tagged(ns, 0)?;
    let reused_while_pinned = other == slot;
    drop(pin);
    super::udp::close(other);
    // Nothing left to close: the socket ended above; the entry goes.
    let released = release(h, Ending::Close).is_ok() && holders(h) == 0;
    if reused_while_pinned {
        return fail("a pinned slot was given to a new socket");
    }
    if !released {
        return fail("a handle whose socket had ended could not be released");
    }

    serial_println!(
        "[native_socket]   handles: resolve to their slot and kind only; a second holder \
         keeps the socket; the last release closes it; a stale handle reaches nothing; a \
         pinned slot is not reused: OK"
    );
    Ok(())
}
