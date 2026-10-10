//! Named pipes (FIFOs): a pipe reached by a name in the filesystem.
//!
//! A FIFO's node -- made by `mkfifo` or `mknod(S_IFIFO)`,
//! [`crate::fs::EntryType::Fifo`] -- holds nothing. Opening it attaches the
//! opener to a kernel pipe ([`crate::ipc::pipe`]) kept for the node while
//! anyone has it open. Everyone who opens one node shares one pipe, found by
//! the node's identity (`FileId`): a rename or a second hard link leads to
//! the same pipe, and a name unlinked while the pipe is open goes on serving
//! whoever has it open. When the last of them closes, the pipe goes with
//! whatever was left in it, and the next open starts a new one -- Linux's
//! `fifo_open` and `put_pipe_info`.
//!
//! ## Opening
//!
//! POSIX's rules, as Linux keeps them (`fs/pipe.c`):
//!
//! | opened for | with no one on the other side |
//! |---|---|
//! | reading | waits for a writer; with `O_NONBLOCK`, opens at once -- reads are then end-of-file, and `poll` shows no hang-up until a writer has come and gone |
//! | writing | waits for a reader; with `O_NONBLOCK`, `ENXIO` |
//! | both (`O_RDWR`, which POSIX leaves undefined) | never waits: the opener is its own reader and writer |
//!
//! A wait ends at any open of the other side, even one closed again since --
//! the counts of opens are compared, not of open ends -- and at a signal,
//! which undoes the open (Linux's `-ERESTARTSYS`).
//!
//! ## Locks
//!
//! [`FIFOS`] before the pipe table: an open finds or makes the node's pipe
//! under it. The pipe module tells this one, holding no lock, when a FIFO's
//! pipe has gone ([`forget`]).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::Path;
use crate::fs::vfs::{FileHold, FileId};
use crate::ipc::pipe::{self, FifoAccess, PipeHandle};
use crate::sync::Mutex;

/// Each FIFO node that has a pipe, by identity: the pipe's number.
///
/// Watched, not a `PreemptSpinMutex`: the pipe table's lock is taken under
/// it (`pipe::fifo_alive`, `pipe::fifo_create`), and a lock with another
/// taken under it is the deadlock detector's to watch (design-decisions
/// 975, A-Q16). Taken when a FIFO is opened or forgotten, never on a read
/// or a write.
static FIFOS: Mutex<BTreeMap<FileId, u64>> = Mutex::named(BTreeMap::new(), b"FIFOS");

/// Open the FIFO at `path` -- already resolved, the final component
/// followed or not as the caller's open asked -- for `access`, waiting for
/// the other side as the module doc says unless `nonblock`. Returns the
/// pipe end, or both ends for [`FifoAccess::Both`]: the caller registers it,
/// and closes it as any pipe end ([`pipe::close`]).
///
/// # Errors
///
/// `NotSupported` if `path` names no FIFO, or one on a filesystem that cannot
/// hold it open by identity; `NoSuchDeviceOrAddress` for a write-only open
/// with `nonblock` and no reader; `Interrupted` for a signal while waiting,
/// with nothing left open; the lookup's own.
pub fn open(path: &Path, access: FifoAccess, nonblock: bool) -> KernelResult<PipeHandle> {
    let hold = crate::fs::Vfs::hold_fifo(path)?;
    open_held(Arc::new(hold), path, access, nonblock)
}

/// [`open`] of the FIFO node `hold` holds, opened before by `path` -- a
/// descriptor's node opened again through `/proc/<pid>/fd`, whatever its name
/// is now.
///
/// # Errors
///
/// As [`open`].
pub fn open_held(
    hold: Arc<FileHold>,
    path: &Path,
    access: FifoAccess,
    nonblock: bool,
) -> KernelResult<PipeHandle> {
    let id = hold.id();
    let (handle, partner) = loop {
        let pipe_id = {
            let mut fifos = FIFOS.lock();
            match fifos.get(&id).copied().filter(|&p| pipe::fifo_alive(p)) {
                Some(p) => p,
                None => {
                    let p = pipe::fifo_create(id, Arc::clone(&hold), path.to_path_buf());
                    fifos.insert(id, p);
                    p
                }
            }
        };
        match pipe::fifo_attach(pipe_id, access, nonblock) {
            Ok(attached) => break attached,
            // Gone between the look and the attach -- its last end closed:
            // look again, and make a new one.
            Err(KernelError::InvalidHandle) => {}
            Err(e) => {
                // A pipe made for this open, which it never attached to.
                if let Some(gone) = pipe::fifo_drop_if_unused(pipe_id) {
                    forget(gone, pipe_id);
                }
                return Err(e);
            }
        }
    };
    // This open's own hold goes here, with no lock held; the pipe keeps the
    // one it was made with.
    drop(hold);
    if let Some(partner) = partner
        && let Err(e) = pipe::fifo_wait_partner(handle, partner)
    {
        pipe::close(handle);
        return Err(e);
    }
    Ok(handle)
}

/// FIFO node `id`'s pipe `pipe_id` has gone, its last end closed: the entry
/// goes -- unless an open has since given the node another pipe.
pub(crate) fn forget(id: FileId, pipe_id: u64) {
    let mut fifos = FIFOS.lock();
    if fifos.get(&id) == Some(&pipe_id) {
        fifos.remove(&id);
    }
}

/// How many FIFO nodes have a pipe now (`/proc`, the self-test).
#[must_use]
pub fn open_count() -> usize {
    FIFOS.lock().len()
}

