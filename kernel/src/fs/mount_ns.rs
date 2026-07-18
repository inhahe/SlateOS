//! Mount namespaces — per-process filesystem view isolation.
//!
//! Mount namespaces provide each process (or group of processes) with
//! its own view of the filesystem mount table.  Changes to mounts in
//! one namespace are invisible to processes in other namespaces.
//!
//! ## Architecture
//!
//! Each namespace starts as a copy of its parent namespace's mount
//! table.  After creation, mount/unmount operations in the namespace
//! affect only that namespace.  The initial (root) namespace is
//! namespace 0 and is the default for all processes.
//!
//! ```text
//! Process A ────► Namespace 0 (root) ──── /, /tmp, /proc, /dev
//!                                     │
//! Process B ────► Namespace 1 ────────── /, /tmp, /proc, /dev, /sandbox
//!                                     │
//! Process C ────► Namespace 2 ────────── /, /tmp (only these visible)
//! ```
//!
//! ## Use cases
//!
//! - Container isolation: each container gets its own mount namespace
//! - Sandbox: restrict which filesystems are visible to a process
//! - chroot alternative: namespace-based root pivot
//! - Build environments: temporary mounts that disappear when done
//!
//! ## Design
//!
//! Namespaces are identified by a `NamespaceId` (u64).  Each namespace
//! stores its own list of mount points.  The VFS can query the active
//! namespace to resolve paths.
//!
//! ## Thread safety
//!
//! The namespace registry is protected by a global Mutex.  Individual
//! namespace operations copy data out to avoid holding the lock across
//! VFS calls.
//!
//! ## Reference
//!
//! Linux: mount_namespaces(7), unshare(2), clone(2) with CLONE_NEWNS

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a mount namespace.
pub type NamespaceId = u64;

/// The root namespace (all processes start here).
pub const ROOT_NAMESPACE: NamespaceId = 0;

/// Maximum number of namespaces.
const MAX_NAMESPACES: usize = 256;

/// A mount entry within a namespace.
#[derive(Debug, Clone)]
pub struct NsMount {
    /// Mount point path (e.g., "/", "/tmp").
    pub mount_path: String,
    /// Filesystem type name (e.g., "ext4", "memfs", "procfs").
    pub fs_type: String,
    /// Whether this mount is read-only in this namespace.
    pub readonly: bool,
}

/// A mount namespace.
#[derive(Debug, Clone)]
struct Namespace {
    /// Namespace ID.
    id: NamespaceId,
    /// Human-readable name (optional).
    name: String,
    /// Parent namespace (None for root).
    parent: Option<NamespaceId>,
    /// Mount table for this namespace.
    mounts: Vec<NsMount>,
    /// Number of processes using this namespace.
    refcount: u32,
    /// Whether this namespace allows creating sub-namespaces.
    allow_nested: bool,
}

