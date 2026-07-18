//! Network shares — SMB/CIFS and NFS mount management.
//!
//! Manages mounting of network file shares, credential caching,
//! auto-mount on login, and performance tuning for network filesystems.
//!
//! ## Architecture
//!
//! ```text
//! File manager / mount command
//!   → netshare::mount(url, mountpoint) → connect + mount
//!
//! Login / autostart
//!   → netshare::auto_mount() → reconnect saved shares
//!
//! Settings panel → Network → Shared Folders
//!   → netshare::list_shares() → show mounted shares
//!
//! Integration:
//!   → credentials (saved mount credentials)
//!   → netsettings (network availability)
//!   → fileshare (our shares to others)
//!   → notifcenter (disconnect alerts)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Network share protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareProtocol {
    Smb2,
    Smb3,
    Nfs3,
    Nfs4,
    WebDav,
    Sshfs,
}

impl ShareProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smb2 => "SMB2",
            Self::Smb3 => "SMB3",
            Self::Nfs3 => "NFSv3",
            Self::Nfs4 => "NFSv4",
            Self::WebDav => "WebDAV",
            Self::Sshfs => "SSHFS",
        }
    }
}

/// Mount state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountState {
    Connected,
    Disconnected,
    Reconnecting,
    Error,
}

impl MountState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Reconnecting => "reconnecting",
            Self::Error => "error",
        }
    }
}

/// A mounted network share.
#[derive(Debug, Clone)]
pub struct NetShare {
    /// Share ID.
    pub id: u32,
    /// Protocol.
    pub protocol: ShareProtocol,
    /// Remote host.
    pub host: String,
    /// Remote path.
    pub remote_path: String,
    /// Local mount point.
    pub mount_point: String,
    /// Username.
    pub username: String,
    /// Mount state.
    pub state: MountState,
    /// Auto-mount on login.
    pub auto_mount: bool,
    /// Read-only mount.
    pub read_only: bool,
    /// Bytes read.
    pub bytes_read: u64,
    /// Bytes written.
    pub bytes_written: u64,
    /// Mount timestamp (ns).
    pub mounted_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SHARES: usize = 50;

struct State {
    shares: Vec<NetShare>,
    next_id: u32,
    total_mounts: u64,
    total_unmounts: u64,
    total_errors: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        shares: Vec::new(),
        next_id: 1,
        total_mounts: 0,
        total_unmounts: 0,
        total_errors: 0,
        ops: 0,
    });
}

/// Mount a network share.
pub fn mount(
    protocol: ShareProtocol, host: &str, remote_path: &str,
    mount_point: &str, username: &str, auto_mount: bool, read_only: bool,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.shares.len() >= MAX_SHARES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.shares.iter().any(|s| s.mount_point == mount_point) {
            return Err(KernelError::AlreadyExists);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.shares.push(NetShare {
            id, protocol,
            host: String::from(host),
            remote_path: String::from(remote_path),
            mount_point: String::from(mount_point),
            username: String::from(username),
            state: MountState::Connected,
            auto_mount, read_only,
            bytes_read: 0, bytes_written: 0,
            mounted_ns: crate::hpet::elapsed_ns(),
        });
        state.total_mounts += 1;
        Ok(id)
    })
}

/// Unmount a network share.
pub fn unmount(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.shares.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.shares.remove(pos);
        state.total_unmounts += 1;
        Ok(())
    })
}

/// Set share connection state.
pub fn set_state(id: u32, new_state: MountState) -> KernelResult<()> {
    with_state(|state| {
        let share = state.shares.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.state = new_state;
        if new_state == MountState::Error {
            state.total_errors += 1;
        }
        Ok(())
    })
}

/// Record I/O on a share.
pub fn record_io(id: u32, bytes_read: u64, bytes_written: u64) -> KernelResult<()> {
    with_state(|state| {
        let share = state.shares.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.bytes_read += bytes_read;
        share.bytes_written += bytes_written;
        Ok(())
    })
}

/// Set auto-mount.
pub fn set_auto_mount(id: u32, auto_mount: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state.shares.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.auto_mount = auto_mount;
        Ok(())
    })
}

/// List all shares.
pub fn list_shares() -> Vec<NetShare> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.shares.clone())
}

/// Get share by ID.
pub fn get_share(id: u32) -> KernelResult<NetShare> {
    with_state(|state| {
        state.shares.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List auto-mount shares.
pub fn auto_mount_shares() -> Vec<NetShare> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.shares.iter().filter(|sh| sh.auto_mount).cloned().collect()
    })
}

/// Statistics: (share_count, connected_count, total_mounts, total_errors, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let connected = s.shares.iter().filter(|sh| sh.state == MountState::Connected).count();
            (s.shares.len(), connected, s.total_mounts, s.total_errors, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netshare::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_shares().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Mount SMB share.
    let id1 = mount(ShareProtocol::Smb3, "fileserver.local", "/share/docs",
        "/mnt/docs", "user", true, false).expect("mount smb");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] mount SMB: OK");

    // 3: Mount NFS share.
    let id2 = mount(ShareProtocol::Nfs4, "nfs.local", "/export/data",
        "/mnt/data", "root", false, true).expect("mount nfs");
    assert_eq!(list_shares().len(), 2);
    crate::serial_println!("  [3/11] mount NFS: OK");

    // 4: Duplicate mount point rejected.
    let r = mount(ShareProtocol::Smb2, "other", "/other", "/mnt/docs", "user", false, false);
    assert!(r.is_err());
    crate::serial_println!("  [4/11] duplicate rejected: OK");

    // 5: Get share info.
    let s = get_share(id1).expect("get share");
    assert_eq!(s.protocol, ShareProtocol::Smb3);
    assert_eq!(s.state, MountState::Connected);
    crate::serial_println!("  [5/11] share info: OK");

    // 6: Record I/O.
    record_io(id1, 1024, 512).expect("io");
    let s = get_share(id1).expect("get 2");
    assert_eq!(s.bytes_read, 1024);
    crate::serial_println!("  [6/11] record I/O: OK");

    // 7: Connection state.
    set_state(id1, MountState::Disconnected).expect("disconnect");
    let s = get_share(id1).expect("get 3");
    assert_eq!(s.state, MountState::Disconnected);
    crate::serial_println!("  [7/11] connection state: OK");

    // 8: Auto-mount list.
    let auto = auto_mount_shares();
    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].id, id1);
    crate::serial_println!("  [8/11] auto-mount: OK");

    // 9: Unmount.
    unmount(id2).expect("unmount");
    assert_eq!(list_shares().len(), 1);
    crate::serial_println!("  [9/11] unmount: OK");

    // 10: Error tracking.
    set_state(id1, MountState::Error).expect("error");
    let (_, _, _, errors, _) = stats();
    assert!(errors >= 1);
    crate::serial_println!("  [10/11] error tracking: OK");

    // 11: Stats.
    let (count, connected, mounts, errors, ops) = stats();
    assert_eq!(count, 1);
    assert!(mounts >= 2);
    assert!(ops > 0);
    let _ = (connected, errors);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("netshare::self_test() — all 11 tests passed");
}
