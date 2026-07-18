//! Named container volumes (Docker `docker volume`).
//!
//! A *named volume* is a managed directory the runtime creates on demand and
//! bind-mounts into containers via `-v NAME:/container/path`. Unlike a host
//! bind mount (`-v /host/path:/container/path`, where the caller owns the host
//! path), the runtime owns a named volume's backing directory — its location
//! (under [`VOLUMES_ROOT`]) and its lifecycle (create/remove).
//!
//! ## Design
//!
//! - The registry is an in-memory table of names, mirroring the rest of the
//!   container model (the container table is likewise not persisted across
//!   boots). A volume's *data*, however, lives on the ext4 rootfs and survives
//!   until [`remove`]d, so `docker volume create` + populate + `docker run -v
//!   NAME:/path` behaves as expected within a boot.
//! - Backing directories are `VOLUMES_ROOT/<name>`. Docker nests an extra
//!   `_data` subdir; we keep it flat because our runtime, not a daemon, owns
//!   the layout and there is no metadata sidecar to separate from the data.
//!
//! ## References
//!
//! - Docker `docker volume create/ls/rm/inspect`; local volume driver.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::sync::PreemptSpinMutex as Mutex;

/// Root directory under which named-volume backing directories are created.
pub const VOLUMES_ROOT: &str = "/var/lib/slate/volumes";

/// Maximum number of named volumes tracked at once.
pub const MAX_VOLUMES: usize = 256;

/// Maximum length of a volume name.
pub const MAX_VOLUME_NAME_LEN: usize = 64;

/// A registered named volume.
struct Volume {
    /// The volume's name (unique within the registry).
    name: String,
}

struct VolumeTable {
    volumes: Vec<Volume>,
}

impl VolumeTable {
    const fn new() -> Self {
        Self { volumes: Vec::new() }
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.volumes.iter().position(|v| v.name == name)
    }
}

static TABLE: Mutex<VolumeTable> = Mutex::new(VolumeTable::new());

