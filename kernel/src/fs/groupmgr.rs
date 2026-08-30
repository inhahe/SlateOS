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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Group type.
///
/// `System` is a **policy**, not a display label. A `System` group is one of
/// the identities the running system is defined in terms of — `root`, `wheel` —
/// and the two rules below are what make the variant mean that:
///
/// 1. `delete_group` refuses a `System` group (`PermissionDenied`).
/// 2. `create_group` refuses to *mint* one (`PermissionDenied`); the `System`
///    set is exactly what `init_defaults` seeds, and is closed thereafter.
///
/// Rule 2 exists because rule 1 alone would be a slot leak: an undeletable
/// group anybody may create is an undeletable group anybody may create 252 of,
/// and `MAX_GROUPS` is 256. The two rules are one decision — "the system's own
/// identity set is fixed at startup" — and neither half stands without the
/// other. See known-issues.md ->
/// `A-GROUPMGR-DELETE-HAS-NO-GUARD-AND-GROUPTYPE-SYSTEM-PROTECTS-NOTHING`.
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
    pub members: Vec<u32>, // UIDs.
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
    if guard.is_some() {
        return;
    }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        groups: alloc::vec![
            Group {
                gid: 0,
                name: String::from("root"),
                group_type: GroupType::System,
                members: Vec::new(),
                description: String::from("System administrators"),
                created_ns: now
            },
            Group {
                gid: 1,
                name: String::from("wheel"),
                group_type: GroupType::System,
                members: Vec::new(),
                description: String::from("Sudo-capable users"),
                created_ns: now
            },
            Group {
                gid: 100,
                name: String::from("users"),
                group_type: GroupType::User,
                members: Vec::new(),
                description: String::from("Regular users"),
                created_ns: now
            },
            Group {
                gid: 999,
                name: String::from("daemon"),
                group_type: GroupType::Service,
                members: Vec::new(),
                description: String::from("System daemons"),
                created_ns: now
            },
        ],
        total_created: 4,
        total_deleted: 0,
        total_member_ops: 0,
        ops: 0,
    });
}

/// List all groups.
pub fn list_groups() -> Vec<Group> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.groups.clone())
}

/// Get group by GID.
pub fn get_group(gid: u32) -> Option<Group> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.groups.iter().find(|g| g.gid == gid).cloned())
}

/// Get group by name.
pub fn get_by_name(name: &str) -> Option<Group> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.groups.iter().find(|g| g.name == name).cloned())
}

/// Create a new group.
///
/// # Errors
///
/// `PermissionDenied` if `gtype` is [`GroupType::System`] — see the note on
/// that variant. `ResourceExhausted` at `MAX_GROUPS`, `AlreadyExists` if the
/// GID or the name is taken.
pub fn create_group(gid: u32, name: &str, gtype: GroupType, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        // Checked before the table is consulted: refusing to mint a `System`
        // group is a statement about the request, not about the table's
        // contents, and reporting `ResourceExhausted` for a full table would
        // suggest the request would have been honoured on an emptier one.
        if gtype == GroupType::System {
            return Err(KernelError::PermissionDenied);
        }
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
            gid,
            name: String::from(name),
            group_type: gtype,
            members: Vec::new(),
            description: String::from(desc),
            created_ns: now,
        });
        state.total_created += 1;
        Ok(())
    })
}

/// Delete a group.
///
/// # Errors
///
/// `NotFound` if no group holds `gid`; `PermissionDenied` if it is a
/// [`GroupType::System`] group — see the note on that variant.
pub fn delete_group(gid: u32) -> KernelResult<()> {
    with_state(|state| {
        // Look the group up *before* removing it. The previous shape was a
        // bare `retain` followed by a length comparison, which cannot consult
        // the entry it just dropped: by the time the outcome is known the
        // evidence needed to judge the request is gone. Deciding first, then
        // acting, is also what makes the refusal total — there is no window in
        // which `root` is removed and then put back.
        let group = state
            .groups
            .iter()
            .find(|g| g.gid == gid)
            .ok_or(KernelError::NotFound)?;
        if group.group_type == GroupType::System {
            return Err(KernelError::PermissionDenied);
        }
        state.groups.retain(|g| g.gid != gid);
        state.total_deleted += 1;
        Ok(())
    })
}

/// Add a member to a group.
pub fn add_member(gid: u32, uid: u32) -> KernelResult<()> {
    with_state(|state| {
        let group = state
            .groups
            .iter_mut()
            .find(|g| g.gid == gid)
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
        let group = state
            .groups
            .iter_mut()
            .find(|g| g.gid == gid)
            .ok_or(KernelError::NotFound)?;
        let before = group.members.len();
        group.members.retain(|&m| m != uid);
        if group.members.len() == before {
            return Err(KernelError::NotFound);
        }
        state.total_member_ops += 1;
        Ok(())
    })
}

/// Get all groups a user belongs to.
pub fn groups_for_user(uid: u32) -> Vec<Group> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.groups
            .iter()
            .filter(|g| g.members.contains(&uid))
            .cloned()
            .collect()
    })
}

