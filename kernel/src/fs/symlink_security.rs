//! Symlink and hardlink security restrictions.
//!
//! Prevents common symlink/hardlink attacks in world-writable directories
//! (like `/tmp`).  These attacks exploit the time gap between checking a
//! symlink's target and actually opening it (TOCTOU).
//!
//! ## Protections
//!
//! ### Protected symlinks (`protected_symlinks`)
//!
//! When following a symlink in a sticky world-writable directory:
//! - The symlink must be owned by the follower (requesting user), OR
//! - The symlink must be owned by the directory owner.
//!
//! This prevents user A from creating a symlink `/tmp/evil -> /etc/shadow`
//! and tricking a privileged process into following it.
//!
//! ### Protected hardlinks (`protected_hardlinks`)
//!
//! When creating a hard link to a file:
//! - The user must own the target file, OR
//! - The user must have read+write permission to the target.
//!
//! This prevents user A from hardlinking to user B's setuid binary
//! (which would keep a copy alive after the original is patched).
//!
//! ### Protected fifos (`protected_fifos`)
//!
//! When opening a FIFO (named pipe) in a sticky world-writable directory:
//! - The FIFO must be owned by the opener, OR
//! - The FIFO must be owned by the directory owner.
//!
//! Prevents data injection attacks via FIFOs in /tmp.
//!
//! ## Configuration
//!
//! All protections can be individually enabled/disabled.  By default:
//! - `protected_symlinks`: ON (matches Linux default since 3.6)
//! - `protected_hardlinks`: ON (matches Linux default since 3.6)
//! - `protected_fifos`: OFF (Linux default, optional hardening)
//!
//! ## Reference
//!
//! Linux: `fs.protected_symlinks`, `fs.protected_hardlinks`,
//!        `fs.protected_fifos` (sysctl parameters)
//! Yama LSM symlink restrictions

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration (global atomics for lock-free hot-path checks)
// ---------------------------------------------------------------------------

static PROTECTED_SYMLINKS: AtomicBool = AtomicBool::new(true);
static PROTECTED_HARDLINKS: AtomicBool = AtomicBool::new(true);
static PROTECTED_FIFOS: AtomicBool = AtomicBool::new(false);

/// Enable or disable symlink protection.
pub fn set_protected_symlinks(enabled: bool) {
    PROTECTED_SYMLINKS.store(enabled, Ordering::Relaxed);
    serial_println!("[symlink_security] protected_symlinks = {}", enabled);
}

/// Enable or disable hardlink protection.
pub fn set_protected_hardlinks(enabled: bool) {
    PROTECTED_HARDLINKS.store(enabled, Ordering::Relaxed);
    serial_println!("[symlink_security] protected_hardlinks = {}", enabled);
}

/// Enable or disable FIFO protection.
pub fn set_protected_fifos(enabled: bool) {
    PROTECTED_FIFOS.store(enabled, Ordering::Relaxed);
    serial_println!("[symlink_security] protected_fifos = {}", enabled);
}

/// Check if symlink protection is enabled.
pub fn is_protected_symlinks() -> bool {
    PROTECTED_SYMLINKS.load(Ordering::Relaxed)
}

/// Check if hardlink protection is enabled.
pub fn is_protected_hardlinks() -> bool {
    PROTECTED_HARDLINKS.load(Ordering::Relaxed)
}

/// Check if FIFO protection is enabled.
pub fn is_protected_fifos() -> bool {
    PROTECTED_FIFOS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Directory properties
// ---------------------------------------------------------------------------

/// Check if a directory is "sticky and world-writable".
///
/// A directory with the sticky bit (mode & 0o1000) and world-write
/// (mode & 0o002) is a restricted directory (e.g., /tmp).
///
/// For our kernel, we check by path convention since sticky bit may
/// not be fully implemented in all filesystems.  `/tmp` is always
/// considered sticky+world-writable.
fn is_sticky_world_writable(dir_path: &str) -> bool {
    // Heuristic: check known sticky world-writable paths.
    dir_path == "/tmp"
        || dir_path.starts_with("/tmp/")
        || dir_path == "/var/tmp"
        || dir_path.starts_with("/var/tmp/")
}

/// Get the "directory owner" UID for a path.
///
/// Returns the UID that owns the parent directory.  For /tmp, this is
/// typically root (UID 0).
fn directory_owner_uid(dir_path: &str) -> u32 {
    // Root owns /tmp and /var/tmp.
    if dir_path == "/tmp"
        || dir_path.starts_with("/tmp/")
        || dir_path == "/var/tmp"
        || dir_path.starts_with("/var/tmp/")
    {
        return 0; // root
    }
    // Default: root.
    0
}

/// Extract the parent directory from a path.
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(pos) => path.get(..pos).unwrap_or("/"),
        None => "/",
    }
}

// ---------------------------------------------------------------------------
// Public API — security checks
// ---------------------------------------------------------------------------

