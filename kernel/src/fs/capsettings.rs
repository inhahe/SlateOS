//! Capability settings panel — manage capability groups, per-user and per-program
//! capability assignments, and per-file/directory capability requirements.
//!
//! This is the settings/configuration layer.  The kernel capability enforcement
//! engine is in a separate module; this module manages the *policy* — which
//! users, programs, and files have which capabilities.
//!
//! ## Design Reference
//!
//! design.txt line 1273: capability groups
//! design.txt line 1274: capabilities for users/programs, requirements for files/dirs
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Capabilities
//!   → capsettings::list_groups() → predefined capability groups
//!   → capsettings::user_caps(uid) → capabilities for a user
//!   → capsettings::program_caps(prog) → capabilities for a program
//!   → capsettings::path_requirements(path) → caps needed to access path
//!
//! Kernel enforcement reads these policies
//!   → capsettings::check_access(uid, program, path) → bool
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Individual capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Read files.
    FileRead,
    /// Write files.
    FileWrite,
    /// Execute programs.
    Execute,
    /// Access network.
    Network,
    /// Listen on ports < 1024.
    BindLowPort,
    /// Raw socket access.
    RawSocket,
    /// Mount/unmount filesystems.
    Mount,
    /// Create/manage users.
    UserAdmin,
    /// Install/remove programs.
    PackageInstall,
    /// Modify system configuration.
    SystemConfig,
    /// Access hardware directly.
    HardwareAccess,
    /// Debug/trace other processes.
    DebugProcess,
    /// Change file ownership.
    Chown,
    /// Override file permissions.
    DacOverride,
    /// Set system clock.
    SetClock,
    /// Reboot/shutdown.
    Reboot,
    /// Load kernel modules.
    ModuleLoad,
    /// Access audit logs.
    AuditRead,
    /// Write audit logs.
    AuditWrite,
    /// Manage capabilities of others.
    CapAdmin,
}

/// A named group of capabilities.
#[derive(Debug, Clone)]
pub struct CapGroup {
    /// Unique ID.
    pub id: u64,
    /// Group name (e.g., "Standard User", "Developer", "Admin").
    pub name: String,
    /// Description.
    pub description: String,
    /// Capabilities in this group.
    pub caps: Vec<Capability>,
    /// Whether this is a built-in group.
    pub builtin: bool,
}

/// Per-user capability assignment.
#[derive(Debug, Clone)]
pub struct UserCapAssignment {
    /// Assignment ID.
    pub id: u64,
    /// User ID.
    pub uid: u64,
    /// User name (for display).
    pub username: String,
    /// Assigned groups.
    pub groups: Vec<u64>,
    /// Additional individual capabilities.
    pub extra_caps: Vec<Capability>,
    /// Denied capabilities (override groups).
    pub denied_caps: Vec<Capability>,
}

/// Per-program capability assignment.
#[derive(Debug, Clone)]
pub struct ProgramCapAssignment {
    /// Assignment ID.
    pub id: u64,
    /// Program path or identifier.
    pub program: String,
    /// Required capabilities to run.
    pub required_caps: Vec<Capability>,
    /// Maximum capabilities the program can use.
    pub max_caps: Vec<Capability>,
    /// Whether to sandbox (drop all non-listed caps).
    pub sandboxed: bool,
}

