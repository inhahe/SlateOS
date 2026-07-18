//! Boot configuration — bootloader settings and boot entry management.
//!
//! Manages GRUB/systemd-boot/UEFI bootloader configuration, boot menu
//! entries, kernel parameters, default boot target, and timeout settings.
//!
//! ## Design Reference
//!
//! design.txt lines 1237-1242:
//! "grub settings — is it possible to allow our OS to modify grub settings
//! when grub was installed by a linux installation?"
//! "For x86 UEFI: consider systemd-boot (simpler than GRUB) or write your
//! own minimal UEFI bootloader. For dual-boot, GRUB is the standard."
//!
//! design.txt line 1251: "boot-up activity listing option"
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Boot Configuration
//!   → bootcfg::set_timeout(seconds)
//!   → bootcfg::set_default(entry_id)
//!   → bootcfg::add_entry(BootEntry { ... })
//!
//! Boot process
//!   → bootcfg::get_active_entry() → kernel path + params
//!   → bootcfg::record_boot() → logs boot event
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

/// Type of bootloader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootloaderType {
    /// GRUB2 (Linux-standard, feature-rich).
    Grub2,
    /// systemd-boot (simple UEFI loader).
    SystemdBoot,
    /// Custom UEFI bootloader (our own).
    CustomUefi,
    /// Direct UEFI boot (no separate bootloader).
    DirectUefi,
}

/// Type of boot entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Our OS kernel.
    SlateOs,
    /// Another Linux installation.
    Linux,
    /// Windows installation.
    Windows,
    /// macOS (unlikely on UEFI PC but possible).
    MacOs,
    /// Recovery/repair mode.
    Recovery,
    /// Memory test (memtest86).
    MemTest,
    /// UEFI firmware settings.
    FirmwareSettings,
    /// Custom/other.
    Custom,
}

/// Console mode for boot display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    /// Text mode (simple, fast).
    Text,
    /// Graphical mode (splash screen).
    Graphical,
    /// Verbose mode (show all boot messages).
    Verbose,
    /// Silent mode (no output until login screen).
    Silent,
}

/// A boot menu entry.
#[derive(Debug, Clone)]
pub struct BootEntry {
    /// Unique entry ID.
    pub id: u64,
    /// Display name in boot menu.
    pub name: String,
    /// Entry type.
    pub kind: EntryKind,
    /// Path to kernel/loader.
    pub kernel_path: String,
    /// Path to initramfs/initrd (if applicable).
    pub initrd_path: String,
    /// Kernel command-line parameters.
    pub parameters: String,
    /// Whether this entry is the default boot target.
    pub is_default: bool,
    /// Whether this entry is hidden from the menu.
    pub hidden: bool,
    /// Whether this entry was auto-detected (vs manually added).
    pub auto_detected: bool,
    /// Position in boot menu (lower = higher).
    pub position: u32,
}

/// A recorded boot event.
#[derive(Debug, Clone)]
pub struct BootEvent {
    /// Boot event ID.
    pub id: u64,
    /// Timestamp when boot started (ns).
    pub boot_ns: u64,
    /// Entry that was booted.
    pub entry_id: u64,
    /// Entry name (snapshot at boot time).
    pub entry_name: String,
    /// Whether boot was successful.
    pub success: bool,
    /// Boot duration in milliseconds (0 = unknown).
    pub duration_ms: u64,
    /// Reason for boot (normal, recovery, firmware-update, etc.).
    pub reason: String,
}

