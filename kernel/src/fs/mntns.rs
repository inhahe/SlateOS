//! Mount namespaces: which mount table a process sees -- Linux's
//! `mnt_namespace` (`man 7 mount_namespaces`).
//!
//! Every process is in one. The **root** namespace's table is the system's,
//! the one the kernel mounted at boot. `unshare(CLONE_NEWNS)` and
//! `clone(CLONE_NEWNS)` give a process a copy of the table it was in
//! ([`create_from`], `fs::Vfs::copy_mount_table`), after which a mount or an
//! unmount in either is not seen in the other: no mount here propagates to
//! another namespace, every mount is private (design-decisions 1555). A copy
//! shares the filesystems, so a file is the same file through either table.
//!
//! Membership is a map from process to namespace, for the processes not in
//! the root, behind a leaf lock: the VFS asks it on every path operation
//! ([`current`]) from whatever context the caller is in, and takes no other
//! lock to do so. While no namespace but the root exists the answer is the
//! root without any lock at all ([`any`]). A namespace lives while a process
//! is in it or a handle holds it (`/proc/<pid>/ns/mnt` opened, `crate::nsfs`),
//! counted by [`retain`] and [`release`]; its table goes with the last
//! (`fs::Vfs::drop_mount_table`).
//!
//! As the UTS namespace is, a mount namespace is a process's here and a
//! thread's on Linux (design-decisions 1554). A task may still resolve its
//! paths in another for a while ([`in_namespace`]): the kernel placing files
//! where only one process's namespace reaches them, before it runs.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::ProcessId;
use crate::sched::task::TaskId;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// A mount namespace's id. [`ROOT`] is the system's.
pub type MntNsId = u64;

/// The root mount namespace: the system's mount table.
pub const ROOT: MntNsId = 0;

/// The most namespaces besides the root that may exist at once, as Linux's
/// `/proc/sys/user/max_mnt_namespaces` bounds what a loop of `unshare` could
/// pin. `ResourceExhausted` (`ENOSPC`) beyond it.
const MAX_NAMESPACES: usize = 1024;

/// Which namespace each process is in, for the processes not in the root.
/// A leaf lock.
static MEMBERS: Mutex<BTreeMap<ProcessId, MntNsId>> = Mutex::named(BTreeMap::new(), b"MNTNSMEM");

/// Every namespace but the root, with how many hold it: the processes in it
/// and the handles open on it. A leaf lock.
static HOLDS: Mutex<BTreeMap<MntNsId, u32>> = Mutex::named(BTreeMap::new(), b"MNTNSREF");

/// How many namespaces besides the root exist: while none does, [`current`]
/// answers the root without a lock.
static OTHERS: AtomicUsize = AtomicUsize::new(0);

/// The next id to give.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The tasks inside [`in_namespace`] now, each with the namespaces it
/// entered, the innermost last. A leaf lock, asked by [`current`].
static ENTERED: Mutex<BTreeMap<TaskId, Vec<MntNsId>>> = Mutex::named(BTreeMap::new(), b"MNTNSENT");

/// How many tasks are inside [`in_namespace`] now, so [`current`] answers
/// without its lock in the common case of none.
static ENTERING: AtomicUsize = AtomicUsize::new(0);

/// Whether any namespace besides the root exists.
#[must_use]
pub fn any() -> bool {
    OTHERS.load(Ordering::Acquire) != 0
}

/// Rename the working directories of the processes in mount namespace `ns`
/// by `rename` -- `None` leaves one as it is -- for a change that moved its
/// mounts (`Vfs::pivot_root_tree`). A process with a root of its own
/// (`chroot`, a container's view) is passed by: its working directory is
/// named from that root, which did not move.
pub(crate) fn rename_working_directories(ns: MntNsId, rename: impl Fn(&[u8]) -> Option<Vec<u8>>) {
    for pid in crate::proc::pcb::pids() {
        if of_process(pid) != ns
            || matches!(crate::proc::pcb::get_root_dir(pid), Some(Some(_)))
            || crate::ipc::namespace::mount_view_for(pid).is_some()
        {
            continue;
        }
        let Some(cwd) = crate::proc::pcb::get_cwd(pid) else {
            continue;
        };
        let Some(renamed) = rename(&cwd) else {
            continue;
        };
        match crate::proc::pcb::set_cwd(pid, renamed) {
            // Gone meanwhile: it has no working directory to keep.
            Ok(()) | Err(KernelError::NoSuchProcess) => {}
            Err(e) => crate::serial_println!(
                "[mntns] process {}'s working directory could not follow the pivot ({:?}); \
                 its old name now leads elsewhere",
                pid,
                e
            ),
        }
    }
}

/// The mount namespace of the process `pid` names: the root for one in it,
/// and for one that does not exist.
#[must_use]
pub fn of_process(pid: ProcessId) -> MntNsId {
    if !any() {
        return ROOT;
    }
    MEMBERS.lock().get(&pid).copied().unwrap_or(ROOT)
}