/// Check whether following a symlink is safe.
///
/// Call this before following a symlink during path resolution.
///
/// Parameters:
/// - `symlink_path`: the path of the symlink being followed.
/// - `symlink_owner_uid`: the UID that owns the symlink.
/// - `follower_uid`: the UID of the process trying to follow it.
///
/// Returns `Ok(())` if safe to follow, `Err(PermissionDenied)` if the
/// symlink follow would be a potential attack.
pub fn check_symlink_follow(
    symlink_path: &str,
    symlink_owner_uid: u32,
    follower_uid: u32,
) -> KernelResult<()> {
    // Skip check if protection is disabled.
    if !PROTECTED_SYMLINKS.load(Ordering::Relaxed) {
        return Ok(());
    }

    // Only restrict in sticky world-writable directories.
    let parent = parent_dir(symlink_path);
    if !is_sticky_world_writable(parent) {
        return Ok(());
    }

    // Rule 1: Follower owns the symlink → safe.
    if symlink_owner_uid == follower_uid {
        return Ok(());
    }

    // Rule 2: Symlink is owned by the directory owner → safe.
    let dir_owner = directory_owner_uid(parent);
    if symlink_owner_uid == dir_owner {
        return Ok(());
    }

    // Rule 3: Follower is root (uid 0) → always safe.
    if follower_uid == 0 {
        return Ok(());
    }

    // Deny: potential symlink attack.
    serial_println!(
        "[symlink_security] DENIED symlink follow: {} (owner={}, follower={})",
        symlink_path, symlink_owner_uid, follower_uid
    );
    Err(KernelError::PermissionDenied)
}

/// Check whether creating a hard link to a file is safe.
///
/// Call this before creating a hard link.
///
/// Parameters:
/// - `target_path`: the existing file being linked to.
/// - `target_owner_uid`: the UID that owns the target file.
/// - `target_mode`: the permission mode of the target file.
/// - `linker_uid`: the UID creating the link.
///
/// Returns `Ok(())` if safe, `Err(PermissionDenied)` if restricted.
pub fn check_hardlink_create(
    target_path: &str,
    target_owner_uid: u32,
    target_mode: u16,
    linker_uid: u32,
) -> KernelResult<()> {
    // Skip if protection disabled.
    if !PROTECTED_HARDLINKS.load(Ordering::Relaxed) {
        return Ok(());
    }

    // Root can always create hard links.
    if linker_uid == 0 {
        return Ok(());
    }

    // Rule 1: User owns the target → safe.
    if target_owner_uid == linker_uid {
        return Ok(());
    }

    // Rule 2: User has read+write permission to the target.
    // Check "other" permissions (bits 2:0) for non-owner access.
    let other_rw = (target_mode & 0o006) == 0o006;
    if other_rw {
        return Ok(());
    }

    // Deny: hardlink to file the user doesn't own or can't read+write.
    serial_println!(
        "[symlink_security] DENIED hardlink: {} (owner={}, linker={}, mode={:o})",
        target_path, target_owner_uid, linker_uid, target_mode
    );
    Err(KernelError::PermissionDenied)
}

/// Check whether opening a FIFO is safe.
///
/// Similar to symlink protection — prevents data injection via FIFOs
/// in world-writable directories.
pub fn check_fifo_open(
    fifo_path: &str,
    fifo_owner_uid: u32,
    opener_uid: u32,
) -> KernelResult<()> {
    if !PROTECTED_FIFOS.load(Ordering::Relaxed) {
        return Ok(());
    }

    let parent = parent_dir(fifo_path);
    if !is_sticky_world_writable(parent) {
        return Ok(());
    }

    // Same rules as symlinks.
    if fifo_owner_uid == opener_uid || opener_uid == 0 {
        return Ok(());
    }

    let dir_owner = directory_owner_uid(parent);
    if fifo_owner_uid == dir_owner {
        return Ok(());
    }

    serial_println!(
        "[symlink_security] DENIED FIFO open: {} (owner={}, opener={})",
        fifo_path, fifo_owner_uid, opener_uid
    );
    Err(KernelError::PermissionDenied)
}

/// Get the current protection status.
#[derive(Debug, Clone, Copy)]
pub struct SymlinkSecurityStatus {
    /// Whether symlink protection is enabled.
    pub protected_symlinks: bool,
    /// Whether hardlink protection is enabled.
    pub protected_hardlinks: bool,
    /// Whether FIFO protection is enabled.
    pub protected_fifos: bool,
}