/// Boot configuration (global settings).
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// Bootloader type.
    pub loader_type: BootloaderType,
    /// Boot menu timeout in seconds (0 = no menu).
    pub timeout_secs: u32,
    /// Console mode.
    pub console_mode: ConsoleMode,
    /// Whether to show boot activity listing.
    pub show_boot_activity: bool,
    /// Whether Secure Boot is enabled.
    pub secure_boot: bool,
    /// Path to GRUB config (if applicable).
    pub grub_config_path: String,
    /// Path to EFI System Partition.
    pub esp_path: String,
    /// Whether dual-boot is detected.
    pub dual_boot: bool,
    /// Resolution for graphical boot (e.g., "1920x1080").
    pub gfx_mode: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: BootConfig,
    entries: Vec<BootEntry>,
    boot_log: Vec<BootEvent>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    config: BootConfig {
        loader_type: BootloaderType::CustomUefi,
        timeout_secs: 5,
        console_mode: ConsoleMode::Graphical,
        show_boot_activity: false,
        secure_boot: false,
        grub_config_path: String::new(),
        esp_path: String::new(),
        dual_boot: false,
        gfx_mode: String::new(),
    },
    entries: Vec::new(),
    boot_log: Vec::new(),
    changes: 0,
});

static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EVENT_ID: AtomicU64 = AtomicU64::new(1);
static BOOT_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current boot configuration.
pub fn get_config() -> BootConfig {
    STATE.lock().config.clone()
}

/// Set bootloader type.
pub fn set_loader_type(loader: BootloaderType) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.loader_type = loader;
    state.changes += 1;
    Ok(())
}

/// Set boot menu timeout.
pub fn set_timeout(seconds: u32) -> KernelResult<()> {
    if seconds > 300 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.config.timeout_secs = seconds;
    state.changes += 1;
    Ok(())
}

/// Set console/display mode.
pub fn set_console_mode(mode: ConsoleMode) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.console_mode = mode;
    state.changes += 1;
    Ok(())
}

/// Toggle boot activity listing (design.txt line 1251).
pub fn set_boot_activity(show: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.show_boot_activity = show;
    state.changes += 1;
    Ok(())
}

/// Set Secure Boot status.
pub fn set_secure_boot(enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.secure_boot = enabled;
    state.changes += 1;
    Ok(())
}

/// Set GRUB config path.
pub fn set_grub_path(path: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.grub_config_path = String::from(path);
    state.changes += 1;
    Ok(())
}

/// Set ESP (EFI System Partition) path.
pub fn set_esp_path(path: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.esp_path = String::from(path);
    state.changes += 1;
    Ok(())
}

/// Set graphical boot resolution.
pub fn set_gfx_mode(mode: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.gfx_mode = String::from(mode);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Boot entries
// ---------------------------------------------------------------------------

/// Add a boot entry.
pub fn add_entry(
    name: &str,
    kind: EntryKind,
    kernel_path: &str,
    initrd_path: &str,
    parameters: &str,
    auto_detected: bool,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.entries.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed);
    let position = state.entries.len() as u32;
    let is_default = state.entries.is_empty(); // first entry is default
    state.entries.push(BootEntry {
        id,
        name: String::from(name),
        kind,
        kernel_path: String::from(kernel_path),
        initrd_path: String::from(initrd_path),
        parameters: String::from(parameters),
        is_default,
        hidden: false,
        auto_detected,
        position,
    });
    state.changes += 1;
    Ok(id)
}

/// Remove a boot entry.
pub fn remove_entry(entry_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let before = state.entries.len();
    state.entries.retain(|e| e.id != entry_id);
    if state.entries.len() == before {
        return Err(KernelError::NotFound);
    }
    // Reassign positions.
    for (i, entry) in state.entries.iter_mut().enumerate() {
        entry.position = i as u32;
    }
    // Ensure there's always a default.
    if !state.entries.is_empty() && !state.entries.iter().any(|e| e.is_default) {
        state.entries[0].is_default = true;
    }
    state.changes += 1;
    Ok(())
}

