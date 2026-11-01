//! Mount namespaces: which mount table a process sees -- Linux's
//! `mnt_namespace` (`man 7 mount_namespaces`).
//!
//! Every process is in one. The **root** namespace's table is the system's,
//! the one the kernel mounted at boot. `unshare(CLONE_NEWNS)` and
//! `clone(CLONE_NEWNS)` give a process a copy of the table it was in
//! ([`create_held_from`], `fs::Vfs::copy_mount_table`), after which a mount
//! or an unmount in either is not seen in the other: no mount here
//! propagates to another namespace, every mount is private
//! (design-decisions 1555). A copy shares the filesystems, so a file is the
//! same file through either table.
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
//! where only one process's namespace reaches them, before it runs. Such a
//! namespace is sealed from its birth ([`create_sealed`]): no process may
//! enter it, whatever authority it holds ([`enter_process`]). The kernel's
//! own holds are [`Held`]s, given up as they drop.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::ProcessId;
use crate::proc::thread::TaskScoped;
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

/// Every namespace but the root, with its record ([`Record`]). A leaf lock.
static HOLDS: Mutex<BTreeMap<MntNsId, Record>> = Mutex::named(BTreeMap::new(), b"MNTNSREF");

/// What is kept of a namespace besides the root: one record, so that what
/// it says goes with the namespace as one.
#[derive(Debug, Clone, Copy)]
struct Record {
    /// How many hold it: the processes in it and the handles open on it.
    holds: u32,
    /// Whether no process may enter it ([`create_sealed`]).
    sealed: bool,
}

/// How many namespaces besides the root exist: while none does, [`current`]
/// answers the root without a lock.
static OTHERS: AtomicUsize = AtomicUsize::new(0);

/// The next id to give.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The tasks inside [`in_namespace`] now, each with the namespace it
/// entered last, which [`current`] answers for it.
static ENTERED: TaskScoped<MntNsId> = TaskScoped::new("entered mount namespace", b"MNTNSENT");

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
    if let Some(ns) = ENTERED.get(task) {
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
    let result = ENTERED.with(ns, body);
    release(ns);
    Ok(result)
}

/// Whether namespace `id` is sealed ([`create_sealed`]): no process may
/// enter it ([`enter_process`]), nor a copy of it.
#[must_use]
pub fn is_sealed(id: MntNsId) -> bool {
    HOLDS.lock().get(&id).is_some_and(|r| r.sealed)
}

/// Whether namespace `id` exists: the root, or one some process or handle
/// holds.
#[must_use]
pub fn exists(id: MntNsId) -> bool {
    id == ROOT || HOLDS.lock().contains_key(&id)
}

/// How many hold namespace `id` -- the processes in it and the handles on
/// it -- 0 for one that is gone, and for the root, which is not counted.
#[must_use]
pub fn holds(id: MntNsId) -> u32 {
    HOLDS.lock().get(&id).map_or(0, |r| r.holds)
}

/// Process `pid` put in namespace `id`, holding it -- what `setns` does
/// (`nsfs::enter`): refused for a sealed namespace, the check and the hold
/// taken as one, so that no seal can land between them.
///
/// # Errors
///
/// `NotPermitted` for a sealed namespace, whatever authority the caller
/// holds; `NotFound` for one that is gone; `NoSuchProcess` for a process
/// that is gone, or goes meanwhile, nothing kept for it.
pub fn enter_process(pid: ProcessId, id: MntNsId) -> KernelResult<()> {
    if id != ROOT {
        let mut holds = HOLDS.lock();
        let record = holds.get_mut(&id).ok_or(KernelError::NotFound)?;
        if record.sealed {
            return Err(KernelError::NotPermitted);
        }
        record.holds = record.holds.saturating_add(1);
    }
    // Dropped unhanded, should the process be gone already.
    let held = Held(id);
    if crate::proc::pcb::state(pid).is_none() {
        return Err(KernelError::NoSuchProcess);
    }
    held.give_to(pid)
}

