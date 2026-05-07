//! Filesystem policy engine — centralized tuning and workload profiles.
//!
//! Provides unified configuration management for all filesystem subsystems
//! (cache, compression, dedup, tmpwatch, reclaim, audit, history, profiling,
//! etc.).  Supports workload-specific presets that tune all subsystems at
//! once, analogous to the scheduler and memory workload profiles.
//!
//! ## Workload Profiles
//!
//! | Profile     | Optimizes for                                    |
//! |-------------|--------------------------------------------------|
//! | Desktop     | Responsiveness, moderate caching, versioning on   |
//! | Server      | Throughput, large caches, audit + profiling on     |
//! | Development | Build speed, large trash, indexing, versioning on  |
//! | Gaming      | Minimal overhead, all extras disabled              |
//!
//! ## Architecture
//!
//! ```text
//! fspolicy apply desktop
//!   → policy::apply_profile(Desktop)
//!     → cache::set_readahead_max(...)
//!     → fcompress::set_enabled(...)
//!     → tmpwatch::set_max_age(...)
//!     → reclaim::set_high_watermark(...)
//!     → history::set_enabled(...)
//!     → audit::disable()
//!     → profile::disable()
//!     → ...
//! ```
//!
//! Individual settings can be overridden after applying a profile.
//! Once any setting is manually changed, `current_profile()` returns
//! `None` to indicate the configuration no longer matches a preset.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Filesystem workload profile presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsProfile {
    /// Desktop use: moderate caching, versioning on, responsive defaults.
    Desktop = 0,
    /// Server use: large caches, compression on, audit enabled, throughput focus.
    Server = 1,
    /// Development use: large trash retention, indexing on, fast builds.
    Development = 2,
    /// Gaming use: minimal overhead, all extras disabled.
    Gaming = 3,
}

impl FsProfile {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Server => "Server",
            Self::Development => "Development",
            Self::Gaming => "Gaming",
        }
    }

    /// Parse a profile name (case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "desktop" | "Desktop" | "0" => Some(Self::Desktop),
            "server" | "Server" | "1" => Some(Self::Server),
            "dev" | "development" | "Development" | "2" => Some(Self::Development),
            "gaming" | "Gaming" | "3" => Some(Self::Gaming),
            _ => None,
        }
    }

    /// All profiles.
    pub const ALL: &'static [FsProfile] = &[
        Self::Desktop, Self::Server, Self::Development, Self::Gaming,
    ];
}