/// Get a boot entry by ID.
pub fn get_entry(entry_id: u64) -> KernelResult<BootEntry> {
    let state = STATE.lock();
    state
        .entries
        .iter()
        .find(|e| e.id == entry_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all boot entries (sorted by position).
pub fn list_entries() -> Vec<BootEntry> {
    let state = STATE.lock();
    let mut entries = state.entries.clone();
    entries.sort_by_key(|e| e.position);
    entries
}

/// Set the default boot entry.
pub fn set_default(entry_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.entries.iter().any(|e| e.id == entry_id) {
        return Err(KernelError::NotFound);
    }
    for entry in &mut state.entries {
        entry.is_default = entry.id == entry_id;
    }
    state.changes += 1;
    Ok(())
}

/// Set kernel parameters for an entry.
pub fn set_parameters(entry_id: u64, params: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let entry = state
        .entries
        .iter_mut()
        .find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    entry.parameters = String::from(params);
    state.changes += 1;
    Ok(())
}

/// Hide or show an entry in the boot menu.
pub fn set_hidden(entry_id: u64, hidden: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let entry = state
        .entries
        .iter_mut()
        .find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    entry.hidden = hidden;
    state.changes += 1;
    Ok(())
}

/// Move an entry to a new position.
pub fn set_position(entry_id: u64, new_pos: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let max_pos = state.entries.len().saturating_sub(1) as u32;
    let pos = new_pos.min(max_pos);
    let entry = state
        .entries
        .iter_mut()
        .find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    entry.position = pos;
    state.changes += 1;
    Ok(())
}

/// Get the default/active boot entry.
pub fn get_active_entry() -> Option<BootEntry> {
    let state = STATE.lock();
    state.entries.iter().find(|e| e.is_default).cloned()
}

// ---------------------------------------------------------------------------
// Boot log
// ---------------------------------------------------------------------------

/// Record a boot event.
pub fn record_boot(entry_id: u64, success: bool, duration_ms: u64, reason: &str) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.boot_log.len() >= 1024 {
        // Trim oldest entries.
        state.boot_log.drain(..256);
    }
    let entry_name = state
        .entries
        .iter()
        .find(|e| e.id == entry_id)
        .map_or_else(|| String::from("unknown"), |e| e.name.clone());
    let id = NEXT_EVENT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    state.boot_log.push(BootEvent {
        id,
        boot_ns: now,
        entry_id,
        entry_name,
        success,
        duration_ms,
        reason: String::from(reason),
    });
    BOOT_COUNT.fetch_add(1, Ordering::Relaxed);
    state.changes += 1;
    Ok(id)
}

/// Get recent boot events.
pub fn boot_log(limit: usize) -> Vec<BootEvent> {
    let state = STATE.lock();
    let start = state.boot_log.len().saturating_sub(limit);
    state.boot_log[start..].to_vec()
}

/// Clear boot log.
pub fn clear_boot_log() {
    STATE.lock().boot_log.clear();
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialize with default boot configuration.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.entries.is_empty() {
        return;
    }

    state.config = BootConfig {
        loader_type: BootloaderType::CustomUefi,
        timeout_secs: 5,
        console_mode: ConsoleMode::Graphical,
        show_boot_activity: false,
        secure_boot: false,
        grub_config_path: String::from("/boot/grub/grub.cfg"),
        esp_path: String::from("/boot/efi"),
        dual_boot: false,
        gfx_mode: String::from("1920x1080"),
    };

    let id1 = NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed);
    state.entries.push(BootEntry {
        id: id1,
        name: String::from("Mint Cinnamon OS"),
        kind: EntryKind::SlateOs,
        kernel_path: String::from("/boot/kernel"),
        initrd_path: String::from("/boot/initrd"),
        parameters: String::from("root=/dev/sda2 quiet splash"),
        is_default: true,
        hidden: false,
        auto_detected: false,
        position: 0,
    });

    let id2 = NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed);
    state.entries.push(BootEntry {
        id: id2,
        name: String::from("Mint Cinnamon OS (Recovery)"),
        kind: EntryKind::Recovery,
        kernel_path: String::from("/boot/kernel"),
        initrd_path: String::from("/boot/initrd"),
        parameters: String::from("root=/dev/sda2 single recovery"),
        is_default: false,
        hidden: false,
        auto_detected: false,
        position: 1,
    });

    let id3 = NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed);
    state.entries.push(BootEntry {
        id: id3,
        name: String::from("UEFI Firmware Settings"),
        kind: EntryKind::FirmwareSettings,
        kernel_path: String::new(),
        initrd_path: String::new(),
        parameters: String::new(),
        is_default: false,
        hidden: false,
        auto_detected: true,
        position: 2,
    });

    state.changes += 1;
}

