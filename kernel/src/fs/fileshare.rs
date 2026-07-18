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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

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
    pub path: String,
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
    pub mount_point: String,
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
    with_state(|state| { state.sharing_enabled = enabled; Ok(()) })
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
    with_state(|state| { state.hostname = String::from(name); Ok(()) })
}

/// Get the sharing hostname.
pub fn hostname() -> String {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(|| String::from("unknown"), |s| s.hostname.clone())
}

/// Set the workgroup.
pub fn set_workgroup(name: &str) -> KernelResult<()> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| { state.workgroup = String::from(name); Ok(()) })
}

// ---------------------------------------------------------------------------
// Local shares
// ---------------------------------------------------------------------------

/// Add a local shared folder.
pub fn add_share(name: &str, path: &str, protocol: ShareProtocol, access: ShareAccess) -> KernelResult<u32> {
    if name.is_empty() || path.is_empty() {
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
            path: String::from(path),
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
        let share = state.local_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
        share.enabled = enabled;
        Ok(())
    })
}

/// Set guest access on a share.
pub fn set_guest_access(id: u32, allowed: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state.local_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
        share.guest_access = allowed;
        Ok(())
    })
}

/// Set share access level.
pub fn set_share_access(id: u32, access: ShareAccess) -> KernelResult<()> {
    with_state(|state| {
        let share = state.local_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
        share.access = access;
        Ok(())
    })
}

/// Set share description.
pub fn set_share_description(id: u32, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        let share = state.local_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
        share.description = String::from(desc);
        Ok(())
    })
}

/// Get a local share by ID.
pub fn get_share(id: u32) -> KernelResult<LocalShare> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.local_shares.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
}

/// List local shares.
pub fn list_shares() -> Vec<LocalShare> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.local_shares.clone())
}

// ---------------------------------------------------------------------------
// Remote shares
// ---------------------------------------------------------------------------

/// Connect to a remote share.
pub fn connect_remote(
    host: &str,
    share_name: &str,
    mount_point: &str,
    protocol: ShareProtocol,
    username: &str,
) -> KernelResult<u32> {
    if host.is_empty() || share_name.is_empty() || mount_point.is_empty() {
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
            mount_point: String::from(mount_point),
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
        let share = state.remote_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
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
        let share = state.remote_shares.iter_mut().find(|s| s.id == id).ok_or(KernelError::NotFound)?;
        share.auto_mount = auto_mount;
        Ok(())
    })
}

/// List remote shares.
pub fn list_remotes() -> Vec<RemoteShare> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.remote_shares.clone())
}

/// Get auto-mount remote shares (for boot).
pub fn auto_mount_shares() -> Vec<RemoteShare> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.remote_shares.iter().filter(|r| r.auto_mount).cloned().collect()
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
            let connected = s.remote_shares.iter().filter(|r| r.status == ShareStatus::Connected).count();
            (s.local_shares.len(), s.remote_shares.len(), s.sharing_enabled, connected, s.ops)
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
    serial_println!("[fileshare]  1/11 initial state OK");

    // Test 2: enable sharing.
    {
        set_sharing_enabled(true).unwrap();
        assert!(is_sharing_enabled());
    }
    serial_println!("[fileshare]  2/11 enable sharing OK");

    // Test 3: hostname.
    {
        set_hostname("fileserver").unwrap();
        assert_eq!(hostname(), "fileserver");
        assert!(set_hostname("").is_err());
    }
    serial_println!("[fileshare]  3/11 hostname OK");

    // Test 4: add local share.
    {
        let id = add_share("Public", "/home/public", ShareProtocol::Smb, ShareAccess::ReadOnly).unwrap();
        let share = get_share(id).unwrap();
        assert_eq!(share.name, "Public");
        assert_eq!(share.path, "/home/public");
        assert_eq!(share.protocol, ShareProtocol::Smb);
        assert!(share.enabled);
    }
    serial_println!("[fileshare]  4/11 add share OK");

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
    serial_println!("[fileshare]  5/11 modify share OK");

    // Test 6: duplicate name.
    {
        assert!(add_share("Public", "/other", ShareProtocol::Smb, ShareAccess::ReadOnly).is_err());
    }
    serial_println!("[fileshare]  6/11 duplicate check OK");

    // Test 7: connect remote.
    {
        let id = connect_remote("192.168.1.100", "Documents", "/mnt/remote", ShareProtocol::Smb, "user1").unwrap();
        let remotes = list_remotes();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes.first().unwrap().host, "192.168.1.100");
        assert_eq!(remotes.first().unwrap().status, ShareStatus::Connected);
        let _ = id;
    }
    serial_println!("[fileshare]  7/11 connect remote OK");

    // Test 8: disconnect.
    {
        let remotes = list_remotes();
        let id = remotes.first().unwrap().id;
        disconnect_remote(id).unwrap();
        let remotes = list_remotes();
        assert_eq!(remotes.first().unwrap().status, ShareStatus::Disconnected);
    }
    serial_println!("[fileshare]  8/11 disconnect OK");

    // Test 9: auto-mount.
    {
        let remotes = list_remotes();
        let id = remotes.first().unwrap().id;
        set_auto_mount(id, true).unwrap();
        let auto = auto_mount_shares();
        assert_eq!(auto.len(), 1);
    }
    serial_println!("[fileshare]  9/11 auto-mount OK");

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
    serial_println!("[fileshare] 10/11 remove OK");

    // Test 11: stats.
    {
        let (local, remote, enabled, _, ops) = stats();
        assert_eq!(local, 0);
        assert_eq!(remote, 0);
        assert!(enabled);
        assert!(ops > 0);
    }
    serial_println!("[fileshare] 11/11 stats OK");

    serial_println!("[fileshare] All self-tests passed.");
}