/// Summary info about a namespace.
#[derive(Debug, Clone)]
pub struct NamespaceInfo {
    pub id: NamespaceId,
    pub name: String,
    pub parent: Option<NamespaceId>,
    pub mount_count: usize,
    pub refcount: u32,
    pub allow_nested: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct NsInner {
    namespaces: BTreeMap<NamespaceId, Namespace>,
    next_id: NamespaceId,
    /// Which namespace each "process" (pid) is in.
    pid_ns: BTreeMap<u64, NamespaceId>,
}

static NAMESPACES: Mutex<NsInner> = Mutex::new(NsInner {
    namespaces: BTreeMap::new(),
    next_id: 1,
    pid_ns: BTreeMap::new(),
});

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the mount namespace subsystem.
///
/// Creates the root namespace (ID 0) with the current global mount table.
pub fn init() {
    let mut inner = NAMESPACES.lock();

    // Only init once.
    if inner.namespaces.contains_key(&ROOT_NAMESPACE) {
        return;
    }

    // Query the current VFS mounts for the root namespace.
    let mounts: Vec<NsMount> = crate::fs::vfs::Vfs::mounts()
        .into_iter()
        .map(|(path, fs_type)| NsMount {
            mount_path: path,
            fs_type,
            readonly: false,
        })
        .collect();

    inner.namespaces.insert(ROOT_NAMESPACE, Namespace {
        id: ROOT_NAMESPACE,
        name: String::from("root"),
        parent: None,
        mounts,
        refcount: 1, // Always at least one reference.
        allow_nested: true,
    });
}

// ---------------------------------------------------------------------------
// Public API — namespace lifecycle
// ---------------------------------------------------------------------------

/// Create a new namespace as a child of `parent`.
///
/// The new namespace starts with a copy of the parent's mount table.
/// Returns the namespace ID.
pub fn create(parent: NamespaceId, name: &str) -> KernelResult<NamespaceId> {
    let mut inner = NAMESPACES.lock();

    if inner.namespaces.len() >= MAX_NAMESPACES {
        return Err(KernelError::ResourceExhausted);
    }

    let parent_ns = inner.namespaces.get(&parent)
        .ok_or(KernelError::NotFound)?;

    if !parent_ns.allow_nested {
        return Err(KernelError::PermissionDenied);
    }

    // Copy parent's mount table.
    let mounts = parent_ns.mounts.clone();

    let id = inner.next_id;
    inner.next_id = inner.next_id.wrapping_add(1);

    inner.namespaces.insert(id, Namespace {
        id,
        name: name.into(),
        parent: Some(parent),
        mounts,
        refcount: 0,
        allow_nested: true,
    });

    Ok(id)
}

/// Destroy a namespace.
///
/// Fails if any processes are still using it.
pub fn destroy(ns_id: NamespaceId) -> KernelResult<()> {
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::PermissionDenied);
    }

    let mut inner = NAMESPACES.lock();

    let ns = inner.namespaces.get(&ns_id).ok_or(KernelError::NotFound)?;
    if ns.refcount > 0 {
        return Err(KernelError::DeviceBusy);
    }

    // Check no child namespaces exist.
    for other in inner.namespaces.values() {
        if other.parent == Some(ns_id) {
            return Err(KernelError::NotEmpty);
        }
    }

    inner.namespaces.remove(&ns_id);
    Ok(())
}

/// List all namespaces.
pub fn list() -> Vec<NamespaceInfo> {
    let inner = NAMESPACES.lock();
    inner.namespaces.values().map(|ns| NamespaceInfo {
        id: ns.id,
        name: ns.name.clone(),
        parent: ns.parent,
        mount_count: ns.mounts.len(),
        refcount: ns.refcount,
        allow_nested: ns.allow_nested,
    }).collect()
}

/// Get info about a specific namespace.
pub fn info(ns_id: NamespaceId) -> KernelResult<NamespaceInfo> {
    let inner = NAMESPACES.lock();
    let ns = inner.namespaces.get(&ns_id).ok_or(KernelError::NotFound)?;
    Ok(NamespaceInfo {
        id: ns.id,
        name: ns.name.clone(),
        parent: ns.parent,
        mount_count: ns.mounts.len(),
        refcount: ns.refcount,
        allow_nested: ns.allow_nested,
    })
}

// ---------------------------------------------------------------------------
// Public API — process binding
// ---------------------------------------------------------------------------

/// Assign a process to a namespace.
///
/// Increments the namespace's refcount.
pub fn enter(pid: u64, ns_id: NamespaceId) -> KernelResult<()> {
    let mut inner = NAMESPACES.lock();

    // Leave old namespace if in one.
    if let Some(&old_ns) = inner.pid_ns.get(&pid) {
        if let Some(ns) = inner.namespaces.get_mut(&old_ns) {
            ns.refcount = ns.refcount.saturating_sub(1);
        }
    }

    let ns = inner.namespaces.get_mut(&ns_id).ok_or(KernelError::NotFound)?;
    ns.refcount = ns.refcount.saturating_add(1);
    inner.pid_ns.insert(pid, ns_id);

    Ok(())
}

/// Remove a process from its namespace (on exit).
pub fn leave(pid: u64) {
    let mut inner = NAMESPACES.lock();
    if let Some(ns_id) = inner.pid_ns.remove(&pid) {
        if let Some(ns) = inner.namespaces.get_mut(&ns_id) {
            ns.refcount = ns.refcount.saturating_sub(1);
        }
    }
}

/// Get which namespace a process is in.
pub fn get_ns(pid: u64) -> NamespaceId {
    NAMESPACES.lock().pid_ns.get(&pid).copied().unwrap_or(ROOT_NAMESPACE)
}

// ---------------------------------------------------------------------------
// Public API — mount operations within a namespace
// ---------------------------------------------------------------------------

