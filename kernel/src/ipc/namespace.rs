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
//! Namespace resolution adds one lock acquisition + a linear scan of
//! rules per path operation.  Typical namespaces have < 10 rules, so
//! this is negligible compared to VFS/filesystem I/O.  The root
//! namespace (ID 0) has no rules and short-circuits immediately.
//!
//! ## References
//!
//! - Linux mount namespaces (unshare(CLONE_NEWNS))
//! - Plan 9 per-process namespaces (the original design)
//! - Design spec: "Per-process namespaces" in design.txt

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};

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
        source_prefix: String,
        target_prefix: String,
    },
    /// Block access to all paths matching this prefix.
    ///
    /// Any path starting with this prefix returns `PermissionDenied`.
    Hide {
        prefix: String,
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
    source_prefix: &str,
    target_prefix: &str,
) -> KernelResult<()> {
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
        source_prefix: String::from(source_prefix),
        target_prefix: String::from(target_prefix),
    });

    Ok(())
}

/// Remove a bind rule from a namespace (by source prefix match).
///
/// Removes the first rule whose source prefix matches exactly.
/// Returns `NotFound` if no such rule exists.
pub fn unbind(ns_id: NamespaceId, source_prefix: &str) -> KernelResult<()> {
    if ns_id == ROOT_NAMESPACE {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_TABLE.lock();
    let ns = table.get_mut(&ns_id)
        .ok_or(KernelError::NotFound)?;

    let pos = ns.rules.iter().position(|rule| match rule {
        NsRule::Bind { source_prefix: s, .. } => s == source_prefix,
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
pub fn hide(ns_id: NamespaceId, prefix: &str) -> KernelResult<()> {
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
        prefix: String::from(prefix),
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
}

/// Query which namespace a process belongs to.
pub fn query(process_id: u64) -> NamespaceId {
    let pns = PROCESS_NS.lock();
    pns.get(&process_id).copied().unwrap_or(ROOT_NAMESPACE)
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
pub fn resolve_path(path: &str) -> KernelResult<String> {
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
pub fn resolve_path_for(process_id: u64, path: &str) -> KernelResult<String> {
    let ns_id = {
        let pns = PROCESS_NS.lock();
        pns.get(&process_id).copied().unwrap_or(ROOT_NAMESPACE)
    };

    if ns_id == ROOT_NAMESPACE {
        // Fast path: no namespace rules, return as-is.
        return Ok(String::from(path));
    }

    let table = NS_TABLE.lock();
    let ns = match table.get(&ns_id) {
        Some(ns) => ns,
        None => {
            // Namespace was destroyed but process still references it.
            // Fall back to root namespace behavior.
            return Ok(String::from(path));
        }
    };

    apply_rules(&ns.rules, path)
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
fn validate_prefix(prefix: &str) -> KernelResult<()> {
    if prefix.is_empty() || !prefix.starts_with('/') {
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
fn apply_rules(rules: &[NsRule], path: &str) -> KernelResult<String> {
    for rule in rules {
        match rule {
            NsRule::Bind { source_prefix, target_prefix } => {
                if let Some(suffix) = strip_prefix_match(path, source_prefix) {
                    // Construct the remapped path.
                    let mut result = String::with_capacity(
                        target_prefix.len().saturating_add(suffix.len())
                    );
                    result.push_str(target_prefix);
                    result.push_str(suffix);
                    return Ok(result);
                }
            }
            NsRule::Hide { prefix } => {
                if path_matches_prefix(path, prefix) {
                    return Err(KernelError::PermissionDenied);
                }
            }
        }
    }

    // No rule matched — pass through unchanged.
    Ok(String::from(path))
}

/// Check if a path starts with the given prefix, respecting path boundaries.
///
/// `/home` matches `/home` and `/home/user` but NOT `/homework`.
fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if path == prefix {
        return true;
    }
    if let Some(after) = path.strip_prefix(prefix) {
        // Must be followed by '/' or end of string to match
        // a directory boundary (not a partial filename).
        return after.starts_with('/');
    }
    false
}

/// Strip a prefix from a path, returning the suffix (including the
/// leading `/` separator).  Returns `None` if the path doesn't match
/// the prefix at a path boundary.
///
/// Examples:
/// - strip_prefix_match("/home/user/file", "/home") → Some("/user/file")
/// - strip_prefix_match("/home", "/home") → Some("")
/// - strip_prefix_match("/homework", "/home") → None
fn strip_prefix_match<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path == prefix {
        return Some("");
    }
    if let Some(suffix) = path.strip_prefix(prefix) {
        if suffix.starts_with('/') {
            return Some(suffix);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run namespace self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[namespace] Running self-tests...");

    test_create_destroy()?;
    test_bind_resolution()?;
    test_hide_resolution()?;
    test_path_boundary_matching()?;
    test_clone_namespace()?;
    test_process_attach_detach()?;

    serial_println!("[namespace] All self-tests PASSED");
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
    let result = apply_rules(&ns_obj.rules, "/data/file.txt")?;
    assert_eq!(result, "/sandbox/data/file.txt");

    // Exact prefix match.
    let result = apply_rules(&ns_obj.rules, "/data")?;
    assert_eq!(result, "/sandbox/data");

    // Path NOT matching (different prefix).
    let result = apply_rules(&ns_obj.rules, "/home/file.txt")?;
    assert_eq!(result, "/home/file.txt");

    // Path with similar prefix that shouldn't match.
    let result = apply_rules(&ns_obj.rules, "/database/file.txt")?;
    assert_eq!(result, "/database/file.txt");

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
    let result = apply_rules(&ns_obj.rules, "/secret/keys.txt");
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        KernelError::PermissionDenied
    ));

    // Exact match.
    let result = apply_rules(&ns_obj.rules, "/secret");
    assert!(result.is_err());

    // Non-matching path passes through.
    let result = apply_rules(&ns_obj.rules, "/public/file.txt")?;
    assert_eq!(result, "/public/file.txt");

    // Similar prefix that shouldn't match.
    let result = apply_rules(&ns_obj.rules, "/secretive/file.txt")?;
    assert_eq!(result, "/secretive/file.txt");

    drop(table);
    destroy(ns)?;

    serial_println!("[namespace]   Hide resolution: OK");
    Ok(())
}

fn test_path_boundary_matching() -> KernelResult<()> {
    // Test the path boundary matching logic directly.
    assert!(path_matches_prefix("/home/user", "/home"));
    assert!(path_matches_prefix("/home", "/home"));
    assert!(!path_matches_prefix("/homework", "/home"));
    assert!(!path_matches_prefix("/hom", "/home"));
    assert!(path_matches_prefix("/a/b/c", "/a"));
    assert!(path_matches_prefix("/a/b/c", "/a/b"));
    assert!(path_matches_prefix("/a/b/c", "/a/b/c"));
    assert!(!path_matches_prefix("/a/b/c", "/a/b/cd"));

    // Test strip_prefix_match.
    assert_eq!(strip_prefix_match("/home/user/file", "/home"), Some("/user/file"));
    assert_eq!(strip_prefix_match("/home", "/home"), Some(""));
    assert_eq!(strip_prefix_match("/homework", "/home"), None);
    assert_eq!(strip_prefix_match("/a/b", "/a"), Some("/b"));

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

    let result = apply_rules(&ns2_obj.rules, "/data/file.txt")?;
    assert_eq!(result, "/sandbox/data/file.txt");

    let result = apply_rules(&ns2_obj.rules, "/secret");
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
