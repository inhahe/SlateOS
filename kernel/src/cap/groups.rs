//! Named capability groups — role-based access control.
//!
//! Capability groups provide a way to bundle related capabilities under
//! a named role.  Users and OS groups can be assigned to capability groups,
//! giving them the combined set of capabilities.
//!
//! ## Design (from design.txt)
//!
//! - **Named groups**: e.g., "network" grants Socket(READ|WRITE),
//!   "filesystem" grants File(READ|WRITE|CREATE|DELETE).
//! - **AND-composition between groups**: if a resource requires groups
//!   A and B, the user must belong to both.
//! - **OR within a group**: having membership in the group satisfies
//!   all capabilities bundled in it.
//! - **Delegation constraint**: a user can't grant a capability group
//!   they don't belong to.
//!
//! ## Architecture
//!
//! A global table of up to 32 named capability groups.  Each group has:
//! - A unique name (max 31 characters).
//! - A unique group ID.
//! - A set of capability grants: (ResourceType, Rights) pairs.
//! - A set of member OS groups (gids) that belong to this cap group.
//!
//! When a process is spawned, its effective capability set is the union of:
//! 1. Capabilities explicitly delegated by the parent.
//! 2. Capabilities from capability groups its uid/gid belongs to.
//!
//! ## Well-known groups
//!
//! The kernel defines a set of built-in capability groups that cover
//! common use cases.  These are created at boot and cannot be removed.
//!
//! | Group name    | Capabilities                              |
//! |---------------|-------------------------------------------|
//! | `admin`       | All resource types, all rights             |
//! | `network`     | Socket(READ\|WRITE)                       |
//! | `filesystem`  | File(READ\|WRITE\|CREATE\|DELETE\|METADATA) |
//! | `driver`      | PortIo(READ\|WRITE), DeviceIrq(READ\|WRITE) |
//! | `process`     | Process(READ\|WRITE), Thread(READ\|WRITE) |
//! | `ipc`         | Channel+Pipe+SharedMemory+EventFd+CompletionPort(READ\|WRITE) |

use crate::sync::Mutex;

use super::rights::Rights;
use super::ResourceType;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum named capability groups.
const MAX_GROUPS: usize = 32;

/// Maximum capability grants per group.
const MAX_CAPS_PER_GROUP: usize = 16;

/// Maximum member GIDs per group.
const MAX_MEMBERS_PER_GROUP: usize = 16;

/// Maximum name length (excluding null terminator).
const MAX_NAME_LEN: usize = 31;

// ---------------------------------------------------------------------------
// Group ID
// ---------------------------------------------------------------------------

/// Unique identifier for a capability group.
pub type CapGroupId = u32;

// ---------------------------------------------------------------------------
// Capability group structure
// ---------------------------------------------------------------------------

/// A capability grant within a group: (resource type, rights).
#[derive(Debug, Clone, Copy)]
pub struct CapGrant {
    pub resource_type: ResourceType,
    pub rights: Rights,
}

/// A named capability group.
struct CapGroup {
    /// Whether this slot is active.
    active: bool,
    /// Whether this is a built-in group (cannot be removed).
    builtin: bool,
    /// Group ID (unique).
    id: CapGroupId,
    /// Human-readable name.
    name: [u8; MAX_NAME_LEN + 1],
    /// Length of the name.
    name_len: usize,
    /// Capability grants bundled in this group.
    caps: [Option<CapGrant>; MAX_CAPS_PER_GROUP],
    /// Number of active grants.
    cap_count: usize,
    /// OS group IDs (gids) that are members of this cap group.
    member_gids: [u32; MAX_MEMBERS_PER_GROUP],
    /// Number of member gids.
    member_count: usize,
}

impl CapGroup {
    const fn empty() -> Self {
        Self {
            active: false,
            builtin: false,
            id: 0,
            name: [0; MAX_NAME_LEN + 1],
            name_len: 0,
            caps: [None; MAX_CAPS_PER_GROUP],
            cap_count: 0,
            member_gids: [0; MAX_MEMBERS_PER_GROUP],
            member_count: 0,
        }
    }