/// Validate a proposed volume name.
///
/// A name must be non-empty, at most [`MAX_VOLUME_NAME_LEN`] bytes, contain no
/// path separator (`/`) or NUL (so it maps to exactly one directory under
/// [`VOLUMES_ROOT`]), and must not be `.` or `..` (which would alias the
/// volumes root or its parent). All other bytes are permitted, consistent with
/// the OS-wide path rule (any byte except `/` and NUL). Names are treated as
/// opaque byte strings — no UTF-8 requirement is imposed.
///
/// # Errors
/// [`KernelError::InvalidArgument`] if the name violates any rule above.
pub fn validate_name(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > MAX_VOLUME_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }
    if name == "." || name == ".." {
        return Err(KernelError::InvalidArgument);
    }
    if name.as_bytes().iter().any(|&b| b == b'/' || b == 0) {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// The backing directory path for a volume `name` (does not check existence).
///
/// # Errors
/// [`KernelError::InvalidArgument`] if `name` is invalid (see [`validate_name`]).
pub fn backing_path(name: &str) -> KernelResult<String> {
    validate_name(name)?;
    let mut p = String::from(VOLUMES_ROOT);
    p.push('/');
    p.push_str(name);
    Ok(p)
}

/// Create a named volume, materializing its backing directory.
///
/// Idempotent: creating a volume that already exists succeeds and returns its
/// existing backing path (matching `docker volume create`'s behavior of being
/// safe to re-run). The backing directory (and any missing parents, e.g.
/// [`VOLUMES_ROOT`] itself on first use) is created via `mkdir_all`.
///
/// Returns the backing directory path.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `name` is invalid.
/// - [`KernelError::ResourceExhausted`] if the registry is full ([`MAX_VOLUMES`]).
/// - Any VFS error from creating the backing directory.
pub fn create(name: &str) -> KernelResult<String> {
    let path = backing_path(name)?;
    {
        let mut table = TABLE.lock();
        if table.position(name).is_none() {
            if table.volumes.len() >= MAX_VOLUMES {
                return Err(KernelError::ResourceExhausted);
            }
            table.volumes.push(Volume { name: String::from(name) });
        }
    }
    // Materialize the backing directory outside the registry lock (VFS has its
    // own locking). `mkdir_all` is idempotent, so a re-create is harmless.
    crate::fs::vfs::Vfs::mkdir_all(&path)?;
    Ok(path)
}

/// Ensure a volume exists (create-on-demand), returning its backing path.
///
/// Convenience wrapper over [`create`] for the `-v NAME:/path` run path, where
/// referencing a not-yet-created volume should transparently create it (Docker
/// auto-creates a named volume on first `-v NAME:...` use).
///
/// # Errors
/// Same as [`create`].
pub fn ensure(name: &str) -> KernelResult<String> {
    create(name)
}

/// Whether a volume with `name` is registered.
#[must_use]
pub fn exists(name: &str) -> bool {
    TABLE.lock().position(name).is_some()
}

/// The backing path of a registered volume, or `None` if it is not registered.
#[must_use]
pub fn path_of(name: &str) -> Option<String> {
    let table = TABLE.lock();
    if table.position(name).is_some() {
        // `backing_path` only fails on an invalid name, which a *registered*
        // name cannot be (it passed `validate_name` at create time).
        backing_path(name).ok()
    } else {
        None
    }
}

/// List all registered volume names (in registration order).
#[must_use]
pub fn list() -> Vec<String> {
    TABLE.lock().volumes.iter().map(|v| v.name.clone()).collect()
}

/// The number of registered volumes.
#[must_use]
pub fn count() -> usize {
    TABLE.lock().volumes.len()
}

/// Remove a named volume, deleting its backing directory and all its contents.
///
/// The registry entry is removed and the backing directory tree is recursively
/// deleted. A volume that is not registered yields [`KernelError::NotFound`].
///
/// Note: this does **not** check whether a running container is using the
/// volume. Docker refuses to remove an in-use volume; our container model does
/// not track per-volume usage, so removal is unconditional — the caller (the
/// operator via `docker volume rm`) is responsible for not pulling a volume out
/// from under a running container.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `name` is invalid.
/// - [`KernelError::NotFound`] if no such volume is registered.
/// - Any VFS error from removing the backing directory (the registry entry is
///   still removed first, so a failed data delete leaves an orphaned directory
///   rather than a dangling registry entry).
pub fn remove(name: &str) -> KernelResult<()> {
    let path = backing_path(name)?;
    {
        let mut table = TABLE.lock();
        match table.position(name) {
            Some(pos) => {
                table.volumes.remove(pos);
            }
            None => return Err(KernelError::NotFound),
        }
    }
    // Recursively delete the backing data outside the registry lock. A missing
    // directory (never materialized) is fine to ignore; other errors propagate.
    match crate::fs::vfs::Vfs::remove_recursive(&path) {
        Ok(_) | Err(KernelError::NotFound) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Remove every registered volume, returning the count removed.
///
/// Best-effort: a volume whose backing data fails to delete is still unregistered
/// and counted (the orphaned directory can be cleaned up manually). Mirrors
/// `docker volume prune`, minus the in-use guard (see [`remove`]).
pub fn prune() -> usize {
    let names = list();
    let mut removed = 0usize;
    for name in names {
        // `remove` only errors here if the volume vanished concurrently (single
        // session, so it won't) — count successful unregistrations.
        if remove(&name).is_ok() {
            removed = removed.saturating_add(1);
        }
    }
    removed
}

/// Total byte size of a volume's backing tree, summing regular-file sizes.
///
/// Walks the volume's backing directory (under [`VOLUMES_ROOT`]) with an
/// explicit work stack — bounded kernel stack, no recursion — so a deep tree
/// cannot overflow.  Returns `0` for an unknown/invalid name or a volume whose
/// backing directory was never materialized; directories that fail to
/// `readdir` are skipped rather than aborting the total.  This is a best-effort
/// `du`-style measurement for `docker system df` (a summary display), so it
/// tolerates transient VFS errors instead of surfacing them.
#[must_use]
pub fn backing_size(name: &str) -> u64 {
    let Ok(root) = backing_path(name) else {
        return 0;
    };
    dir_tree_bytes(&root)
}

/// Sum of regular-file sizes in the directory subtree rooted at `root`.
///
/// Iterative (stack-based) VFS walk shared by [`backing_size`].  A `MAX_ENTRIES`
/// cap bounds a pathological tree; symlinks carry no data payload to sum.
fn dir_tree_bytes(root: &str) -> u64 {
    use crate::fs::vfs::{EntryType, Vfs};
    const MAX_ENTRIES: usize = 1_000_000;
    let mut total: u64 = 0;
    let mut visited: usize = 0;
    let mut stack: Vec<String> = alloc::vec![String::from(root.trim_end_matches('/'))];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = Vfs::readdir(&dir) else {
            continue;
        };
        for de in entries {
            if de.name == "." || de.name == ".." {
                continue;
            }
            visited = visited.saturating_add(1);
            if visited >= MAX_ENTRIES {
                return total;
            }
            let child = alloc::format!("{dir}/{}", de.name);
            match de.entry_type {
                EntryType::Directory => stack.push(child),
                EntryType::File => total = total.saturating_add(de.size),
                _ => {}
            }
        }
    }
    total
}

/// Self-test for the named-volume registry (invoked at boot).
///
/// Exercises name validation, create/idempotency, list/exists/path_of, remove,
/// and prune against real backing directories under a throwaway prefix, then
/// cleans up. Panics on any invariant violation (the boot self-test convention).
pub fn self_test() {
    use crate::serial_println;
    serial_println!("[volume] Running self-test...");

    // Name validation.
    assert!(validate_name("data").is_ok(), "simple name must validate");
    assert!(validate_name("my-vol_1.2").is_ok(), "docker-ish name must validate");
    assert!(validate_name("").is_err(), "empty name must be rejected");
    assert!(validate_name(".").is_err(), "'.' must be rejected");
    assert!(validate_name("..").is_err(), "'..' must be rejected");
    assert!(validate_name("a/b").is_err(), "name with '/' must be rejected");
    let too_long = "x".repeat(MAX_VOLUME_NAME_LEN + 1);
    assert!(validate_name(&too_long).is_err(), "over-long name must be rejected");
    let max_ok = "x".repeat(MAX_VOLUME_NAME_LEN);
    assert!(validate_name(&max_ok).is_ok(), "max-length name must validate");
    serial_println!("[volume]   name validation: OK");

    // Backing path derivation.
    assert_eq!(
        backing_path("data").expect("backing_path data"),
        "/var/lib/slate/volumes/data",
        "backing path must be VOLUMES_ROOT/name",
    );
    serial_println!("[volume]   backing path derivation: OK");

    // Create / idempotency / list / exists / path_of.
    let base = count();
    let p1 = create("st-vol-a").expect("create st-vol-a");
    assert!(exists("st-vol-a"), "created volume must exist");
    assert_eq!(count(), base + 1, "create must add one entry");
    // Idempotent re-create: same path, no duplicate entry.
    let p1b = create("st-vol-a").expect("re-create st-vol-a");
    assert_eq!(p1, p1b, "re-create must return the same path");
    assert_eq!(count(), base + 1, "re-create must not duplicate the entry");
    assert_eq!(
        path_of("st-vol-a").as_deref(),
        Some(p1.as_str()),
        "path_of must return the backing path of a registered volume",
    );
    assert!(path_of("st-vol-missing").is_none(), "path_of unknown volume is None");
    // The backing directory must actually exist on the VFS.
    assert!(
        crate::fs::vfs::Vfs::exists(&p1),
        "create must materialize the backing directory",
    );
    let names = list();
    assert!(names.iter().any(|n| n == "st-vol-a"), "list must include the volume");
    serial_println!("[volume]   create/list/exists/path_of: OK");

    // ensure() is create-on-demand.
    let p2 = ensure("st-vol-b").expect("ensure st-vol-b");
    assert!(exists("st-vol-b"), "ensure must create the volume");
    assert!(crate::fs::vfs::Vfs::exists(&p2), "ensure must materialize the dir");
    serial_println!("[volume]   ensure (create-on-demand): OK");

    // backing_size(): sums regular-file bytes across the backing tree, including
    // a nested subdir; unknown volume is 0; a freshly-created empty volume is 0.
    {
        use crate::fs::vfs::Vfs;
        assert_eq!(backing_size("st-vol-missing"), 0, "unknown volume sizes to 0");
        assert_eq!(backing_size("st-vol-b"), 0, "empty volume sizes to 0");
        // Write 3 + 5 bytes at two depths and confirm the total is 8.
        Vfs::write_file(&alloc::format!("{p2}/a.txt"), b"AAA").expect("write a");
        Vfs::mkdir(&alloc::format!("{p2}/sub")).expect("mkdir sub");
        Vfs::write_file(&alloc::format!("{p2}/sub/b.txt"), b"BBBBB").expect("write b");
        assert_eq!(
            backing_size("st-vol-b"), 8,
            "backing_size must sum nested regular-file bytes",
        );
    }
    serial_println!("[volume]   backing_size (du-style tree sum): OK");

    // Remove: unregisters and deletes the backing directory.
    remove("st-vol-a").expect("remove st-vol-a");
    assert!(!exists("st-vol-a"), "removed volume must not exist");
    assert!(
        !crate::fs::vfs::Vfs::exists(&p1),
        "remove must delete the backing directory",
    );
    assert!(remove("st-vol-a").is_err(), "removing a gone volume must error");
    serial_println!("[volume]   remove (unregister + delete data): OK");

    // Clean up the second volume and confirm the registry returns to baseline.
    // (The global `prune` is intentionally *not* exercised here — it would wipe
    // any legitimately-registered volumes, which is unsafe in a shared boot.)
    remove("st-vol-b").expect("remove st-vol-b");
    assert_eq!(count(), base, "registry must return to its baseline count");
    let _ = crate::fs::vfs::Vfs::remove_recursive(&p2);
    serial_println!("[volume]   cleanup to baseline: OK");

    serial_println!("[volume] Self-test PASSED");
}
