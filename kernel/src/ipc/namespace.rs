//! Per-process filesystem namespace support.
//!
//! Provides path isolation and remapping for process sandboxing.
//! Each process belongs to a namespace that defines which filesystem
//! paths are visible and how they're mapped.
//!
//! ## Design
//!
//! A namespace is a set of ordered rules that transform paths before
//! they reach the VFS.  Rules are evaluated in order (first match wins):
//!
//! - **Bind** rules remap a path prefix to another prefix.
//!   E.g., `/home/user` → `/sandbox/home` makes the process see
//!   a restricted view of the user's home directory.
//!
//! - **Hide** rules block access to a path prefix entirely.
//!   E.g., hide `/proc/self/ns` prevents the process from inspecting
//!   its own namespace.
//!
//! ## Inheritance
//!
//! When a child process is created, it inherits its parent's namespace
//! ID by default.  The parent can create a new namespace (optionally
//! cloning rules from another) and assign it before spawning the child.
//!
//! ## Syscall Interface
//!
//! | Syscall | Number | Description |
//! |---------|--------|-------------|
//! | SYS_NS_CREATE | 290 | Create a new namespace |
//! | SYS_NS_BIND | 291 | Add a bind (remapping) rule |
//! | SYS_NS_UNBIND | 292 | Remove a bind rule |
//! | SYS_NS_HIDE | 293 | Add a hide rule |
//! | SYS_NS_ATTACH | 294 | Set a process's namespace |
//! | SYS_NS_QUERY | 295 | Get a process's current namespace ID |
//!
//! ## Performance
//!
//! [`resolve_path`] runs before **every** path operation in the VFS, so its
//! cost is charged to every open, stat, read and write in the system.
//!
//! This section used to claim that resolution "adds one lock acquisition"
//! and that "the root namespace (ID 0) has no rules and short-circuits
//! immediately".  Both were false, and being written down is probably why
//! nobody checked: the pass-through case took **three** global spinlocks
//! ([`PROCESS_NS`], [`PROCESS_ROOT`], and `THREAD_OWNERS` via
//! [`crate::proc::thread::owner_process`]) plus a heap allocation, and then
//! returned its argument byte-for-byte unchanged.  Measured at **1948 ns**
//! under QEMU-TCG — 30% of an entire `stat("/")`, where an uncontended
//! global spinlock costs ~500 ns and is the dominant primitive.
//!
//! [`NS_FEATURES_ACTIVE`] is the fix: a single relaxed-ish atomic load that
//! short-circuits the whole function while no process has a namespace, a
//! chroot or a volume mount.  See its docs for why it is monotonic.
//!
//! Once namespaces *are* in use, resolution costs those three locks plus a
//! linear scan of the namespace's rules; typical namespaces have < 10 rules,
//! which is negligible beside the locks.
//!
//! ## References
//!
//! - Linux mount namespaces (unshare(CLONE_NEWNS))
//! - Plan 9 per-process namespaces (the original design)
//! - Design spec: "Per-process namespaces" in design.txt

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::serial_println;
use crate::sync::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Namespace identifier.  0 = root (default, no remapping).
pub type NamespaceId = u64;

/// The root namespace — all paths pass through unmodified.
pub const ROOT_NAMESPACE: NamespaceId = 0;

/// Maximum number of rules per namespace (prevents abuse).
const MAX_RULES_PER_NS: usize = 64;

/// Maximum number of namespaces (prevents resource exhaustion).
const MAX_NAMESPACES: usize = 256;

/// Maximum number of volume (bind) mounts a single process may hold
/// (prevents an unbounded per-process mount list).
const MAX_VOLUMES_PER_PROCESS: usize = 64;

/// Maximum length of a path prefix in a rule.
const MAX_PREFIX_LEN: usize = 1024;

/// A single namespace rule.
#[derive(Debug, Clone)]
enum NsRule {
    /// Remap paths matching `source_prefix` to `target_prefix`.
    ///
    /// E.g., Bind { source: "/home", target: "/sandbox/home" }
    /// means `/home/user/file.txt` → `/sandbox/home/user/file.txt`.
    Bind {
        source_prefix: PathBuf,
        target_prefix: PathBuf,
    },
    /// Block access to all paths matching this prefix.
    ///
    /// Any path starting with this prefix returns `PermissionDenied`.
    Hide {
        prefix: PathBuf,
    },
}

/// A namespace definition.
#[derive(Debug, Clone)]
struct Namespace {
    /// Unique ID.
    id: NamespaceId,
    /// Ordered rules (first match wins).
    rules: Vec<NsRule>,
    /// Parent namespace (for hierarchical resolution, 0 = no parent).
    parent_id: NamespaceId,
    /// Reference count (number of processes using this namespace).
    refcount: u64,
}

// ---------------------------------------------------------------------------
// Global namespace table
// ---------------------------------------------------------------------------

/// Next namespace ID to allocate.
static NEXT_NS_ID: AtomicU64 = AtomicU64::new(1);

/// Whether *any* namespace feature is in use anywhere in the system.
///
/// Clear means: no process has been assigned a namespace ([`PROCESS_NS`]),
/// given a filesystem root ([`PROCESS_ROOT`]), or given a volume mount
/// ([`PROCESS_MOUNTS`]).  In that state every branch [`resolve_path_for`]
/// could take is the identity branch, so it can return its argument without
/// consulting anything — which is the point: the three global spinlocks it
/// would otherwise take cost ~1500 ns of the 1948 ns measured for this
/// function, on the hot path of every VFS operation in the kernel.
///
/// This is the rarely-used-feature pattern (Linux's static keys), degraded to
/// an atomic load because we have no code patching.
///
/// **Monotonic by design: set, never cleared.**  Clearing it when the last
/// container tears down would be racy — a resolution already past the load
/// would take the fast path while state it should have seen is being torn
/// down — and would buy nothing worth that risk, since the cost of remaining
/// on the slow path after containers have been used once is exactly the cost
/// we have today.  The flag is therefore an assertion that the system has
/// *never* used namespaces, not that it is not using them right now.
///
/// Ordering: [`Ordering::Release`] on set, [`Ordering::Acquire`] on load, so a
/// resolution that observes the flag set also observes the map insertion that
/// set it.  A resolution that observes it clear is correct regardless of
/// ordering, because the only thing it can be racing is an insertion for a
/// *different* process — a process cannot be resolving a path inside the same
/// syscall that first gives it a namespace.
static NS_FEATURES_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Mark that namespace state has become non-trivial.  See [`NS_FEATURES_ACTIVE`].
///
/// Called at every site that inserts into [`PROCESS_NS`], [`PROCESS_ROOT`] or
/// [`PROCESS_MOUNTS`].  Deliberately *not* called on removal.
fn mark_ns_features_active() {
    NS_FEATURES_ACTIVE.store(true, Ordering::Release);
}

/// Test-only: whether the namespace fast path is still available.
///
/// Exposed so the benchmark can print it, because "the fast path did not
/// help" and "the fast path was never taken" are different findings and
/// guessing between them after the fact is how the last two attributions on
/// this path went wrong.
#[must_use]
pub fn ns_features_active() -> bool {
    NS_FEATURES_ACTIVE.load(Ordering::Acquire)
}

/// Restore the fast path, but **only** when there is genuinely no namespace
/// state — returns `false` and changes nothing otherwise.
///
/// Exists so the self-test can check each of the three publishing sites
/// (`attach`, `set_root`, `add_volume`) from a clean state.  Without it only
/// whichever site ran first could be checked: the flag is monotonic, so after
/// that "assert the flag is set afterwards" is trivially true and proves
/// nothing — a check that cannot fire is indistinguishable from one that
/// passes.
///
/// Safe despite [`NS_FEATURES_ACTIVE`] being documented as monotonic, and the
/// guard is what makes it so: clearing the flag while all three maps are empty
/// cannot disable enforcement, because with the maps empty the slow path
/// returns the identity anyway.  It is the *ground truth* being restored, not
/// overridden.  The monotonicity in production is a policy (avoid teardown
/// races), not a safety requirement, and this respects it by refusing to act
/// when any state exists.
#[must_use]
pub fn reset_ns_features_if_trivial() -> bool {
    // Hold all three locks across the check-and-clear so state cannot appear
    // between deciding it is empty and acting on that decision.
    //
    // LOCK ORDER: PROCESS_NS -> PROCESS_ROOT -> PROCESS_MOUNTS. This and
    // `ns_state_is_trivial` are the only places that hold more than one of the
    // three at a time, and both use this order. (Everywhere else -- `detach`,
    // `resolve_path_for` -- acquires them strictly one at a time, so they
    // cannot participate in a cycle.)
    let pns = PROCESS_NS.lock();
    let roots = PROCESS_ROOT.lock();
    let mounts = PROCESS_MOUNTS.lock();
    if !(pns.is_empty() && roots.is_empty() && mounts.is_empty()) {
        return false;
    }
    NS_FEATURES_ACTIVE.store(false, Ordering::Release);
    true
}

/// Whether the three maps [`NS_FEATURES_ACTIVE`] summarises are all empty.
///
/// This is the ground truth the flag caches.  The flag being stale in the
/// *clear* direction — state present, flag clear — is not a slow path, it is
/// **silently disabled sandboxing**: a jailed process would resolve paths
/// against the host root.  So the fast path asserts this in debug builds
/// rather than trusting that every present and future insertion site
/// remembered to call [`mark_ns_features_active`].
///
/// Takes three locks, hence debug-only.
#[cfg(debug_assertions)]
fn ns_state_is_trivial() -> bool {
    PROCESS_NS.lock().is_empty()
        && PROCESS_ROOT.lock().is_empty()
        && PROCESS_MOUNTS.lock().is_empty()
}