    /// Get the name as a string slice.
    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }
}

// ---------------------------------------------------------------------------
// Global group table
// ---------------------------------------------------------------------------

/// Global table of capability groups.
static GROUPS: Mutex<[CapGroup; MAX_GROUPS]> = Mutex::new({
    const EMPTY: CapGroup = CapGroup::empty();
    [EMPTY; MAX_GROUPS]
});

/// Next group ID counter.
static NEXT_ID: Mutex<CapGroupId> = Mutex::new(100); // Built-ins use 1-99.

// ---------------------------------------------------------------------------
// Built-in group IDs
// ---------------------------------------------------------------------------

/// Built-in group: system administrators (all capabilities).
pub const GROUP_ADMIN: CapGroupId = 1;

/// Built-in group: network access.
pub const GROUP_NETWORK: CapGroupId = 2;

/// Built-in group: filesystem access.
pub const GROUP_FILESYSTEM: CapGroupId = 3;

/// Built-in group: device driver access.
pub const GROUP_DRIVER: CapGroupId = 4;

/// Built-in group: process management.
pub const GROUP_PROCESS: CapGroupId = 5;

/// Built-in group: IPC primitives.
pub const GROUP_IPC: CapGroupId = 6;

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize built-in capability groups.
///
/// Called once during kernel boot to create the default set of groups.
/// All built-in groups have OS group 0 (root) as a member.
pub fn init() {
    let mut groups = GROUPS.lock();

    // admin: all resource types, all rights.
    install_builtin(&mut groups, GROUP_ADMIN, b"admin", &[
        CapGrant { resource_type: ResourceType::Channel, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Pipe, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::SharedMemory, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::EventFd, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::CompletionPort, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Process, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Thread, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::PortIo, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::DeviceIrq, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::File, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Socket, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Service, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::Namespace, rights: Rights::ALL },
        CapGrant { resource_type: ResourceType::IoScheduler, rights: Rights::ALL },
    ], 0); // gid 0 = root

    // network: socket access.
    install_builtin(&mut groups, GROUP_NETWORK, b"network", &[
        CapGrant {
            resource_type: ResourceType::Socket,
            rights: Rights::READ.union(Rights::WRITE),
        },
    ], 0);

    // filesystem: file access.
    install_builtin(&mut groups, GROUP_FILESYSTEM, b"filesystem", &[
        CapGrant {
            resource_type: ResourceType::File,
            rights: Rights::READ
                .union(Rights::WRITE)
                .union(Rights::CREATE)
                .union(Rights::DELETE)
                .union(Rights::METADATA),
        },
    ], 0);

    // driver: port I/O and IRQ access.
    install_builtin(&mut groups, GROUP_DRIVER, b"driver", &[
        CapGrant {
            resource_type: ResourceType::PortIo,
            rights: Rights::READ.union(Rights::WRITE),
        },
        CapGrant {
            resource_type: ResourceType::DeviceIrq,
            rights: Rights::READ.union(Rights::WRITE),
        },
    ], 0);

    // process: process and thread management.
    install_builtin(&mut groups, GROUP_PROCESS, b"process", &[
        CapGrant {
            resource_type: ResourceType::Process,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
        CapGrant {
            resource_type: ResourceType::Thread,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
    ], 0);

    // ipc: IPC primitive access.
    install_builtin(&mut groups, GROUP_IPC, b"ipc", &[
        CapGrant {
            resource_type: ResourceType::Channel,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
        CapGrant {
            resource_type: ResourceType::Pipe,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
        CapGrant {
            resource_type: ResourceType::SharedMemory,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
        CapGrant {
            resource_type: ResourceType::EventFd,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
        CapGrant {
            resource_type: ResourceType::CompletionPort,
            rights: Rights::READ.union(Rights::WRITE).union(Rights::CREATE),
        },
    ], 0);

    serial_println!("[cap] 6 built-in capability groups initialized");
}

/// Install a built-in group.
fn install_builtin(
    groups: &mut [CapGroup; MAX_GROUPS],
    id: CapGroupId,
    name: &[u8],
    grants: &[CapGrant],
    member_gid: u32,
) {
    // Find a free slot.
    for group in groups.iter_mut() {
        if !group.active {
            group.active = true;
            group.builtin = true;
            group.id = id;
            group.name_len = name.len().min(MAX_NAME_LEN);
            group.name[..group.name_len].copy_from_slice(&name[..group.name_len]);
            group.cap_count = grants.len().min(MAX_CAPS_PER_GROUP);
            for (i, g) in grants.iter().enumerate().take(group.cap_count) {
                group.caps[i] = Some(*g);
            }
            group.member_gids[0] = member_gid;
            group.member_count = 1;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new named capability group.
///
/// Returns the group ID, or error if the table is full or name is invalid.
pub fn create(name: &str) -> KernelResult<CapGroupId> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    let mut groups = GROUPS.lock();

    // Check for duplicate name.
    for group in groups.iter() {
        if group.active && group.name_str() == name {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find a free slot.
    let slot = groups.iter().position(|g| !g.active)
        .ok_or(KernelError::OutOfMemory)?;

    let id = {
        let mut next = NEXT_ID.lock();
        let id = *next;
        *next = id.saturating_add(1);
        id
    };

    let group = &mut groups[slot];
    group.active = true;
    group.builtin = false;
    group.id = id;
    group.name_len = name.len();
    group.name[..name.len()].copy_from_slice(name.as_bytes());
    group.cap_count = 0;
    group.member_count = 0;

    serial_println!("[cap] Capability group '{}' created (id={})", name, id);
    Ok(id)
}

/// Remove a capability group by ID.
///
/// Built-in groups cannot be removed.
pub fn remove(group_id: CapGroupId) -> KernelResult<()> {
    let mut groups = GROUPS.lock();
    for group in groups.iter_mut() {
        if group.active && group.id == group_id {
            if group.builtin {
                return Err(KernelError::PermissionDenied);
            }
            group.active = false;
            return Ok(());
        }
    }
    Err(KernelError::InvalidHandle)
}

/// Add a capability grant to a group.
pub fn add_cap(group_id: CapGroupId, grant: CapGrant) -> KernelResult<()> {
    let mut groups = GROUPS.lock();
    for group in groups.iter_mut() {
        if group.active && group.id == group_id {
            if group.cap_count >= MAX_CAPS_PER_GROUP {
                return Err(KernelError::OutOfMemory);
            }
            // Find first empty slot.
            for cap in group.caps.iter_mut() {
                if cap.is_none() {
                    *cap = Some(grant);
                    group.cap_count = group.cap_count.saturating_add(1);
                    return Ok(());
                }
            }
            return Err(KernelError::OutOfMemory);
        }
    }
    Err(KernelError::InvalidHandle)
}

/// Add an OS group (gid) as a member of a capability group.
pub fn add_member(group_id: CapGroupId, gid: u32) -> KernelResult<()> {
    let mut groups = GROUPS.lock();
    for group in groups.iter_mut() {
        if group.active && group.id == group_id {
            // Check for duplicate.
            for i in 0..group.member_count {
                if group.member_gids[i] == gid {
                    return Ok(()); // Already a member.
                }
            }
            if group.member_count >= MAX_MEMBERS_PER_GROUP {
                return Err(KernelError::OutOfMemory);
            }
            group.member_gids[group.member_count] = gid;
            group.member_count = group.member_count.saturating_add(1);
            return Ok(());
        }
    }
    Err(KernelError::InvalidHandle)
}

/// Remove an OS group (gid) from a capability group.
pub fn remove_member(group_id: CapGroupId, gid: u32) -> KernelResult<()> {
    let mut groups = GROUPS.lock();
    for group in groups.iter_mut() {
        if group.active && group.id == group_id {
            for i in 0..group.member_count {
                if group.member_gids[i] == gid {
                    // Swap with last and shrink.
                    let last = group.member_count.saturating_sub(1);
                    group.member_gids[i] = group.member_gids[last];
                    group.member_count = last;
                    return Ok(());
                }
            }
            return Ok(()); // Not a member — not an error.
        }
    }
    Err(KernelError::InvalidHandle)
}

/// Check whether a process with the given gids has a specific capability
/// through group membership.
///
/// Returns `true` if any of the process's groups belongs to a capability
/// group that grants the requested (resource_type, rights).
pub fn check_access(
    uid: u32,
    primary_gid: u32,
    supplementary_gids: &[u32],
    resource_type: ResourceType,
    required_rights: Rights,
) -> bool {
    // UID 0 (root) bypasses all group checks.
    if uid == 0 {
        return true;
    }

    let groups = GROUPS.lock();
    for group in groups.iter() {
        if !group.active {
            continue;
        }

        // Check if the process is a member.
        let is_member = group.member_gids[..group.member_count]
            .iter()
            .any(|&gid| gid == primary_gid || supplementary_gids.contains(&gid));

        if !is_member {
            continue;
        }

        // Check if any grant in this group satisfies the request.
        for cap in group.caps.iter().flatten() {
            if cap.resource_type == resource_type && cap.rights.contains(required_rights) {
                return true;
            }
        }
    }

    false
}

/// Find a capability group by name.
pub fn find_by_name(name: &str) -> Option<CapGroupId> {
    let groups = GROUPS.lock();
    for group in groups.iter() {
        if group.active && group.name_str() == name {
            return Some(group.id);
        }
    }
    None
}

/// Check if a process (identified by its gids) is a member of a specific
/// capability group.
///
/// Returns `true` if any of the process's gids (primary or supplementary)
/// matches a member gid of the group (OR within the group's member list).
pub fn is_member(group_id: CapGroupId, primary_gid: u32, supplementary_gids: &[u32]) -> bool {
    let groups = GROUPS.lock();
    for group in groups.iter() {
        if group.active && group.id == group_id {
            return group.member_gids[..group.member_count]
                .iter()
                .any(|&gid| gid == primary_gid || supplementary_gids.contains(&gid));
        }
    }
    false // Group not found — treat as non-member.
}

/// Get the number of active capability groups.
pub fn count() -> usize {
    let groups = GROUPS.lock();
    groups.iter().filter(|g| g.active).count()
}

/// List all active capability groups (for kshell/procfs).
///
/// Returns a list of (id, name, cap_count, member_count, builtin).
pub fn list() -> alloc::vec::Vec<(CapGroupId, alloc::string::String, usize, usize, bool)> {
    let groups = GROUPS.lock();
    let mut result = alloc::vec::Vec::new();
    for group in groups.iter() {
        if group.active {
            result.push((
                group.id,
                alloc::string::String::from(group.name_str()),
                group.cap_count,
                group.member_count,
                group.builtin,
            ));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run capability groups self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cap/groups] Running capability groups self-test...");

    test_builtins_exist()?;
    test_create_remove()?;
    test_member_management()?;
    test_access_check()?;
    test_name_lookup()?;

    serial_println!("[cap/groups] Capability groups self-test PASSED");
    Ok(())
}

/// Test 1: built-in groups exist after init.
fn test_builtins_exist() -> KernelResult<()> {
    let c = count();
    if c < 6 {
        serial_println!("[cap/groups]   FAIL: expected >= 6 groups, got {}", c);
        return Err(KernelError::InternalError);
    }

    // Verify well-known names.
    for name in &["admin", "network", "filesystem", "driver", "process", "ipc"] {
        if find_by_name(name).is_none() {
            serial_println!("[cap/groups]   FAIL: built-in '{}' not found", name);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap/groups]   Built-in groups: OK");
    Ok(())
}

/// Test 2: create and remove a custom group.
fn test_create_remove() -> KernelResult<()> {
    let id = create("test_group")?;

    // Should be findable.
    if find_by_name("test_group").is_none() {
        serial_println!("[cap/groups]   FAIL: created group not found");
        return Err(KernelError::InternalError);
    }

    // Duplicate name rejected.
    match create("test_group") {
        Err(KernelError::AlreadyExists) => {}
        other => {
            serial_println!("[cap/groups]   FAIL: duplicate returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Remove it.
    remove(id)?;

    // Should be gone.
    if find_by_name("test_group").is_some() {
        serial_println!("[cap/groups]   FAIL: group still exists after remove");
        return Err(KernelError::InternalError);
    }

    // Built-in can't be removed.
    match remove(GROUP_ADMIN) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[cap/groups]   FAIL: admin remove returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap/groups]   Create/remove: OK");
    Ok(())
}

/// Test 3: member management.
fn test_member_management() -> KernelResult<()> {
    let id = create("test_members")?;

    // Add members.
    add_member(id, 100)?; // gid 100
    add_member(id, 200)?; // gid 200

    // Duplicate is a no-op, not an error.
    add_member(id, 100)?;

    // Remove one.
    remove_member(id, 100)?;

    // Clean up.
    remove(id)?;

    serial_println!("[cap/groups]   Member management: OK");
    Ok(())
}

/// Test 4: access check through group membership.
fn test_access_check() -> KernelResult<()> {
    let id = create("test_access")?;

    // Grant Socket(READ|WRITE) to this group.
    add_cap(id, CapGrant {
        resource_type: ResourceType::Socket,
        rights: Rights::READ.union(Rights::WRITE),
    })?;

    // Add gid 500 as member.
    add_member(id, 500)?;

    // UID=1, GID=500 → should have Socket(READ).
    if !check_access(1, 500, &[], ResourceType::Socket, Rights::READ) {
        serial_println!("[cap/groups]   FAIL: member should have Socket READ");
        remove(id)?;
        return Err(KernelError::InternalError);
    }

    // UID=1, GID=999 → should NOT have Socket(READ).
    if check_access(1, 999, &[], ResourceType::Socket, Rights::READ) {
        serial_println!("[cap/groups]   FAIL: non-member should not have access");
        remove(id)?;
        return Err(KernelError::InternalError);
    }

    // UID=1, GID=999, supplementary=[500] → should have access via supplementary.
    if !check_access(1, 999, &[500], ResourceType::Socket, Rights::READ) {
        serial_println!("[cap/groups]   FAIL: supplementary gid should grant access");
        remove(id)?;
        return Err(KernelError::InternalError);
    }

    // UID=0 always bypasses.
    if !check_access(0, 999, &[], ResourceType::Socket, Rights::READ) {
        serial_println!("[cap/groups]   FAIL: root should bypass group checks");
        remove(id)?;
        return Err(KernelError::InternalError);
    }

    remove(id)?;
    serial_println!("[cap/groups]   Access check: OK");
    Ok(())
}

/// Test 5: name lookup.
fn test_name_lookup() -> KernelResult<()> {
    // Built-in names.
    if find_by_name("admin") != Some(GROUP_ADMIN) {
        serial_println!("[cap/groups]   FAIL: admin lookup wrong");
        return Err(KernelError::InternalError);
    }
    if find_by_name("network") != Some(GROUP_NETWORK) {
        serial_println!("[cap/groups]   FAIL: network lookup wrong");
        return Err(KernelError::InternalError);
    }

    // Non-existent name.
    if find_by_name("nonexistent").is_some() {
        serial_println!("[cap/groups]   FAIL: phantom group exists");
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap/groups]   Name lookup: OK");
    Ok(())
}
