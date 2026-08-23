//! File sharing — SMB/NFS network share configuration.
//!
//! Manages local shared folders (exported to the network) and
//! remote share connections (mounted from other machines).
//! Supports SMB/CIFS and NFS protocols.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Sharing → File Sharing
//!   → fileshare::add_share() / connect_remote()
//!
//! File manager integration
//!   → fileshare::list_shares() for network sidebar
//!   → fileshare::mount_remote() for browsing
//!
//! Integration:
//!   → fwsettings (open SMB/NFS ports)
//!   → useracct (share permissions per user)
//!   → credentials (stored passwords for remote)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_LOCAL_SHARES: usize = 64;
const MAX_REMOTE_SHARES: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Sharing protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareProtocol {
    /// SMB/CIFS (Windows sharing).
    Smb,
    /// NFS (Unix sharing).
    Nfs,
    /// WebDAV.
    WebDav,
    /// SFTP.
    Sftp,
}

impl ShareProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smb => "SMB",
            Self::Nfs => "NFS",
            Self::WebDav => "WebDAV",
            Self::Sftp => "SFTP",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Smb => 445,
            Self::Nfs => 2049,
            Self::WebDav => 443,
            Self::Sftp => 22,
        }
    }
}

/// Share access level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareAccess {
    /// Read-only.
    ReadOnly,
    /// Read-write.
    ReadWrite,
    /// Full control (includes delete, rename).
    FullControl,
}

impl ShareAccess {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read Only",
            Self::ReadWrite => "Read/Write",
            Self::FullControl => "Full Control",
        }
    }
}

/// Remote share connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareStatus {
    /// Not connected.
    Disconnected,
    /// Connecting.
    Connecting,
    /// Connected and mounted.
    Connected,
    /// Authentication failed.
    AuthFailed,
    /// Connection error.
    Error,
}

impl ShareStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::AuthFailed => "Auth Failed",
            Self::Error => "Error",
        }
    }
}

/// A local shared folder.
#[derive(Debug, Clone)]
pub struct LocalShare {
    /// Share ID.
    pub id: u32,
    /// Share name (network-visible).
    pub name: String,
    /// Local path being shared.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): this names a
    /// local directory, which may contain any byte but `/` and NUL.  The
    /// share *name* above stays a `String` -- that one is a network-visible
    /// protocol identifier, not a filesystem name.
    pub path: PathBuf,
    /// Protocol.
    pub protocol: ShareProtocol,
    /// Default access level.
    pub access: ShareAccess,
    /// Whether the share is active.
    pub enabled: bool,
    /// Guest access allowed.
    pub guest_access: bool,
    /// Description.
    pub description: String,
    /// Connected user count.
    pub connected_users: u32,
    /// Allow browsing (visible in network).
    pub browseable: bool,
}

/// A remote share connection.
#[derive(Debug, Clone)]
pub struct RemoteShare {
    /// Connection ID.
    pub id: u32,
    /// Remote host.
    pub host: String,
    /// Share path on remote.
    pub share_name: String,
    /// Local mount point.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): a mount point
    /// is an ordinary local directory.  `host`/`share_name`/`username` above
    /// stay `String` -- those are protocol identifiers.
    pub mount_point: PathBuf,
    /// Protocol.
    pub protocol: ShareProtocol,
    /// Username (empty for guest/kerberos).
    pub username: String,
    /// Connection status.
    pub status: ShareStatus,
    /// Auto-mount on boot.
    pub auto_mount: bool,
    /// Store credentials.
    pub save_credentials: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct FileShareState {
    local_shares: Vec<LocalShare>,
    remote_shares: Vec<RemoteShare>,
    sharing_enabled: bool,
    hostname: String,
    workgroup: String,
    next_local_id: u32,
    next_remote_id: u32,
    ops: u64,
}

static STATE: Mutex<Option<FileShareState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut FileShareState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the file sharing subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(FileShareState {
        local_shares: Vec::new(),
        remote_shares: Vec::new(),
        sharing_enabled: false,
        hostname: String::from("mycomputer"),
        workgroup: String::from("WORKGROUP"),
        next_local_id: 1,
        next_remote_id: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

/// Enable or disable file sharing.
pub fn set_sharing_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.sharing_enabled = enabled;
        Ok(())
    })
}

/// Check if sharing is enabled.
pub fn is_sharing_enabled() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.sharing_enabled)
}

/// Set the hostname for sharing.
pub fn set_hostname(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 63 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.hostname = String::from(name);
        Ok(())
    })
}

/// Get the sharing hostname.
pub fn hostname() -> String {
    let guard = STATE.lock();
    guard
        .as_ref()
        .map_or_else(|| String::from("unknown"), |s| s.hostname.clone())
}

/// Set the workgroup.
pub fn set_workgroup(name: &str) -> KernelResult<()> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.workgroup = String::from(name);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Local shares
// ---------------------------------------------------------------------------

