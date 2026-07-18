//! Group Manager — user group management.
//!
//! Manages system groups: creation, deletion, membership, and
//! primary/supplementary group assignments. Companion to useracct.
//!
//! ## Architecture
//!
//! ```text
//! Group management
//!   → groupmgr::create(name, gid) → create group
//!   → groupmgr::add_member(gid, uid) → add user to group
//!   → groupmgr::remove_member(gid, uid) → remove user from group
//!   → groupmgr::list() → list all groups
//!
//! Integration:
//!   → useracct (user accounts)
//!   → acl (access control lists)
//!   → apppermissions (app permissions)
//!   → fileshare (file sharing)
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

/// Group type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupType {
    System,
    User,
    Service,
}

impl GroupType {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::User => "User",
            Self::Service => "Service",
        }
    }
}

/// A group entry.
#[derive(Debug, Clone)]
pub struct Group {
    pub gid: u32,
    pub name: String,
    pub group_type: GroupType,
    pub members: Vec<u32>,   // UIDs.
    pub description: String,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GROUPS: usize = 256;

struct State {
    groups: Vec<Group>,
    total_created: u64,
    total_deleted: u64,
    total_member_ops: u64,
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

/// Initialise the group manager with the standard system-group SKELETON.
///
/// The group definitions (gid, name, type, description) are a legitimate
/// compiled-in skeleton — the universal Unix system groups that every install
/// ships, analogous to a default `/etc/group`. They are configuration, not
/// observations, so they are valid defaults.
///
/// Their MEMBER lists, however, are observations of which users belong to which
/// group, and must come from the real user database — not be fabricated. The
/// previous implementation seeded `wheel` with UID 1000 and `users` with UIDs
/// 1000/1001 (UID 1001 does not exist in `useracct` at all), which `/proc` and
/// the `groupmgr` shell command surfaced as real group memberships. So every
/// group starts with an EMPTY member list; memberships are populated via
/// `add_member()` when users are actually assigned.
///
/// DEFERRED PROPER FIX: wire group membership to `useracct` so the two stay
/// consistent. NOTE (tech debt): `useracct` keeps its OWN, conflicting group
/// list (e.g. gid 1 = "users" there vs "wheel" here) — the two group databases
/// should be unified into a single source of truth.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        groups: alloc::vec![
            Group { gid: 0, name: String::from("root"), group_type: GroupType::System,
                members: Vec::new(), description: String::from("System administrators"),
                created_ns: now },
            Group { gid: 1, name: String::from("wheel"), group_type: GroupType::System,
                members: Vec::new(), description: String::from("Sudo-capable users"),
                created_ns: now },
            Group { gid: 100, name: String::from("users"), group_type: GroupType::User,
                members: Vec::new(), description: String::from("Regular users"),
                created_ns: now },
            Group { gid: 999, name: String::from("daemon"), group_type: GroupType::Service,
                members: Vec::new(), description: String::from("System daemons"),
                created_ns: now },
        ],
        total_created: 4,
        total_deleted: 0,
        total_member_ops: 0,
        ops: 0,
    });
}

/// List all groups.
pub fn list_groups() -> Vec<Group> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.groups.clone())
}

/// Get group by GID.
pub fn get_group(gid: u32) -> Option<Group> {
    STATE.lock().as_ref().and_then(|s| s.groups.iter().find(|g| g.gid == gid).cloned())
}

/// Get group by name.
pub fn get_by_name(name: &str) -> Option<Group> {
    STATE.lock().as_ref().and_then(|s| s.groups.iter().find(|g| g.name == name).cloned())
}

/// Create a new group.
pub fn create_group(gid: u32, name: &str, gtype: GroupType, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.groups.len() >= MAX_GROUPS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.groups.iter().any(|g| g.gid == gid) {
            return Err(KernelError::AlreadyExists);
        }
        if state.groups.iter().any(|g| g.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        let now = crate::hpet::elapsed_ns();
        state.groups.push(Group {
            gid, name: String::from(name), group_type: gtype,
            members: Vec::new(), description: String::from(desc),
            created_ns: now,
        });
        state.total_created += 1;
        Ok(())
    })
}