/// Per-path capability requirement.
#[derive(Debug, Clone)]
pub struct PathRequirement {
    /// Requirement ID.
    pub id: u64,
    /// Path pattern (supports trailing `*` wildcard).
    pub path: String,
    /// Capabilities required to access this path.
    pub required_caps: Vec<Capability>,
    /// Whether to apply recursively to subdirectories.
    pub recursive: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    groups: Vec<CapGroup>,
    user_assignments: Vec<UserCapAssignment>,
    program_assignments: Vec<ProgramCapAssignment>,
    path_requirements: Vec<PathRequirement>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    groups: Vec::new(),
    user_assignments: Vec::new(),
    program_assignments: Vec::new(),
    path_requirements: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Group management
// ---------------------------------------------------------------------------

/// Create a capability group.
pub fn create_group(name: &str, description: &str, caps: &[Capability]) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.groups.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.groups.push(CapGroup {
        id,
        name: String::from(name),
        description: String::from(description),
        caps: caps.to_vec(),
        builtin: false,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a group (built-in groups cannot be removed).
pub fn remove_group(group_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let g = state.groups.iter().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    if g.builtin {
        return Err(KernelError::PermissionDenied);
    }
    state.groups.retain(|g| g.id != group_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a group.
pub fn get_group(group_id: u64) -> KernelResult<CapGroup> {
    STATE.lock().groups.iter().find(|g| g.id == group_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all groups.
pub fn list_groups() -> Vec<CapGroup> {
    STATE.lock().groups.clone()
}

/// Add a capability to a group.
pub fn group_add_cap(group_id: u64, cap: Capability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let g = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    if !g.caps.contains(&cap) {
        g.caps.push(cap);
    }
    state.changes += 1;
    Ok(())
}

/// Remove a capability from a group.
pub fn group_remove_cap(group_id: u64, cap: Capability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let g = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    g.caps.retain(|c| *c != cap);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// User assignments
// ---------------------------------------------------------------------------

/// Assign capabilities to a user.
pub fn assign_user(uid: u64, username: &str) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.user_assignments.iter().any(|a| a.uid == uid) {
        return Err(KernelError::AlreadyExists);
    }
    if state.user_assignments.len() >= 256 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.user_assignments.push(UserCapAssignment {
        id,
        uid,
        username: String::from(username),
        groups: Vec::new(),
        extra_caps: Vec::new(),
        denied_caps: Vec::new(),
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Add a user to a capability group.
pub fn user_add_group(uid: u64, group_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.groups.iter().any(|g| g.id == group_id) {
        return Err(KernelError::NotFound);
    }
    let a = state.user_assignments.iter_mut().find(|a| a.uid == uid)
        .ok_or(KernelError::NotFound)?;
    if !a.groups.contains(&group_id) {
        a.groups.push(group_id);
    }
    state.changes += 1;
    Ok(())
}

/// Remove a user from a capability group.
pub fn user_remove_group(uid: u64, group_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let a = state.user_assignments.iter_mut().find(|a| a.uid == uid)
        .ok_or(KernelError::NotFound)?;
    a.groups.retain(|g| *g != group_id);
    state.changes += 1;
    Ok(())
}

/// Add an extra capability to a user.
pub fn user_add_cap(uid: u64, cap: Capability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let a = state.user_assignments.iter_mut().find(|a| a.uid == uid)
        .ok_or(KernelError::NotFound)?;
    if !a.extra_caps.contains(&cap) {
        a.extra_caps.push(cap);
    }
    state.changes += 1;
    Ok(())
}

/// Deny a capability for a user (overrides group grants).
pub fn user_deny_cap(uid: u64, cap: Capability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let a = state.user_assignments.iter_mut().find(|a| a.uid == uid)
        .ok_or(KernelError::NotFound)?;
    if !a.denied_caps.contains(&cap) {
        a.denied_caps.push(cap);
    }
    state.changes += 1;
    Ok(())
}

/// Get effective capabilities for a user.
pub fn user_effective_caps(uid: u64) -> KernelResult<Vec<Capability>> {
    let state = STATE.lock();
    let a = state.user_assignments.iter().find(|a| a.uid == uid)
        .ok_or(KernelError::NotFound)?;

    let mut caps = Vec::new();
    // Collect from groups.
    for gid in &a.groups {
        if let Some(g) = state.groups.iter().find(|g| g.id == *gid) {
            for cap in &g.caps {
                if !caps.contains(cap) {
                    caps.push(*cap);
                }
            }
        }
    }
    // Add extras.
    for cap in &a.extra_caps {
        if !caps.contains(cap) {
            caps.push(*cap);
        }
    }
    // Remove denied.
    caps.retain(|c| !a.denied_caps.contains(c));
    Ok(caps)
}

/// Get user assignment.
pub fn get_user_assignment(uid: u64) -> KernelResult<UserCapAssignment> {
    STATE.lock().user_assignments.iter().find(|a| a.uid == uid).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all user assignments.
pub fn list_user_assignments() -> Vec<UserCapAssignment> {
    STATE.lock().user_assignments.clone()
}

// ---------------------------------------------------------------------------
// Program assignments
// ---------------------------------------------------------------------------

/// Set capabilities for a program.
pub fn assign_program(program: &str, required: &[Capability], max: &[Capability],
    sandboxed: bool) -> KernelResult<u64>
{
    let mut state = STATE.lock();
    if state.program_assignments.len() >= 512 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.program_assignments.push(ProgramCapAssignment {
        id,
        program: String::from(program),
        required_caps: required.to_vec(),
        max_caps: max.to_vec(),
        sandboxed,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove program assignment.
pub fn remove_program(program: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.program_assignments.iter().any(|a| a.program == program) {
        return Err(KernelError::NotFound);
    }
    state.program_assignments.retain(|a| a.program != program);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get program assignment.
pub fn get_program(program: &str) -> KernelResult<ProgramCapAssignment> {
    STATE.lock().program_assignments.iter()
        .find(|a| a.program == program).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all program assignments.
pub fn list_programs() -> Vec<ProgramCapAssignment> {
    STATE.lock().program_assignments.clone()
}

// ---------------------------------------------------------------------------
// Path requirements
// ---------------------------------------------------------------------------

/// Set capability requirements for a path.
pub fn set_path_requirement(path: &str, caps: &[Capability], recursive: bool) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.path_requirements.len() >= 512 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.path_requirements.push(PathRequirement {
        id,
        path: String::from(path),
        required_caps: caps.to_vec(),
        recursive,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a path requirement.
pub fn remove_path_requirement(req_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.path_requirements.iter().any(|r| r.id == req_id) {
        return Err(KernelError::NotFound);
    }
    state.path_requirements.retain(|r| r.id != req_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get requirements for a specific path (checks prefix matches).
pub fn path_requirements(path: &str) -> Vec<PathRequirement> {
    let state = STATE.lock();
    state.path_requirements.iter()
        .filter(|r| {
            if r.path.ends_with('*') {
                let prefix = &r.path[..r.path.len() - 1];
                path.starts_with(prefix)
            } else if r.recursive {
                path.starts_with(&*r.path)
            } else {
                r.path == path
            }
        })
        .cloned()
        .collect()
}

/// List all path requirements.
pub fn list_path_requirements() -> Vec<PathRequirement> {
    STATE.lock().path_requirements.clone()
}

/// Check if a user has sufficient caps to access a path.
pub fn check_access(uid: u64, path: &str) -> KernelResult<bool> {
    let user_caps = user_effective_caps(uid)?;
    let reqs = path_requirements(path);
    for req in &reqs {
        for cap in &req.required_caps {
            if !user_caps.contains(cap) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_builtin_group(state: &mut State, name: &str, desc: &str, caps: &[Capability]) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.groups.push(CapGroup {
        id,
        name: String::from(name),
        description: String::from(desc),
        caps: caps.to_vec(),
        builtin: true,
    });
    id
}

/// Initialise default capability groups and assignments.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.groups.is_empty() {
        return;
    }

    let std_id = add_builtin_group(&mut state, "Standard User",
        "Basic file access, execution, and networking",
        &[Capability::FileRead, Capability::FileWrite, Capability::Execute,
          Capability::Network]);

    let dev_id = add_builtin_group(&mut state, "Developer",
        "Standard plus debug, raw sockets, package install",
        &[Capability::FileRead, Capability::FileWrite, Capability::Execute,
          Capability::Network, Capability::DebugProcess, Capability::RawSocket,
          Capability::PackageInstall]);

    let _admin_id = add_builtin_group(&mut state, "Administrator",
        "Full system access including user and capability management",
        &[Capability::FileRead, Capability::FileWrite, Capability::Execute,
          Capability::Network, Capability::BindLowPort, Capability::RawSocket,
          Capability::Mount, Capability::UserAdmin, Capability::PackageInstall,
          Capability::SystemConfig, Capability::Chown, Capability::DacOverride,
          Capability::SetClock, Capability::Reboot, Capability::AuditRead,
          Capability::AuditWrite, Capability::CapAdmin]);

    add_builtin_group(&mut state, "Network Service",
        "Network access with low port binding",
        &[Capability::FileRead, Capability::Network, Capability::BindLowPort,
          Capability::RawSocket]);

    add_builtin_group(&mut state, "Restricted",
        "Read-only file access, no network",
        &[Capability::FileRead, Capability::Execute]);

    // Default user assignments.
    let admin_groups: Vec<u64> = state.groups.iter()
        .filter(|g| g.name == "Administrator")
        .map(|g| g.id).collect();
    let root_assign_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.user_assignments.push(UserCapAssignment {
        id: root_assign_id,
        uid: 0,
        username: String::from("root"),
        groups: admin_groups,
        extra_caps: Vec::new(),
        denied_caps: Vec::new(),
    });

    let user_assign_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.user_assignments.push(UserCapAssignment {
        id: user_assign_id,
        uid: 1000,
        username: String::from("user"),
        groups: vec![std_id, dev_id],
        extra_caps: Vec::new(),
        denied_caps: Vec::new(),
    });

    // Default path requirements.
    let sys_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.path_requirements.push(PathRequirement {
        id: sys_id,
        path: String::from("/etc/"),
        required_caps: vec![Capability::SystemConfig],
        recursive: true,
    });

    let audit_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.path_requirements.push(PathRequirement {
        id: audit_id,
        path: String::from("/var/log/audit/"),
        required_caps: vec![Capability::AuditRead],
        recursive: true,
    });

    state.changes += 1;
}

/// Return (group_count, user_count, program_count, path_count, ops).
pub fn stats() -> (usize, usize, usize, usize, u64) {
    let state = STATE.lock();
    (state.groups.len(),
     state.user_assignments.len(),
     state.program_assignments.len(),
     state.path_requirements.len(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.groups.clear();
    state.user_assignments.clear();
    state.program_assignments.clear();
    state.path_requirements.clear();
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: create groups.
    serial_println!("capsettings::self_test 1: create groups");
    let g1 = create_group("TestGroup", "test", &[Capability::FileRead, Capability::Network])?;
    let g2 = create_group("DevGroup", "dev", &[Capability::DebugProcess, Capability::Execute])?;
    assert_eq!(list_groups().len(), 2);

    // Test 2: modify groups.
    serial_println!("capsettings::self_test 2: modify groups");
    group_add_cap(g1, Capability::FileWrite)?;
    let g = get_group(g1)?;
    assert_eq!(g.caps.len(), 3);
    group_remove_cap(g1, Capability::Network)?;
    let g = get_group(g1)?;
    assert_eq!(g.caps.len(), 2);

    // Test 3: user assignments.
    serial_println!("capsettings::self_test 3: user assignments");
    assign_user(100, "testuser")?;
    user_add_group(100, g1)?;
    user_add_group(100, g2)?;
    user_add_cap(100, Capability::Mount)?;
    user_deny_cap(100, Capability::DebugProcess)?;
    let caps = user_effective_caps(100)?;
    assert!(caps.contains(&Capability::FileRead));
    assert!(caps.contains(&Capability::Mount));
    assert!(!caps.contains(&Capability::DebugProcess)); // denied

    // Test 4: program assignments.
    serial_println!("capsettings::self_test 4: program assignments");
    assign_program("/usr/bin/server",
        &[Capability::Network, Capability::BindLowPort],
        &[Capability::Network, Capability::BindLowPort, Capability::FileRead],
        true)?;
    let prog = get_program("/usr/bin/server")?;
    assert!(prog.sandboxed);
    assert_eq!(prog.required_caps.len(), 2);
    remove_program("/usr/bin/server")?;
    assert!(get_program("/usr/bin/server").is_err());

    // Test 5: path requirements.
    serial_println!("capsettings::self_test 5: path requirements");
    let r1 = set_path_requirement("/secure/", &[Capability::SystemConfig], true)?;
    let reqs = path_requirements("/secure/data/file.txt");
    assert_eq!(reqs.len(), 1);
    let reqs = path_requirements("/other/file.txt");
    assert_eq!(reqs.len(), 0);
    remove_path_requirement(r1)?;

    // Test 6: access check.
    serial_println!("capsettings::self_test 6: access check");
    set_path_requirement("/admin/", &[Capability::SystemConfig], true)?;
    // User 100 doesn't have SystemConfig.
    let allowed = check_access(100, "/admin/config.yaml")?;
    assert!(!allowed);
    user_add_cap(100, Capability::SystemConfig)?;
    let allowed = check_access(100, "/admin/config.yaml")?;
    assert!(allowed);

    // Test 7: init defaults.
    serial_println!("capsettings::self_test 7: init defaults");
    clear_all();
    init_defaults();
    let groups = list_groups();
    assert!(groups.len() >= 5);
    let users = list_user_assignments();
    assert!(users.len() >= 2);
    // Built-in groups cannot be removed.
    assert!(remove_group(groups[0].id).is_err());

    clear_all();
    serial_println!("capsettings::self_test: all 7 tests passed");
    Ok(())
}