/// A hold on a namespace, given up when it is dropped: one of the caller's
/// own, which it may hand to a process instead ([`Held::give_to`]). What
/// every hold the kernel takes for itself is, so that no exit path forgets
/// one.
#[must_use = "a namespace held for nothing is released at once"]
pub struct Held(MntNsId);

impl Held {
    /// The namespace held.
    #[must_use]
    pub fn id(&self) -> MntNsId {
        self.0
    }

    /// Process `pid` put in the namespace, the hold handed to it: its end
    /// gives it up, and the hold it had on the namespace it leaves is given
    /// up now.
    ///
    /// # Errors
    ///
    /// `NoSuchProcess` for a process that ended before its membership was
    /// made -- its end's clearing of its membership came first -- which
    /// gives the hold back here, since nothing else would: the membership
    /// made for it is taken away again, and only that one, should the pid
    /// already name another process.
    pub fn give_to(self, pid: ProcessId) -> KernelResult<()> {
        let id = core::mem::ManuallyDrop::new(self).0;
        set_process(pid, id);
        if crate::proc::pcb::state(pid).is_none() {
            leave(pid, id);
            return Err(KernelError::NoSuchProcess);
        }
        Ok(())
    }
}

/// A [`Held`] on a namespace sealed from its birth ([`create_sealed`]):
/// what may be handed something to keep from every process but the one it
/// is given to.
#[must_use = "a namespace held for nothing is released at once"]
pub struct Sealed(Held);

impl Sealed {
    /// The namespace held.
    #[must_use]
    pub fn id(&self) -> MntNsId {
        self.0.id()
    }

    /// The hold, as any other.
    pub fn into_held(self) -> Held {
        self.0
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        release(self.0);
    }
}

/// A new namespace with a copy of `from`'s table, held by the caller: a
/// sealed one if `from` is, as a copy of a sealed namespace holds what the
/// seal keeps.
///
/// # Errors
///
/// `ResourceExhausted` past [`MAX_NAMESPACES`]; `NotFound` for a `from` that
/// does not exist; copying the table's.
pub fn create_held_from(from: MntNsId) -> KernelResult<Held> {
    create(from, false).map(Held)
}

/// A new namespace, a copy of the root's, sealed from its birth: no process
/// may enter it at any moment of its life ([`enter_process`], which every
/// `setns` and `SYS_NAMESPACE_ENTER_PROCESS` goes through, whatever
/// authority it holds), nor a copy of it, and nothing but its maker's hold
/// holds it yet -- for the kernel to place a process's secret in
/// ([`in_namespace`]) before handing the hold to that process
/// ([`Held::give_to`]), so that it and its children alone reach it.
///
/// # Errors
///
/// [`create_held_from`]'s.
pub fn create_sealed() -> KernelResult<Sealed> {
    create(ROOT, true).map(|id| Sealed(Held(id)))
}

