//! Cloud sync — cloud storage synchronization service.
//!
//! Manages bidirectional file synchronization with cloud providers,
//! handling conflict resolution, selective sync, bandwidth throttling,
//! and offline access.
//!
//! ## Architecture
//!
//! ```text
//! File change notification
//!   → cloudsync::on_local_change(path) → queue upload
//!
//! Timer / network event
//!   → cloudsync::poll_remote() → download changes
//!
//! Settings panel → Cloud Accounts
//!   → cloudsync::add_provider() / configure()
//!
//! Integration:
//!   → changetrack (local file change detection)
//!   → netsettings (network availability)
//!   → datausage (bandwidth tracking)
//!   → notifcenter (sync conflict notifications)
//!   → fileversion (version before overwrite)
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

/// Cloud provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    GenericWebDav,
    NextCloud,
    Dropbox,
    GoogleDrive,
    OneDrive,
    S3Compatible,
}

impl CloudProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::GenericWebDav => "WebDAV",
            Self::NextCloud => "NextCloud",
            Self::Dropbox => "Dropbox",
            Self::GoogleDrive => "Google Drive",
            Self::OneDrive => "OneDrive",
            Self::S3Compatible => "S3",
        }
    }
}

/// Sync state for a file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Synced,
    Uploading,
    Downloading,
    Pending,
    Conflict,
    Error,
    Excluded,
    OnlineOnly,
}

impl SyncState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Uploading => "uploading",
            Self::Downloading => "downloading",
            Self::Pending => "pending",
            Self::Conflict => "conflict",
            Self::Error => "error",
            Self::Excluded => "excluded",
            Self::OnlineOnly => "online-only",
        }
    }
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Keep both (rename the conflicting file).
    KeepBoth,
    /// Local wins.
    LocalWins,
    /// Remote wins.
    RemoteWins,
    /// Ask user.
    Ask,
}

impl ConflictStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::KeepBoth => "Keep Both",
            Self::LocalWins => "Local Wins",
            Self::RemoteWins => "Remote Wins",
            Self::Ask => "Ask",
        }
    }
}

/// A sync account.
#[derive(Debug, Clone)]
pub struct SyncAccount {
    /// Account ID.
    pub id: u32,
    /// Provider type.
    pub provider: CloudProvider,
    /// Account name / email.
    pub account_name: String,
    /// Local sync folder path.
    pub local_path: PathBuf,
    /// Remote root path.
    ///
    /// Byte-typed like the local side (design-decisions.md §261): the remote
    /// root mirrors a local subtree, so any name legal in the local filesystem
    /// has to be expressible here or the mapping is not round-trippable.
    pub remote_path: PathBuf,
    /// Whether sync is enabled.
    pub enabled: bool,
    /// Conflict resolution strategy.
    pub conflict_strategy: ConflictStrategy,
    /// Bandwidth limit in KiB/s (0 = unlimited).
    pub bandwidth_limit_kbps: u32,
    /// Total bytes uploaded.
    pub bytes_uploaded: u64,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
    /// Files synced.
    pub files_synced: u64,
    /// Current conflicts.
    pub conflicts: u32,
    /// Last sync timestamp (ns).
    pub last_sync_ns: u64,
}

/// A sync conflict entry.
#[derive(Debug, Clone)]
pub struct SyncConflict {
    pub account_id: u32,
    pub path: PathBuf,
    pub local_modified_ns: u64,
    pub remote_modified_ns: u64,
    pub local_size: u64,
    pub remote_size: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ACCOUNTS: usize = 10;
const MAX_CONFLICTS: usize = 100;
const MAX_EXCLUDED: usize = 200;

struct State {
    accounts: Vec<SyncAccount>,
    conflicts: Vec<SyncConflict>,
    /// Exclude globs, held as raw bytes rather than `String`.
    ///
    /// A pattern names files, so it has to be able to spell anything a name
    /// can spell -- `bu\xFFild/` is a legal directory and therefore a legal
    /// thing to exclude (design-decisions.md §261).
    excluded_patterns: Vec<Vec<u8>>,
    next_id: u32,
    total_syncs: u64,
    total_conflicts: u64,
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

    let excluded = alloc::vec![
        b"*.tmp".to_vec(),
        b"*.swp".to_vec(),
        b".git/".to_vec(),
        b"node_modules/".to_vec(),
        b"*.o".to_vec(),
        b"thumbs.db".to_vec(),
    ];

