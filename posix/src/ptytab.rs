//! Pseudo-terminal pair records — the bridge between `SYS_PTY_CREATE` and
//! `open("/dev/pts/<n>")`.
//!
//! ## Why this table has to exist
//!
//! Linux obtains the two ends of a pty separately: `open("/dev/ptmx")`
//! yields the master, and the slave is obtained afterwards by opening the
//! filesystem path `/dev/pts/N` that `ptsname` reports.  Because a *third*
//! process could open that path in between, Linux needs `TIOCSPTLCK`,
//! `unlockpt` and `grantpt` to hold the slave shut until the owner is
//! ready.
//!
//! Our kernel closes that window instead of guarding it:
//! `SYS_PTY_CREATE` (544) returns **both** handles at once, in `rax` and
//! `rdx`, and there is deliberately no open-the-slave-by-name syscall
//! (see `requests/a-b-pty-the-tty-layer-is-now-n-devices-and-a-pty-object-exists.md`).
//!
//! That is the better design and it costs one small thing: the slave
//! handle arrives *before* anybody has asked for it.  `openpty(3)`'s
//! contract, which CPython's `pty` module and every terminal emulator
//! ported from Linux relies on, is nonetheless `posix_openpt` →
//! `ptsname` → `open(that name)`.  So libc has to hold the slave handle
//! between those two moments.  This module is that holding place, and it
//! is the entire reason `/dev/pts/<n>` can be a plain `open()` rather than
//! a device-node concept in the VFS — which is the trade lane A explicitly
//! accepted rather than build one for two hardcoded names.
//!
//! ## What a record's lifetime is
//!
//! | Event | Effect |
//! |---|---|
//! | `posix_openpt` succeeds | [`note_pair`] records `(tty_id, master, slave)`; the slave is *unclaimed* |
//! | `open("/dev/pts/<n>")` | [`claim_slave`] hands the slave handle out once and marks it claimed |
//! | last master fd closes | [`retire_master`] drops the record, and reports the slave as an **orphan** if it was never claimed |
//! | last slave fd closes | [`retire_slave`] marks the slave gone, so a later `open` of the same name cannot resurrect a dead handle |
//!
//! The orphan case is the one worth stating plainly, because nothing else
//! catches it: `SYS_PTY_CREATE` opens both ends, so a caller who takes a
//! master and never opens the slave still owes the kernel a close on the
//! slave.  `openpty` failing at `tcsetattr` and unwinding is exactly that
//! caller.  Without the orphan report the device would survive with no fd
//! anywhere able to name it.
//!
//! ## Not a capability
//!
//! Holding a record grants nothing.  The kernel checks that the calling
//! *process* owns a pty handle before it will act on it, and it does so
//! before looking at any buffer argument, so a forged `tty_id` here buys
//! an attacker a handle the kernel will refuse. The table is a convenience
//! for reuniting two halves of one `openpty`, not an authority store.
//!
//! ## Storage
//!
//! Per-process on the target (`static mut`, single-threaded posix
//! process); per-*thread* on host builds, for the same reason
//! [`crate::fdtable`] is — libtest runs each test on its own thread, and a
//! process-global table would let one test's `posix_openpt` collide with
//! another's assertions.  See design-decisions.md §110.

/// Maximum number of pty pairs a process can have outstanding at once.
///
/// A pair occupies a slot only between `posix_openpt` and the master's
/// close, so this bounds *concurrently open* terminals, not terminals
/// created over a process's life.  64 is chosen against the fd table's
/// 256: a pty pair costs two fds, so a process cannot hold more than 128
/// pairs whatever this says, and a terminal emulator with 64 live tabs is
/// already well past what the fd table would allow it to do anything with.
pub const MAX_PAIRS: usize = 64;

/// One `SYS_PTY_CREATE` result, waiting for its two halves to be reunited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pair {
    /// The kernel's `TtyId`, which is what `ptsname` formats into
    /// `/dev/pts/<id>` and what `open` parses back out.
    tty_id: u32,
    /// The master handle, `(tty_id << 1) | 0`.
    master: u64,
    /// The slave handle, `(tty_id << 1) | 1`.
    slave: u64,
    /// Whether an `open("/dev/pts/<n>")` has taken the slave handle.
    ///
    /// Tracked rather than derived from the fd table: a slave that was
    /// claimed and then closed has *no* fd naming it either, and closing
    /// it a second time on the master's behalf would drop a refcount the
    /// kernel had already dropped.
    slave_claimed: bool,
    /// Whether the slave end has been closed (either by its fd, or as an
    /// orphan when the master went).  A record in this state exists only
    /// to stop `/dev/pts/<n>` handing out a dead handle.
    slave_gone: bool,
}