/// Add a mount point to a namespace.
pub fn ns_mount(ns_id: NamespaceId, mount_path: &str, fs_type: &str, readonly: bool) -> KernelResult<()> {
    let mut inner = NAMESPACES.lock();
    let ns = inner.namespaces.get_mut(&ns_id).ok_or(KernelError::NotFound)?;

    // Check for duplicate.
    if ns.mounts.iter().any(|m| m.mount_path == mount_path) {
        return Err(KernelError::AlreadyExists);
    }

    ns.mounts.push(NsMount {
        mount_path: mount_path.into(),
        fs_type: fs_type.into(),
        readonly,
    });

    Ok(())
}

/// Remove a mount point from a namespace.
pub fn ns_unmount(ns_id: NamespaceId, mount_path: &str) -> KernelResult<()> {
    let mut inner = NAMESPACES.lock();
    let ns = inner.namespaces.get_mut(&ns_id).ok_or(KernelError::NotFound)?;

    let before = ns.mounts.len();
    ns.mounts.retain(|m| m.mount_path != mount_path);

    if ns.mounts.len() == before {
        return Err(KernelError::NotFound);
    }

    Ok(())
}

/// List mounts visible in a namespace.
pub fn ns_mounts(ns_id: NamespaceId) -> KernelResult<Vec<NsMount>> {
    let inner = NAMESPACES.lock();
    let ns = inner.namespaces.get(&ns_id).ok_or(KernelError::NotFound)?;
    Ok(ns.mounts.clone())
}

