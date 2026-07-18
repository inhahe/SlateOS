//! Service resource limits — per-service cgroup-equivalent controls.
//!
//! Associates resource limits with named services so that when a service
//! is started (whether manually or via socket activation), the kernel
//! automatically applies the configured limits to its process.
//!
//! ## Design
//!
//! From design decisions: "Resource management — uses cgroups to limit
//! CPU/memory/IO per service."  We implement this as a service-level
//! configuration layer on top of the existing per-process ResourceLimits
//! infrastructure.
//!
//! ## How It Works
//!
//! 1. The administrator (or init system) calls `set_service_limits("dns", limits)`
//!    to configure limits for a service before it starts.
//! 2. When the service process is spawned, the process subsystem calls
//!    `get_service_limits("dns")` to retrieve the limits.
//! 3. The limits are applied to the process via `mm::rlimits::apply_limits()`.
//!
//! ## Integration Points
//!
//! - **Socket activation**: when `trigger_service_spawn()` launches a
//!   socket-activated service, it looks up service limits and passes
//!   them to the process spawner.
//! - **Init/service manager**: when starting a service from a service
//!   definition, applies configured limits.
//! - **Kshell `slimit` command**: inspect and configure service limits.
//!
//! ## Lock Ordering
//!
//! `SERVICE_LIMITS` does not call into rlimits, scheduler, or service registry.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::mm::rlimits::ResourceLimits;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of service limit configurations.
const MAX_SERVICE_LIMITS: usize = 64;

/// Maximum service name length.
const MAX_NAME_LEN: usize = 128;

// ---------------------------------------------------------------------------
// Service limit entry
// ---------------------------------------------------------------------------

/// A service resource limit configuration.
struct ServiceLimitEntry {
    /// Whether this slot is active.
    active: bool,
    /// Service name (matches what the service registers with).
    name: [u8; MAX_NAME_LEN],
    /// Length of the name.
    name_len: usize,
    /// Resource limits for this service.
    limits: ResourceLimits,
}

impl ServiceLimitEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            name: [0; MAX_NAME_LEN],
            name_len: 0,
            limits: ResourceLimits::unlimited(),
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