type Table = [Option<Pair>; MAX_PAIRS];

const EMPTY: Table = [None; MAX_PAIRS];

/// Backing storage — per-process on target, per-thread on host.
///
/// Mirrors [`crate::fdtable`]'s `fd_store` exactly, including the absence
/// of a `reset()`: a fresh test thread gets a fresh table for free, so
/// per-thread storage *is* the reset.
mod store {
    use super::{EMPTY, Table};

    #[cfg(target_os = "none")]
    mod imp {
        use super::{EMPTY, Table};

        static mut PAIRS: Table = EMPTY;

        pub(super) fn table() -> *mut Table {
            &raw mut PAIRS
        }
    }

    #[cfg(not(target_os = "none"))]
    mod imp {
        use super::{EMPTY, Table};
        use core::cell::UnsafeCell;

        std::thread_local! {
            static PAIRS: UnsafeCell<Table> = const { UnsafeCell::new(EMPTY) };
        }

        /// Used only during thread-local teardown, when the real table has
        /// already been dropped — a late `close()` from a destructor must
        /// scribble somewhere harmless rather than panic.
        static mut FALLBACK: Table = EMPTY;

        pub(super) fn table() -> *mut Table {
            PAIRS.try_with(UnsafeCell::get).unwrap_or(&raw mut FALLBACK)
        }
    }

    #[inline]
    pub(super) fn table() -> *mut Table {
        imp::table()
    }
}

/// Run `f` over the pair table.
///
/// The table is per-process (target) or per-thread (host) state with no
/// concurrent reader, so a `&mut` borrow taken for the duration of `f` is
/// exclusive by construction.
fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    // SAFETY: `store::table()` returns a pointer to a `static mut` reached
    // only from a single-threaded posix process (target) or to this
    // thread's own cell (host).  Neither is aliased across the call, and
    // `f` cannot re-enter — every function in this module goes through
    // here and none of them call each other.
    let table = unsafe { &mut *store::table() };
    f(table)
}

/// Record a freshly created pair.
///
/// Returns `false` when the table is full, which the caller must treat as
/// a failed `posix_openpt` — a master fd whose slave cannot be found again
/// is a terminal nothing can ever be run on, and handing one out would
/// turn a resource limit into a mysterious `ENOENT` from `open` much later.
pub fn note_pair(tty_id: u32, master: u64, slave: u64) -> bool {
    with_table(|table| {
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(Pair {
                    tty_id,
                    master,
                    slave,
                    slave_claimed: false,
                    slave_gone: false,
                });
                return true;
            }
        }
        false
    })
}

/// Hand out the slave handle for `/dev/pts/<tty_id>`.
///
/// Returns `None` if there is no such pair in this process, or if its
/// slave end has already been closed.  Both are `ENOENT` at the `open`
/// call site: the name does not refer to anything this process can reach.
///
/// Repeated opens of the same name are allowed and return the same handle
/// value, matching what `dup` does for every other kind — the fd table's
/// reference scan is what decides when the kernel close fires, so two fds
/// naming one handle is the normal case rather than an error.
pub fn claim_slave(tty_id: u32) -> Option<u64> {
    with_table(|table| {
        for pair in table.iter_mut().flatten() {
            if pair.tty_id == tty_id && !pair.slave_gone {
                pair.slave_claimed = true;
                return Some(pair.slave);
            }
        }
        None
    })
}

/// Retire a master handle whose last fd has just been closed.
///
/// Returns `Some(slave_handle)` when that slave was **never claimed** — an
/// orphan the caller must close, because no fd will ever name it.  See the
/// module docs; this is the one case the fd table cannot see.
///
/// A handle that is not a recorded master is ignored, so this is safe to
/// call with either end's handle.
pub fn retire_master(handle: u64) -> Option<u64> {
    with_table(|table| {
        for slot in table.iter_mut() {
            let Some(pair) = *slot else { continue };
            if pair.master != handle {
                continue;
            }
            // The record goes either way: the master is gone, so
            // `/dev/pts/<n>` has nothing left to attach a new fd to that
            // would not immediately read EOF.
            *slot = None;
            return if pair.slave_claimed || pair.slave_gone {
                None
            } else {
                Some(pair.slave)
            };
        }
        None
    })
}