/// Return (entry_count, boot_event_count, total_boots, changes).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    let entries = state.entries.len();
    let events = state.boot_log.len();
    let boots = BOOT_COUNT.load(Ordering::Relaxed);
    (entries, events, boots, state.changes)
}

pub fn reset_stats() {
    BOOT_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.entries.clear();
    state.boot_log.clear();
    state.config = BootConfig {
        loader_type: BootloaderType::CustomUefi,
        timeout_secs: 5,
        console_mode: ConsoleMode::Graphical,
        show_boot_activity: false,
        secure_boot: false,
        grub_config_path: String::new(),
        esp_path: String::new(),
        dual_boot: false,
        gfx_mode: String::new(),
    };
    state.changes = 0;
    NEXT_ENTRY_ID.store(1, Ordering::Relaxed);
    NEXT_EVENT_ID.store(1, Ordering::Relaxed);
    BOOT_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: add entries.
    serial_println!("bootcfg::self_test 1: add entries");
    let e1 = add_entry("TestOS", EntryKind::SlateOs, "/boot/kernel", "/boot/initrd", "root=/dev/sda1 quiet", false)?;
    let e2 = add_entry("Linux", EntryKind::Linux, "/boot/vmlinuz", "/boot/initramfs", "root=/dev/sda2", true)?;
    assert_eq!(list_entries().len(), 2);
    // First entry is automatically default.
    let entry = get_entry(e1)?;
    assert!(entry.is_default);

    // Test 2: set default.
    serial_println!("bootcfg::self_test 2: set default");
    set_default(e2)?;
    let entry1 = get_entry(e1)?;
    let entry2 = get_entry(e2)?;
    assert!(!entry1.is_default);
    assert!(entry2.is_default);

    // Test 3: configuration settings.
    serial_println!("bootcfg::self_test 3: configuration");
    set_timeout(10)?;
    set_console_mode(ConsoleMode::Verbose)?;
    set_boot_activity(true)?;
    set_gfx_mode("2560x1440")?;
    let cfg = get_config();
    assert_eq!(cfg.timeout_secs, 10);
    assert_eq!(cfg.console_mode, ConsoleMode::Verbose);
    assert!(cfg.show_boot_activity);
    assert_eq!(cfg.gfx_mode, "2560x1440");
    // Invalid timeout rejected.
    assert!(set_timeout(999).is_err());

    // Test 4: entry properties.
    serial_println!("bootcfg::self_test 4: entry properties");
    set_parameters(e1, "root=/dev/sda1 debug")?;
    set_hidden(e2, true)?;
    let entry1 = get_entry(e1)?;
    let entry2 = get_entry(e2)?;
    assert_eq!(entry1.parameters, "root=/dev/sda1 debug");
    assert!(entry2.hidden);

    // Test 5: boot log.
    serial_println!("bootcfg::self_test 5: boot log");
    record_boot(e1, true, 3500, "normal")?;
    record_boot(e1, true, 3200, "normal")?;
    record_boot(e2, false, 0, "recovery")?;
    let log = boot_log(10);
    assert_eq!(log.len(), 3);
    assert!(log[0].success);
    assert!(!log[2].success);

    // Test 6: remove entry and default reassignment.
    serial_println!("bootcfg::self_test 6: remove entry");
    remove_entry(e2)?;
    assert_eq!(list_entries().len(), 1);
    // e1 should become default again.
    let entry1 = get_entry(e1)?;
    assert!(entry1.is_default);

    // Test 7: active entry.
    serial_println!("bootcfg::self_test 7: active entry");
    let active = get_active_entry().expect("should have active entry");
    assert_eq!(active.id, e1);
    assert_eq!(active.name, "TestOS");

    clear_all();
    serial_println!("bootcfg::self_test: all 7 tests passed");
    Ok(())
}