/// The mount namespace the calling task resolves paths in: the one it last
/// entered with [`in_namespace`], if it is inside one; else its process's,
/// the root for a kernel task, and for a task acting with the kernel's
/// authority (`proc::thread::as_kernel`), as `ipc::namespace` resolves its
/// paths.
#[must_use]
pub fn current() -> MntNsId {
    if !any() {
        return ROOT;
    }
    let task = crate::sched::current_task_id();
    if ENTERING.load(Ordering::Acquire) != 0
        && let Some(&ns) = ENTERED.lock().get(&task).and_then(|entered| entered.last())
    {
        return ns;
    }
    crate::proc::thread::acting_process(task)
        .filter(|&pid| pid != 0)
        .map_or(ROOT, of_process)
}

/// `body` run with the calling task resolving every path in namespace `ns`
/// -- each lookup, mount and pin it makes -- rather than in its process's
/// (the root, for a kernel task), as Linux's `setns(CLONE_NEWNS)` would
/// move a thread, and moved back after. Nothing else about the task
/// changes: a kernel task stays one, judged by no permission gate. For the
/// kernel placing files in a namespace of a process's own before the
/// process runs, where no other namespace reaches them (`linuxdrv`'s WiFi
/// network file). `ns` is held for the length of `body`. Nests: an inner
/// call ends only its own part.
///
/// # Errors
///
/// `NotFound`, `body` not run, for an `ns` that does not exist.
pub fn in_namespace<R>(ns: MntNsId, body: impl FnOnce() -> R) -> KernelResult<R> {
    if !retain(ns) {
        return Err(KernelError::NotFound);
    }
    let task = crate::sched::current_task_id();
    {
        let mut entered = ENTERED.lock();
        let stack = entered.entry(task).or_default();
        if stack.is_empty() {
            ENTERING.fetch_add(1, Ordering::AcqRel);
        }
        stack.push(ns);
    }
    let result = body();
    {
        let mut entered = ENTERED.lock();
        let ended = entered.get_mut(&task).is_some_and(|stack| {
            stack.pop();
            stack.is_empty()
        });
        if ended {
            entered.remove(&task);
            ENTERING.fetch_sub(1, Ordering::AcqRel);
        }
    }
    release(ns);
    Ok(result)
}

/// A new namespace with a copy of `from`'s table, held once by the caller
/// (who hands that hold to the process it puts there, [`set_process`]).
///
/// # Errors
///
/// `ResourceExhausted` past [`MAX_NAMESPACES`]; `NotFound` for a `from` that
/// does not exist.
pub fn create_from(from: MntNsId) -> KernelResult<MntNsId> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        let mut holds = HOLDS.lock();
        if holds.len() >= MAX_NAMESPACES {
            return Err(KernelError::ResourceExhausted);
        }
        holds.insert(id, 1);
    }
    OTHERS.fetch_add(1, Ordering::AcqRel);
    // The table, after the hold exists: a lookup that sees the id finds the
    // table, and one that does not yet cannot be in it.
    if let Err(e) = crate::fs::Vfs::copy_mount_table(from, id) {
        HOLDS.lock().remove(&id);
        OTHERS.fetch_sub(1, Ordering::AcqRel);
        return Err(e);
    }
    Ok(id)
}

/// One more hold on `id` -- a process put there, a handle opened. `false`
/// if it is gone (the root always exists, and is not counted).
pub fn retain(id: MntNsId) -> bool {
    if id == ROOT {
        return true;
    }
    match HOLDS.lock().get_mut(&id) {
        Some(n) => {
            *n = n.saturating_add(1);
            true
        }
        None => false,
    }
}