/// Add a local shared folder.
pub fn add_share(
    name: &str,
    path: impl AsRef<Path>,
    protocol: ShareProtocol,
    access: ShareAccess,
) -> KernelResult<u32> {
    let path = path.as_ref();
    if name.is_empty() || path.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.local_shares.len() >= MAX_LOCAL_SHARES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.local_shares.iter().any(|s| s.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_local_id;
        state.next_local_id += 1;
        state.local_shares.push(LocalShare {
            id,
            name: String::from(name),
            path: path.to_path_buf(),
            protocol,
            access,
            enabled: true,
            guest_access: false,
            description: String::new(),
            connected_users: 0,
            browseable: true,
        });
        Ok(id)
    })
}

/// Remove a local share.
pub fn remove_share(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.local_shares.iter().position(|s| s.id == id) {
            state.local_shares.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Enable or disable a local share.
pub fn set_share_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .local_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.enabled = enabled;
        Ok(())
    })
}

/// Set guest access on a share.
pub fn set_guest_access(id: u32, allowed: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .local_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.guest_access = allowed;
        Ok(())
    })
}

/// Set share access level.
pub fn set_share_access(id: u32, access: ShareAccess) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .local_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.access = access;
        Ok(())
    })
}

/// Set share description.
pub fn set_share_description(id: u32, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .local_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.description = String::from(desc);
        Ok(())
    })
}

/// Get a local share by ID.
pub fn get_share(id: u32) -> KernelResult<LocalShare> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state
        .local_shares
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List local shares.
pub fn list_shares() -> Vec<LocalShare> {
    let guard = STATE.lock();
    guard
        .as_ref()
        .map_or_else(Vec::new, |s| s.local_shares.clone())
}

// ---------------------------------------------------------------------------
// Remote shares
// ---------------------------------------------------------------------------

/// Connect to a remote share.
pub fn connect_remote(
    host: &str,
    share_name: &str,
    mount_point: impl AsRef<Path>,
    protocol: ShareProtocol,
    username: &str,
) -> KernelResult<u32> {
    let mount_point = mount_point.as_ref();
    if host.is_empty() || share_name.is_empty() || mount_point.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.remote_shares.len() >= MAX_REMOTE_SHARES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_remote_id;
        state.next_remote_id += 1;
        state.remote_shares.push(RemoteShare {
            id,
            host: String::from(host),
            share_name: String::from(share_name),
            mount_point: mount_point.to_path_buf(),
            protocol,
            username: String::from(username),
            status: ShareStatus::Connected,
            auto_mount: false,
            save_credentials: false,
        });
        Ok(id)
    })
}

/// Disconnect a remote share.
pub fn disconnect_remote(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .remote_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.status = ShareStatus::Disconnected;
        Ok(())
    })
}

/// Remove a remote share config.
pub fn remove_remote(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.remote_shares.iter().position(|s| s.id == id) {
            state.remote_shares.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Set auto-mount on boot.
pub fn set_auto_mount(id: u32, auto_mount: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .remote_shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.auto_mount = auto_mount;
        Ok(())
    })
}

/// List remote shares.
pub fn list_remotes() -> Vec<RemoteShare> {
    let guard = STATE.lock();
    guard
        .as_ref()
        .map_or_else(Vec::new, |s| s.remote_shares.clone())
}

/// Get auto-mount remote shares (for boot).
pub fn auto_mount_shares() -> Vec<RemoteShare> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.remote_shares
            .iter()
            .filter(|r| r.auto_mount)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (local_count, remote_count, sharing_enabled, connected_remotes, ops).
