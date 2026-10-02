//! Descriptors in transit: a reference to the object behind one process's
//! descriptor, held by the kernel while it travels to another -- a Unix
//! socket's `SCM_RIGHTS`, and `pidfd_getfd`.
//!
//! ## The reference
//!
//! A [`Passed`] is one holder of its object, exactly as one more process
//! holding it would be: `fork` and `dup` count holders the same way, and an
//! object ends with its last. [`Passed::take`] takes it from the sender's
//! descriptor. It then ends one of two ways:
//!
//! - **landed** ([`Passed::land`]): a receiving process installed it, and the
//!   reference is now that process's, released when the process closes its
//!   last descriptor for the object or exits;
//! - **dropped**: never received -- its socket closed with it queued, the
//!   receiver had no room for it (`MSG_CTRUNC`), a `read(2)` passed over it,
//!   or the receiver held the object already (a process holds one reference
//!   per object however many descriptors name it). Dropping releases it.
//!
//! The descriptors one message carries travel together as a [`Bundle`].
//!
//! ## Why a release waits for [`drain`]
//!
//! Releasing a reference can end its object, and ending an object takes that
//! object's own lock -- a Unix socket ends under `unix_socket`'s table, a
//! stream's queued descriptors under `stream_socket`'s. But values of this
//! type are dropped exactly where such locks are held: a socket's queue is
//! emptied under its table. So dropping one only queues the release, and
//! [`drain`] performs the queued releases; every path that can drop one calls
//! it once it holds no lock. A missed drain delays a release until the next
//! drain anywhere; it never loses one.
//!
//! The queue also flattens what would otherwise be recursion as deep as an
//! attacker cares to make it: a socket released ends, which empties its
//! queue, which releases the sockets in flight there, which empty theirs.
//! Each step only queues, and one loop does them in turn.
//!
//! ## Accounting
//!
//! - **Per user**, what Linux keeps as `user->unix_inflight`: how many
//!   descriptors a user has sent that nobody has yet received or dropped
//!   ([`Passed::charge`], [`in_flight_for`]). A send by a user already over
//!   its `RLIMIT_NOFILE` is refused (`ETOOMANYREFS`), which bounds what a
//!   process can pin in queues it never reads.
//! - **Unix sockets in transit** ([`unix_sockets_in_flight`]): while there
//!   are none, no cycle of sockets carrying each other can exist, and
//!   `unix_socket`'s garbage collection has nothing to look for.
//!
//! ## Lock order
//!
//! `RELEASES`, `CHARGES` and `DRAINERS` are leaves: taken under any other
//! lock (a drop may happen anywhere), and nothing is taken under them.

use crate::error::{KernelError, KernelResult};
use crate::proc::linux_fd::{FdEntry, HandleKind};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// The most descriptors one message carries: Linux's `SCM_MAX_FD`. More is
/// `EINVAL`.
pub const MAX_PER_MESSAGE: usize = 253;

/// One reference to the object behind a descriptor, owned by the kernel while
/// it is in transit. See the module documentation.
#[derive(Debug)]
pub struct Passed {
    /// The descriptor as the sender had it: kind, handle, and the open file
    /// description's status flags, which travel with it.
    entry: FdEntry,
    /// The user this counts against while in flight, if it is charged.
    charged: Option<u32>,
}