    *guard = Some(State {
        accounts: Vec::new(),
        conflicts: Vec::new(),
        excluded_patterns: excluded,
        next_id: 1,
        total_syncs: 0,
        total_conflicts: 0,
        ops: 0,
    });
}

/// Add a cloud sync account.
pub fn add_account(
    provider: CloudProvider,
    account_name: &str,
    local_path: impl AsRef<Path>,
    remote_path: impl AsRef<Path>,
) -> KernelResult<u32> {
    let (local_path, remote_path) = (local_path.as_ref(), remote_path.as_ref());
    with_state(|state| {
        if state.accounts.len() >= MAX_ACCOUNTS {
            return Err(KernelError::ResourceExhausted);
        }
        if state
            .accounts
            .iter()
            .any(|a| a.account_name == account_name && a.provider == provider)
        {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.accounts.push(SyncAccount {
            id,
            provider,
            account_name: String::from(account_name),
            local_path: local_path.to_path_buf(),
            remote_path: remote_path.to_path_buf(),
            enabled: true,
            conflict_strategy: ConflictStrategy::KeepBoth,
            bandwidth_limit_kbps: 0,
            bytes_uploaded: 0,
            bytes_downloaded: 0,
            files_synced: 0,
            conflicts: 0,
            last_sync_ns: 0,
        });
        Ok(id)
    })
}

/// Remove a sync account.
pub fn remove_account(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state
            .accounts
            .iter()
            .position(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        state.accounts.remove(pos);
        state.conflicts.retain(|c| c.account_id != id);
        Ok(())
    })
}

/// Enable/disable sync for an account.
pub fn set_account_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let acct = state
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        acct.enabled = enabled;
        Ok(())
    })
}

/// Set conflict resolution strategy.
pub fn set_conflict_strategy(id: u32, strategy: ConflictStrategy) -> KernelResult<()> {
    with_state(|state| {
        let acct = state
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        acct.conflict_strategy = strategy;
        Ok(())
    })
}

/// Set bandwidth limit.
pub fn set_bandwidth_limit(id: u32, kbps: u32) -> KernelResult<()> {
    with_state(|state| {
        let acct = state
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        acct.bandwidth_limit_kbps = kbps;
        Ok(())
    })
}

/// Record a sync event (for simulation/tracking).
pub fn record_sync(id: u32, uploaded: u64, downloaded: u64, files: u64) -> KernelResult<()> {
    with_state(|state| {
        let acct = state
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        acct.bytes_uploaded += uploaded;
        acct.bytes_downloaded += downloaded;
        acct.files_synced += files;
        acct.last_sync_ns = crate::hpet::elapsed_ns();
        state.total_syncs += 1;
        Ok(())
    })
}

/// Report a sync conflict.
pub fn report_conflict(
    account_id: u32,
    path: impl AsRef<Path>,
    local_size: u64,
    remote_size: u64,
) -> KernelResult<()> {
    let path = path.as_ref();
    with_state(|state| {
        if state.conflicts.len() >= MAX_CONFLICTS {
            state.conflicts.remove(0);
        }
        let now = crate::hpet::elapsed_ns();
        state.conflicts.push(SyncConflict {
            account_id,
            path: path.to_path_buf(),
            local_modified_ns: now,
            remote_modified_ns: now,
            local_size,
            remote_size,
        });
        if let Some(acct) = state.accounts.iter_mut().find(|a| a.id == account_id) {
            acct.conflicts += 1;
        }
        state.total_conflicts += 1;
        Ok(())
    })
}

/// Add an exclude pattern.
pub fn add_exclude(pattern: impl AsRef<[u8]>) -> KernelResult<()> {
    let pattern = pattern.as_ref();
    with_state(|state| {
        if state.excluded_patterns.len() >= MAX_EXCLUDED {
            return Err(KernelError::ResourceExhausted);
        }
        if !state.excluded_patterns.iter().any(|p| p == pattern) {
            state.excluded_patterns.push(pattern.to_vec());
        }
        Ok(())
    })
}

/// Remove an exclude pattern.
pub fn remove_exclude(pattern: impl AsRef<[u8]>) -> KernelResult<()> {
    let pattern = pattern.as_ref();
    with_state(|state| {
        let pos = state
            .excluded_patterns
            .iter()
            .position(|p| p == pattern)
            .ok_or(KernelError::NotFound)?;
        state.excluded_patterns.remove(pos);
        Ok(())
    })
}

/// List accounts.
pub fn list_accounts() -> Vec<SyncAccount> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.accounts.clone())
}

/// List conflicts.
pub fn list_conflicts() -> Vec<SyncConflict> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.conflicts.clone())
}

/// List exclude patterns.
pub fn list_excludes() -> Vec<Vec<u8>> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.excluded_patterns.clone())
}