/// The fast-path guard: true when nothing can change the caller's path.
///
/// Factored out so the debug-only cross-check against [`ns_state_is_trivial`]
/// exists once instead of at each of the four call sites, where three of them
/// would eventually be updated and the fourth would not.
fn ns_fast_path_available() -> bool {
    // NB: the *only* direct read of the flag outside `mark_ns_features_active`
    // and `ns_features_active`. A blind textual replace turned this line into a
    // self-call once already -- the same substring-out-of-context mistake that
    // has now bitten three times in this tree.
    let clear = !NS_FEATURES_ACTIVE.load(Ordering::Acquire);
    #[cfg(debug_assertions)]
    if clear {
        debug_assert!(
            ns_state_is_trivial(),
            "namespace state exists but NS_FEATURES_ACTIVE is clear: some \
             insertion site is missing mark_ns_features_active(), and \
             sandboxing is silently not being enforced"
        );
    }
    clear
}

/// Global table of all namespaces.
///
/// The root namespace (ID 0) is implicit — it's not stored here.
/// Any process with namespace_id = 0 gets unmodified path access.
static NS_TABLE: Mutex<BTreeMap<NamespaceId, Namespace>> =
    Mutex::new(BTreeMap::new());

/// Per-process namespace assignment.
///
/// Maps ProcessId (u64) → NamespaceId.  Processes not in this table
/// are in the root namespace (ID 0).
static PROCESS_NS: Mutex<BTreeMap<u64, NamespaceId>> =
    Mutex::new(BTreeMap::new());

/// Per-process filesystem root (chroot).
///
/// Maps ProcessId (u64) → an absolute, normalized host path prefix that
/// the process's view of the filesystem is rooted at (e.g. a container's
/// overlay rootfs `/containers/<id>/rootfs`).  Processes absent from this
/// table see the host root `/` (no jail).
///
/// This is distinct from `PROCESS_NS` Bind/Hide rules: those remap paths
/// *within* the guest's view, whereas the root re-anchors that whole view
/// onto a host subtree and **clamps `..`** so a guest absolute path can
/// never escape above its root (the security-critical difference from a
/// plain prefix Bind rule).  See `apply_root` / `normalize_jailed`.
static PROCESS_ROOT: Mutex<BTreeMap<u64, PathBuf>> =
    Mutex::new(BTreeMap::new());

/// A single volume (bind) mount: a guest-path prefix that resolves to an
/// arbitrary host-path target instead of being prefixed with the container
/// rootfs.  This is the Docker `-v host:guest` mechanism.
#[derive(Clone)]
struct VolumeMount {
    /// Guest absolute path the volume is mounted at (e.g. `/data`), already
    /// normalized (no trailing slash except the root case, no `.`/`..`).
    guest_prefix: PathBuf,
    /// Host absolute path the volume's contents live at (e.g.
    /// `/host/shared`), already normalized.
    host_target: PathBuf,
    /// When `true`, writes into this volume's subtree are denied (`EROFS`).
    /// The Docker `-v host:guest:ro` mechanism. Path *resolution* is
    /// unaffected — only mutating operations are gated (see
    /// [`check_writable_for`]).
    read_only: bool,
}

/// Per-process volume (bind) mounts.
///
/// Maps ProcessId (u64) → an ordered list of [`VolumeMount`]s.  Consulted
/// during the chroot-resolution step ([`resolve_path_for`] step 2): after a
/// guest absolute path is `..`-clamped within the jail, the **longest**
/// matching volume prefix wins and the path is re-anchored under that
/// volume's host target instead of under the container rootfs.  A process
/// absent from this table has no volumes (every path resolves against its
/// rootfs as before).
///
/// Security: matching happens on the already-`..`-clamped guest path
/// (`normalize_jailed`), so a guest cannot use `..` to slip *into* a volume
/// it shouldn't reach, nor *out* of a volume's subtree — the whole path is
/// normalized against the guest root `/` before any volume is considered.
static PROCESS_MOUNTS: Mutex<BTreeMap<u64, Vec<VolumeMount>>> =
    Mutex::new(BTreeMap::new());

/// Set of processes whose container **root filesystem** is read-only.
///
/// This is the Docker `--read-only` mechanism: the container rootfs is
/// mounted read-only, so any write that resolves into the rootfs (i.e. does
/// *not* land in a writable volume) is denied with `EROFS`.  Writable (`:rw`)
/// volumes punch holes through this: a write into a writable volume's subtree
/// is still permitted, exactly as Docker allows `-v ...:rw` mounts to remain
/// writable inside a `--read-only` container.  Read-only (`:ro`) volumes stay
/// read-only as before.
///
/// Only meaningful for a *jailed* process (one with a `PROCESS_ROOT`): without
/// a chroot root there is no container rootfs to make read-only, so the flag
/// is ignored.  A process absent from this set has a writable rootfs.
static PROCESS_ROOT_RO: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// Per-process UTS hostname override (the Docker `--hostname` mechanism).
///
/// A process present in this map sees the stored hostname from `uname(2)` /
/// `gethostname(2)` instead of the global system hostname.  This models a
/// per-container UTS namespace: a container's init process is given the
/// container hostname, so a Linux program inside the container reads it
/// rather than the host's name.  Absent → the process sees the global
/// hostname (`nameservice::get_hostname`).
///
/// Like the other per-process namespace maps it is keyed by PID and cleared
/// in [`detach`], so a later process reusing the PID does not inherit a stale
/// hostname.  (Child processes do not currently inherit it automatically —
/// the same limitation as `PROCESS_ROOT`; container children are expected to
/// be registered via the container layer.)
static PROCESS_HOSTNAME: Mutex<BTreeMap<u64, String>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new namespace.
///
/// If `clone_from` is non-zero, copies the rules from the specified
/// namespace.  Otherwise creates an empty namespace.
///
/// Returns the new namespace's ID.
pub fn create(clone_from: NamespaceId) -> KernelResult<NamespaceId> {
    let mut table = NS_TABLE.lock();

    if table.len() >= MAX_NAMESPACES {
        return Err(KernelError::ResourceExhausted);
    }

    let id = NEXT_NS_ID.fetch_add(1, Ordering::Relaxed);

    let rules = if clone_from != ROOT_NAMESPACE {
        // Clone rules from the source namespace.
        let source = table.get(&clone_from)
            .ok_or(KernelError::NotFound)?;
        source.rules.clone()
    } else {
        Vec::new()
    };

    let ns = Namespace {
        id,
        rules,
        parent_id: clone_from,
        refcount: 0,
    };

    table.insert(id, ns);

    serial_println!(
        "[namespace] Created namespace {} (cloned from {})",
        id, clone_from
    );

    Ok(id)
}

/// Destroy a namespace (only if refcount is 0).
///
/// Called when no processes reference this namespace anymore.
/// Returns `ResourceExhausted` if processes are still attached.
pub fn destroy(ns_id: NamespaceId) -> KernelResult<()> {
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_TABLE.lock();
    let ns = table.get(&ns_id)
        .ok_or(KernelError::NotFound)?;

    if ns.refcount > 0 {
        return Err(KernelError::ResourceExhausted);
    }

    table.remove(&ns_id);

    serial_println!("[namespace] Destroyed namespace {}", ns_id);
    Ok(())
}

/// Add a bind (remapping) rule to a namespace.
///
/// Paths starting with `source_prefix` will be rewritten to start
/// with `target_prefix` instead.  The rule is appended to the end
/// of the rule list (last priority; existing rules match first).
///
/// # Arguments
///
/// - `ns_id` — target namespace.
/// - `source_prefix` — the path prefix to match (must start with `/`).
/// - `target_prefix` — the replacement prefix (must start with `/`).
pub fn bind(
    ns_id: NamespaceId,
    source_prefix: impl AsRef<Path>,
    target_prefix: impl AsRef<Path>,
) -> KernelResult<()> {
    let (source_prefix, target_prefix) =
        (source_prefix.as_ref(), target_prefix.as_ref());
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::InvalidArgument);
    }

    validate_prefix(source_prefix)?;
    validate_prefix(target_prefix)?;

    let mut table = NS_TABLE.lock();
    let ns = table.get_mut(&ns_id)
        .ok_or(KernelError::NotFound)?;

    if ns.rules.len() >= MAX_RULES_PER_NS {
        return Err(KernelError::ResourceExhausted);
    }

    ns.rules.push(NsRule::Bind {
        source_prefix: source_prefix.to_path_buf(),
        target_prefix: target_prefix.to_path_buf(),
    });

    Ok(())
}