/// One hold on `id` given up; the namespace and its table go with the last.
/// Never called with a lock the VFS takes held: the table's end syncs the
/// filesystems only it had mounted.
pub fn release(id: MntNsId) {
    if id == ROOT {
        return;
    }
    let gone = {
        let mut holds = HOLDS.lock();
        let last = holds.get_mut(&id).is_some_and(|n| {
            *n = n.saturating_sub(1);
            *n == 0
        });
        if last {
            holds.remove(&id);
        }
        last
    };
    if gone {
        crate::fs::Vfs::drop_mount_table(id);
        OTHERS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Put process `pid` in namespace `id`, handing it the caller's hold on `id`
/// and giving up its hold on the one it leaves.
pub fn set_process(pid: ProcessId, id: MntNsId) {
    let old = {
        let mut members = MEMBERS.lock();
        if id == ROOT {
            members.remove(&pid)
        } else {
            members.insert(pid, id)
        }
    };
    if let Some(old) = old {
        release(old);
    }
}

/// `child` goes in `parent`'s namespace, as a fork's or a spawn's child
/// does. Nothing for a parent in the root.
pub fn inherit(parent: ProcessId, child: ProcessId) {
    let ns = of_process(parent);
    if ns != ROOT && retain(ns) {
        set_process(child, ns);
    }
}

/// Process `pid` is gone: its hold on its namespace goes with it.
pub fn process_gone(pid: ProcessId) {
    if !any() {
        return;
    }
    let old = MEMBERS.lock().remove(&pid);
    if let Some(old) = old {
        release(old);
    }
}

/// Process `pid` into a new namespace, a copy of the one it leaves --
/// `unshare(CLONE_NEWNS)`, and `clone(CLONE_NEWNS)`'s child.
///
/// # Errors
///
/// [`create_from`]'s.
pub fn unshare(pid: ProcessId) -> KernelResult<()> {
    let id = create_from(of_process(pid))?;
    set_process(pid, id);
    Ok(())
}

/// One namespace as `/proc/namespaces` and the kernel shell's `namespace`
/// command report it.
#[derive(Debug, Clone)]
pub struct NamespaceInfo {
    /// Its id; [`ROOT`] the system's.
    pub id: MntNsId,
    /// The processes in it and the handles open on it (the root's are not
    /// counted).
    pub holds: u32,
    /// How many processes are in it.
    pub processes: usize,
    /// How many mounts its table has.
    pub mounts: usize,
}

/// Every mount namespace, the root's first.
#[must_use]
pub fn list() -> Vec<NamespaceInfo> {
    let holds: Vec<(MntNsId, u32)> = HOLDS.lock().iter().map(|(&id, &n)| (id, n)).collect();
    let members: Vec<MntNsId> = MEMBERS.lock().values().copied().collect();
    let mut out = Vec::with_capacity(holds.len().saturating_add(1));
    out.push(NamespaceInfo {
        id: ROOT,
        holds: 0,
        processes: crate::proc::pcb::pids().len().saturating_sub(members.len()),
        mounts: crate::fs::Vfs::mount_count_in(ROOT),
    });
    for (id, n) in holds {
        out.push(NamespaceInfo {
            id,
            holds: n,
            processes: members.iter().filter(|&&m| m == id).count(),
            mounts: crate::fs::Vfs::mount_count_in(id),
        });
    }
    out
}

/// A copy of the root's table sees the root's mounts, takes a mount of its
/// own that the root does not see, and goes with its last hold.
///
/// # Errors
///
/// `InternalError` on the first check that fails.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    let fail = |what: &str| {
        serial_println!("[mntns]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    serial_println!("[mntns] Running self-test...");
    let root_mounts = crate::fs::Vfs::mount_count_in(ROOT);
    let ns = create_from(ROOT)?;
    let copied = crate::fs::Vfs::mount_count_in(ns) == root_mounts;
    let mounted = crate::fs::Vfs::mount_in(
        ns,
        "/mntns-selftest",
        alloc::boxed::Box::new(crate::fs::memfs::MemFs::new()),
    );
    let private = crate::fs::Vfs::mount_count_in(ns) == root_mounts.saturating_add(1)
        && crate::fs::Vfs::mount_count_in(ROOT) == root_mounts;
    let held = retain(ns);
    release(ns);
    let survives = crate::fs::Vfs::mount_count_in(ns) == root_mounts.saturating_add(1);
    release(ns);
    let gone = crate::fs::Vfs::mount_count_in(ns) == 0 && !HOLDS.lock().contains_key(&ns);
    // A kernel task inside a namespace resolves its paths there, and only
    // while it is: a mount made inside lands in that namespace's table, and
    // the root's own lookups never see it.
    let entered = entered_check()?;
    if !copied {
        return fail("a new namespace did not start with a copy of the root's mounts");
    }
    if mounted.is_err() || !private {
        return fail("a mount in a new namespace failed, or the root saw it");
    }
    if !(held && survives && gone) {
        return fail("a namespace's table did not live exactly as long as its holds");
    }
    if let Some(what) = entered {
        return fail(what);
    }
    serial_println!(
        "[mntns]   a copy of the table, a private mount, freed with the last hold; a kernel task's paths resolved in a namespace it entered, and only while it was in it: OK"
    );
    Ok(())
}

/// [`in_namespace`] checked: what was not so, `None` when all was.
///
/// # Errors
///
/// [`create_from`]'s.
fn entered_check() -> KernelResult<Option<&'static str>> {
    use crate::fs::Vfs;
    const AT: &str = "/mntns-selftest-entered";
    let ns = create_from(ROOT)?;
    let inside = in_namespace(ns, || {
        let here = current();
        let mounted = Vfs::mount(AT, alloc::boxed::Box::new(crate::fs::memfs::MemFs::new()));
        (here, mounted.is_ok(), Vfs::is_mount_point(AT))
    });
    let (in_root, after) = (Vfs::is_mount_point(AT), current());
    let in_ns = Vfs::mounts_full_in(ns)
        .iter()
        .any(|(path, _, _)| path.as_path() == crate::fs::path::Path::new(AT));
    let nested = in_namespace(ns, || {
        in_namespace(ROOT, current).ok() == Some(ROOT) && current() == ns
    });
    release(ns);
    let missing = in_namespace(ns, current);
    Ok(match inside {
        _ if missing.is_ok() => Some("a namespace that is gone was entered"),
        Ok((here, true, true)) if here == ns && in_ns && !in_root && after == ROOT => {
            (nested != Ok(true)).then_some("a nested namespace did not end only its own part")
        }
        _ => Some(
            "a kernel task inside a namespace did not resolve its paths there, or kept doing so after",
        ),
    })
}