pub fn stats() -> (usize, usize, bool, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let connected = s
                .remote_shares
                .iter()
                .filter(|r| r.status == ShareStatus::Connected)
                .count();
            (
                s.local_shares.len(),
                s.remote_shares.len(),
                s.sharing_enabled,
                connected,
                s.ops,
            )
        }
        None => (0, 0, false, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file sharing module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[fileshare] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        assert!(!is_sharing_enabled());
        assert_eq!(hostname(), "mycomputer");
        assert!(list_shares().is_empty());
        assert!(list_remotes().is_empty());
    }
    serial_println!("[fileshare]  1/12 initial state OK");

    // Test 2: enable sharing.
    {
        set_sharing_enabled(true).unwrap();
        assert!(is_sharing_enabled());
    }
    serial_println!("[fileshare]  2/12 enable sharing OK");

    // Test 3: hostname.
    {
        set_hostname("fileserver").unwrap();
        assert_eq!(hostname(), "fileserver");
        assert!(set_hostname("").is_err());
    }
    serial_println!("[fileshare]  3/12 hostname OK");

    // Test 4: add local share.
    {
        let id = add_share(
            "Public",
            "/home/public",
            ShareProtocol::Smb,
            ShareAccess::ReadOnly,
        )
        .unwrap();
        let share = get_share(id).unwrap();
        assert_eq!(share.name, "Public");
        assert_eq!(share.path.as_path(), Path::new("/home/public"));
        assert_eq!(share.protocol, ShareProtocol::Smb);
        assert!(share.enabled);
    }
    serial_println!("[fileshare]  4/12 add share OK");

    // Test 5: modify share.
    {
        let shares = list_shares();
        let id = shares.first().unwrap().id;
        set_share_access(id, ShareAccess::ReadWrite).unwrap();
        set_guest_access(id, true).unwrap();
        set_share_description(id, "Public files").unwrap();
        let s = get_share(id).unwrap();
        assert_eq!(s.access, ShareAccess::ReadWrite);
        assert!(s.guest_access);
        assert_eq!(s.description, "Public files");
    }
    serial_println!("[fileshare]  5/12 modify share OK");

    // Test 6: duplicate name.
    {
        assert!(
            add_share(
                "Public",
                "/other",
                ShareProtocol::Smb,
                ShareAccess::ReadOnly
            )
            .is_err()
        );
    }
    serial_println!("[fileshare]  6/12 duplicate check OK");

    // Test 7: connect remote.
    {
        let id = connect_remote(
            "192.168.1.100",
            "Documents",
            "/mnt/remote",
            ShareProtocol::Smb,
            "user1",
        )
        .unwrap();
        let remotes = list_remotes();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes.first().unwrap().host, "192.168.1.100");
        assert_eq!(remotes.first().unwrap().status, ShareStatus::Connected);
        let _ = id;
    }
    serial_println!("[fileshare]  7/12 connect remote OK");

    // Test 8: disconnect.
    {
        let remotes = list_remotes();
        let id = remotes.first().unwrap().id;
        disconnect_remote(id).unwrap();
        let remotes = list_remotes();
        assert_eq!(remotes.first().unwrap().status, ShareStatus::Disconnected);
    }
    serial_println!("[fileshare]  8/12 disconnect OK");

    // Test 9: auto-mount.
    {
        let remotes = list_remotes();
        let id = remotes.first().unwrap().id;
        set_auto_mount(id, true).unwrap();
        let auto = auto_mount_shares();
        assert_eq!(auto.len(), 1);
    }
    serial_println!("[fileshare]  9/12 auto-mount OK");

    // Test 10: remove.
    {
        let shares = list_shares();
        let id = shares.first().unwrap().id;
        remove_share(id).unwrap();
        assert!(list_shares().is_empty());
        let remotes = list_remotes();
        let id = remotes.first().unwrap().id;
        remove_remote(id).unwrap();
        assert!(list_remotes().is_empty());
    }
    serial_println!("[fileshare] 10/12 remove OK");

    // Test 11: stats.
    {
        let (local, remote, enabled, _, ops) = stats();
        assert_eq!(local, 0);
        assert_eq!(remote, 0);
        assert!(enabled);
        assert!(ops > 0);
    }
    serial_println!("[fileshare] 11/12 stats OK");

    // Test 12: non-UTF-8 local paths and mount points (design-decisions.md 261).
    //
    // A shared folder and a mount point are both ordinary local directories,
    // so either may be named with bytes that have no UTF-8 spelling.  While
    // these fields were `String`s such a directory could not be shared or
    // mounted at all.  Two mount points differing only in such a byte are
    // distinct directories and must stay distinct rows.
    {
        let share_path = Path::new(&b"/srv/fsh_\xFFp"[..]);
        let mp_a = Path::new(&b"/mnt/fsh_\xFFm"[..]);
        let mp_b = Path::new(&b"/mnt/fsh_\xFEm"[..]);

        let lid = add_share(
            "nonutf8",
            share_path,
            ShareProtocol::Smb,
            ShareAccess::ReadOnly,
        )
        .expect("add non-utf8 share");
        let got = get_share(lid).expect("get non-utf8 share");
        assert_eq!(got.path.as_path(), share_path);
        assert_eq!(got.path.as_path().as_bytes(), b"/srv/fsh_\xFFp");

        let ra = connect_remote("nu.local", "docs", mp_a, ShareProtocol::Smb, "user")
            .expect("connect non-utf8 a");
        let rb = connect_remote("nu.local", "docs", mp_b, ShareProtocol::Smb, "user")
            .expect("connect non-utf8 b");
        assert_ne!(ra, rb);
        let remotes = list_remotes();
        let sa = remotes.iter().find(|r| r.id == ra).expect("remote a");
        let sb = remotes.iter().find(|r| r.id == rb).expect("remote b");
        assert_eq!(sa.mount_point.as_path(), mp_a);
        assert_eq!(sb.mount_point.as_path(), mp_b);
        assert_ne!(sa.mount_point, sb.mount_point);

        // Restore the empty registry test 11 left behind.
        remove_share(lid).expect("remove non-utf8 share");
        remove_remote(ra).expect("remove remote a");
        remove_remote(rb).expect("remove remote b");
        assert!(list_shares().is_empty());
        assert!(list_remotes().is_empty());
    }
    serial_println!("[fileshare] 12/12 non-UTF-8 paths OK");

    // Leave NO residue.  The tests above enable sharing and set the hostname
    // to "fileserver"; leaving that behind would make `fileshare show` report
    // sharing switched on and the machine renamed, neither of which the user
    // asked for.  `init_defaults()` is only ever called explicitly (there is
    // no lazy init -- `with_state` returns NotSupported when uninitialised),
    // so resetting to `None` restores exactly the state a fresh boot has.
    *STATE.lock() = None;

    serial_println!("[fileshare] All self-tests passed.");
}