static SERVICE_LIMITS: Mutex<[ServiceLimitEntry; MAX_SERVICE_LIMITS]> = Mutex::new({
    const EMPTY: ServiceLimitEntry = ServiceLimitEntry::empty();
    [EMPTY; MAX_SERVICE_LIMITS]
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Configure resource limits for a named service.
///
/// If limits already exist for this service, they are replaced.
pub fn set_service_limits(name: &str, limits: ResourceLimits) -> KernelResult<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = SERVICE_LIMITS.lock();

    // Check if the service already has limits — update in place.
    for entry in table.iter_mut() {
        if entry.active && entry.name_str() == name {
            entry.limits = limits;
            serial_println!("[slimits] Updated limits for '{}': {}", name, limits);
            return Ok(());
        }
    }

    // Find a free slot.
    let slot = table.iter_mut()
        .find(|e| !e.active)
        .ok_or(KernelError::OutOfMemory)?;

    slot.active = true;
    slot.name_len = name.len();
    slot.name[..name.len()].copy_from_slice(name.as_bytes());
    slot.limits = limits;

    serial_println!("[slimits] Set limits for '{}': {}", name, limits);
    Ok(())
}

/// Get the configured resource limits for a named service.
///
/// Returns `None` if no limits are configured (service runs unlimited).
pub fn get_service_limits(name: &str) -> Option<ResourceLimits> {
    let table = SERVICE_LIMITS.lock();
    for entry in table.iter() {
        if entry.active && entry.name_str() == name {
            return Some(entry.limits);
        }
    }
    None
}

/// Remove resource limits configuration for a named service.
pub fn remove_service_limits(name: &str) -> KernelResult<()> {
    let mut table = SERVICE_LIMITS.lock();
    for entry in table.iter_mut() {
        if entry.active && entry.name_str() == name {
            entry.active = false;
            serial_println!("[slimits] Removed limits for '{}'", name);
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// List all configured service limits.
///
/// Returns (name, limits) pairs.
pub fn list_all() -> Vec<(String, ResourceLimits)> {
    let table = SERVICE_LIMITS.lock();
    let mut result = Vec::new();
    for entry in table.iter() {
        if entry.active {
            result.push((
                String::from(entry.name_str()),
                entry.limits,
            ));
        }
    }
    result
}

/// Count of services with configured limits.
pub fn count() -> usize {
    let table = SERVICE_LIMITS.lock();
    table.iter().filter(|e| e.active).count()
}

// ---------------------------------------------------------------------------
// Predefined service limit profiles
// ---------------------------------------------------------------------------

/// Predefined limit profiles for common service types.
#[derive(Debug, Clone, Copy)]
pub enum ServiceProfile {
    /// Background daemon: modest resources.
    /// 16 MiB RSS, 10% CPU, 4 threads, 64 handles.
    Daemon,
    /// Network service: moderate resources.
    /// 64 MiB RSS, 25% CPU, 16 threads, 256 handles.
    NetworkService,
    /// Critical system service: generous resources.
    /// 256 MiB RSS, 50% CPU, 64 threads, 512 handles.
    SystemService,
    /// Restricted/sandboxed service: tight limits.
    /// 8 MiB RSS, 5% CPU, 2 threads, 32 handles.
    Sandboxed,
}

impl ServiceProfile {
    /// Convert a profile into concrete resource limits.
    #[must_use]
    pub const fn to_limits(self) -> ResourceLimits {
        match self {
            Self::Daemon => ResourceLimits {
                max_rss_frames: 1024,     // 16 MiB @ 16 KiB/frame
                cpu_quota_pct: 10,
                max_threads: 4,
                max_handles: 64,
            },
            Self::NetworkService => ResourceLimits {
                max_rss_frames: 4096,     // 64 MiB @ 16 KiB/frame
                cpu_quota_pct: 25,
                max_threads: 16,
                max_handles: 256,
            },
            Self::SystemService => ResourceLimits {
                max_rss_frames: 16384,    // 256 MiB @ 16 KiB/frame
                cpu_quota_pct: 50,
                max_threads: 64,
                max_handles: 512,
            },
            Self::Sandboxed => ResourceLimits {
                max_rss_frames: 512,      // 8 MiB @ 16 KiB/frame
                cpu_quota_pct: 5,
                max_threads: 2,
                max_handles: 32,
            },
        }
    }
}

/// Apply a predefined profile to a service.
pub fn apply_profile(name: &str, profile: ServiceProfile) -> KernelResult<()> {
    set_service_limits(name, profile.to_limits())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run service limits self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[slimits] Running service limits self-test...");

    test_set_get()?;
    test_update()?;
    test_remove()?;
    test_profiles()?;
    test_list()?;

    serial_println!("[slimits] Service limits self-test PASSED");
    Ok(())
}

/// Test 1: set and get service limits.
fn test_set_get() -> KernelResult<()> {
    let limits = ResourceLimits {
        max_rss_frames: 100,
        cpu_quota_pct: 25,
        max_threads: 8,
        max_handles: 64,
    };

    set_service_limits("test.slimit1", limits)?;

    let got = get_service_limits("test.slimit1");
    if got.is_none() {
        serial_println!("[slimits]   FAIL: limits not found after set");
        remove_service_limits("test.slimit1").ok();
        return Err(KernelError::InternalError);
    }
    let g = got.unwrap_or_default();
    if g.max_rss_frames != 100 || g.cpu_quota_pct != 25 || g.max_threads != 8 || g.max_handles != 64 {
        serial_println!("[slimits]   FAIL: limits mismatch after set");
        remove_service_limits("test.slimit1").ok();
        return Err(KernelError::InternalError);
    }

    // Unlisted service returns None.
    if get_service_limits("nonexistent").is_some() {
        serial_println!("[slimits]   FAIL: found limits for nonexistent service");
        remove_service_limits("test.slimit1").ok();
        return Err(KernelError::InternalError);
    }

    remove_service_limits("test.slimit1").ok();
    serial_println!("[slimits]   Set/get: OK");
    Ok(())
}

/// Test 2: updating existing limits replaces them.
fn test_update() -> KernelResult<()> {
    let limits1 = ResourceLimits {
        max_rss_frames: 100,
        cpu_quota_pct: 10,
        max_threads: 4,
        max_handles: 32,
    };
    let limits2 = ResourceLimits {
        max_rss_frames: 200,
        cpu_quota_pct: 20,
        max_threads: 8,
        max_handles: 64,
    };

    set_service_limits("test.slimit2", limits1)?;
    set_service_limits("test.slimit2", limits2)?;

    let got = get_service_limits("test.slimit2").unwrap_or_default();
    if got.max_rss_frames != 200 || got.cpu_quota_pct != 20 {
        serial_println!("[slimits]   FAIL: update didn't replace limits");
        remove_service_limits("test.slimit2").ok();
        return Err(KernelError::InternalError);
    }

    remove_service_limits("test.slimit2").ok();
    serial_println!("[slimits]   Update: OK");
    Ok(())
}

/// Test 3: remove service limits.
fn test_remove() -> KernelResult<()> {
    let limits = ResourceLimits {
        max_rss_frames: 50,
        cpu_quota_pct: 5,
        max_threads: 2,
        max_handles: 16,
    };

    set_service_limits("test.slimit3", limits)?;

    // Should find it.
    if get_service_limits("test.slimit3").is_none() {
        serial_println!("[slimits]   FAIL: limits not found before remove");
        return Err(KernelError::InternalError);
    }

    remove_service_limits("test.slimit3")?;

    // Should not find it.
    if get_service_limits("test.slimit3").is_some() {
        serial_println!("[slimits]   FAIL: limits still found after remove");
        return Err(KernelError::InternalError);
    }

    // Removing nonexistent returns NotFound.
    match remove_service_limits("test.slimit3") {
        Err(KernelError::NotFound) => {}
        other => {
            serial_println!("[slimits]   FAIL: remove nonexistent returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[slimits]   Remove: OK");
    Ok(())
}

/// Test 4: predefined profiles produce correct limits.
fn test_profiles() -> KernelResult<()> {
    let daemon = ServiceProfile::Daemon.to_limits();
    if daemon.max_rss_frames != 1024 || daemon.cpu_quota_pct != 10 {
        serial_println!("[slimits]   FAIL: Daemon profile wrong");
        return Err(KernelError::InternalError);
    }

    let sandboxed = ServiceProfile::Sandboxed.to_limits();
    if sandboxed.max_rss_frames != 512 || sandboxed.cpu_quota_pct != 5 {
        serial_println!("[slimits]   FAIL: Sandboxed profile wrong");
        return Err(KernelError::InternalError);
    }

    // Apply a profile to a service.
    apply_profile("test.slimit4", ServiceProfile::NetworkService)?;
    let got = get_service_limits("test.slimit4").unwrap_or_default();
    if got.max_rss_frames != 4096 || got.cpu_quota_pct != 25 || got.max_threads != 16 {
        serial_println!("[slimits]   FAIL: NetworkService profile wrong after apply");
        remove_service_limits("test.slimit4").ok();
        return Err(KernelError::InternalError);
    }

    remove_service_limits("test.slimit4").ok();
    serial_println!("[slimits]   Profiles: OK");
    Ok(())
}

/// Test 5: list all service limits.
fn test_list() -> KernelResult<()> {
    set_service_limits("test.slimit5a", ServiceProfile::Daemon.to_limits())?;
    set_service_limits("test.slimit5b", ServiceProfile::Sandboxed.to_limits())?;

    let all = list_all();
    let has_a = all.iter().any(|(n, _)| n == "test.slimit5a");
    let has_b = all.iter().any(|(n, _)| n == "test.slimit5b");

    if !has_a || !has_b {
        serial_println!("[slimits]   FAIL: list missing entries");
        remove_service_limits("test.slimit5a").ok();
        remove_service_limits("test.slimit5b").ok();
        return Err(KernelError::InternalError);
    }

    remove_service_limits("test.slimit5a").ok();
    remove_service_limits("test.slimit5b").ok();
    serial_println!("[slimits]   List: OK");
    Ok(())
}
