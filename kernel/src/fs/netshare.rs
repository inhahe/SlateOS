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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): a path on the
    /// remote server is a byte string chosen by that server's filesystem,
    /// which is under no obligation to be UTF-8 -- and less so than a local
    /// one, since we do not even control which filesystem produced it.
    pub remote_path: PathBuf,
    /// Local mount point.
    ///
    /// A `PathBuf` for the same reason: a mount point is an ordinary local
    /// directory, whose name may contain any byte but `/` and NUL.
    pub mount_point: PathBuf,
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
    if guard.is_some() {
        return;
    }
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
    protocol: ShareProtocol,
    host: &str,
    remote_path: impl AsRef<Path>,
    mount_point: impl AsRef<Path>,
    username: &str,
    auto_mount: bool,
    read_only: bool,
) -> KernelResult<u32> {
    let (remote_path, mount_point) = (remote_path.as_ref(), mount_point.as_ref());
    with_state(|state| {
        if state.shares.len() >= MAX_SHARES {
            return Err(KernelError::ResourceExhausted);
        }
        if state
            .shares
            .iter()
            .any(|s| s.mount_point.as_path() == mount_point)
        {
            return Err(KernelError::AlreadyExists);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.shares.push(NetShare {
            id,
            protocol,
            host: String::from(host),
            remote_path: remote_path.to_path_buf(),
            mount_point: mount_point.to_path_buf(),
            username: String::from(username),
            state: MountState::Connected,
            auto_mount,
            read_only,
            bytes_read: 0,
            bytes_written: 0,
            mounted_ns: crate::hpet::elapsed_ns(),
        });
        state.total_mounts += 1;
        Ok(id)
    })
}

/// Unmount a network share.
pub fn unmount(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state
            .shares
            .iter()
            .position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.shares.remove(pos);
        state.total_unmounts += 1;
        Ok(())
    })
}

/// Set share connection state.
pub fn set_state(id: u32, new_state: MountState) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .shares
            .iter_mut()
            .find(|s| s.id == id)
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
        let share = state
            .shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.bytes_read += bytes_read;
        share.bytes_written += bytes_written;
        Ok(())
    })
}

/// Set auto-mount.
pub fn set_auto_mount(id: u32, auto_mount: bool) -> KernelResult<()> {
    with_state(|state| {
        let share = state
            .shares
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        share.auto_mount = auto_mount;
        Ok(())
    })
}

/// List all shares.
pub fn list_shares() -> Vec<NetShare> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.shares.clone())
}

/// Get share by ID.
pub fn get_share(id: u32) -> KernelResult<NetShare> {
    with_state(|state| {
        state
            .shares
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List auto-mount shares.
pub fn auto_mount_shares() -> Vec<NetShare> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.shares
            .iter()
            .filter(|sh| sh.auto_mount)
            .cloned()
            .collect()
    })
}