/// Note that a slave handle's last fd has been closed.
///
/// Keeps the record (the master may still be open) but marks the slave
/// dead, so a subsequent `open("/dev/pts/<n>")` gets `ENOENT` rather than
/// a handle the kernel has already released.
///
/// A handle that is not a recorded slave is ignored, so this is safe to
/// call with either end's handle.
pub fn retire_slave(handle: u64) {
    with_table(|table| {
        for pair in table.iter_mut().flatten() {
            if pair.slave == handle {
                pair.slave_gone = true;
                pair.slave_claimed = true;
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary `openpty` sequence: create, then claim the slave by
    /// name, then close both.  Nothing is reported as an orphan, because
    /// the slave got an fd of its own and that fd's close covers it.
    #[test]
    fn claimed_slave_is_not_an_orphan() {
        assert!(note_pair(7, 14, 15));
        assert_eq!(claim_slave(7), Some(15));
        assert_eq!(retire_master(14), None);
    }

    /// The failure path `openpty` actually takes when `tcsetattr` fails:
    /// the master is closed and the slave was never opened by name.  That
    /// slave handle is live in the kernel and has no fd, so it must come
    /// back as an orphan.
    #[test]
    fn unclaimed_slave_comes_back_as_an_orphan() {
        assert!(note_pair(8, 16, 17));
        assert_eq!(retire_master(16), Some(17));
        // The record is gone with it — a second close must not report the
        // same orphan twice, which would drop a refcount that is no longer
        // held.
        assert_eq!(retire_master(16), None);
    }

    /// A slave that was claimed and then closed is *also* not an orphan.
    /// This is the case a naive "does any fd name it?" test gets wrong:
    /// no fd names it, but the kernel refcount was already dropped by that
    /// fd's own close.
    #[test]
    fn claimed_then_closed_slave_is_not_an_orphan() {
        assert!(note_pair(9, 18, 19));
        assert_eq!(claim_slave(9), Some(19));
        retire_slave(19);
        assert_eq!(retire_master(18), None);
    }

    /// Once the slave end is closed, its name stops resolving — reopening
    /// it would hand out a handle the kernel has already released.
    #[test]
    fn a_closed_slave_cannot_be_reopened_by_name() {
        assert!(note_pair(10, 20, 21));
        assert_eq!(claim_slave(10), Some(21));
        retire_slave(21);
        assert_eq!(claim_slave(10), None);
    }

    /// Two fds on one slave is the normal case (`dup`, or a second
    /// `open` of the same name), so claiming twice returns the same
    /// handle rather than failing.
    #[test]
    fn claiming_twice_returns_the_same_handle() {
        assert!(note_pair(11, 22, 23));
        assert_eq!(claim_slave(11), Some(23));
        assert_eq!(claim_slave(11), Some(23));
    }

    /// An unknown name resolves to nothing, which is `ENOENT` at the
    /// `open` site rather than a panic or a zero handle.
    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        assert_eq!(claim_slave(4_000_000), None);
    }

    /// Both retire functions take either end's handle, because
    /// `close_pty_handle` cannot tell which it has without decoding the
    /// low bit — and decoding it there would put the handle encoding in a
    /// second place.
    #[test]
    fn retiring_with_the_wrong_end_is_a_no_op() {
        assert!(note_pair(12, 24, 25));
        // Slave handle offered to the master retirer: no match, no orphan.
        assert_eq!(retire_master(25), None);
        // Master handle offered to the slave retirer: no match, and the
        // record survives intact.
        retire_slave(24);
        assert_eq!(claim_slave(12), Some(25));
    }

    /// The table is finite and says so, rather than silently dropping a
    /// pair and turning it into an `ENOENT` from `open` much later.
    #[test]
    fn a_full_table_refuses_rather_than_forgetting() {
        let mut i = 0u32;
        while i < MAX_PAIRS as u32 {
            assert!(note_pair(i, u64::from(i) * 2, u64::from(i) * 2 + 1));
            i = i.wrapping_add(1);
        }
        assert!(!note_pair(9999, 1_000, 1_001));
        // Freeing one slot makes room again.
        assert_eq!(retire_master(0), Some(1));
        assert!(note_pair(9999, 1_000, 1_001));
    }

    /// A record disappears with its master, so a name that resolved a
    /// moment ago stops resolving once the master's last fd is gone.
    /// This is what makes the "opened once" rule in the module docs true
    /// rather than merely intended.
    #[test]
    fn a_retired_masters_name_stops_resolving() {
        assert!(note_pair(13, 26, 27));
        assert_eq!(claim_slave(13), Some(27));
        assert_eq!(retire_master(26), None);
        assert_eq!(claim_slave(13), None);
    }
}