impl Passed {
    /// Take one more reference to the object behind `entry` -- a descriptor
    /// the caller holds -- for it to travel.
    ///
    /// # Errors
    ///
    /// `InvalidHandle` if the object has ended (its last holder closed it
    /// between the caller's lookup and now) or its holder count is full.
    pub fn take(entry: FdEntry) -> KernelResult<Self> {
        let raw = entry.raw_handle;
        let held = match entry.kind {
            // No object behind them: the console, a pidfd's process number,
            // the stateless sound control device.
            HandleKind::Console | HandleKind::PidFd | HandleKind::AlsaControl => Ok(()),
            HandleKind::File => crate::fs::handle::dup_shared(raw).map(|_| ()),
            HandleKind::Pipe => {
                super::pipe::dup(super::pipe::PipeHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::EventFd => {
                super::eventfd::dup(super::eventfd::EventFdHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::MemFd => {
                super::memfd::dup(super::memfd::MemFdHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::Epoll => {
                super::epoll::dup(super::epoll::EpollHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::SignalFd => {
                super::signalfd::dup(super::signalfd::SignalFdHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::Timerfd => {
                super::timerfd::dup(super::timerfd::TimerFdHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::Inotify => {
                super::inotify::dup(super::inotify::InotifyHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::AlsaPcm => {
                super::alsa_pcm::dup(super::alsa_pcm::AlsaPcmHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::DrmCard => {
                crate::drm::card_fd::dup(crate::drm::card_fd::DrmCardHandle::from_raw(raw))
                    .map(|_| ())
            }
            HandleKind::Evdev => {
                crate::evdev_fd::dup(crate::evdev_fd::EvdevHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::Socket => {
                crate::net::socket::dup(crate::net::socket::SocketHandle::from_raw(raw)).map(|_| ())
            }
            HandleKind::Channel => {
                super::channel::dup(super::channel::ChannelHandle::from_raw(raw))
            }
            HandleKind::ServiceListener => {
                super::service::dup_listener(super::service::ServiceListenerHandle::from_raw(raw))
            }
            HandleKind::UnixSocket => {
                super::unix_socket::dup(super::unix_socket::UnixHandle::from_raw(raw)).map(|_| ())
            }
        };
        held.map_err(|_| KernelError::InvalidHandle)?;
        if entry.kind == HandleKind::UnixSocket {
            UNIX_IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
        }
        Ok(Self {
            entry,
            charged: None,
        })
    }

    /// Count this against `uid` until it lands or is dropped -- the sender's
    /// user, as Linux's `unix_inflight` charges `scm->fp->user`. A second
    /// charge is ignored.
    pub fn charge(&mut self, uid: u32) {
        if self.charged.is_some() {
            return;
        }
        let mut charges = CHARGES.lock();
        let n = charges.entry(uid).or_insert(0);
        *n = n.saturating_add(1);
        self.charged = Some(uid);
    }

    /// The descriptor this is a reference for, as the sender had it.
    #[must_use]
    pub const fn entry(&self) -> FdEntry {
        self.entry
    }

    /// The Unix socket this is a reference to, if it is one.
    #[must_use]
    pub const fn unix_socket(&self) -> Option<u64> {
        match self.entry.kind {
            HandleKind::UnixSocket => Some(self.entry.raw_handle),
            _ => None,
        }
    }

    /// The reference is now a process's: the receiver installed it. Gives the
    /// entry back, and leaves the reference held.
    #[must_use]
    pub fn land(mut self) -> FdEntry {
        self.settle();
        let entry = self.entry;
        // The reference is the receiver's now: nothing left to release.
        core::mem::forget(self);
        entry
    }

    /// Undo what this counted while in transit.
    fn settle(&mut self) {
        if let Some(uid) = self.charged.take() {
            let mut charges = CHARGES.lock();
            if let Some(n) = charges.get_mut(&uid) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    charges.remove(&uid);
                }
            }
        }
        if self.entry.kind == HandleKind::UnixSocket {
            UNIX_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Drop for Passed {
    fn drop(&mut self) {
        self.settle();
        if !self.entry.kind.needs_kernel_close() {
            return;
        }
        let mut queue = RELEASES.lock();
        if queue.try_reserve(1).is_ok() {
            queue.push(self.entry);
        } else {
            // Out of memory for one more entry: the reference stays held, and
            // its object with it, until the machine is restarted. Said loudly,
            // since nothing else will ever say it.
            serial_println!(
                "[passed] out of memory queueing a release: {:?} {:#x} leaks",
                self.entry.kind,
                self.entry.raw_handle
            );
        }
    }
}

/// The descriptors one message carries, in the order the sender gave them.
///
/// Shared, so that a receive that only looks (`MSG_PEEK`) can hold the
/// references alive while it takes new ones of its own, without taking them
/// off the message; see [`Bundle::into_passed`].
#[derive(Clone)]
pub struct Bundle(Arc<Vec<Passed>>);

/// One message's descriptors are equal to themselves and their copies, and to
/// nothing else: two messages each carrying the same descriptor carry two
/// references.
impl PartialEq for Bundle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Bundle {}

impl core::fmt::Debug for Bundle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Bundle({} descriptors)", self.0.len())
    }
}

impl Bundle {
    /// A bundle of `passed`, or `None` when it is empty -- a message carries
    /// descriptors or it does not.
    #[must_use]
    pub fn new(passed: Vec<Passed>) -> Option<Self> {
        (!passed.is_empty()).then(|| Self(Arc::new(passed)))
    }

    /// The Unix sockets among them, by id: what garbage collection follows.
    pub fn unix_sockets(&self) -> impl Iterator<Item = u64> + '_ {
        self.0.iter().filter_map(Passed::unix_socket)
    }

    /// Whether another copy exists: a receive is looking at the message right
    /// now, and may be about to give its descriptors to a process.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        Arc::strong_count(&self.0) > 1
    }

    /// The references, for a receiver: these very ones when this is the only
    /// copy; otherwise -- the message stays queued for a later receive, or
    /// another receive is looking at it -- a new reference to each object,
    /// taken while this copy keeps the objects alive. An object that cannot
    /// be referenced again is left out.
    ///
    /// Call with no lock held: taking a reference takes its object's lock.
    #[must_use]
    pub fn into_passed(self) -> Vec<Passed> {
        match Arc::try_unwrap(self.0) {
            Ok(passed) => passed,
            Err(shared) => {
                let fresh = shared
                    .iter()
                    .filter_map(|p| Passed::take(p.entry).ok())
                    .collect();
                // The last copy may be this one by now; its references are
                // queued for release, and the caller drains.
                drop(shared);
                fresh
            }
        }
    }
}

/// Releases waiting for [`drain`].
static RELEASES: Mutex<Vec<FdEntry>> = Mutex::new(Vec::new());

/// Tasks inside [`drain`] now: a release that reaches `drain` again on the
/// same task returns at once, and the outer loop takes over its work.
static DRAINERS: Mutex<Vec<TaskId>> = Mutex::new(Vec::new());

/// In flight per user: descriptors sent and neither received nor dropped.
static CHARGES: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());

/// Unix sockets with a [`Passed`] reference outstanding.
static UNIX_IN_FLIGHT: AtomicU64 = AtomicU64::new(0);

/// Perform every queued release. Call with no lock held, from a task (a
/// release may end an object, and ending one may wait -- an `AF_INET`
/// socket's last close is a round trip to the network daemon).
pub fn drain() {
    let me = sched::current_task_id();
    {
        let mut drainers = DRAINERS.lock();
        if drainers.contains(&me) {
            return;
        }
        if drainers.try_reserve(1).is_err() {
            // Nothing is lost: the queue waits for the next drain.
            return;
        }
        drainers.push(me);
    }
    loop {
        let next = RELEASES.lock().pop();
        let Some(entry) = next else {
            break;
        };
        release(entry);
    }
    DRAINERS.lock().retain(|&t| t != me);
}

/// Drop one reference to the object behind `entry` -- one holder fewer, and
/// the object ends with its last -- without touching any process's records,
/// since the reference was no process's.
fn release(entry: FdEntry) {
    if let Some(resource) = entry.kind.resource_type() {
        super::cleanup_handles(&[(resource, entry.raw_handle)]);
    }
}

/// How many descriptors `uid` has in flight.
#[must_use]
pub fn in_flight_for(uid: u32) -> u32 {
    CHARGES.lock().get(&uid).copied().unwrap_or(0)
}

/// How many Unix sockets have a reference in transit.
#[must_use]
pub fn unix_sockets_in_flight() -> u64 {
    UNIX_IN_FLIGHT.load(Ordering::Acquire)
}