/// A new namespace with a copy of `from`'s table, held once by the caller:
/// sealed if `sealed` or if `from` is.
fn create(from: MntNsId, sealed: bool) -> KernelResult<MntNsId> {
    let id = {
        let mut holds = HOLDS.lock();
        if holds.len() >= MAX_NAMESPACES {
            return Err(KernelError::ResourceExhausted);
        }
        // A copy of a sealed namespace holds what the seal keeps -- the copy
        // of its mounts -- so is sealed too, from its birth; one of a
        // namespace that is gone is none.
        let from_sealed = match holds.get(&from) {
            Some(record) => record.sealed,
            None if from == ROOT => false,
            None => return Err(KernelError::NotFound),
        };
        // An id given only to a namespace made: none spent on a refusal.
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        holds.insert(
            id,
            Record {
                holds: 1,
                sealed: sealed || from_sealed,
            },
        );
        id
    };
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
        Some(record) => {
            record.holds = record.holds.saturating_add(1);
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
        let last = holds.get_mut(&id).is_some_and(|record| {
            record.holds = record.holds.saturating_sub(1);
            record.holds == 0
        });
        if last {
            holds.remove(&id);
        }
        last
    };
    if gone {
        // Its record, the seal with it, went first: a copy racing this finds
        // no namespace to copy (`create`), never one unsealed.
        crate::fs::Vfs::drop_mount_table(id);
        OTHERS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Put process `pid` in namespace `id`, handing it the caller's hold on `id`
/// and giving up its hold on the one it leaves. Private: every way in is
/// one of [`enter_process`], which refuses a sealed namespace,
/// [`Held::give_to`] and [`inherit`].
fn set_process(pid: ProcessId, id: MntNsId) {
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
/// [`create_held_from`]'s; `NotFound` for a namespace that went before it
/// could be held for the copy; `NoSuchProcess` for a process that ended
/// meanwhile ([`Held::give_to`]).
pub fn unshare(pid: ProcessId) -> KernelResult<()> {
    // Held while it is copied, so that its table, and its seal, are there to
    // copy whatever the process does meanwhile.
    let from = of_process(pid);
    if !retain(from) {
        return Err(KernelError::NotFound);
    }
    let copied = create_held_from(from);
    release(from);
    copied?.give_to(pid)
}

/// Process `pid`'s membership of namespace `id` taken away, and its hold
/// given up -- that membership alone: none of another namespace, which a
/// process now under the same pid may have.
fn leave(pid: ProcessId, id: MntNsId) {
    let left = {
        let mut members = MEMBERS.lock();
        let ours = members.get(&pid) == Some(&id);
        if ours {
            members.remove(&pid);
        }
        ours
    };
    if left {
        release(id);
    }
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
    /// Whether no process may enter it ([`create_sealed`]).
    pub sealed: bool,
}

/// Every mount namespace, the root's first.
#[must_use]
pub fn list() -> Vec<NamespaceInfo> {
    let records: Vec<(MntNsId, Record)> = HOLDS.lock().iter().map(|(&id, &r)| (id, r)).collect();
    let members: Vec<MntNsId> = MEMBERS.lock().values().copied().collect();
    let mut out = Vec::with_capacity(records.len().saturating_add(1));
    out.push(NamespaceInfo {
        id: ROOT,
        holds: 0,
        processes: crate::proc::pcb::pids().len().saturating_sub(members.len()),
        mounts: crate::fs::Vfs::mount_count_in(ROOT),
        sealed: false,
    });
    for (id, record) in records {
        out.push(NamespaceInfo {
            id,
            holds: record.holds,
            processes: members.iter().filter(|&&m| m == id).count(),
            mounts: crate::fs::Vfs::mount_count_in(id),
            sealed: record.sealed,
        });
    }
    out
}

/// A copy of the root's table sees the root's mounts, takes a mount of its
/// own that the root does not see, and goes with its last hold; a kernel
/// task's paths resolve in a namespace it entered, and only while it is in
/// it ([`in_namespace`]); and a sealed namespace's entry is refused, its
/// copies sealed, its seal gone with it ([`create_sealed`]).
///
/// # Errors
///
/// `InternalError` when a check of the table fails, on the first; when one
/// of the others does, after each failing one is said.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    let fail = |what: &str| {
        serial_println!("[mntns]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    serial_println!("[mntns] Running self-test...");
    let root_mounts = crate::fs::Vfs::mount_count_in(ROOT);
    let made = create_held_from(ROOT)?;
    let ns = made.id();
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
    drop(made);
    let gone = crate::fs::Vfs::mount_count_in(ns) == 0 && !exists(ns);
    // Each part's failure said, not only the first: one does not hide
    // another. A kernel task inside a namespace resolves its paths there,
    // and only while it is (entered_check); a sealed namespace keeps every
    // process out, and its copies too (sealed_check).
    let [sealed, copied_sealed] = sealed_check()?;
    let parts = [entered_check()?, sealed, copied_sealed];
    if !copied {
        return fail("a new namespace did not start with a copy of the root's mounts");
    }
    if mounted.is_err() || !private {
        return fail("a mount in a new namespace failed, or the root saw it");
    }
    if !(held && survives && gone) {
        return fail("a namespace's table did not live exactly as long as its holds");
    }
    let mut failed = false;
    for what in parts.into_iter().flatten() {
        serial_println!("[mntns]   FAIL: {}", what);
        failed = true;
    }
    if failed {
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[mntns]   a copy of the table, a private mount, freed with the last hold; a kernel task's paths resolved in a namespace it entered, and only while it was in it: OK"
    );
    Ok(())
}

/// [`create_sealed`] checked: an open namespace's entry gets past the
/// seal's refusal, a sealed one's does not and keeps no hold, a copy of a
/// sealed namespace is sealed, and the seal goes with the namespace: what
/// was not so of the entry and of the copy, each `None` when it was. No
/// process is moved: the pid entered with names none, which an open
/// namespace's entry finds after the seal's check, and a sealed one's never
/// reaches.
///
/// # Errors
///
/// [`create_held_from`]'s, and [`create_sealed`]'s.
fn sealed_check() -> KernelResult<[Option<&'static str>; 2]> {
    use crate::nsfs::NsKind;
    const NO_SUCH_PROCESS: ProcessId = ProcessId::MAX;
    let open = create_held_from(ROOT)?;
    let opened = crate::nsfs::enter(NO_SUCH_PROCESS, NsKind::Mnt, open.id());
    drop(open);
    let held = create_sealed()?;
    let ns = held.id();
    let shut = crate::nsfs::enter(NO_SUCH_PROCESS, NsKind::Mnt, ns);
    let copy = create_held_from(ns);
    let sealed = is_sealed(ns);
    // Its error, said below, is not kept: the copy goes first.
    let copy_sealed = copy
        .as_ref()
        .map(|copy| is_sealed(copy.id()))
        .map_err(|_| ());
    if let Err(e) = &copy {
        crate::serial_println!("[mntns]   a copy of a sealed namespace could not be made: {e:?}");
    }
    drop(copy);
    // The entry refused took no hold: the namespace goes with this one.
    let only_this = holds(ns) == 1;
    drop(held);
    let gone = !exists(ns) && !is_sealed(ns);
    Ok([
        match (opened, shut) {
            (Err(KernelError::NoSuchProcess), Err(KernelError::NotPermitted))
                if sealed && only_this && gone =>
            {
                None
            }
            _ => Some(
                "a sealed namespace's entry was not refused, a refused entry kept a hold, or the seal outlived its namespace",
            ),
        },
        match copy_sealed {
            Ok(true) => None,
            Ok(false) => Some("a copy of a sealed namespace was not sealed"),
            Err(()) => {
                Some("a copy of a sealed namespace could not be made, so its seal was not checked")
            }
        },
    ])
}

/// [`in_namespace`] checked: what was not so, `None` when all was.
///
/// # Errors
///
/// [`create_held_from`]'s.
fn entered_check() -> KernelResult<Option<&'static str>> {
    use crate::fs::Vfs;
    const AT: &str = "/mntns-selftest-entered";
    let held = create_held_from(ROOT)?;
    let ns = held.id();
    let holders = ENTERED.holders();
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
    // Every task's entry gone with its last call: the count back where it
    // was, so `current` asks no lock it need not.
    let balanced = ENTERED.holders() == holders;
    drop(held);
    let missing = in_namespace(ns, current);
    Ok(match inside {
        _ if missing.is_ok() => Some("a namespace that is gone was entered"),
        _ if !balanced => {
            Some("a task that left every namespace it entered is still counted inside one")
        }
        Ok((here, true, true)) if here == ns && in_ns && !in_root && after == ROOT => {
            (nested != Ok(true)).then_some("a nested namespace did not end only its own part")
        }
        _ => Some(
            "a kernel task inside a namespace did not resolve its paths there, or kept doing so after",
        ),
    })
}