/// Get account info.
pub fn get_account(id: u32) -> KernelResult<SyncAccount> {
    with_state(|state| {
        state
            .accounts
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Statistics: (account_count, total_syncs, total_conflicts, active_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active = s.accounts.iter().filter(|a| a.enabled).count();
            (
                s.accounts.len(),
                s.total_syncs,
                s.total_conflicts,
                active,
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
    crate::serial_println!("cloudsync::self_test() — running tests...");
    init_defaults();

    // A pattern no user would plausibly have configured, so test 8 can add it
    // and take it away again without touching a real exclusion.
    const TEST_EXCLUDE: &[u8] = b"*.cloudsync-selftest.bak";

    // 1: Baseline.
    //
    // Deliberately *not* `assert!(list_accounts().is_empty())`.  This suite is
    // reachable from the `cloudsync test` shell command, so a user can run it
    // twice -- and the previous version left account `id1`, its conflict and
    // an extra exclude behind, so the second run panicked the kernel on this
    // very line.  (`init_defaults` early-returns once state exists, so it
    // cannot restore the fresh-boot condition the old assertion assumed.)
    // Everything below is stated relative to what the module already held,
    // and the tail restores it exactly.
    let (base_accts, base_syncs, base_conflicts, base_active, _) = stats();
    let base_conflict_rows = list_conflicts().len();
    let base_excludes = list_excludes().len();
    if base_accts + 2 > MAX_ACCOUNTS {
        // Refuse to evict a user's configuration in order to test.  A skipped
        // suite is a visible gap; a suite that deletes cloud accounts to make
        // room is a data-loss bug wearing a test's clothes.
        crate::serial_println!(
            "cloudsync::self_test() — skipped: {}/{} account slots in use, suite needs 2 free",
            base_accts,
            MAX_ACCOUNTS
        );
        return;
    }
    assert_eq!(list_accounts().len(), base_accts);
    crate::serial_println!("  [1/12] baseline ({} accounts): OK", base_accts);

    // 2: Add account.
    //
    // Account names are in the reserved `.invalid` TLD and carry a `selftest`
    // marker: `add_account` rejects a duplicate (provider, account_name), so a
    // plausible-looking fixture such as `user@cloud.example` risks colliding
    // with something the user really configured and failing here.
    let id1 = add_account(
        CloudProvider::NextCloud,
        "cloudsync-selftest-a@invalid",
        "/home/user/sync",
        "/",
    )
    .expect("add nc");
    assert!(id1 > 0);
    crate::serial_println!("  [2/12] add account: OK");

    // 3: Add second account.
    let id2 = add_account(
        CloudProvider::Dropbox,
        "cloudsync-selftest-b@invalid",
        "/home/user/dropbox",
        "/",
    )
    .expect("add dbx");
    assert_eq!(list_accounts().len(), base_accts + 2);
    crate::serial_println!("  [3/12] multiple accounts: OK");

    // 4: Duplicate rejected.
    let r = add_account(
        CloudProvider::NextCloud,
        "cloudsync-selftest-a@invalid",
        "/dup",
        "/dup",
    );
    assert!(r.is_err());
    crate::serial_println!("  [4/12] duplicate rejected: OK");

    // 5: Record sync.
    record_sync(id1, 1024, 2048, 5).expect("record sync");
    let acct = get_account(id1).expect("get acct");
    assert_eq!(acct.bytes_uploaded, 1024);
    assert_eq!(acct.files_synced, 5);
    crate::serial_println!("  [5/12] record sync: OK");

    // 6: Report conflict.
    report_conflict(id1, "/docs/readme.txt", 100, 200).expect("conflict");
    assert_eq!(list_conflicts().len(), base_conflict_rows + 1);
    crate::serial_println!("  [6/12] report conflict: OK");

    // 7: Set conflict strategy.
    set_conflict_strategy(id1, ConflictStrategy::LocalWins).expect("set strategy");
    let acct = get_account(id1).expect("get acct 2");
    assert_eq!(acct.conflict_strategy, ConflictStrategy::LocalWins);
    crate::serial_println!("  [7/12] conflict strategy: OK");

    // 8: Exclude patterns.
    //
    // The old version asserted `list_excludes().len() >= 6` -- the count of the
    // defaults.  That is a user-mutable set (`remove_exclude` is a public API and
    // a shell command), so it is not a property of this module; the property that
    // *is* one is that a pattern added is visible and a pattern removed is gone.
    // `*.bak` was also a poor choice of fixture: a user may well have it already,
    // in which case the old `len() > excludes.len()` never held.
    assert!(
        !list_excludes().iter().any(|e| e == TEST_EXCLUDE),
        "TEST_EXCLUDE must not already be configured"
    );
    add_exclude(TEST_EXCLUDE).expect("add exclude");
    assert_eq!(list_excludes().len(), base_excludes + 1);
    assert!(list_excludes().iter().any(|e| e == TEST_EXCLUDE));
    crate::serial_println!("  [8/12] excludes: OK");

    // 9: Disable account.
    set_account_enabled(id2, false).expect("disable");
    let acct = get_account(id2).expect("get acct 3");
    assert!(!acct.enabled);
    crate::serial_println!("  [9/12] disable account: OK");

    // 10: Remove account.
    remove_account(id2).expect("remove");
    assert_eq!(list_accounts().len(), base_accts + 1);
    crate::serial_println!("  [10/12] remove account: OK");

    // 11: Stats.
    //
    // `total_syncs`/`total_conflicts` are lifetime counters with no reset, so
    // they can only be asserted as having advanced by at least what this suite
    // contributed.  `active` is a live count and is exact.
    let (accts, syncs, conflicts, active, ops) = stats();
    assert_eq!(accts, base_accts + 1);
    assert!(syncs > base_syncs);
    assert!(conflicts > base_conflicts);
    assert_eq!(active, base_active + 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/12] stats: OK");

    // 12: Non-UTF-8 paths (design-decisions.md §261).
    //
    // `\xFF` and `\xFE` are both invalid as the first byte of a UTF-8 sequence,
    // so under the old `String` typing both folded to U+FFFD and the two sync
    // folders below became the same key.  That is not a cosmetic bug for this
    // module: two accounts sharing one local path means the conflict rows of
    // one are attributed to the other, and the exclude list would silently
    // apply a pattern to a directory it was never written for.
    let dir_a = Path::new(&b"/home/user/cs_\xFFsync"[..]);
    let dir_b = Path::new(&b"/home/user/cs_\xFEsync"[..]);
    let rem_a = Path::new(&b"/remote/cs_\xFFroot"[..]);
    let id3 = add_account(
        CloudProvider::S3Compatible,
        "cloudsync-selftest-c@invalid",
        dir_a,
        rem_a,
    )
    .expect("add non-utf8 acct");
    let a3 = get_account(id3).expect("get non-utf8 acct");
    assert_eq!(a3.local_path.as_path(), dir_a);
    assert_eq!(
        a3.local_path.as_path().as_bytes(),
        b"/home/user/cs_\xFFsync"
    );
    assert_eq!(a3.remote_path.as_path(), rem_a);
    assert_ne!(
        a3.local_path.as_path(),
        dir_b,
        "\\xFF must not fold to \\xFE"
    );

    // A conflict path is the thing the user is shown and asked to resolve, so
    // it has to come back byte-for-byte or they act on the wrong file.
    let conflict_path = Path::new(&b"/home/user/cs_\xFFsync/re\xFEport.txt"[..]);
    report_conflict(id3, conflict_path, 11, 22).expect("non-utf8 conflict");
    let c3 = list_conflicts();
    let row = c3
        .iter()
        .find(|c| c.account_id == id3)
        .expect("non-utf8 conflict row");
    assert_eq!(row.path.as_path(), conflict_path);
    assert_eq!(
        row.path.as_path().as_bytes(),
        b"/home/user/cs_\xFFsync/re\xFEport.txt"
    );

    // Exclude patterns name files, so they are byte-typed too.
    let pat = &b"*.cloudsync-selftest-\xFF.bak"[..];
    add_exclude(pat).expect("add non-utf8 exclude");
    assert!(list_excludes().iter().any(|e| e == pat));
    assert!(
        !list_excludes()
            .iter()
            .any(|e| e == &b"*.cloudsync-selftest-\xFE.bak"[..]),
        "a \\xFE pattern must not match the \\xFF one that was added"
    );
    remove_exclude(pat).expect("remove non-utf8 exclude");
    assert!(!list_excludes().iter().any(|e| e == pat));

    remove_account(id3).expect("cleanup: remove id3");
    crate::serial_println!("  [12/12] non-UTF-8 sync paths: OK");

    // Cleanup: put the sync registry back exactly as it was found, so a second
    // `cloudsync test` sees the same baseline this one did.  `remove_account`
    // also drops that account's conflict rows, which is what restores the
    // conflict list.
    remove_account(id1).expect("cleanup: remove id1");
    remove_exclude(TEST_EXCLUDE).expect("cleanup: remove exclude");
    assert_eq!(
        list_accounts().len(),
        base_accts,
        "self_test must leave the sync registry as it found it"
    );
    assert_eq!(
        list_conflicts().len(),
        base_conflict_rows,
        "self_test must leave the conflict list as it found it"
    );
    assert_eq!(
        list_excludes().len(),
        base_excludes,
        "self_test must leave the exclude list as it found it"
    );

    crate::serial_println!("cloudsync::self_test() — all 12 tests passed");
}