/// Check if a path is visible in a namespace.
///
/// A path is visible if it is under any mount point in the namespace.
pub fn is_visible(ns_id: NamespaceId, path: &str) -> KernelResult<bool> {
    let inner = NAMESPACES.lock();
    let ns = inner.namespaces.get(&ns_id).ok_or(KernelError::NotFound)?;

    for mount in &ns.mounts {
        if path == mount.mount_path || path.starts_with(&alloc::format!("{}/", mount.mount_path.trim_end_matches('/'))) {
            return Ok(true);
        }
        // Root mount covers everything.
        if mount.mount_path == "/" {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a path is writable in a namespace.
///
/// A path is writable if it's under a non-readonly mount.
pub fn is_writable(ns_id: NamespaceId, path: &str) -> KernelResult<bool> {
    let inner = NAMESPACES.lock();
    let ns = inner.namespaces.get(&ns_id).ok_or(KernelError::NotFound)?;

    // Find the longest matching mount point.
    let mut best_match: Option<&NsMount> = None;
    let mut best_len = 0;

    for mount in &ns.mounts {
        let mp = mount.mount_path.trim_end_matches('/');
        if path == mp || path.starts_with(&alloc::format!("{}/", mp)) || mp == "/" {
            let len = if mp == "/" { 0 } else { mp.len() };
            if len >= best_len {
                best_match = Some(mount);
                best_len = len;
            }
        }
    }

    match best_match {
        Some(mount) => Ok(!mount.readonly),
        None => Ok(false),
    }
}

/// Set a mount as read-only or read-write in a namespace.
pub fn set_readonly(ns_id: NamespaceId, mount_path: &str, readonly: bool) -> KernelResult<()> {
    let mut inner = NAMESPACES.lock();
    let ns = inner.namespaces.get_mut(&ns_id).ok_or(KernelError::NotFound)?;

    for mount in &mut ns.mounts {
        if mount.mount_path == mount_path {
            mount.readonly = readonly;
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

/// Set whether a namespace allows nested child namespaces.
pub fn set_allow_nested(ns_id: NamespaceId, allow: bool) -> KernelResult<()> {
    let mut inner = NAMESPACES.lock();
    let ns = inner.namespaces.get_mut(&ns_id).ok_or(KernelError::NotFound)?;
    ns.allow_nested = allow;
    Ok(())
}

/// Get the number of active namespaces.
pub fn count() -> usize {
    NAMESPACES.lock().namespaces.len()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the mount namespace module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[mount_ns] Running self-test...");

    // Initialize.
    init();

    // --- Test 1: Root namespace exists ---
    {
        let i = info(ROOT_NAMESPACE)?;
        if i.name != "root" {
            serial_println!("[mount_ns]   ERROR: root name is '{}'", i.name);
            return Err(KernelError::InternalError);
        }
        if i.mount_count == 0 {
            serial_println!("[mount_ns]   ERROR: root has no mounts");
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   root namespace: OK ({} mounts)", i.mount_count);
    }

    // --- Test 2: Create child namespace ---
    let child_id;
    {
        child_id = create(ROOT_NAMESPACE, "test_child")?;
        let ci = info(child_id)?;
        if ci.parent != Some(ROOT_NAMESPACE) {
            serial_println!("[mount_ns]   ERROR: wrong parent");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   create child: OK (id={})", child_id);
    }

    // --- Test 3: Child inherits parent mounts ---
    {
        let root_mounts = ns_mounts(ROOT_NAMESPACE)?;
        let child_mounts = ns_mounts(child_id)?;
        if child_mounts.len() != root_mounts.len() {
            serial_println!("[mount_ns]   ERROR: mount count mismatch ({} vs {})",
                child_mounts.len(), root_mounts.len());
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   mount inheritance: OK");
    }

    // --- Test 4: Mount in child is invisible to parent ---
    {
        ns_mount(child_id, "/sandbox", "memfs", false)?;
        let child_mounts = ns_mounts(child_id)?;
        let root_mounts = ns_mounts(ROOT_NAMESPACE)?;

        let child_has_sandbox = child_mounts.iter().any(|m| m.mount_path == "/sandbox");
        let root_has_sandbox = root_mounts.iter().any(|m| m.mount_path == "/sandbox");

        if !child_has_sandbox {
            serial_println!("[mount_ns]   ERROR: /sandbox not in child");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        if root_has_sandbox {
            serial_println!("[mount_ns]   ERROR: /sandbox leaked to root");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   mount isolation: OK");
    }

    // --- Test 5: Path visibility ---
    {
        let visible = is_visible(child_id, "/sandbox/file.txt")?;
        if !visible {
            serial_println!("[mount_ns]   ERROR: /sandbox/file.txt not visible in child");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   path visibility: OK");
    }

    // --- Test 6: Read-only mount ---
    {
        set_readonly(child_id, "/sandbox", true)?;
        let writable = is_writable(child_id, "/sandbox/file.txt")?;
        if writable {
            serial_println!("[mount_ns]   ERROR: read-only mount is writable");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        set_readonly(child_id, "/sandbox", false)?;
        serial_println!("[mount_ns]   read-only mount: OK");
    }

    // --- Test 7: Process binding ---
    {
        let test_pid = 99999u64;
        enter(test_pid, child_id)?;
        let ns = get_ns(test_pid);
        if ns != child_id {
            serial_println!("[mount_ns]   ERROR: process not in child ns");
            leave(test_pid);
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }

        let ci = info(child_id)?;
        if ci.refcount == 0 {
            serial_println!("[mount_ns]   ERROR: refcount not incremented");
            leave(test_pid);
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }

        leave(test_pid);
        let ns_after = get_ns(test_pid);
        if ns_after != ROOT_NAMESPACE {
            serial_println!("[mount_ns]   ERROR: process still in child after leave");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   process binding: OK");
    }

    // --- Test 8: Cannot destroy root namespace ---
    {
        let result = destroy(ROOT_NAMESPACE);
        if result.is_ok() {
            serial_println!("[mount_ns]   ERROR: root destroy allowed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   root protection: OK");
    }

    // --- Test 9: Cannot destroy busy namespace ---
    {
        let test_pid = 99998u64;
        enter(test_pid, child_id)?;
        let result = destroy(child_id);
        if result.is_ok() {
            serial_println!("[mount_ns]   ERROR: busy destroy allowed");
            leave(test_pid);
            return Err(KernelError::InternalError);
        }
        leave(test_pid);
        serial_println!("[mount_ns]   busy protection: OK");
    }

    // --- Test 10: Unmount from namespace ---
    {
        ns_unmount(child_id, "/sandbox")?;
        let child_mounts = ns_mounts(child_id)?;
        let has_sandbox = child_mounts.iter().any(|m| m.mount_path == "/sandbox");
        if has_sandbox {
            serial_println!("[mount_ns]   ERROR: /sandbox still mounted after unmount");
            let _ = destroy(child_id);
            return Err(KernelError::InternalError);
        }
        serial_println!("[mount_ns]   namespace unmount: OK");
    }

    // --- Cleanup ---
    let _ = destroy(child_id);

    serial_println!("[mount_ns] Self-test passed (10 tests).");
    Ok(())
}