/// Get current protection settings.
pub fn status() -> SymlinkSecurityStatus {
    SymlinkSecurityStatus {
        protected_symlinks: is_protected_symlinks(),
        protected_hardlinks: is_protected_hardlinks(),
        protected_fifos: is_protected_fifos(),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for symlink and hardlink security restrictions.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[symlink_security] Running self-test...");

    // Save state.
    let orig_sym = is_protected_symlinks();
    let orig_hard = is_protected_hardlinks();
    let orig_fifo = is_protected_fifos();

    // --- Test 1: Symlink protection enabled ---
    {
        set_protected_symlinks(true);

        // User 1000 follows their own symlink in /tmp → allowed.
        let r = check_symlink_follow("/tmp/mylink", 1000, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: own symlink denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // Root-owned symlink in /tmp, followed by user 1000 → allowed (root owns /tmp).
        let r = check_symlink_follow("/tmp/rootlink", 0, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: root symlink in /tmp denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // User 2000's symlink in /tmp, followed by user 1000 → DENIED.
        let r = check_symlink_follow("/tmp/evillink", 2000, 1000);
        if r.is_ok() {
            serial_println!("[symlink_security]   ERROR: foreign symlink in /tmp allowed");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // Root following anything → always allowed.
        let r = check_symlink_follow("/tmp/evillink", 2000, 0);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: root follow denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        serial_println!("[symlink_security]   symlink protection OK");
    }

    // --- Test 2: Symlink outside /tmp → always allowed ---
    {
        // User 2000's symlink in /home/user, followed by user 1000 → allowed (not sticky).
        let r = check_symlink_follow("/home/user/link", 2000, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: non-sticky dir symlink denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        serial_println!("[symlink_security]   non-sticky passthrough OK");
    }

    // --- Test 3: Symlink protection disabled ---
    {
        set_protected_symlinks(false);

        // Foreign symlink in /tmp → allowed when disabled.
        let r = check_symlink_follow("/tmp/evillink", 2000, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: disabled symlink check denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        set_protected_symlinks(true);
        serial_println!("[symlink_security]   disabled bypass OK");
    }

    // --- Test 4: Hardlink protection ---
    {
        set_protected_hardlinks(true);

        // User owns the target → allowed.
        let r = check_hardlink_create("/usr/bin/prog", 1000, 0o755, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: own file hardlink denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // User doesn't own, but has read+write (other=rw-) → allowed.
        let r = check_hardlink_create("/usr/bin/prog", 0, 0o756, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: rw-accessible hardlink denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // User doesn't own and no other-rw → DENIED.
        let r = check_hardlink_create("/usr/bin/suid", 0, 0o755, 1000);
        if r.is_ok() {
            serial_println!("[symlink_security]   ERROR: unauthorized hardlink allowed");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // Root → always allowed.
        let r = check_hardlink_create("/usr/bin/suid", 1000, 0o700, 0);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: root hardlink denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        serial_println!("[symlink_security]   hardlink protection OK");
    }

    // --- Test 5: FIFO protection ---
    {
        set_protected_fifos(true);

        // User owns the FIFO → allowed.
        let r = check_fifo_open("/tmp/myfifo", 1000, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: own FIFO denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // Root-owned FIFO in /tmp → allowed.
        let r = check_fifo_open("/tmp/rootfifo", 0, 1000);
        if r.is_err() {
            serial_println!("[symlink_security]   ERROR: root FIFO in /tmp denied");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        // Foreign FIFO in /tmp → DENIED.
        let r = check_fifo_open("/tmp/evilfifo", 2000, 1000);
        if r.is_ok() {
            serial_println!("[symlink_security]   ERROR: foreign FIFO in /tmp allowed");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }

        set_protected_fifos(false);
        serial_println!("[symlink_security]   FIFO protection OK");
    }

    // --- Test 6: Status reporting ---
    {
        let st = status();
        if !st.protected_symlinks || !st.protected_hardlinks || st.protected_fifos {
            serial_println!("[symlink_security]   ERROR: status mismatch");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }
        serial_println!("[symlink_security]   status reporting OK");
    }

    // --- Test 7: parent_dir helper ---
    {
        if parent_dir("/tmp/file") != "/tmp" {
            serial_println!("[symlink_security]   ERROR: parent_dir /tmp/file");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }
        if parent_dir("/file") != "/" {
            serial_println!("[symlink_security]   ERROR: parent_dir /file");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }
        if parent_dir("/a/b/c") != "/a/b" {
            serial_println!("[symlink_security]   ERROR: parent_dir /a/b/c");
            restore(orig_sym, orig_hard, orig_fifo);
            return Err(KernelError::InternalError);
        }
        serial_println!("[symlink_security]   parent_dir helper OK");
    }

    // Restore.
    restore(orig_sym, orig_hard, orig_fifo);

    serial_println!("[symlink_security] Self-test passed (7 tests).");
    Ok(())
}

fn restore(sym: bool, hard: bool, fifo: bool) {
    PROTECTED_SYMLINKS.store(sym, Ordering::Relaxed);
    PROTECTED_HARDLINKS.store(hard, Ordering::Relaxed);
    PROTECTED_FIFOS.store(fifo, Ordering::Relaxed);
}