/// The in-kernel half of the FIFO tests: a node made in memfs, opened and
/// closed every way that does not need a second task to wait -- the
/// nonblocking opens and their `ENXIO`, `O_RDWR`, bytes through, end of file
/// and broken pipe, `poll`'s hang-up kept back from a reader that opened
/// before any writer, a node unlinked while open, and the pipe going with
/// the last close. The blocking opens are the ring-3 test's
/// (`spawn::self_test_linux_fifo`).
///
/// # Errors
///
/// `InternalError` on the first check that fails.
pub fn self_test() -> KernelResult<()> {
    use crate::fs::Vfs;
    use crate::serial_println;

    let fail = |what: &str| {
        serial_println!("[fifo]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    serial_println!("[fifo] Running self-test...");
    let path = Path::new("/tmp/.fifo-selftest");
    // A leftover from an interrupted run is replaced.
    let _ = Vfs::remove(path);
    if let Err(e) = Vfs::mknod_fifo(path, 0o600) {
        serial_println!("[fifo]   mknod_fifo: {:?}", e);
        return fail("a FIFO could not be made in /tmp");
    }
    if Vfs::lstat(path).map(|e| e.entry_type) != Ok(crate::fs::EntryType::Fifo) {
        return fail("the node is not a FIFO");
    }
    if Vfs::mknod_fifo(path, 0o600) != Err(KernelError::AlreadyExists) {
        return fail("a second FIFO was made over the first");
    }

    // Write-only and nonblocking with no reader: ENXIO, and no pipe left.
    if open(path, FifoAccess::Write, true) != Err(KernelError::NoSuchDeviceOrAddress) {
        return fail("a nonblocking writer with no reader was not ENXIO");
    }
    let before = open_count();

    // A nonblocking reader with no writer opens; it sees end of file, and
    // poll no hang-up yet.
    let Ok(r) = open(path, FifoAccess::Read, true) else {
        return fail("a nonblocking reader did not open");
    };
    let mut buf = [0u8; 8];
    if pipe::try_read(r, &mut buf) != Ok(0) {
        return fail("a reader with no writer did not read end of file");
    }
    if pipe::poll_status(r) & 0x10 != 0 {
        return fail("poll showed hang-up before any writer came");
    }
    // A writer now opens at once, and its bytes reach the reader.
    let Ok(w) = open(path, FifoAccess::Write, true) else {
        return fail("a nonblocking writer with a reader did not open");
    };
    if pipe::try_write(w, b"abc") != Ok(3)
        || pipe::try_read(r, &mut buf) != Ok(3)
        || buf.get(..3) != Some(b"abc".as_slice())
    {
        return fail("bytes did not go through");
    }
    if pipe::try_read(r, &mut buf) != Err(KernelError::WouldBlock) {
        return fail("an empty FIFO with a writer did not block a read");
    }
    // The writer goes: end of file, and hang-up now.
    pipe::close(w);
    if pipe::try_read(r, &mut buf) != Ok(0) || pipe::poll_status(r) & 0x10 == 0 {
        return fail("the writer's close was not end of file and hang-up");
    }

    // Unlinked while open: the pipe goes on, and a reopen of the node by its
    // hold reaches it.
    let Ok(hold) = Vfs::hold_fifo(path) else {
        return fail("the node could not be held");
    };
    let hold = Arc::new(hold);
    if Vfs::remove(path).is_err() {
        return fail("the node could not be unlinked");
    }
    let Ok(w2) = open_held(Arc::clone(&hold), path, FifoAccess::Write, true) else {
        return fail("the unlinked node's pipe could not be opened by its hold");
    };
    drop(hold);
    if pipe::try_write(w2, b"z") != Ok(1) || pipe::try_read(r, &mut buf) != Ok(1) {
        return fail("the unlinked node's pipe did not carry bytes");
    }
    // The reader goes: the writer's next write is a broken pipe.
    pipe::close(r);
    if pipe::try_write(w2, b"z") != Err(KernelError::ChannelClosed) {
        return fail("a write with no reader left was not a broken pipe");
    }
    pipe::close(w2);
    if open_count() != before {
        return fail("the pipe did not go with its last end");
    }

    // O_RDWR, on a node made again: never waits, reads what it wrote, and
    // is its own writer -- no end of file while it is open.
    if Vfs::mknod_fifo(path, 0o600).is_err() {
        return fail("the FIFO could not be made again");
    }
    let Ok(both) = open(path, FifoAccess::Both, false) else {
        return fail("an O_RDWR open did not open at once");
    };
    if pipe::try_write(both, b"rw") != Ok(2)
        || pipe::try_read(both, &mut buf) != Ok(2)
        || pipe::try_read(both, &mut buf) != Err(KernelError::WouldBlock)
    {
        return fail("an O_RDWR end did not read what it wrote, or saw end of file");
    }
    // A writer while it is open does not wait.
    let Ok(w3) = open(path, FifoAccess::Write, false) else {
        return fail("a writer did not open while an O_RDWR end was open");
    };
    pipe::close(w3);
    pipe::close(both);
    if open_count() != before {
        return fail("the O_RDWR pipe did not go with its last end");
    }
    let _ = Vfs::remove(path);

    serial_println!(
        "[fifo]   nonblocking opens and ENXIO, bytes through, end of file and hang-up, \
         broken pipe, unlinked while open, O_RDWR: OK"
    );
    serial_println!("[fifo] Self-test PASSED");
    Ok(())
}