/// Delete a group.
pub fn delete_group(gid: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.groups.len();
        state.groups.retain(|g| g.gid != gid);
        if state.groups.len() == before { return Err(KernelError::NotFound); }
        state.total_deleted += 1;
        Ok(())
    })
}

/// Add a member to a group.
pub fn add_member(gid: u32, uid: u32) -> KernelResult<()> {
    with_state(|state| {
        let group = state.groups.iter_mut().find(|g| g.gid == gid)
            .ok_or(KernelError::NotFound)?;
        if group.members.contains(&uid) {
            return Err(KernelError::AlreadyExists);
        }
        group.members.push(uid);
        state.total_member_ops += 1;
        Ok(())
    })
}

/// Remove a member from a group.
pub fn remove_member(gid: u32, uid: u32) -> KernelResult<()> {
    with_state(|state| {
        let group = state.groups.iter_mut().find(|g| g.gid == gid)
            .ok_or(KernelError::NotFound)?;
        let before = group.members.len();
        group.members.retain(|&m| m != uid);
        if group.members.len() == before { return Err(KernelError::NotFound); }
        state.total_member_ops += 1;
        Ok(())
    })
}

/// Get all groups a user belongs to.
pub fn groups_for_user(uid: u32) -> Vec<Group> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.groups.iter().filter(|g| g.members.contains(&uid)).cloned().collect()
    })
}

/// Statistics: (group_count, total_created, total_deleted, total_member_ops, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.groups.len(), s.total_created, s.total_deleted, s.total_member_ops, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("groupmgr::self_test() — running tests...");

    // Residue-free: start from a known-empty state.
    *STATE.lock() = None;
    init_defaults();

    // 1: Default group SKELETON — 4 groups, all with EMPTY memberships
    // (we never fabricate which users belong to which group).
    let groups = list_groups();
    assert_eq!(groups.len(), 4);
    assert!(groups.iter().all(|g| g.members.is_empty()));
    crate::serial_println!("  [1/8] skeleton (empty members): OK");

    // 2: Get group.
    let g = get_group(0).expect("get");
    assert_eq!(g.name, "root");
    assert_eq!(g.group_type, GroupType::System);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Get by name. wheel starts empty; membership is added explicitly.
    let g = get_by_name("wheel").expect("by_name");
    assert_eq!(g.gid, 1);
    assert!(g.members.is_empty());
    add_member(1, 1000).expect("add wheel member");
    assert!(get_by_name("wheel").expect("by_name2").members.contains(&1000));
    crate::serial_println!("  [3/8] by_name: OK");

    // 4: Create group.
    create_group(500, "developers", GroupType::User, "Dev team").expect("create");
    assert_eq!(list_groups().len(), 5);
    assert!(create_group(500, "dup", GroupType::User, "").is_err());
    crate::serial_println!("  [4/8] create: OK");

    // 5: Add/remove members.
    add_member(500, 1000).expect("add");
    add_member(500, 1001).expect("add2");
    let g = get_group(500).expect("get2");
    assert_eq!(g.members.len(), 2);
    remove_member(500, 1001).expect("rm");
    let g = get_group(500).expect("get3");
    assert_eq!(g.members.len(), 1);
    crate::serial_println!("  [5/8] members: OK");

    // 6: Groups for user. UID 1000 was added to wheel (test 3) and developers
    // (test 5); no memberships are fabricated at init.
    let user_groups = groups_for_user(1000);
    assert_eq!(user_groups.len(), 2);
    crate::serial_println!("  [6/8] groups_for_user: OK");

    // 7: Delete group.
    delete_group(500).expect("delete");
    assert_eq!(list_groups().len(), 4);
    assert!(delete_group(999_999).is_err());
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, created, deleted, member_ops, ops) = stats();
    assert_eq!(count, 4);
    assert!(created >= 5);
    assert!(deleted >= 1);
    assert!(member_ops >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Residue-free: leave no fixtures behind.
    *STATE.lock() = None;

    crate::serial_println!("groupmgr::self_test() — all 8 tests passed");
}