/// Remove a bind rule from a namespace (by source prefix match).
///
/// Removes the first rule whose source prefix matches exactly.
/// Returns `NotFound` if no such rule exists.
pub fn unbind(
    ns_id: NamespaceId,
    source_prefix: impl AsRef<Path>,
) -> KernelResult<()> {
    let source_prefix = source_prefix.as_ref();
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_TABLE.lock();
    let ns = table.get_mut(&ns_id)
        .ok_or(KernelError::NotFound)?;

    let pos = ns.rules.iter().position(|rule| match rule {
        NsRule::Bind { source_prefix: s, .. } => s.as_path() == source_prefix,
        _ => false,
    });

    match pos {
        Some(idx) => {
            ns.rules.remove(idx);
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Add a hide rule to a namespace.
///
/// All paths starting with this prefix will be blocked with
/// `PermissionDenied`.  The rule is appended to the end of the rule
/// list.
pub fn hide(ns_id: NamespaceId, prefix: impl AsRef<Path>) -> KernelResult<()> {
    let prefix = prefix.as_ref();
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::InvalidArgument);
    }

    validate_prefix(prefix)?;

    let mut table = NS_TABLE.lock();
    let ns = table.get_mut(&ns_id)
        .ok_or(KernelError::NotFound)?;

    if ns.rules.len() >= MAX_RULES_PER_NS {
        return Err(KernelError::ResourceExhausted);
    }

    ns.rules.push(NsRule::Hide {
        prefix: prefix.to_path_buf(),
    });

    Ok(())
}

/// Attach a process to a namespace.
///
/// The process will use this namespace for all future path resolution.
/// Pass `ROOT_NAMESPACE` (0) to return to the default namespace.
pub fn attach(process_id: u64, ns_id: NamespaceId) -> KernelResult<()> {
    if ns_id != ROOT_NAMESPACE {
        // Verify the namespace exists and increment refcount.
        let mut table = NS_TABLE.lock();
        let ns = table.get_mut(&ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.refcount = ns.refcount.saturating_add(1);
    }

    let mut pns = PROCESS_NS.lock();

    // Decrement refcount on the old namespace (if any).
    if let Some(&old_ns_id) = pns.get(&process_id) {
        if old_ns_id != ROOT_NAMESPACE {
            // Must drop PROCESS_NS before locking NS_TABLE to avoid
            // potential lock-order issues.  Actually, we can do this
            // inline since we already dropped the NS_TABLE lock above
            // ... wait, no.  Let's do this carefully.
            //
            // We can't hold both locks simultaneously — store the old
            // ID and decrement after.
            drop(pns);
            {
                let mut table = NS_TABLE.lock();
                if let Some(old_ns) = table.get_mut(&old_ns_id) {
                    old_ns.refcount = old_ns.refcount.saturating_sub(1);
                }
            }
            pns = PROCESS_NS.lock();
        }
    }

    if ns_id == ROOT_NAMESPACE {
        pns.remove(&process_id);
    } else {
        pns.insert(process_id, ns_id);
        // Namespace state is now non-trivial: disable the resolve fast path.
        // Set while still holding `pns` so no resolver can observe the
        // insertion without also observing the flag. See `NS_FEATURES_ACTIVE`.
        mark_ns_features_active();
    }

    Ok(())
}

/// Detach a process from its namespace (called on process exit).
///
/// Decrements the namespace's refcount.  If the refcount reaches 0,
/// the namespace is eligible for cleanup (but not automatically
/// destroyed — that requires an explicit `destroy()` call or GC).
pub fn detach(process_id: u64) {
    let ns_id = {
        let mut pns = PROCESS_NS.lock();
        pns.remove(&process_id).unwrap_or(ROOT_NAMESPACE)
    };

    if ns_id != ROOT_NAMESPACE {
        let mut table = NS_TABLE.lock();
        if let Some(ns) = table.get_mut(&ns_id) {
            ns.refcount = ns.refcount.saturating_sub(1);
        }
    }

    // Also drop any filesystem-root jail and volume mounts this process
    // held, so a later process that happens to reuse the same PID does not
    // inherit them.
    PROCESS_ROOT.lock().remove(&process_id);
    PROCESS_MOUNTS.lock().remove(&process_id);
    PROCESS_ROOT_RO.lock().remove(&process_id);
    PROCESS_HOSTNAME.lock().remove(&process_id);
}

/// Query which namespace a process belongs to.
pub fn query(process_id: u64) -> NamespaceId {
    let pns = PROCESS_NS.lock();
    pns.get(&process_id).copied().unwrap_or(ROOT_NAMESPACE)
}

/// Set a process's filesystem root (chroot/pivot_root).
///
/// After this call, every absolute path the process resolves is
/// normalized within the jail (with `..` clamped at the jail root, so it
/// cannot escape) and then re-anchored under `root`.  For example, with
/// `root = "/containers/c1/rootfs"`, the guest path `/bin/sh` resolves to
/// the host path `/containers/c1/rootfs/bin/sh`, and `/../etc/passwd`
/// resolves to `/containers/c1/rootfs/etc/passwd` (the `..` is clamped).
///
/// `root` must be an absolute path.  It is normalized before storage, so
/// trailing slashes and `.`/`..` components are collapsed.  Setting a root
/// that normalizes to `/` is equivalent to clearing the jail (a no-op
/// root), so the process returns to the host root.
pub fn set_root(process_id: u64, root: impl AsRef<Path>) -> KernelResult<()> {
    let root = root.as_ref();
    validate_prefix(root)?;
    let normalized = normalize_jailed(root);
    if normalized.as_path() == Path::new("/") {
        // A root of "/" means "no jail" — clear any existing root rather
        // than storing a prefix that would double leading slashes.
        PROCESS_ROOT.lock().remove(&process_id);
        return Ok(());
    }
    let mut roots = PROCESS_ROOT.lock();
    roots.insert(process_id, normalized);
    // See the matching call in `attach`: set under the lock that publishes
    // the state, so the flag can never lag the map.
    mark_ns_features_active();
    drop(roots);
    Ok(())
}

/// Clear a process's filesystem root, returning it to the host root.
///
/// Called when a jailed process exits or is removed from its container.
/// Idempotent: clearing a process that has no root is a no-op.
pub fn clear_root(process_id: u64) {
    PROCESS_ROOT.lock().remove(&process_id);
    PROCESS_ROOT_RO.lock().remove(&process_id);
}

/// Mark (or unmark) a process's container root filesystem as read-only.
///
/// This is the Docker `--read-only` mechanism.  When set, any write that
/// resolves into the container rootfs — i.e. does not land in a writable
/// volume — is denied with `EROFS` (see [`check_writable_for`]).  Writable
/// (`:rw`) volumes remain writable; read-only (`:ro`) volumes remain
/// read-only.  Only meaningful for a jailed process; a process with no
/// chroot root ignores the flag (its rootfs is the host root).
///
/// `read_only == false` clears the flag (rootfs becomes writable again).
pub fn set_root_read_only(process_id: u64, read_only: bool) {
    let mut set = PROCESS_ROOT_RO.lock();
    if read_only {
        set.insert(process_id);
    } else {
        set.remove(&process_id);
    }
}

/// Query whether a process's container root filesystem is read-only.
pub fn is_root_read_only(process_id: u64) -> bool {
    PROCESS_ROOT_RO.lock().contains(&process_id)
}

/// Set a process's UTS hostname override (the Docker `--hostname` mechanism).
///
/// After this call, `uname(2)`/`gethostname(2)` from the process (and any
/// process the container layer registers with the same hostname) report
/// `name` instead of the global system hostname.
///
/// # Errors
///
/// - `InvalidArgument` if `name` is empty, longer than 64 bytes (the
///   `__NEW_UTS_LEN` field width), or contains a NUL byte.
pub fn set_hostname(process_id: u64, name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 || name.as_bytes().contains(&0) {
        return Err(KernelError::InvalidArgument);
    }
    PROCESS_HOSTNAME.lock().insert(process_id, String::from(name));
    Ok(())
}

/// Query a process's UTS hostname override, if any.
///
/// Returns `None` when the process has no override and should see the global
/// system hostname.
#[must_use]
pub fn hostname_for(process_id: u64) -> Option<String> {
    PROCESS_HOSTNAME.lock().get(&process_id).cloned()
}

/// Clear a process's UTS hostname override (return it to the global hostname).
///
/// Idempotent: clearing a process with no override is a no-op.
pub fn clear_hostname(process_id: u64) {
    PROCESS_HOSTNAME.lock().remove(&process_id);
}

/// Query a process's filesystem root, if any.
///
/// Returns `None` if the process is not jailed (sees the host root).
pub fn get_root(process_id: u64) -> Option<PathBuf> {
    PROCESS_ROOT.lock().get(&process_id).cloned()
}

/// Add a volume (bind) mount to a process: the guest path `guest_prefix`
/// resolves to the host path `host_target` instead of being prefixed with
/// the container rootfs.  This is the Docker `-v host_target:guest_prefix`
/// mechanism.
///
/// Both paths must be absolute.  They are normalized before storage.  A
/// `guest_prefix` that normalizes to `/` (the guest root) is rejected:
/// re-rooting the entire guest view is the job of [`set_root`], not a
/// volume.  Re-adding a volume at an existing `guest_prefix` replaces it.
///
/// Volumes are matched **longest-prefix-first** at resolution time, so a
/// volume at `/data/cache` correctly shadows a volume at `/data`.
///
/// `read_only == true` marks the volume so that writes into its subtree are
/// rejected with `EROFS` (see [`check_writable_for`]); resolution itself is
/// unaffected. Re-adding at an existing prefix overwrites both the host
/// target and the read-only flag.
pub fn add_volume(
    process_id: u64,
    guest_prefix: impl AsRef<Path>,
    host_target: impl AsRef<Path>,
    read_only: bool,
) -> KernelResult<()> {
    let (guest_prefix, host_target) =
        (guest_prefix.as_ref(), host_target.as_ref());
    validate_prefix(guest_prefix)?;
    validate_prefix(host_target)?;
    let guest = normalize_jailed(guest_prefix);
    if guest.as_path() == Path::new("/") {
        // A volume at the guest root would shadow the entire rootfs — that
        // is what `set_root` is for, not a volume.
        return Err(KernelError::InvalidArgument);
    }
    let host = normalize_jailed(host_target);
    let mut mounts = PROCESS_MOUNTS.lock();
    // `entry(..).or_default()` itself makes the map non-empty, so the flag
    // must be set here rather than only on the success paths below: an early
    // `return Err(ResourceExhausted)` still leaves an entry behind.
    mark_ns_features_active();
    let list = mounts.entry(process_id).or_default();
    // Replace an existing volume at the same guest prefix rather than
    // stacking duplicates (last-writer-wins, matching Docker re-mount).
    if let Some(existing) = list.iter_mut().find(|v| v.guest_prefix == guest) {
        existing.host_target = host;
        existing.read_only = read_only;
        return Ok(());
    }
    if list.len() >= MAX_VOLUMES_PER_PROCESS {
        return Err(KernelError::ResourceExhausted);
    }
    list.push(VolumeMount { guest_prefix: guest, host_target: host, read_only });
    Ok(())
}

/// Remove all volume mounts for a process.
///
/// Called when a jailed process exits (alongside [`clear_root`]) so a later
/// process that reuses the same PID does not inherit stale volumes.
/// Idempotent.
pub fn clear_mounts(process_id: u64) {
    PROCESS_MOUNTS.lock().remove(&process_id);
}

/// Number of volume mounts a process has (diagnostics/testing).
pub fn volume_count(process_id: u64) -> usize {
    PROCESS_MOUNTS.lock().get(&process_id).map_or(0, Vec::len)
}

/// A single entry in a jailed process's own mount view.
///
/// Used to render a container process's `/proc/<pid>/mountinfo` from *its*
/// perspective (its rootfs jail and volumes) rather than leaking the host's
/// global mount table.  The `host_target` is the resolved host path backing
/// the mount, so the caller can look up the real filesystem type serving it.
#[derive(Debug, Clone)]
pub struct MountViewEntry {
    /// Mount point as the *guest* sees it (`/` for the rootfs, the volume's
    /// guest prefix otherwise).
    pub guest_path: PathBuf,
    /// Host path whose backing filesystem serves this mount.  For the rootfs
    /// entry this is the jail root; for a volume it is the volume's host
    /// target.  The caller resolves the fstype from the global mount table.
    pub host_target: PathBuf,
    /// `true` if writes into this mount are denied (`EROFS`): a `:ro` volume,
    /// or any rootfs path under a `--read-only` container root.
    pub read_only: bool,
}

/// Return a jailed (container) process's own filesystem mount view, or `None`
/// if the process is not jailed (it sees the host's global mount table).
///
/// The first entry is always the container rootfs at guest `/` (read-only iff
/// the container root is read-only), followed by each volume/tmpfs bind mount
/// at its guest prefix, in insertion order.  This is the data behind a
/// container process's `/proc/<pid>/mountinfo`: a process inside a container
/// must see *its* mounts, not the host's, both for correctness and to avoid
/// leaking the host mount topology into the container.
#[must_use]
pub fn mount_view_for(process_id: u64) -> Option<Vec<MountViewEntry>> {
    let root = PROCESS_ROOT.lock().get(&process_id).cloned()?;
    let root_ro = PROCESS_ROOT_RO.lock().contains(&process_id);
    let mut view = Vec::new();
    // The rootfs is the guest's `/`, backed by the jail root on the host.
    view.push(MountViewEntry {
        guest_path: PathBuf::from("/"),
        host_target: root,
        read_only: root_ro,
    });
    if let Some(list) = PROCESS_MOUNTS.lock().get(&process_id) {
        for v in list {
            view.push(MountViewEntry {
                guest_path: v.guest_prefix.clone(),
                host_target: v.host_target.clone(),
                read_only: v.read_only,
            });
        }
    }
    Some(view)
}

/// Find the longest-matching volume for a (jail-normalized) guest path.
///
/// Returns `(host_target, suffix)` where `suffix` is the *relative*
/// remainder of the guest path after the volume's `guest_prefix` (empty when
/// the path *is* the mount point).  `None` if no volume matches.
fn longest_volume_match(
    list: &[VolumeMount],
    normalized_guest: &Path,
) -> Option<(PathBuf, PathBuf)> {
    let mut best: Option<&VolumeMount> = None;
    for v in list {
        if normalized_guest.starts_with(&v.guest_prefix)
            && best.is_none_or(|b| v.guest_prefix.len() > b.guest_prefix.len())
        {
            best = Some(v);
        }
    }
    let v = best?;
    // `starts_with` already confirmed a component-boundary match above, so
    // `strip_prefix` cannot fail; the fallback keeps this panic-free.
    let suffix = normalized_guest
        .strip_prefix(&v.guest_prefix)
        .unwrap_or_default();
    Some((v.host_target.clone(), suffix))
}

/// Resolve a path through the current process's namespace.
///
/// This is the main integration point with the VFS.  Called before
/// any path operation (read, write, stat, etc.) to apply namespace
/// rules.
///
/// Returns:
/// - `Ok(translated_path)` — the path after namespace translation.
/// - `Err(PermissionDenied)` — the path is hidden in this namespace.
///
/// For the root namespace, this is a no-op (returns the input path
/// unchanged).
pub fn resolve_path(path: impl AsRef<Path>) -> KernelResult<PathBuf> {
    let path = path.as_ref();
    // Fast path: while no namespace feature has ever been used, the result is
    // the input and nothing below can change that -- so skip `owner_process`'s
    // `THREAD_OWNERS.lock()` as well as the two locks in `resolve_path_for`.
    // See `NS_FEATURES_ACTIVE`.
    if ns_fast_path_available() {
        return Ok(path.to_path_buf());
    }
    let task_id = crate::sched::current_task_id();
    let process_id = crate::proc::thread::owner_process(task_id)
        .unwrap_or(0);

    resolve_path_for(process_id, path)
}

/// Resolve a path for a specific process's namespace.
///
/// Separated from `resolve_path()` to allow resolving on behalf of
/// other processes (e.g., for `execve` path lookup in the child's
/// namespace context).
pub fn resolve_path_for(
    process_id: u64,
    path: impl AsRef<Path>,
) -> KernelResult<PathBuf> {
    let path = path.as_ref();
    // Same fast path as `resolve_path`, repeated here because this is the
    // shared entry point (41 call sites) and most of them do not come through
    // `resolve_path`. The flag is global, so it is valid for *any*
    // `process_id`: if no process has ever had a namespace, a root or a
    // volume, then neither has this one.
    if ns_fast_path_available() {
        return Ok(path.to_path_buf());
    }
    // Step 1: apply Bind/Hide rules in the guest's path space.
    let ns_id = {
        let pns = PROCESS_NS.lock();
        pns.get(&process_id).copied().unwrap_or(ROOT_NAMESPACE)
    };

    let translated = if ns_id == ROOT_NAMESPACE {
        // No namespace rules, pass through.
        path.to_path_buf()
    } else {
        let table = NS_TABLE.lock();
        match table.get(&ns_id) {
            Some(ns) => apply_rules(&ns.rules, path)?,
            None => {
                // Namespace was destroyed but process still references it.
                // Fall back to root namespace behavior.
                path.to_path_buf()
            }
        }
    };

    // Step 2: re-anchor the (guest) path under the process's filesystem
    // root, if it has one.  This is the chroot/pivot_root step — the guest
    // view from step 1 is physically rooted on a host subtree, with `..`
    // clamped so the jail cannot be escaped.
    let root = { PROCESS_ROOT.lock().get(&process_id).cloned() };
    match root {
        Some(r) => {
            // Volume (bind) mounts only apply within a jail: they re-anchor a
            // guest subtree onto an arbitrary host target instead of under the
            // container rootfs.  Most jailed processes have none, so avoid the
            // clone when the list is empty.
            let volumes = {
                PROCESS_MOUNTS.lock().get(&process_id).cloned().unwrap_or_default()
            };
            if volumes.is_empty() {
                Ok(apply_root(&r, &translated))
            } else {
                Ok(apply_root_with_volumes(&r, &volumes, &translated))
            }
        }
        None => Ok(translated),
    }
}

/// Check whether the current process may *write* to `path`.
///
/// Returns `Err(KernelError::ReadOnlyFilesystem)` (→ `EROFS`) when `path`
/// resolves into a read-only volume mount, `Ok(())` otherwise. This is the
/// write-side companion to [`resolve_path`]: read/stat operations call
/// `resolve_path`, while mutating operations (open-for-write, mkdir, rmdir,
/// unlink, rename, symlink, link, truncate, chmod, xattr writes) additionally
/// call this. For processes with no read-only volumes and a writable rootfs —
/// every non-container process, and writable containers without `:ro` mounts —
/// this is a cheap `Ok(())`.
pub fn check_writable(path: impl AsRef<Path>) -> KernelResult<()> {
    // Skip `owner_process`'s `THREAD_OWNERS.lock()` too -- the process id is
    // not needed to answer when nothing is gated. See `NS_FEATURES_ACTIVE`.
    if ns_fast_path_available() {
        return Ok(());
    }
    let task_id = crate::sched::current_task_id();
    let process_id = crate::proc::thread::owner_process(task_id).unwrap_or(0);
    check_writable_for(process_id, path.as_ref())
}

/// Like [`check_writable`] but for a specific process (used by tests and by
/// callers acting on another process's behalf).
///
/// Mirrors the volume-matching pipeline of [`resolve_path_for`]: it applies
/// the same step-1 namespace translation and `..`-clamping normalization, then
/// finds the longest-matching volume prefix and decides as follows:
///
/// - matched a read-only (`:ro`) volume → denied (`EROFS`);
/// - matched a writable (`:rw`) volume → allowed (even under a read-only root);
/// - matched no volume (path is in the container rootfs) → denied if the
///   rootfs is read-only (`--read-only`), allowed otherwise.
///
/// Read-only enforcement only applies to *jailed* processes (volumes and the
/// read-only-root flag only take effect within a chroot root); an unjailed
/// process has no volume re-anchoring and is always writable here.
pub fn check_writable_for(
    process_id: u64,
    path: impl AsRef<Path>,
) -> KernelResult<()> {
    let path = path.as_ref();
    // Fast path, before any lock. The existing one below is *after* two global
    // spinlock acquisitions, which at ~500 ns each is most of its cost -- it
    // avoids the path walk, not the locks.
    //
    // Sound because a denial here requires a `PROCESS_ROOT` entry: the check at
    // the end of this preamble returns `Ok` for any process without one, and
    // `PROCESS_ROOT_RO` alone therefore cannot deny. Every `PROCESS_ROOT`
    // insertion sets `NS_FEATURES_ACTIVE`, so a clear flag proves no process
    // can be denied. See `NS_FEATURES_ACTIVE`.
    if ns_fast_path_available() {
        return Ok(());
    }
    let volumes = {
        let mounts = PROCESS_MOUNTS.lock();
        mounts.get(&process_id).cloned().unwrap_or_default()
    };
    let root_ro = PROCESS_ROOT_RO.lock().contains(&process_id);
    // Fast path: a process with no read-only volumes and a writable rootfs has
    // nothing gated — every non-container process, and writable containers
    // without `:ro` mounts, take this cheap exit.
    if volumes.is_empty() && !root_ro {
        return Ok(());
    }
    // Volumes and the read-only-root flag only apply within a chroot jail;
    // without a root there is no container rootfs and no re-anchored volumes,
    // so nothing is read-only-gated.
    if PROCESS_ROOT.lock().get(&process_id).is_none() {
        return Ok(());
    }
    // Step 1: the same namespace (Bind/Hide) translation the resolver applies.
    let ns_id = {
        let pns = PROCESS_NS.lock();
        pns.get(&process_id).copied().unwrap_or(ROOT_NAMESPACE)
    };
    let translated = if ns_id == ROOT_NAMESPACE {
        path.to_path_buf()
    } else {
        let table = NS_TABLE.lock();
        match table.get(&ns_id) {
            Some(ns) => apply_rules(&ns.rules, path)?,
            None => path.to_path_buf(),
        }
    };
    // A relative path is canonicalized against the cwd before reaching the
    // mutating VFS ops, which then re-check with an absolute path; allow here.
    if !translated.is_absolute() {
        return Ok(());
    }
    // Step 2: longest-prefix volume match on the `..`-clamped guest path,
    // identical to `longest_volume_match` used during resolution.
    let normalized = normalize_jailed(&translated);
    let mut best: Option<&VolumeMount> = None;
    for v in &volumes {
        if normalized.starts_with(&v.guest_prefix)
            && best.is_none_or(|b| v.guest_prefix.len() > b.guest_prefix.len())
        {
            best = Some(v);
        }
    }
    match best {
        // A read-only (`:ro`) volume always denies writes.
        Some(v) if v.read_only => Err(KernelError::ReadOnlyFilesystem),
        // A writable (`:rw`) volume always permits writes, even when the
        // container rootfs is read-only — this is the Docker behaviour where
        // `-v ...:rw` punches a writable hole through a `--read-only` root.
        Some(_) => Ok(()),
        // No volume matched: the path lives in the container rootfs.  Deny if
        // the rootfs was mounted read-only (`--read-only`), allow otherwise.
        None if root_ro => Err(KernelError::ReadOnlyFilesystem),
        None => Ok(()),
    }
}

/// Reverse of [`apply_root`]: recover the guest path from a resolved host
/// path for a specific process.
///
/// Given a host path that was produced by jailing a guest path under the
/// process's filesystem root, strip the root prefix to return the guest
/// view.  Used by `fchdir`, which derives the new cwd from an open dirfd's
/// resolved *host* path but must store cwd as a *guest* path — otherwise
/// `getcwd` would leak the jail's host location and a subsequent relative
/// path would be canonicalized against the host cwd and then jailed a
/// second time (double-jail).  See TD32 part (b) / design-decisions §45.
///
/// Behaviour:
/// - **Unjailed process** (no root): the host path *is* the guest path —
///   returned unchanged.
/// - `host == root`: maps back to the guest root `/`.
/// - `host` within `root` (at a path boundary): the suffix (which begins
///   with `/`) is the guest path.
/// - `host` not within `root` (unexpected — a dirfd the process opened is
///   always within its jail): returned unchanged as a best-effort fallback.
///
/// This also reverses **volume (bind) mounts**: a host path inside a
/// volume's `host_target` maps back to the volume's `guest_prefix` (longest
/// match wins), mirroring `apply_root_with_volumes`.
///
/// **Limitation:** this reverses the chroot (`apply_root`) and volume layers,
/// but not any namespace Bind/Hide remapping from step 1 of
/// `resolve_path_for`.  The container runtime isolates with the chroot jail
/// plus volumes (no step-1 Bind rules on a jailed process), so the reversal
/// is exact for that use case.  A process that combined step-1 Bind rules
/// *and* a chroot jail and then called `fchdir` would get the post-Bind guest
/// path; recovering the pre-Bind path would require storing the original
/// guest path on the open handle.
pub fn unjail_path_for(process_id: u64, host: impl AsRef<Path>) -> PathBuf {
    let host = host.as_ref();
    let root = match PROCESS_ROOT.lock().get(&process_id).cloned() {
        Some(r) => r,
        None => return host.to_path_buf(),
    };
    // A host path inside a volume target maps back to that volume's guest
    // prefix (longest host_target match wins, mirroring forward resolution
    // in `apply_root_with_volumes`).  Checked before the rootfs strip
    // because a volume's contents live *outside* the rootfs subtree.
    {
        let mounts = PROCESS_MOUNTS.lock();
        if let Some(list) = mounts.get(&process_id) {
            let mut best: Option<&VolumeMount> = None;
            for v in list {
                if host.starts_with(&v.host_target)
                    && best.is_none_or(|b| {
                        v.host_target.len() > b.host_target.len()
                    })
                {
                    best = Some(v);
                }
            }
            if let Some(v) = best {
                // `starts_with` above guarantees the strip succeeds; the
                // fallback keeps this panic-free.
                let suffix =
                    host.strip_prefix(&v.host_target).unwrap_or_default();
                let mut result = v.guest_prefix.clone();
                result.push(&suffix);
                return result;
            }
        }
    }
    // Covers `host == root` too: the suffix is then empty and the guest path
    // is the jail root `/`.
    if let Some(suffix) = host.strip_prefix(&root) {
        let mut result = PathBuf::from("/");
        result.push(&suffix);
        return result;
    }
    // Host path is not within this process's root or any volume — unexpected;
    // return unchanged rather than fabricate a guest path.
    host.to_path_buf()
}

/// Get the number of active namespaces (for diagnostics).
pub fn active_count() -> usize {
    NS_TABLE.lock().len()
}

/// Get the namespace's rule count (for diagnostics/testing).
pub fn rule_count(ns_id: NamespaceId) -> KernelResult<usize> {
    if ns_id == ROOT_NAMESPACE {
        return Ok(0);
    }
    let table = NS_TABLE.lock();
    let ns = table.get(&ns_id)
        .ok_or(KernelError::NotFound)?;
    Ok(ns.rules.len())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Validate a path prefix.
fn validate_prefix(prefix: &Path) -> KernelResult<()> {
    if prefix.is_empty() || !prefix.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    if prefix.len() > MAX_PREFIX_LEN {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// Apply namespace rules to a path.
///
/// Rules are evaluated in order — first match wins.
/// Prefix matching here is **component-aligned** ([`Path::starts_with`]): a
/// Bind or Hide rule on `/home` covers `/home` and `/home/user` but never
/// `/homework`.  A byte-prefix test would let a Hide rule on `/secret` also
/// block the unrelated `/secretariat`, and — worse — would let a Bind rule
/// splice a target prefix into the middle of a filename.
fn apply_rules(rules: &[NsRule], path: &Path) -> KernelResult<PathBuf> {
    for rule in rules {
        match rule {
            NsRule::Bind { source_prefix, target_prefix } => {
                if let Some(suffix) = path.strip_prefix(source_prefix) {
                    // Construct the remapped path.  `suffix` is relative, so
                    // `push` re-inserts the separator (and is a no-op when the
                    // path *is* the mount point).
                    let mut result = target_prefix.clone();
                    result.push(&suffix);
                    return Ok(result);
                }
            }
            NsRule::Hide { prefix } => {
                if path.starts_with(prefix) {
                    return Err(KernelError::PermissionDenied);
                }
            }
        }
    }

    // No rule matched — pass through unchanged.
    Ok(path.to_path_buf())
}

/// Re-anchor a guest path under a filesystem root (chroot).
///
/// `root` is an already-normalized absolute host prefix without a trailing
/// slash (e.g. `/containers/c1/rootfs`).  `path` is the guest path to jail.
///
/// Only absolute guest paths are jailed.  Relative paths are returned
/// unchanged: they are resolved against the process's current working
/// directory by a higher layer, and that cwd is itself within the jail, so
/// the eventual absolute path stays contained.  (Until per-process cwd is
/// jailed end-to-end, relative-path containment depends on that layer —
/// see known-issues.)
///
/// Absolute paths are first normalized *within the jail*: `.` components
/// are dropped and `..` is clamped at the jail root (a `..` at the top is a
/// no-op, exactly like Linux chroot), so the result can never reference a
/// host path above `root`.  The normalized guest path is then prefixed with
/// `root`.
fn apply_root(root: &Path, path: &Path) -> PathBuf {
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    let normalized = normalize_jailed(path);
    if normalized.as_path() == Path::new("/") {
        // Guest root maps to the jail root itself.
        return root.to_path_buf();
    }
    let mut result = PathBuf::with_capacity(
        root.len().saturating_add(normalized.len()),
    );
    result.extend_bytes(root.as_bytes());
    // `normalized` starts with `/`, so plain concatenation is already
    // correctly separated (and `root` never carries a trailing slash — it is
    // stored through `normalize_jailed`).
    result.extend_bytes(normalized.as_bytes());
    result
}

/// Like [`apply_root`], but consults the process's volume (bind) mounts
/// first.
///
/// Resolution order, after `..`-clamping the guest path within the jail:
/// 1. If the normalized guest path falls under a volume's `guest_prefix`
///    (longest match wins), re-anchor it under that volume's `host_target`.
/// 2. Otherwise prefix it with the container `root` exactly like
///    [`apply_root`].
///
/// Because the path is normalized (`..` clamped against the guest root `/`)
/// *before* any volume is considered, a guest cannot use `..` to escape a
/// volume's subtree or to climb out of one volume and into another — the
/// security property of the bare chroot is preserved.
fn apply_root_with_volumes(
    root: &Path,
    volumes: &[VolumeMount],
    path: &Path,
) -> PathBuf {
    if !path.is_absolute() {
        // Relative — handled by the cwd layer, same as `apply_root`.
        return path.to_path_buf();
    }
    let normalized = normalize_jailed(path);
    if let Some((host_target, suffix)) =
        longest_volume_match(volumes, &normalized)
    {
        let mut result = host_target;
        // `suffix` is relative; an empty one leaves the mount point itself.
        result.push(&suffix);
        return result;
    }
    if normalized.as_path() == Path::new("/") {
        return root.to_path_buf();
    }
    let mut result = PathBuf::with_capacity(
        root.len().saturating_add(normalized.len()),
    );
    result.extend_bytes(root.as_bytes());
    result.extend_bytes(normalized.as_bytes());
    result
}

/// Normalize an absolute path, clamping `..` so it cannot rise above `/`.
///
/// Returns a canonical absolute path with no `.`/`..`/empty components and
/// no trailing slash (except the bare root `/`).  Unlike a generic
/// normalizer, a `..` with nothing left to pop is silently ignored rather
/// than ascending — this is what makes the result safe to use as a jailed
/// path (a guest cannot climb out of its root with `..`).
fn normalize_jailed(path: &Path) -> PathBuf {
    let mut stack: Vec<&Path> = Vec::new();
    // `components()` already drops empty components (leading/duplicate
    // slashes), so only `.`/`..` need handling here.
    for comp in path.components() {
        match comp.as_bytes() {
            b"." => {}
            // Clamp at root: popping an empty stack stays at root.
            b".." => {
                stack.pop();
            }
            _ => stack.push(comp),
        }
    }
    if stack.is_empty() {
        return PathBuf::from("/");
    }
    let mut result = PathBuf::with_capacity(path.len());
    for comp in &stack {
        result.extend_bytes(b"/");
        result.extend_bytes(comp.as_bytes());
    }
    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run namespace self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[namespace] Running self-tests...");

    // First, before anything below creates namespace state: the fast-path
    // flag. Ordering matters -- see the test's own comment.
    test_ns_fast_path_flag()?;

    test_create_destroy()?;
    test_bind_resolution()?;
    test_hide_resolution()?;
    test_path_boundary_matching()?;
    test_clone_namespace()?;
    test_process_attach_detach()?;
    test_process_root()?;
    test_volume_mounts()?;
    test_hostname()?;

    serial_println!("[namespace] All self-tests PASSED");
    Ok(())
}

/// Test that every site which creates namespace state disables the resolve
/// fast path.
///
/// This is a **security** test wearing a performance test's clothes.
/// `NS_FEATURES_ACTIVE` short-circuits `resolve_path`/`check_writable` before
/// any lock; if a site that jails a process, attaches it to a namespace, or
/// gives it a read-only volume fails to set the flag, those calls keep taking
/// the fast path and the sandbox is silently not enforced.  A chroot escape
/// with no error message anywhere.
///
/// Each of the three sites is checked **from a clean state**, using
/// `reset_ns_features_if_trivial()` between them.  That matters: the flag is
/// monotonic, so without the reset only the first site checked would prove
/// anything and the other two assertions would pass no matter what the code
/// did.  Each check is therefore: reset, confirm the fast path is genuinely
/// available, perform the operation, and assert that resolution now *observes*
/// it — asserting the behaviour rather than the flag, since the behaviour is
/// what the flag exists to protect.
fn test_ns_fast_path_flag() -> KernelResult<()> {
    // Synthetic PIDs with no live process behind them, as in test_process_root.
    const PID_ROOT: u64 = 78_001;
    const PID_NS: u64 = 78_002;
    const PID_VOL: u64 = 78_003;

    // Runs first in `self_test()`, so nothing should have created namespace
    // state yet. If this fails, an earlier boot step created some and the rest
    // of this test would be checking nothing.
    assert!(
        reset_ns_features_if_trivial(),
        "namespace state already exists at self-test entry",
    );
    assert!(!ns_features_active());
    // Fast path really is being taken: the identity result below is what it
    // returns, and the slow path would return the same, so this alone is not
    // evidence -- it is the precondition for the three checks that follow.
    assert_eq!(resolve_path_for(PID_ROOT, "/bin/sh")?.as_path(), Path::new("/bin/sh"));

    // --- Site 1: set_root (worst failure mode: chroot escape) ---
    set_root(PID_ROOT, "/containers/fp/rootfs")?;
    assert!(ns_features_active(), "set_root did not disable the fast path");
    assert_eq!(
        resolve_path_for(PID_ROOT, "/bin/sh")?.as_path(),
        Path::new("/containers/fp/rootfs/bin/sh"),
        "jailed path resolved outside the jail",
    );
    clear_root(PID_ROOT);

    // --- Site 2: attach (failure mode: Bind/Hide rules bypassed) ---
    assert!(reset_ns_features_if_trivial(), "site 1 leaked state");
    let ns = create(ROOT_NAMESPACE)?;
    hide(ns, "/secret")?;
    // Creating and populating a namespace nobody is attached to must not
    // itself disable the fast path -- it changes no process's view.
    assert!(!ns_features_active(), "creating an unattached namespace armed the flag");
    attach(PID_NS, ns)?;
    assert!(ns_features_active(), "attach did not disable the fast path");
    assert!(
        resolve_path_for(PID_NS, "/secret/key").is_err(),
        "hidden path resolved for a process in the hiding namespace",
    );
    detach(PID_NS);
    destroy(ns)?;

    // --- Site 3: add_volume (failure mode: :ro volume writable) ---
    //
    // `add_volume` deliberately goes FIRST here, before `set_root`. A volume
    // only takes effect inside a jail, so the natural order is set_root then
    // add_volume -- but set_root arms the flag, and then "assert the flag is
    // armed after add_volume" is true no matter what add_volume does. Arming
    // it from the volume alone is the only way this assertion can fail.
    assert!(reset_ns_features_if_trivial(), "site 2 leaked state");
    add_volume(PID_VOL, "/data", "/host/data", true)?;
    assert!(ns_features_active(), "add_volume did not disable the fast path");
    set_root(PID_VOL, "/containers/fp2/rootfs")?;
    assert!(
        check_writable_for(PID_VOL, "/data/file").is_err(),
        "read-only volume accepted a write",
    );
    clear_mounts(PID_VOL);
    clear_root(PID_VOL);

    // Leave the flag matching reality so the boot that follows gets the fast
    // path it would have had. Not required for correctness -- a stale-set flag
    // is only slow -- but leaving it armed would make every later benchmark
    // measure the slow path and quietly misattribute the result, which is the
    // exact failure this whole investigation has been about.
    assert!(reset_ns_features_if_trivial(), "site 3 leaked state");

    serial_println!("[namespace]   fast-path flag: OK (3 sites, each from clean state)");
    Ok(())
}

/// Test the per-process UTS hostname override (Docker `--hostname`).
fn test_hostname() -> KernelResult<()> {
    // A synthetic, never-scheduled PID so there is no live process to clear
    // the override mid-test (mirrors test_process_root / test_volume_mounts).
    const PID: u64 = 77_777;

    // No override by default.
    assert!(hostname_for(PID).is_none());

    // Set and read back.
    set_hostname(PID, "web-01")?;
    assert_eq!(hostname_for(PID).as_deref(), Some("web-01"));

    // Replace.
    set_hostname(PID, "db-02")?;
    assert_eq!(hostname_for(PID).as_deref(), Some("db-02"));

    // Invalid names are rejected and leave the prior value intact.
    assert!(set_hostname(PID, "").is_err());
    let too_long = "x".repeat(65);
    assert!(set_hostname(PID, &too_long).is_err());
    assert!(set_hostname(PID, "a\0b").is_err());
    assert_eq!(hostname_for(PID).as_deref(), Some("db-02"));

    // Clear returns to the global hostname.
    clear_hostname(PID);
    assert!(hostname_for(PID).is_none());

    // detach() must also drop the override (PID-reuse safety).
    set_hostname(PID, "ephemeral")?;
    detach(PID);
    assert!(hostname_for(PID).is_none());

    serial_println!("[namespace]   Per-process hostname (--hostname): OK");
    Ok(())
}

fn test_create_destroy() -> KernelResult<()> {
    let ns = create(ROOT_NAMESPACE)?;
    assert_ne!(ns, ROOT_NAMESPACE);

    let count = rule_count(ns)?;
    assert_eq!(count, 0);

    destroy(ns)?;

    // Double-destroy should fail.
    assert!(destroy(ns).is_err());

    serial_println!("[namespace]   Create/destroy: OK");
    Ok(())
}

fn test_bind_resolution() -> KernelResult<()> {
    let ns = create(ROOT_NAMESPACE)?;

    // Add a bind rule: /data → /sandbox/data
    bind(ns, "/data", "/sandbox/data")?;

    // Simulate resolution with a known namespace.
    let table = NS_TABLE.lock();
    let ns_obj = table.get(&ns).unwrap();

    // Path matching the bind prefix.
    let result = apply_rules(&ns_obj.rules, Path::new("/data/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/sandbox/data/file.txt"));

    // Exact prefix match.
    let result = apply_rules(&ns_obj.rules, Path::new("/data"))?;
    assert_eq!(result.as_path(), Path::new("/sandbox/data"));

    // Path NOT matching (different prefix).
    let result = apply_rules(&ns_obj.rules, Path::new("/home/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/home/file.txt"));

    // Path with similar prefix that shouldn't match.
    let result = apply_rules(&ns_obj.rules, Path::new("/database/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/database/file.txt"));

    drop(table);
    destroy(ns)?;

    serial_println!("[namespace]   Bind resolution: OK");
    Ok(())
}

fn test_hide_resolution() -> KernelResult<()> {
    let ns = create(ROOT_NAMESPACE)?;

    // Hide /secret
    hide(ns, "/secret")?;

    let table = NS_TABLE.lock();
    let ns_obj = table.get(&ns).unwrap();

    // Path matching the hide prefix should return PermissionDenied.
    let result = apply_rules(&ns_obj.rules, Path::new("/secret/keys.txt"));
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        KernelError::PermissionDenied
    ));

    // Exact match.
    let result = apply_rules(&ns_obj.rules, Path::new("/secret"));
    assert!(result.is_err());

    // Non-matching path passes through.
    let result = apply_rules(&ns_obj.rules, Path::new("/public/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/public/file.txt"));

    // Similar prefix that shouldn't match.
    let result = apply_rules(&ns_obj.rules, Path::new("/secretive/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/secretive/file.txt"));

    drop(table);
    destroy(ns)?;

    serial_println!("[namespace]   Hide resolution: OK");
    Ok(())
}

fn test_path_boundary_matching() -> KernelResult<()> {
    // Prefix matching is component-aligned, so a rule on `/home` can never
    // reach `/homework`.  This used to be hand-rolled here (`starts_with`
    // plus a boundary-byte check); it now delegates to `Path`, which cannot
    // be fooled by a shared byte prefix or by a trailing slash on the rule.
    for (path, prefix, want) in [
        ("/home/user", "/home", true),
        ("/home", "/home", true),
        ("/homework", "/home", false),
        ("/hom", "/home", false),
        ("/a/b/c", "/a", true),
        ("/a/b/c", "/a/b", true),
        ("/a/b/c", "/a/b/c", true),
        ("/a/b/c", "/a/b/cd", false),
        // A rule prefix that carries a trailing slash must behave exactly
        // like one without: the old byte-prefix idiom got this backwards
        // and matched nothing at all.
        ("/home/user", "/home/", true),
        ("/homework", "/home/", false),
    ] {
        assert_eq!(
            Path::new(path).starts_with(Path::new(prefix)),
            want,
            "boundary match"
        );
    }

    // Stripping yields the *relative* remainder, which `PathBuf::push`
    // re-joins with the correct separator (and an empty remainder means the
    // path is the prefix itself).
    for (path, prefix, want) in [
        ("/home/user/file", "/home", Some("user/file")),
        ("/home", "/home", Some("")),
        ("/homework", "/home", None),
        ("/a/b", "/a", Some("b")),
    ] {
        assert_eq!(
            Path::new(path).strip_prefix(Path::new(prefix)),
            want.map(PathBuf::from),
            "strip_prefix"
        );
    }

    // Non-UTF-8 components match by bytes: a jail or Hide rule on a
    // directory whose name is not valid UTF-8 is still enforceable.
    assert!(Path::new(b"/data/\xff/x").starts_with(Path::new(b"/data/\xff")));
    assert!(!Path::new(b"/data/\xfe/x").starts_with(Path::new(b"/data/\xff")));

    serial_println!("[namespace]   Path boundary matching: OK");
    Ok(())
}

fn test_clone_namespace() -> KernelResult<()> {
    let ns1 = create(ROOT_NAMESPACE)?;
    bind(ns1, "/data", "/sandbox/data")?;
    hide(ns1, "/secret")?;

    // Clone ns1.
    let ns2 = create(ns1)?;

    // ns2 should have the same rules.
    let count = rule_count(ns2)?;
    assert_eq!(count, 2);

    // Verify the cloned rules work.
    let table = NS_TABLE.lock();
    let ns2_obj = table.get(&ns2).unwrap();

    let result = apply_rules(&ns2_obj.rules, Path::new("/data/file.txt"))?;
    assert_eq!(result.as_path(), Path::new("/sandbox/data/file.txt"));

    let result = apply_rules(&ns2_obj.rules, Path::new("/secret"));
    assert!(result.is_err());

    drop(table);
    destroy(ns2)?;
    destroy(ns1)?;

    serial_println!("[namespace]   Clone namespace: OK");
    Ok(())
}

fn test_process_attach_detach() -> KernelResult<()> {
    let ns = create(ROOT_NAMESPACE)?;

    // Attach a fake process ID to the namespace.
    let fake_pid: u64 = 99999;
    attach(fake_pid, ns)?;

    // Query should return our namespace.
    let queried = query(fake_pid);
    assert_eq!(queried, ns);

    // Refcount should be 1.
    {
        let table = NS_TABLE.lock();
        let ns_obj = table.get(&ns).unwrap();
        assert_eq!(ns_obj.refcount, 1);
    }

    // Detach.
    detach(fake_pid);

    // Query should return root.
    let queried = query(fake_pid);
    assert_eq!(queried, ROOT_NAMESPACE);

    // Refcount should be 0.
    {
        let table = NS_TABLE.lock();
        let ns_obj = table.get(&ns).unwrap();
        assert_eq!(ns_obj.refcount, 0);
    }

    destroy(ns)?;

    serial_println!("[namespace]   Process attach/detach: OK");
    Ok(())
}

fn test_process_root() -> KernelResult<()> {
    // Use a PID unlikely to collide with a live process.
    let pid: u64 = 88888;

    // Helpers below assume this PID starts un-jailed.
    clear_root(pid);
    assert!(get_root(pid).is_none());

    // No root: paths pass through unchanged.
    assert_eq!(resolve_path_for(pid, "/bin/sh")?.as_path(), Path::new("/bin/sh"));
    // unjail is a no-op for an unjailed process (host path == guest path).
    assert_eq!(unjail_path_for(pid, "/bin/sh").as_path(), Path::new("/bin/sh"));

    // Jail the process to a container rootfs.
    set_root(pid, "/containers/c1/rootfs")?;
    assert_eq!(
        get_root(pid).as_deref(),
        Some(Path::new("/containers/c1/rootfs")),
    );

    // Absolute paths are re-anchored under the root.
    assert_eq!(
        resolve_path_for(pid, "/bin/sh")?.as_path(),
        Path::new("/containers/c1/rootfs/bin/sh"),
    );
    // Guest root maps to the jail root itself.
    assert_eq!(resolve_path_for(pid, "/")?.as_path(), Path::new("/containers/c1/rootfs"));
    // `.` and duplicate slashes collapse.
    assert_eq!(
        resolve_path_for(pid, "/./bin//sh")?.as_path(),
        Path::new("/containers/c1/rootfs/bin/sh"),
    );

    // `..` is clamped at the jail root — no escape.
    assert_eq!(
        resolve_path_for(pid, "/../etc/passwd")?.as_path(),
        Path::new("/containers/c1/rootfs/etc/passwd"),
    );
    assert_eq!(
        resolve_path_for(pid, "/a/../../b")?.as_path(),
        Path::new("/containers/c1/rootfs/b"),
    );
    assert_eq!(
        resolve_path_for(pid, "/../../../..")?.as_path(),
        Path::new("/containers/c1/rootfs"),
    );

    // Relative paths are left for the cwd layer (not jailed here).
    assert_eq!(resolve_path_for(pid, "rel/path")?.as_path(), Path::new("rel/path"));

    // --- Non-idempotency guard (double-jail regression) ---
    //
    // `apply_root` blindly prefixes the jail root, so resolving a path that is
    // ALREADY anchored under the jail root re-prefixes it (double-jail).  This
    // is by design — the namespace layer assumes its input is a *guest* path.
    // It is precisely why every handle-backed VFS op (read_at, write_at, …)
    // must call the `_resolved` worker on the host path captured at open()
    // rather than re-running `resolve_follow`.  This assertion pins the
    // behaviour so a future refactor that accidentally makes handle ops
    // re-resolve will be caught here.
    let once = resolve_path_for(pid, "/bin/sh")?;
    assert_eq!(once.as_path(), Path::new("/containers/c1/rootfs/bin/sh"));
    let twice = resolve_path_for(pid, &once)?;
    assert_eq!(
        twice.as_path(),
        Path::new("/containers/c1/rootfs/containers/c1/rootfs/bin/sh"),
        "re-resolving an already-jailed path must double-jail (handle ops \
         must therefore use the _resolved workers, not re-resolve)",
    );

    // --- unjail_path_for round-trip (fchdir / *at dirfd guest-cwd) ---
    //
    // fchdir and the *at-with-dirfd syscalls derive a path from an open
    // dirfd's *resolved host* path but must store/feed a *guest* path, so
    // they call `unjail_path_for` to strip the jail root.  Verify it is the
    // exact inverse of `apply_root` for the jailed pid (root is
    // "/containers/c1/rootfs" here).
    assert_eq!(unjail_path_for(pid, &once).as_path(), Path::new("/bin/sh"));
    assert_eq!(unjail_path_for(pid, "/containers/c1/rootfs").as_path(), Path::new("/"));
    // Round-trip: unjail(resolve(guest)) recovers the normalized guest path.
    let g = resolve_path_for(pid, "/a/b/../c")?;
    assert_eq!(g.as_path(), Path::new("/containers/c1/rootfs/a/c"));
    assert_eq!(unjail_path_for(pid, &g).as_path(), Path::new("/a/c"));
    // A host path not within the jail is returned unchanged (defensive).
    assert_eq!(unjail_path_for(pid, "/elsewhere/x").as_path(), Path::new("/elsewhere/x"));

    // A root that normalizes to "/" clears the jail.
    set_root(pid, "/")?;
    assert!(get_root(pid).is_none());
    assert_eq!(resolve_path_for(pid, "/bin/sh")?.as_path(), Path::new("/bin/sh"));

    // Trailing slashes and `.`/`..` in the root are normalized away.
    set_root(pid, "/containers/c2/./rootfs/")?;
    assert_eq!(
        get_root(pid).as_deref(),
        Some(Path::new("/containers/c2/rootfs")),
    );
    assert_eq!(
        resolve_path_for(pid, "/bin/sh")?.as_path(),
        Path::new("/containers/c2/rootfs/bin/sh"),
    );

    // detach() must drop the jail (PID reuse safety).
    detach(pid);
    assert!(get_root(pid).is_none());
    assert_eq!(resolve_path_for(pid, "/bin/sh")?.as_path(), Path::new("/bin/sh"));

    // A non-absolute root is rejected.
    assert!(set_root(pid, "relative/root").is_err());
    assert!(set_root(pid, "").is_err());

    serial_println!("[namespace]   Process filesystem root (chroot): OK");
    Ok(())
}

/// Exercise volume (bind) mounts layered on top of a chroot jail.
fn test_volume_mounts() -> KernelResult<()> {
    let pid: u64 = 88889;
    clear_root(pid);
    clear_mounts(pid);
    assert_eq!(volume_count(pid), 0);

    // Jail to a container rootfs, then mount a host directory as a volume.
    set_root(pid, "/containers/v1/rootfs")?;
    add_volume(pid, "/data", "/host/shared", false)?;
    assert_eq!(volume_count(pid), 1);

    // The mount point itself resolves to the volume target.
    assert_eq!(resolve_path_for(pid, "/data")?.as_path(), Path::new("/host/shared"));
    // Paths under the volume resolve under the target (escaping the rootfs).
    assert_eq!(
        resolve_path_for(pid, "/data/file.txt")?.as_path(),
        Path::new("/host/shared/file.txt"),
    );
    assert_eq!(
        resolve_path_for(pid, "/data/sub/x")?.as_path(),
        Path::new("/host/shared/sub/x"),
    );
    // Non-volume paths still resolve under the rootfs.
    assert_eq!(
        resolve_path_for(pid, "/bin/sh")?.as_path(),
        Path::new("/containers/v1/rootfs/bin/sh"),
    );

    // SECURITY: `..` is clamped against the guest root *before* volume
    // matching, so a guest cannot climb out of the volume into the host.
    // `/data/../secret` normalizes to `/secret`, which no longer matches the
    // `/data` volume and resolves under the rootfs — not the host target.
    assert_eq!(
        resolve_path_for(pid, "/data/../secret")?.as_path(),
        Path::new("/containers/v1/rootfs/secret"),
    );
    // Repeated `..` cannot escape the volume's host subtree upward either.
    assert_eq!(
        resolve_path_for(pid, "/data/../../etc")?.as_path(),
        Path::new("/containers/v1/rootfs/etc"),
    );
    // `..` *within* the volume stays in the volume.
    assert_eq!(
        resolve_path_for(pid, "/data/sub/../x")?.as_path(),
        Path::new("/host/shared/x"),
    );

    // Longest-prefix wins: a nested volume shadows the parent.
    add_volume(pid, "/data/cache", "/fastcache", false)?;
    assert_eq!(volume_count(pid), 2);
    assert_eq!(resolve_path_for(pid, "/data/cache/a")?.as_path(), Path::new("/fastcache/a"));
    assert_eq!(resolve_path_for(pid, "/data/cache")?.as_path(), Path::new("/fastcache"));
    // A sibling under the parent volume is unaffected.
    assert_eq!(resolve_path_for(pid, "/data/other")?.as_path(), Path::new("/host/shared/other"));

    // Re-adding at an existing guest prefix replaces (does not stack).
    add_volume(pid, "/data", "/host/shared2", false)?;
    assert_eq!(volume_count(pid), 2);
    assert_eq!(resolve_path_for(pid, "/data/file")?.as_path(), Path::new("/host/shared2/file"));

    // Reverse mapping (fchdir into a volume): host path → guest path.
    assert_eq!(unjail_path_for(pid, "/host/shared2/file").as_path(), Path::new("/data/file"));
    assert_eq!(unjail_path_for(pid, "/host/shared2").as_path(), Path::new("/data"));
    assert_eq!(unjail_path_for(pid, "/fastcache/a").as_path(), Path::new("/data/cache/a"));
    // Rootfs paths still reverse to their guest path.
    assert_eq!(
        unjail_path_for(pid, "/containers/v1/rootfs/bin/sh").as_path(),
        Path::new("/bin/sh"),
    );

    // Trailing slashes / `.` in volume args are normalized away.
    add_volume(pid, "/logs/", "/var/log/./c1/", false)?;
    assert_eq!(resolve_path_for(pid, "/logs/app.log")?.as_path(), Path::new("/var/log/c1/app.log"));

    // Read-only volumes: writes under a read-only volume are rejected with
    // EROFS, while reads/path resolution still work.  Add a read-only volume
    // and verify check_writable_for enforces it.
    add_volume(pid, "/ro", "/host/ro-target", true)?;
    assert_eq!(resolve_path_for(pid, "/ro/file")?.as_path(), Path::new("/host/ro-target/file"));
    assert!(check_writable_for(pid, "/ro/file").is_err());
    assert!(check_writable_for(pid, "/ro").is_err());
    // A read-write volume permits writes.
    assert!(check_writable_for(pid, "/data/file").is_ok());
    // Non-volume (rootfs) paths are writable (rootfs is not read-only here).
    assert!(check_writable_for(pid, "/bin/sh").is_ok());
    // Re-adding the same guest prefix as read-write clears the read-only flag.
    add_volume(pid, "/ro", "/host/ro-target", false)?;
    assert!(check_writable_for(pid, "/ro/file").is_ok());

    // Read-only ROOT (Docker `--read-only`): mark the rootfs read-only and
    // verify writes into the rootfs are denied while writable volumes still
    // permit writes (a `:rw` volume punches a hole through the RO root).
    assert!(!is_root_read_only(pid));
    set_root_read_only(pid, true);
    assert!(is_root_read_only(pid));
    // Rootfs path now denied.
    assert!(check_writable_for(pid, "/bin/sh").is_err());
    assert!(check_writable_for(pid, "/etc/hosts").is_err());
    // Writable volume still permits writes through the read-only root.
    assert!(check_writable_for(pid, "/data/file").is_ok());
    assert!(check_writable_for(pid, "/ro/file").is_ok());
    // A read-only volume under a read-only root is of course still denied.
    add_volume(pid, "/ro", "/host/ro-target", true)?;
    assert!(check_writable_for(pid, "/ro/file").is_err());
    // Clearing the read-only-root flag restores rootfs writability.
    set_root_read_only(pid, false);
    assert!(!is_root_read_only(pid));
    assert!(check_writable_for(pid, "/bin/sh").is_ok());
    // Reset the /ro volume back to writable for the no-op-after-detach check.
    add_volume(pid, "/ro", "/host/ro-target", false)?;

    // A volume at the guest root is rejected (that is `set_root`'s job).
    assert!(add_volume(pid, "/", "/whatever", false).is_err());
    // Non-absolute args are rejected.
    assert!(add_volume(pid, "data", "/host", false).is_err());
    assert!(add_volume(pid, "/data", "host", false).is_err());
    assert!(add_volume(pid, "", "/host", false).is_err());

    // detach() drops volumes (PID-reuse safety).
    detach(pid);
    assert_eq!(volume_count(pid), 0);
    assert!(get_root(pid).is_none());
    // After detach, the former volume path is a plain host path again.
    assert_eq!(resolve_path_for(pid, "/data/file")?.as_path(), Path::new("/data/file"));

    serial_println!("[namespace]   Volume (bind) mounts: OK");
    Ok(())
}