/// Statistics: (group_count, total_created, total_deleted, total_member_ops, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.groups.len(),
            s.total_created,
            s.total_deleted,
            s.total_member_ops,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("groupmgr::self_test() — running tests...");

    // Residue-free: start from a known-empty state.
    *STATE.lock() = None;
    init_defaults();

    // 1: Default group SKELETON — 4 groups, all with EMPTY memberships
    // (we never fabricate which users belong to which group).
    let groups = list_groups();
    assert_eq!(groups.len(), 4);
    assert!(groups.iter().all(|g| g.members.is_empty()));
    crate::serial_println!("  [1/10] skeleton (empty members): OK");

    // 2: Get group.
    let g = get_group(0).expect("get");
    assert_eq!(g.name, "root");
    assert_eq!(g.group_type, GroupType::System);
    crate::serial_println!("  [2/10] get: OK");

    // 3: Get by name. wheel starts empty; membership is added explicitly.
    let g = get_by_name("wheel").expect("by_name");
    assert_eq!(g.gid, 1);
    assert!(g.members.is_empty());
    add_member(1, 1000).expect("add wheel member");
    assert!(
        get_by_name("wheel")
            .expect("by_name2")
            .members
            .contains(&1000)
    );
    crate::serial_println!("  [3/10] by_name: OK");

    // 4: Create group.
    create_group(500, "developers", GroupType::User, "Dev team").expect("create");
    assert_eq!(list_groups().len(), 5);
    assert!(create_group(500, "dup", GroupType::User, "").is_err());
    crate::serial_println!("  [4/10] create: OK");

    // 5: Add/remove members.
    add_member(500, 1000).expect("add");
    add_member(500, 1001).expect("add2");
    let g = get_group(500).expect("get2");
    assert_eq!(g.members.len(), 2);
    remove_member(500, 1001).expect("rm");
    let g = get_group(500).expect("get3");
    assert_eq!(g.members.len(), 1);
    crate::serial_println!("  [5/10] members: OK");

    // 6: Groups for user. UID 1000 was added to wheel (test 3) and developers
    // (test 5); no memberships are fabricated at init.
    let user_groups = groups_for_user(1000);
    assert_eq!(user_groups.len(), 2);
    crate::serial_println!("  [6/10] groups_for_user: OK");

    // 7: Delete group.
    delete_group(500).expect("delete");
    assert_eq!(list_groups().len(), 4);
    assert!(delete_group(999_999).is_err());
    crate::serial_println!("  [7/10] delete: OK");

    // 8: A System group cannot be deleted -- and the refusal is by TYPE, not by
    // GID, so it covers `wheel` at GID 1 exactly as it covers `root` at GID 0.
    // Both halves are asserted: an implementation that returns an error and
    // deletes the group anyway passes the first assertion alone.
    assert_eq!(delete_group(0), Err(KernelError::PermissionDenied));
    assert_eq!(delete_group(1), Err(KernelError::PermissionDenied));
    assert!(get_group(0).is_some(), "root survived its refused deletion");
    assert!(
        get_group(1).is_some(),
        "wheel survived its refused deletion"
    );
    // A non-System group at a low GID is still deletable: the guard reads the
    // type, and nothing here is protecting small numbers.
    assert_eq!(get_group(100).map(|g| g.group_type), Some(GroupType::User));
    delete_group(100).expect("a User group is deletable whatever its GID");
    create_group(100, "users", GroupType::User, "Regular users").expect("restore");
    crate::serial_println!("  [8/10] system groups are undeletable: OK");

    // 9: ...and a System group cannot be minted either, which is what stops the
    // rule above from being a way to fill the table with 252 immortal groups.
    assert_eq!(
        create_group(4242, "zzsys", GroupType::System, ""),
        Err(KernelError::PermissionDenied)
    );
    assert!(
        get_group(4242).is_none(),
        "the refused System group was not created"
    );
    // The same request as a Service group is fine -- it is the label that is
    // reserved, not the GID or the name.
    create_group(4242, "zzsys", GroupType::Service, "").expect("Service is not reserved");
    delete_group(4242).expect("and what may be created may be deleted");
    crate::serial_println!("  [9/10] system groups cannot be minted: OK");

    // 10: Stats.
    let (count, created, deleted, member_ops, ops) = stats();
    assert_eq!(count, 4);
    assert!(created >= 5);
    assert!(deleted >= 1);
    assert!(member_ops >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    // Leave the table EMPTY, not DEAD: clear the fixtures, then re-open it.
    // Clearing alone would switch this module off for the rest of the boot
    // -- `init_defaults` runs once, that once is here, and every later write
    // would take the `NotSupported` arm and be dropped by a caller that must
    // not let statistics fail a real operation.  known-issues.md:
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT.
    *STATE.lock() = None;
    init_defaults();

    crate::serial_println!("groupmgr::self_test() — all 10 tests passed");
}