/// Statistics: (share_count, connected_count, total_mounts, total_errors, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let connected = s
                .shares
                .iter()
                .filter(|sh| sh.state == MountState::Connected)
                .count();
            (
                s.shares.len(),
                connected,
                s.total_mounts,
                s.total_errors,
                s.ops,
            )
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

    // 1: Baseline.
    //
    // Deliberately *not* `assert!(list_shares().is_empty())`.  This suite is
    // reachable from the `netshare selftest` shell command, so a user can run
    // it twice -- and the previous version left share `id1` mounted, so the
    // second run panicked the kernel on this very line.  Every count below is
    // relative to this baseline, and the cleanup at the end restores it
    // exactly, which makes the suite re-runnable and safe to wire into boot.
    let baseline = list_shares().len();
    crate::serial_println!("  [1/12] baseline ({} shares): OK", baseline);

    // 2: Mount SMB share.
    let id1 = mount(
        ShareProtocol::Smb3,
        "fileserver.local",
        "/share/docs",
        "/mnt/docs",
        "user",
        true,
        false,
    )
    .expect("mount smb");
    assert!(id1 > 0);
    crate::serial_println!("  [2/12] mount SMB: OK");

    // 3: Mount NFS share.
    let id2 = mount(
        ShareProtocol::Nfs4,
        "nfs.local",
        "/export/data",
        "/mnt/data",
        "root",
        false,
        true,
    )
    .expect("mount nfs");
    assert_eq!(list_shares().len(), baseline + 2);
    crate::serial_println!("  [3/12] mount NFS: OK");

    // 4: Duplicate mount point rejected.
    let r = mount(
        ShareProtocol::Smb2,
        "other",
        "/other",
        "/mnt/docs",
        "user",
        false,
        false,
    );
    assert!(r.is_err());
    crate::serial_println!("  [4/12] duplicate rejected: OK");

    // 5: Get share info.
    let s = get_share(id1).expect("get share");
    assert_eq!(s.protocol, ShareProtocol::Smb3);
    assert_eq!(s.state, MountState::Connected);
    crate::serial_println!("  [5/12] share info: OK");

    // 6: Record I/O.
    record_io(id1, 1024, 512).expect("io");
    let s = get_share(id1).expect("get 2");
    assert_eq!(s.bytes_read, 1024);
    crate::serial_println!("  [6/12] record I/O: OK");

    // 7: Connection state.
    set_state(id1, MountState::Disconnected).expect("disconnect");
    let s = get_share(id1).expect("get 3");
    assert_eq!(s.state, MountState::Disconnected);
    crate::serial_println!("  [7/12] connection state: OK");

    // 8: Auto-mount list.
    //
    // Stated as membership rather than `len() == 1` so a pre-existing
    // auto-mount share in the baseline does not fail the suite: what is
    // being tested is that the flag selects, not how many shares exist.
    let auto = auto_mount_shares();
    assert!(auto.iter().any(|s| s.id == id1));
    assert!(!auto.iter().any(|s| s.id == id2));
    crate::serial_println!("  [8/12] auto-mount: OK");

    // 9: Unmount.
    unmount(id2).expect("unmount");
    assert_eq!(list_shares().len(), baseline + 1);
    crate::serial_println!("  [9/12] unmount: OK");

    // 10: Error tracking.
    set_state(id1, MountState::Error).expect("error");
    let (_, _, _, errors, _) = stats();
    assert!(errors >= 1);
    crate::serial_println!("  [10/12] error tracking: OK");

    // 11: Stats.
    let (count, connected, mounts, errors, ops) = stats();
    assert_eq!(count, baseline + 1);
    assert!(mounts >= 2);
    assert!(ops > 0);
    let _ = (connected, errors);
    crate::serial_println!("  [11/12] stats: OK");

    // 12: non-UTF-8 paths (design-decisions.md 261).
    //
    // The mount point is an ordinary local directory and the remote path
    // comes from a server whose filesystem we do not control, so neither can
    // be required to be UTF-8.  While both were `String`, two shares whose
    // mount points differed only in a byte with no UTF-8 spelling collided
    // on the duplicate check -- the second mount was refused as a duplicate
    // of a directory it had nothing to do with.
    {
        let remote_a = Path::new(&b"/export/ns_\xFFr"[..]);
        let mp_a = Path::new(&b"/mnt/ns_\xFFm"[..]);
        let mp_b = Path::new(&b"/mnt/ns_\xFEm"[..]);

        let ida = mount(
            ShareProtocol::Smb3,
            "nu.local",
            remote_a,
            mp_a,
            "user",
            false,
            false,
        )
        .expect("mount non-utf8 a");
        // A near-identical mount point is a *different* directory, so this
        // must be accepted, not rejected as a duplicate.
        let idb = mount(
            ShareProtocol::Smb3,
            "nu.local",
            remote_a,
            mp_b,
            "user",
            false,
            false,
        )
        .expect("mount non-utf8 b");

        // Re-using one of them is still a duplicate.
        assert!(
            mount(
                ShareProtocol::Smb3,
                "nu.local",
                remote_a,
                mp_a,
                "user",
                false,
                false,
            )
            .is_err()
        );

        // Both paths round-trip byte-exactly.
        let sa = get_share(ida).expect("get non-utf8 a");
        assert_eq!(sa.mount_point.as_path(), mp_a);
        assert_eq!(sa.remote_path.as_path(), remote_a);
        let sb = get_share(idb).expect("get non-utf8 b");
        assert_eq!(sb.mount_point.as_path(), mp_b);

        unmount(ida).expect("unmount non-utf8 a");
        unmount(idb).expect("unmount non-utf8 b");
        crate::serial_println!("  [12/12] non-UTF-8 paths: OK");
    }

    // Cleanup: restore the baseline exactly.
    //
    // `id1` is the only share the suite still holds -- it is deliberately
    // left mounted through tests 7-11 because they inspect its state.  It
    // must go now: leaving it behind is what made the previous version of
    // this suite panic on its second run.
    unmount(id1).expect("cleanup: unmount id1");
    assert_eq!(
        list_shares().len(),
        baseline,
        "self_test must leave the share registry as it found it"
    );

    crate::serial_println!("netshare::self_test() — all 12 tests passed");
}