/// A single tunable filesystem parameter.
#[derive(Debug, Clone)]
pub struct FsSetting {
    /// Dot-separated key (e.g., "cache.readahead_max").
    pub key: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Current value as a string.
    pub value: String,
    /// Profile presets: [Desktop, Server, Dev, Gaming].
    pub presets: [&'static str; 4],
}

/// Policy engine statistics.
#[derive(Debug, Clone, Default)]
pub struct PolicyStats {
    /// Number of profile applications.
    pub profiles_applied: u64,
    /// Number of individual setting changes.
    pub settings_changed: u64,
    /// Number of setting queries.
    pub settings_queried: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Currently active profile (0-3 = profile ID, u64::MAX = none/custom).
static ACTIVE_PROFILE: AtomicU64 = AtomicU64::new(u64::MAX);
static PROFILES_APPLIED: AtomicU64 = AtomicU64::new(0);
static SETTINGS_CHANGED: AtomicU64 = AtomicU64::new(0);
static SETTINGS_QUERIED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Setting definitions
// ---------------------------------------------------------------------------

/// Registry of all tunable filesystem parameters.
///
/// Each entry defines the key, description, how to get/set it, and the
/// four profile preset values.
struct SettingDef {
    key: &'static str,
    description: &'static str,
    /// Profile presets: [Desktop, Server, Dev, Gaming].
    presets: [&'static str; 4],
    /// Get current value as string.
    getter: fn() -> String,
    /// Set value from string. Returns Ok(()) or error message.
    setter: fn(&str) -> Result<(), &'static str>,
}

/// All tunable settings. Order matters for display.
fn setting_defs() -> &'static [SettingDef] {
    use alloc::format;
    // We need 'static lifetime, so use a leaked Box. This is called
    // infrequently and the data is small + permanent.
    static DEFS: spin::Once<&'static [SettingDef]> = spin::Once::new();
    DEFS.call_once(|| {
        let defs: Vec<SettingDef> = alloc::vec![
            // -- Cache --
            SettingDef {
                key: "cache.readahead_max",
                description: "Maximum readahead blocks per file",
                presets: ["64", "256", "64", "32"],
                getter: || format!("{}", super::cache::get_readahead_max()),
                setter: |v| {
                    let n: u32 = v.parse().map_err(|_| "invalid number")?;
                    super::cache::set_readahead_max(n);
                    Ok(())
                },
            },
            SettingDef {
                key: "cache.readahead_initial",
                description: "Initial readahead blocks for new files",
                presets: ["4", "16", "4", "2"],
                getter: || format!("{}", super::cache::get_readahead_initial()),
                setter: |v| {
                    let n: u32 = v.parse().map_err(|_| "invalid number")?;
                    super::cache::set_readahead_initial(n);
                    Ok(())
                },
            },
            SettingDef {
                key: "cache.dirty_expire_ms",
                description: "Dirty buffer cache expiry (milliseconds)",
                presets: ["5000", "30000", "5000", "3000"],
                getter: || format!("{}", super::cache::get_dirty_expire_ns() / 1_000_000),
                setter: |v| {
                    let ms: u64 = v.parse().map_err(|_| "invalid number")?;
                    super::cache::set_dirty_expire_ns(ms.saturating_mul(1_000_000));
                    Ok(())
                },
            },
            // -- Compression --
            SettingDef {
                key: "compress.enabled",
                description: "Automatic filesystem compression",
                presets: ["false", "true", "false", "false"],
                getter: || format!("{}", super::fcompress::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::fcompress::set_enabled(true),
                        "false" | "0" | "off" => super::fcompress::set_enabled(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            SettingDef {
                key: "compress.min_size",
                description: "Minimum file size for auto-compression (bytes)",
                presets: ["4096", "1024", "4096", "4096"],
                getter: || format!("{}", super::fcompress::min_size()),
                setter: |v| {
                    let n: u64 = v.parse().map_err(|_| "invalid number")?;
                    super::fcompress::set_min_size(n);
                    Ok(())
                },
            },
            // -- Deduplication --
            SettingDef {
                key: "dedup.enabled",
                description: "Deduplication scanner enabled",
                presets: ["false", "true", "false", "false"],
                getter: || format!("{}", super::dedup::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::dedup::set_enabled(true),
                        "false" | "0" | "off" => super::dedup::set_enabled(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            // -- Temporary file cleanup --
            SettingDef {
                key: "tmpwatch.enabled",
                description: "Automatic /tmp cleanup",
                presets: ["true", "true", "true", "false"],
                getter: || format!("{}", super::tmpwatch::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::tmpwatch::set_enabled(true),
                        "false" | "0" | "off" => super::tmpwatch::set_enabled(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            SettingDef {
                key: "tmpwatch.max_age_hours",
                description: "Temp file maximum age (hours)",
                presets: ["24", "6", "48", "12"],
                getter: || format!("{}", super::tmpwatch::max_age() / 3600),
                setter: |v| {
                    let h: u64 = v.parse().map_err(|_| "invalid number")?;
                    super::tmpwatch::set_max_age(h.saturating_mul(3600));
                    Ok(())
                },
            },
            // -- Disk space reclaim --
            SettingDef {
                key: "reclaim.enabled",
                description: "Automatic disk space reclamation",
                presets: ["true", "true", "true", "true"],
                getter: || format!("{}", super::reclaim::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::reclaim::set_enabled(true),
                        "false" | "0" | "off" => super::reclaim::set_enabled(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            SettingDef {
                key: "reclaim.high_watermark",
                description: "Disk usage % that triggers reclamation",
                presets: ["90", "85", "90", "95"],
                getter: || {
                    let (high, _) = super::reclaim::watermarks();
                    format!("{}", high)
                },
                setter: |v| {
                    let pct: u64 = v.parse().map_err(|_| "invalid number")?;
                    if pct > 100 { return Err("percent must be 0-100"); }
                    super::reclaim::set_high_watermark(pct);
                    Ok(())
                },
            },
            SettingDef {
                key: "reclaim.low_watermark",
                description: "Disk usage % target after reclamation",
                presets: ["80", "70", "80", "85"],
                getter: || {
                    let (_, low) = super::reclaim::watermarks();
                    format!("{}", low)
                },
                setter: |v| {
                    let pct: u64 = v.parse().map_err(|_| "invalid number")?;
                    if pct > 100 { return Err("percent must be 0-100"); }
                    super::reclaim::set_low_watermark(pct);
                    Ok(())
                },
            },
            // -- File versioning / history --
            SettingDef {
                key: "history.enabled",
                description: "File version history recording",
                presets: ["true", "true", "true", "false"],
                getter: || format!("{}", super::history::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::history::set_enabled(true),
                        "false" | "0" | "off" => super::history::set_enabled(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            SettingDef {
                key: "history.auto_version",
                description: "Auto-record version on file write",
                presets: ["true", "true", "true", "false"],
                getter: || format!("{}", super::history::is_auto_version_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::history::set_auto_version(true),
                        "false" | "0" | "off" => super::history::set_auto_version(false),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            // -- Audit --
            SettingDef {
                key: "audit.enabled",
                description: "Filesystem audit logging",
                presets: ["false", "true", "false", "false"],
                getter: || format!("{}", super::audit::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::audit::enable(),
                        "false" | "0" | "off" => super::audit::disable(),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
            // -- I/O Profiling --
            SettingDef {
                key: "profile.enabled",
                description: "Filesystem I/O profiling",
                presets: ["false", "true", "false", "false"],
                getter: || format!("{}", super::profile::is_enabled()),
                setter: |v| {
                    match v {
                        "true" | "1" | "on" => super::profile::enable(),
                        "false" | "0" | "off" => super::profile::disable(),
                        _ => return Err("expected true/false"),
                    }
                    Ok(())
                },
            },
        ];
        // Leak into 'static — small, permanent, allocated once.
        let boxed: &'static [SettingDef] = alloc::boxed::Box::leak(defs.into_boxed_slice());
        boxed
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply a filesystem workload profile.
///
/// Configures all tunable filesystem parameters to match the preset.
/// Any individual settings changed afterward will cause
/// `current_profile()` to return `None`.
pub fn apply_profile(profile: FsProfile) {
    let idx = profile as usize;
    let defs = setting_defs();

    for def in defs {
        let preset_value = def.presets[idx];
        if let Err(e) = (def.setter)(preset_value) {
            serial_println!("[policy] Warning: failed to set {}: {}", def.key, e);
        }
    }

    ACTIVE_PROFILE.store(idx as u64, Ordering::Relaxed);
    PROFILES_APPLIED.fetch_add(1, Ordering::Relaxed);
    serial_println!("[policy] Applied filesystem profile: {}", profile.label());
}

/// Get the currently active profile, or `None` if manually tuned.
pub fn current_profile() -> Option<FsProfile> {
    let id = ACTIVE_PROFILE.load(Ordering::Relaxed);
    match id {
        0 => Some(FsProfile::Desktop),
        1 => Some(FsProfile::Server),
        2 => Some(FsProfile::Development),
        3 => Some(FsProfile::Gaming),
        _ => None,
    }
}

/// Get a specific setting's current value.
pub fn get_setting(key: &str) -> Option<String> {
    SETTINGS_QUERIED.fetch_add(1, Ordering::Relaxed);
    let defs = setting_defs();
    for def in defs {
        if def.key == key {
            return Some((def.getter)());
        }
    }
    None
}

/// Set a specific setting's value.
///
/// After calling this, `current_profile()` will return `None` since the
/// configuration no longer matches any preset.
pub fn set_setting(key: &str, value: &str) -> Result<(), &'static str> {
    let defs = setting_defs();
    for def in defs {
        if def.key == key {
            (def.setter)(value)?;
            // Mark profile as custom since user overrode a value.
            ACTIVE_PROFILE.store(u64::MAX, Ordering::Relaxed);
            SETTINGS_CHANGED.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    }
    Err("unknown setting")
}

/// List all settings with current values, descriptions, and presets.
pub fn list_settings() -> Vec<FsSetting> {
    SETTINGS_QUERIED.fetch_add(1, Ordering::Relaxed);
    let defs = setting_defs();
    let mut result = Vec::with_capacity(defs.len());
    for def in defs {
        result.push(FsSetting {
            key: def.key,
            description: def.description,
            value: (def.getter)(),
            presets: def.presets,
        });
    }
    result
}

/// Export current configuration as a text block.
pub fn export_settings() -> String {
    let mut out = String::with_capacity(1024);
    let profile = current_profile();
    out.push_str("# Filesystem Policy Configuration\n");
    if let Some(p) = profile {
        out.push_str("# Profile: ");
        out.push_str(p.label());
        out.push('\n');
    } else {
        out.push_str("# Profile: custom\n");
    }
    out.push('\n');

    let defs = setting_defs();
    for def in defs {
        out.push_str("# ");
        out.push_str(def.description);
        out.push('\n');
        out.push_str(def.key);
        out.push_str(" = ");
        out.push_str(&(def.getter)());
        out.push('\n');
    }
    out
}

/// Get the preset value for a setting under a specific profile.
pub fn preset_value(key: &str, profile: FsProfile) -> Option<&'static str> {
    let idx = profile as usize;
    let defs = setting_defs();
    for def in defs {
        if def.key == key {
            return Some(def.presets[idx]);
        }
    }
    None
}

/// Check whether the current configuration matches any preset exactly.
pub fn detect_profile() -> Option<FsProfile> {
    let defs = setting_defs();
    'outer: for &profile in FsProfile::ALL {
        let idx = profile as usize;
        for def in defs {
            let current = (def.getter)();
            if current != def.presets[idx] {
                continue 'outer;
            }
        }
        return Some(profile);
    }
    None
}

/// Quick summary statistics.
pub fn stats() -> PolicyStats {
    PolicyStats {
        profiles_applied: PROFILES_APPLIED.load(Ordering::Relaxed),
        settings_changed: SETTINGS_CHANGED.load(Ordering::Relaxed),
        settings_queried: SETTINGS_QUERIED.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[policy] Running self-test...");

    test_profile_parse();
    test_list_settings();
    test_get_set();
    test_apply_profile();
    test_detect_profile();
    test_export();

    serial_println!("[policy] Self-test passed (6 tests).");
    Ok(())
}

fn test_profile_parse() {
    assert_eq!(FsProfile::from_name("desktop"), Some(FsProfile::Desktop));
    assert_eq!(FsProfile::from_name("server"), Some(FsProfile::Server));
    assert_eq!(FsProfile::from_name("dev"), Some(FsProfile::Development));
    assert_eq!(FsProfile::from_name("gaming"), Some(FsProfile::Gaming));
    assert_eq!(FsProfile::from_name("0"), Some(FsProfile::Desktop));
    assert_eq!(FsProfile::from_name("3"), Some(FsProfile::Gaming));
    assert_eq!(FsProfile::from_name("bogus"), None);
    serial_println!("[policy]   profile_parse: ok");
}

fn test_list_settings() {
    let settings = list_settings();
    assert!(!settings.is_empty());
    // Every setting must have a key and description.
    for s in &settings {
        assert!(!s.key.is_empty());
        assert!(!s.description.is_empty());
        assert!(!s.value.is_empty());
    }
    serial_println!("[policy]   list_settings: ok");
}

fn test_get_set() {
    // Read a known setting.
    let val = get_setting("cache.readahead_max");
    assert!(val.is_some());

    // Set a setting.
    let result = set_setting("cache.readahead_max", "128");
    assert!(result.is_ok());

    let val2 = get_setting("cache.readahead_max");
    assert_eq!(val2.as_deref(), Some("128"));

    // After manual set, profile should be None.
    assert!(current_profile().is_none());

    // Unknown setting returns error.
    let err = set_setting("nonexistent.key", "42");
    assert!(err.is_err());

    serial_println!("[policy]   get/set: ok");
}

fn test_apply_profile() {
    // Apply Desktop profile.
    apply_profile(FsProfile::Desktop);
    assert_eq!(current_profile(), Some(FsProfile::Desktop));

    // Verify a setting matches the Desktop preset.
    let val = get_setting("compress.enabled");
    assert_eq!(val.as_deref(), Some("false"));

    // Apply Server profile.
    apply_profile(FsProfile::Server);
    assert_eq!(current_profile(), Some(FsProfile::Server));

    // Server enables compression.
    let val = get_setting("compress.enabled");
    assert_eq!(val.as_deref(), Some("true"));

    // Manual override breaks profile detection.
    let _ = set_setting("compress.enabled", "false");
    assert!(current_profile().is_none());

    // Restore to Desktop for subsequent tests.
    apply_profile(FsProfile::Desktop);

    serial_println!("[policy]   apply_profile: ok");
}

fn test_detect_profile() {
    // After applying Desktop, detect should find it.
    apply_profile(FsProfile::Desktop);
    let detected = detect_profile();
    assert_eq!(detected, Some(FsProfile::Desktop));

    // After manual change, detect should return None (or different).
    let _ = set_setting("cache.readahead_max", "999");
    let detected = detect_profile();
    assert!(detected.is_none());

    // Restore.
    apply_profile(FsProfile::Desktop);
    serial_println!("[policy]   detect_profile: ok");
}

fn test_export() {
    apply_profile(FsProfile::Server);
    let text = export_settings();
    assert!(text.contains("Profile: Server"));
    assert!(text.contains("cache.readahead_max"));
    assert!(text.contains("compress.enabled"));

    // Restore.
    apply_profile(FsProfile::Desktop);
    serial_println!("[policy]   export: ok");
}
