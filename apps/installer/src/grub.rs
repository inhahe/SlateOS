//! GRUB integration for dual-boot scenarios.
//!
//! Detects existing GRUB installations and adds/removes/updates a menu entry
//! for SlateOS alongside any existing Linux (or other) entries. The module never
//! modifies `grub.cfg` directly — it writes a numbered script in `/etc/grub.d/`
//! and then invokes `update-grub` (or `grub2-mkconfig`) to regenerate the
//! master configuration.
//!
//! Two boot strategies are supported:
//!
//! * **Chainload** — GRUB chainloads the Limine UEFI bootloader, which in turn
//!   boots the kernel.  Recommended for UEFI systems.
//! * **Direct** — GRUB loads the kernel directly via `multiboot2`.  Useful on
//!   legacy-BIOS systems or when Limine is not installed.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during GRUB integration.
#[derive(Debug)]
pub enum GrubError {
    /// No GRUB installation was found on the system.
    GrubNotFound,
    /// The GRUB configuration directory or file is not writable.
    ConfigNotWritable(String),
    /// An SlateOS entry already exists when trying to install a new one.
    EntryAlreadyExists,
    /// No SlateOS entry exists when trying to update or remove one.
    EntryNotFound,
    /// Running `update-grub` / `grub2-mkconfig` failed.
    UpdateFailed(String),
    /// A provided path is syntactically or semantically invalid.
    InvalidPath(String),
    /// An underlying I/O error.
    Io(io::Error),
}

impl fmt::Display for GrubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GrubNotFound => write!(f, "no GRUB installation found"),
            Self::ConfigNotWritable(p) => write!(f, "GRUB config not writable: {p}"),
            Self::EntryAlreadyExists => write!(f, "Slate OS GRUB entry already exists"),
            Self::EntryNotFound => write!(f, "Slate OS GRUB entry not found"),
            Self::UpdateFailed(msg) => write!(f, "GRUB update failed: {msg}"),
            Self::InvalidPath(p) => write!(f, "invalid path: {p}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl From<io::Error> for GrubError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ============================================================================
// GRUB version / install info
// ============================================================================

/// Known GRUB versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrubVersion {
    /// GRUB 2.x (the modern, actively maintained branch).
    Grub2,
    /// GRUB Legacy (0.9x) — mostly extinct but still encountered.
    Legacy,
}

/// Information about an existing GRUB installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrubInstall {
    /// Which major version of GRUB is installed.
    pub version: GrubVersion,
    /// Absolute path to the primary `grub.cfg`.
    pub config_path: PathBuf,
    /// EFI system partition mount-point, if any.
    pub efi_partition: Option<PathBuf>,
}

impl GrubInstall {
    /// Returns `true` if the detected GRUB install is on a UEFI system.
    pub fn is_efi(&self) -> bool {
        self.efi_partition.is_some()
    }
}

// ============================================================================
// GRUB configuration snapshot
// ============================================================================

/// Represents the high-level GRUB configuration of interest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrubConfig {
    /// Absolute path to `grub.cfg`.
    pub grub_cfg_path: String,
    /// Absolute path to the custom-scripts directory (`/etc/grub.d/`).
    pub custom_dir: String,
    /// GRUB menu timeout in seconds.
    pub timeout: u32,
    /// Name of the default boot entry.
    pub default_entry: String,
    /// Whether `os-prober` is enabled.
    pub os_prober_enabled: bool,
}

// ============================================================================
// Menu entry types
// ============================================================================

/// Strategy for booting SlateOS from GRUB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrubEntryType {
    /// Chainload the Limine EFI bootloader (recommended for UEFI systems).
    Chainload,
    /// Boot the kernel directly via GRUB `multiboot2` (legacy BIOS or no Limine).
    Direct,
}

/// A GRUB menu entry for SlateOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrubEntry {
    /// Menu entry title shown in GRUB (e.g. "Slate OS 1.0").
    pub title: String,
    /// Path to the kernel binary or Limine EFI binary, relative to the root
    /// partition (e.g. `/EFI/slateos/limine.efi` or `/boot/kernel.elf`).
    pub kernel_path: String,
    /// GRUB device for the root partition (e.g. `(hd0,gpt3)`).
    pub root_partition: String,
    /// Partition UUID used by `search --fs-uuid`.
    pub uuid: String,
    /// Optional path to an initial ramdisk.
    pub initrd_path: Option<String>,
    /// Extra kernel command-line parameters.
    pub kernel_params: Vec<String>,
    /// Whether to chainload Limine or boot the kernel directly.
    pub entry_type: GrubEntryType,
}

// ============================================================================
// Entry generation
// ============================================================================

/// Name of the script file we place in `/etc/grub.d/`.
pub const CUSTOM_SCRIPT_NAME: &str = "40_slateos";

/// Marker embedded inside our generated script so we can reliably identify it.
const SLATEOS_MARKER: &str = "### Slate OS GRUB entry — managed by Slate OS installer ###";

/// Generate the GRUB `menuentry` text for the given [`GrubEntry`].
///
/// The returned string is a complete, self-contained `menuentry` block ready to
/// be embedded in a script that writes to stdout (the `/etc/grub.d/` pattern).
pub fn generate_entry(entry: &GrubEntry) -> String {
    match entry.entry_type {
        GrubEntryType::Chainload => generate_chainload_entry(entry),
        GrubEntryType::Direct => generate_direct_entry(entry),
    }
}

fn generate_chainload_entry(entry: &GrubEntry) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(&format!("menuentry \"{}\" {{\n", entry.title));
    out.push_str("    insmod part_gpt\n");
    out.push_str("    insmod chain\n");
    out.push_str("    insmod fat\n");

    // Prefer UUID-based search when a UUID is provided, fall back to device.
    if !entry.uuid.is_empty() {
        out.push_str(&format!(
            "    search --no-floppy --fs-uuid --set=root {}\n",
            entry.uuid
        ));
    } else {
        out.push_str(&format!("    set root='{}'\n", entry.root_partition));
    }

    out.push_str(&format!("    chainloader {}\n", entry.kernel_path));
    out.push_str("}\n");
    out
}

fn generate_direct_entry(entry: &GrubEntry) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(&format!("menuentry \"{}\" {{\n", entry.title));
    out.push_str("    insmod part_gpt\n");
    out.push_str("    insmod multiboot2\n");

    if !entry.uuid.is_empty() {
        out.push_str(&format!(
            "    search --no-floppy --fs-uuid --set=root {}\n",
            entry.uuid
        ));
    } else {
        out.push_str(&format!("    set root='{}'\n", entry.root_partition));
    }

    let params = entry.kernel_params.join(" ");
    if params.is_empty() {
        out.push_str(&format!("    multiboot2 {}\n", entry.kernel_path));
    } else {
        out.push_str(&format!(
            "    multiboot2 {} {}\n",
            entry.kernel_path, params
        ));
    }

    if let Some(ref initrd) = entry.initrd_path {
        out.push_str(&format!("    module2 {}\n", initrd));
    }

    out.push_str("}\n");
    out
}

/// Generate the full `/etc/grub.d/40_slateos` script content for the given entry.
///
/// The script is a standard GRUB custom-entry executable: it prints the menu
/// entry to stdout so that `update-grub` / `grub2-mkconfig` can incorporate it.
pub fn generate_custom_script(entry: &GrubEntry) -> String {
    let menu_entry = generate_entry(entry);
    let mut script = String::with_capacity(512);
    script.push_str("#!/bin/sh\n");
    script.push_str(&format!("{SLATEOS_MARKER}\n"));
    script.push_str("exec tail -n +3 \"$0\"\n");
    script.push_str(&menu_entry);
    script
}

// ============================================================================
// GRUB detection
// ============================================================================

/// Well-known paths where `grub.cfg` is typically found.
const GRUB_CFG_CANDIDATES: &[&str] = &[
    "/boot/grub/grub.cfg",
    "/boot/grub2/grub.cfg",
    "/boot/efi/EFI/fedora/grub.cfg",
    "/boot/efi/EFI/ubuntu/grub.cfg",
    "/boot/efi/EFI/debian/grub.cfg",
    "/boot/efi/EFI/centos/grub.cfg",
    "/boot/efi/EFI/BOOT/grub.cfg",
];

/// Well-known custom-script directories.
const GRUB_D_CANDIDATES: &[&str] = &["/etc/grub.d"];

/// Detects an existing GRUB installation on the live filesystem.
pub struct GrubDetector {
    /// Override for the filesystem root (useful for testing against a staging
    /// directory instead of `/`).
    root: PathBuf,
}

impl GrubDetector {
    /// Create a detector that scans the real root filesystem.
    pub fn new() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }

    /// Create a detector scoped to an arbitrary root path (for testing or
    /// chroot-based installs).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Scan well-known paths and return the first detected GRUB installation,
    /// or `None` if GRUB does not appear to be installed.
    pub fn detect(&self) -> Option<GrubInstall> {
        for candidate in GRUB_CFG_CANDIDATES {
            let full = self.root.join(candidate.trim_start_matches('/'));
            if full.is_file() {
                let version = if candidate.contains("grub2") {
                    GrubVersion::Grub2
                } else {
                    // Both `/boot/grub/grub.cfg` and EFI paths are GRUB 2 in
                    // practice; true legacy GRUB uses `menu.lst`.
                    GrubVersion::Grub2
                };

                let efi_partition = self.detect_efi_partition();

                return Some(GrubInstall {
                    version,
                    config_path: full,
                    efi_partition,
                });
            }
        }
        None
    }

    /// Detect the EFI system partition by checking `/sys/firmware/efi` and
    /// common EFI mount-points.
    fn detect_efi_partition(&self) -> Option<PathBuf> {
        let efi_fw = self.root.join("sys/firmware/efi");
        if !efi_fw.is_dir() {
            return None;
        }

        for candidate in &["/boot/efi", "/efi"] {
            let p = self.root.join(candidate.trim_start_matches('/'));
            if p.is_dir() {
                return Some(p);
            }
        }

        // Fallback: if /sys/firmware/efi exists we know it's UEFI, but we
        // could not locate the ESP mount-point.
        Some(self.root.join("boot/efi"))
    }

    /// Detect the path to the custom-scripts directory (e.g. `/etc/grub.d/`).
    pub fn detect_custom_dir(&self) -> Option<PathBuf> {
        for candidate in GRUB_D_CANDIDATES {
            let full = self.root.join(candidate.trim_start_matches('/'));
            if full.is_dir() {
                return Some(full);
            }
        }
        None
    }
}

impl Default for GrubDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GRUB configuration parsing (lightweight, cfg-only)
// ============================================================================

/// Extract a simple `key=value` or `key value` setting from `grub.cfg` or
/// `/etc/default/grub` content.  Returns the value as a string.
fn extract_grub_setting<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip comments.
        if trimmed.starts_with('#') {
            continue;
        }
        // `KEY=VALUE` form (e.g. GRUB_TIMEOUT=5)
        if let Some(rest) = trimmed.strip_prefix(key)
            && let Some(val) = rest.strip_prefix('=')
        {
            return Some(val.trim().trim_matches('"'));
        }
        // `set key=value` form (inside grub.cfg)
        if let Some(rest) = trimmed.strip_prefix("set ")
            && let Some(rest) = rest.strip_prefix(key)
            && let Some(val) = rest.strip_prefix('=')
        {
            return Some(val.trim().trim_matches('"').trim_matches('\''));
        }
    }
    None
}

/// Try to parse a [`GrubConfig`] from known filesystem paths.
pub fn parse_grub_config(root: &Path) -> Option<GrubConfig> {
    let detector = GrubDetector::with_root(root);
    let install = detector.detect()?;
    let custom_dir = detector
        .detect_custom_dir()
        .unwrap_or_else(|| root.join("etc/grub.d"));

    // Read grub.cfg to extract timeout / default.
    let cfg_text = fs::read_to_string(&install.config_path).ok()?;
    let timeout = extract_grub_setting(&cfg_text, "timeout")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(5);
    let default_entry = extract_grub_setting(&cfg_text, "default")
        .unwrap_or("0")
        .to_owned();

    // Check /etc/default/grub for os-prober.
    let defaults_path = root.join("etc/default/grub");
    let os_prober_enabled = if let Ok(defaults) = fs::read_to_string(defaults_path) {
        extract_grub_setting(&defaults, "GRUB_DISABLE_OS_PROBER") != Some("true")
    } else {
        true
    };

    Some(GrubConfig {
        grub_cfg_path: install.config_path.to_string_lossy().into_owned(),
        custom_dir: custom_dir.to_string_lossy().into_owned(),
        timeout,
        default_entry,
        os_prober_enabled,
    })
}

// ============================================================================
// UUID helpers
// ============================================================================

/// Validate that a string looks like a filesystem UUID.
///
/// Accepts both formats commonly emitted by `blkid`:
/// * GPT / ext4 style: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`
/// * FAT/vfat short form: `XXXX-XXXX`
pub fn is_valid_uuid(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Long form (36 chars with dashes).
    if s.len() == 36 {
        return s.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    }
    // Short form (9 chars with one dash, e.g. "1234-5678").
    if s.len() == 9 {
        return s.chars().enumerate().all(|(i, c)| {
            if i == 4 {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    }
    false
}

/// Extract a UUID from a `/dev/disk/by-uuid/` symlink target or `blkid`
/// output line.  Returns the UUID substring, if found.
pub fn extract_uuid(text: &str) -> Option<&str> {
    // Try to find a long-form UUID.
    for (i, _) in text.match_indices(|c: char| c.is_ascii_hexdigit()) {
        if i + 36 <= text.len() && is_valid_uuid(&text[i..i + 36]) {
            return Some(&text[i..i + 36]);
        }
    }
    // Try short-form.
    for (i, _) in text.match_indices(|c: char| c.is_ascii_hexdigit()) {
        if i + 9 <= text.len() && is_valid_uuid(&text[i..i + 9]) {
            return Some(&text[i..i + 9]);
        }
    }
    None
}

// ============================================================================
// GrubInstaller — entry lifecycle management
// ============================================================================

/// Manages the lifecycle of the SlateOS GRUB menu entry.
///
/// All mutations go through a numbered script in `/etc/grub.d/` (default:
/// `40_slateos`).  The installer never modifies `grub.cfg` directly.
pub struct GrubInstaller {
    /// Path to the custom-script directory (`/etc/grub.d/`).
    custom_dir: PathBuf,
}

impl GrubInstaller {
    /// Create an installer targeting the given custom-script directory.
    pub fn new(custom_dir: impl Into<PathBuf>) -> Self {
        Self {
            custom_dir: custom_dir.into(),
        }
    }

    /// Path to our custom script file.
    fn script_path(&self) -> PathBuf {
        self.custom_dir.join(CUSTOM_SCRIPT_NAME)
    }

    /// Install a new GRUB entry for SlateOS.
    ///
    /// Fails with [`GrubError::EntryAlreadyExists`] if the custom script file
    /// is already present.
    pub fn install(&self, entry: &GrubEntry) -> Result<(), GrubError> {
        let path = self.script_path();
        if path.exists() {
            return Err(GrubError::EntryAlreadyExists);
        }

        if !self.custom_dir.is_dir() {
            return Err(GrubError::InvalidPath(
                self.custom_dir.to_string_lossy().into_owned(),
            ));
        }

        let script = generate_custom_script(entry);
        fs::write(&path, &script)?;

        // Make the script executable (Unix).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Remove the SlateOS GRUB entry.
    ///
    /// Fails with [`GrubError::EntryNotFound`] if the script does not exist.
    pub fn uninstall(&self) -> Result<(), GrubError> {
        let path = self.script_path();
        if !path.exists() {
            return Err(GrubError::EntryNotFound);
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    /// Update an existing SlateOS entry with new parameters.
    ///
    /// Fails with [`GrubError::EntryNotFound`] if the script does not exist.
    pub fn update(&self, entry: &GrubEntry) -> Result<(), GrubError> {
        let path = self.script_path();
        if !path.exists() {
            return Err(GrubError::EntryNotFound);
        }

        let script = generate_custom_script(entry);
        fs::write(&path, &script)?;
        Ok(())
    }

    /// Check whether our custom-script file exists and contains the SlateOS
    /// marker.
    pub fn verify(&self) -> Result<bool, GrubError> {
        let path = self.script_path();
        if !path.exists() {
            return Ok(false);
        }
        let contents = fs::read_to_string(&path)?;
        Ok(contents.contains(SLATEOS_MARKER))
    }
}

// ============================================================================
// GrubUpdateRunner — config regeneration
// ============================================================================

/// Well-known commands for regenerating `grub.cfg`.
const UPDATE_COMMANDS: &[&[&str]] = &[
    &["update-grub"],
    &["grub2-mkconfig", "-o", "/boot/grub2/grub.cfg"],
    &["grub-mkconfig", "-o", "/boot/grub/grub.cfg"],
];

/// Trigger GRUB configuration regeneration.
pub struct GrubUpdateRunner {
    /// Optional output path override (used by `grub2-mkconfig -o`).
    output_path: Option<PathBuf>,
}

impl GrubUpdateRunner {
    /// Create a new runner with default settings.
    pub fn new() -> Self {
        Self { output_path: None }
    }

    /// Override the output path passed to `grub2-mkconfig -o`.
    pub fn with_output_path(output_path: impl Into<PathBuf>) -> Self {
        Self {
            output_path: Some(output_path.into()),
        }
    }

    /// Run the first available GRUB config-generation command.
    ///
    /// Returns `Ok(())` on success, or [`GrubError::UpdateFailed`] if the
    /// command exits with a non-zero status, or [`GrubError::GrubNotFound`] if
    /// no known command could be found.
    pub fn update_grub(&self) -> Result<(), GrubError> {
        for cmd_args in UPDATE_COMMANDS {
            let program = cmd_args[0];

            // Check whether the command exists on PATH.
            let which = Command::new("which").arg(program).output();
            let found = match which {
                Ok(out) => out.status.success(),
                Err(_) => false,
            };
            if !found {
                continue;
            }

            let mut cmd = Command::new(program);
            if cmd_args.len() > 1 {
                if let Some(ref out_path) = self.output_path {
                    // Use the caller-provided output path instead of the
                    // default baked into the candidate list.
                    cmd.arg("-o").arg(out_path);
                } else {
                    for arg in &cmd_args[1..] {
                        cmd.arg(arg);
                    }
                }
            }

            let output = cmd
                .output()
                .map_err(|e| GrubError::UpdateFailed(format!("failed to run {program}: {e}")))?;

            if output.status.success() {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GrubError::UpdateFailed(format!(
                "{program} exited with {}: {}",
                output.status, stderr
            )));
        }

        Err(GrubError::GrubNotFound)
    }

    /// Detect which update command is available without running it.
    pub fn detect_command() -> Option<&'static str> {
        for cmd_args in UPDATE_COMMANDS {
            let program = cmd_args[0];
            let which = Command::new("which").arg(program).output();
            if let Ok(out) = which
                && out.status.success()
            {
                return Some(program);
            }
        }
        None
    }
}

impl Default for GrubUpdateRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // -- helpers --------------------------------------------------------------

    /// Create a temporary directory tree that looks like a GRUB installation.
    fn make_grub_tree(dir: &Path, cfg_subpath: &str, cfg_content: &str) {
        let full = dir.join(cfg_subpath);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, cfg_content).unwrap();
    }

    fn sample_entry_chainload() -> GrubEntry {
        GrubEntry {
            title: "Slate OS 1.0".into(),
            kernel_path: "/EFI/slateos/limine.efi".into(),
            root_partition: "(hd0,gpt1)".into(),
            uuid: "ABCD-1234".into(),
            initrd_path: None,
            kernel_params: vec![],
            entry_type: GrubEntryType::Chainload,
        }
    }

    fn sample_entry_direct() -> GrubEntry {
        GrubEntry {
            title: "Slate OS 1.0".into(),
            kernel_path: "/boot/kernel.elf".into(),
            root_partition: "(hd0,gpt3)".into(),
            uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
            initrd_path: Some("/boot/initrd.img".into()),
            kernel_params: vec!["console=ttyS0".into(), "debug".into()],
            entry_type: GrubEntryType::Direct,
        }
    }

    // -- entry generation (chainload) -----------------------------------------

    #[test]
    fn test_generate_chainload_entry_with_uuid() {
        let entry = sample_entry_chainload();
        let text = generate_entry(&entry);

        assert!(text.contains("menuentry \"Slate OS 1.0\""));
        assert!(text.contains("insmod chain"));
        assert!(text.contains("insmod part_gpt"));
        assert!(text.contains("insmod fat"));
        assert!(text.contains("search --no-floppy --fs-uuid --set=root ABCD-1234"));
        assert!(text.contains("chainloader /EFI/slateos/limine.efi"));
        assert!(text.starts_with("menuentry"));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn test_generate_chainload_entry_without_uuid() {
        let mut entry = sample_entry_chainload();
        entry.uuid = String::new();
        let text = generate_entry(&entry);

        assert!(text.contains("set root='(hd0,gpt1)'"));
        assert!(!text.contains("search"));
    }

    // -- entry generation (direct) --------------------------------------------

    #[test]
    fn test_generate_direct_entry_with_uuid_and_params() {
        let entry = sample_entry_direct();
        let text = generate_entry(&entry);

        assert!(text.contains("menuentry \"Slate OS 1.0\""));
        assert!(text.contains("insmod multiboot2"));
        assert!(text.contains(
            "search --no-floppy --fs-uuid --set=root a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        ));
        assert!(text.contains("multiboot2 /boot/kernel.elf console=ttyS0 debug"));
        assert!(text.contains("module2 /boot/initrd.img"));
    }

    #[test]
    fn test_generate_direct_entry_without_initrd() {
        let mut entry = sample_entry_direct();
        entry.initrd_path = None;
        let text = generate_entry(&entry);

        assert!(!text.contains("module2"));
    }

    #[test]
    fn test_generate_direct_entry_no_params() {
        let mut entry = sample_entry_direct();
        entry.kernel_params.clear();
        let text = generate_entry(&entry);

        assert!(text.contains("multiboot2 /boot/kernel.elf\n"));
    }

    #[test]
    fn test_generate_direct_entry_without_uuid() {
        let mut entry = sample_entry_direct();
        entry.uuid = String::new();
        let text = generate_entry(&entry);

        assert!(text.contains("set root='(hd0,gpt3)'"));
    }

    // -- custom script generation ---------------------------------------------

    #[test]
    fn test_generate_custom_script_has_shebang_and_marker() {
        let entry = sample_entry_chainload();
        let script = generate_custom_script(&entry);

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(SLATEOS_MARKER));
        assert!(script.contains("exec tail"));
        assert!(script.contains("menuentry"));
    }

    #[test]
    fn test_custom_script_contains_full_entry() {
        let entry = sample_entry_direct();
        let script = generate_custom_script(&entry);
        let plain = generate_entry(&entry);

        assert!(script.contains(&plain));
    }

    // -- UUID validation ------------------------------------------------------

    #[test]
    fn test_valid_long_uuid() {
        assert!(is_valid_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    }

    #[test]
    fn test_valid_short_uuid() {
        assert!(is_valid_uuid("ABCD-1234"));
    }

    #[test]
    fn test_invalid_uuid_empty() {
        assert!(!is_valid_uuid(""));
    }

    #[test]
    fn test_invalid_uuid_bad_chars() {
        assert!(!is_valid_uuid("ZZZZ-1234"));
    }

    #[test]
    fn test_invalid_uuid_wrong_length() {
        assert!(!is_valid_uuid("a1b2c3d4-e5f6"));
    }

    #[test]
    fn test_invalid_uuid_missing_dashes() {
        assert!(!is_valid_uuid("a1b2c3d4e5f67890abcdef1234567890"));
    }

    // -- UUID extraction ------------------------------------------------------

    #[test]
    fn test_extract_uuid_long_form() {
        let line = "UUID=a1b2c3d4-e5f6-7890-abcd-ef1234567890 /boot ext4 defaults 0 2";
        assert_eq!(
            extract_uuid(line),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
    }

    #[test]
    fn test_extract_uuid_short_form() {
        let line = "/dev/disk/by-uuid/ABCD-1234 -> ../../sda1";
        assert_eq!(extract_uuid(line), Some("ABCD-1234"));
    }

    #[test]
    fn test_extract_uuid_none() {
        assert_eq!(extract_uuid("no uuid here"), None);
    }

    // -- GRUB detection -------------------------------------------------------

    #[test]
    fn test_detect_grub_boot_grub() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/grub/grub.cfg", "set timeout=5\n");

        let detector = GrubDetector::with_root(&tmp);
        let install = detector.detect().expect("should detect GRUB");

        assert_eq!(install.version, GrubVersion::Grub2);
        assert!(install.config_path.ends_with("boot/grub/grub.cfg"));
    }

    #[test]
    fn test_detect_grub_boot_grub2() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/grub2/grub.cfg", "set timeout=10\n");

        let detector = GrubDetector::with_root(&tmp);
        let install = detector.detect().expect("should detect GRUB2");
        assert_eq!(install.version, GrubVersion::Grub2);
    }

    #[test]
    fn test_detect_grub_none() {
        let tmp = tempdir();
        let detector = GrubDetector::with_root(&tmp);
        assert!(detector.detect().is_none());
    }

    #[test]
    fn test_detect_efi_partition() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/grub/grub.cfg", "");
        fs::create_dir_all(tmp.join("sys/firmware/efi")).unwrap();
        fs::create_dir_all(tmp.join("boot/efi")).unwrap();

        let detector = GrubDetector::with_root(&tmp);
        let install = detector.detect().expect("should detect");
        assert!(install.is_efi());
        assert!(
            install
                .efi_partition
                .as_ref()
                .unwrap()
                .ends_with("boot/efi")
        );
    }

    #[test]
    fn test_detect_no_efi() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/grub/grub.cfg", "");

        let detector = GrubDetector::with_root(&tmp);
        let install = detector.detect().expect("should detect");
        assert!(!install.is_efi());
    }

    #[test]
    fn test_detect_custom_dir() {
        let tmp = tempdir();
        fs::create_dir_all(tmp.join("etc/grub.d")).unwrap();

        let detector = GrubDetector::with_root(&tmp);
        let dir = detector.detect_custom_dir().expect("should find grub.d");
        assert!(dir.ends_with("etc/grub.d"));
    }

    // -- GRUB config parsing --------------------------------------------------

    #[test]
    fn test_extract_grub_setting_equals() {
        let content = "GRUB_TIMEOUT=10\nGRUB_DEFAULT=saved\n";
        assert_eq!(extract_grub_setting(content, "GRUB_TIMEOUT"), Some("10"));
        assert_eq!(extract_grub_setting(content, "GRUB_DEFAULT"), Some("saved"));
    }

    #[test]
    fn test_extract_grub_setting_set_form() {
        let content = "set timeout=5\nset default=\"0\"\n";
        assert_eq!(extract_grub_setting(content, "timeout"), Some("5"));
        assert_eq!(extract_grub_setting(content, "default"), Some("0"));
    }

    #[test]
    fn test_extract_grub_setting_skip_comments() {
        let content = "# GRUB_TIMEOUT=99\nGRUB_TIMEOUT=5\n";
        assert_eq!(extract_grub_setting(content, "GRUB_TIMEOUT"), Some("5"));
    }

    #[test]
    fn test_parse_grub_config_full() {
        let tmp = tempdir();
        make_grub_tree(
            &tmp,
            "boot/grub/grub.cfg",
            "set timeout=7\nset default=\"Slate OS\"\n",
        );
        fs::create_dir_all(tmp.join("etc/grub.d")).unwrap();
        fs::create_dir_all(tmp.join("etc/default")).unwrap();
        fs::write(
            tmp.join("etc/default/grub"),
            "GRUB_DISABLE_OS_PROBER=false\n",
        )
        .unwrap();

        let cfg = parse_grub_config(&tmp).expect("should parse");
        assert_eq!(cfg.timeout, 7);
        assert_eq!(cfg.default_entry, "Slate OS");
        assert!(cfg.os_prober_enabled);
    }

    #[test]
    fn test_parse_grub_config_os_prober_disabled() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/grub/grub.cfg", "set timeout=5\n");
        fs::create_dir_all(tmp.join("etc/grub.d")).unwrap();
        fs::create_dir_all(tmp.join("etc/default")).unwrap();
        fs::write(
            tmp.join("etc/default/grub"),
            "GRUB_DISABLE_OS_PROBER=true\n",
        )
        .unwrap();

        let cfg = parse_grub_config(&tmp).expect("should parse");
        assert!(!cfg.os_prober_enabled);
    }

    // -- GrubInstaller lifecycle ----------------------------------------------

    #[test]
    fn test_installer_install_and_verify() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry = sample_entry_chainload();

        installer.install(&entry).expect("install should succeed");
        assert!(installer.verify().expect("verify should not error"));

        let contents = fs::read_to_string(installer.script_path()).unwrap();
        assert!(contents.contains(SLATEOS_MARKER));
        assert!(contents.contains("chainloader /EFI/slateos/limine.efi"));
    }

    #[test]
    fn test_installer_install_already_exists() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry = sample_entry_chainload();

        installer.install(&entry).unwrap();
        let result = installer.install(&entry);
        assert!(matches!(result, Err(GrubError::EntryAlreadyExists)));
    }

    #[test]
    fn test_installer_uninstall() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry = sample_entry_chainload();

        installer.install(&entry).unwrap();
        installer.uninstall().expect("uninstall should succeed");
        assert!(!installer.verify().expect("verify should not error"));
    }

    #[test]
    fn test_installer_uninstall_not_found() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let result = installer.uninstall();
        assert!(matches!(result, Err(GrubError::EntryNotFound)));
    }

    #[test]
    fn test_installer_update() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry1 = sample_entry_chainload();
        installer.install(&entry1).unwrap();

        let mut entry2 = sample_entry_chainload();
        entry2.title = "Slate OS 2.0".into();
        installer.update(&entry2).expect("update should succeed");

        let contents = fs::read_to_string(installer.script_path()).unwrap();
        assert!(contents.contains("Slate OS 2.0"));
        assert!(!contents.contains("Slate OS 1.0"));
    }

    #[test]
    fn test_installer_update_not_found() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry = sample_entry_chainload();
        let result = installer.update(&entry);
        assert!(matches!(result, Err(GrubError::EntryNotFound)));
    }

    #[test]
    fn test_installer_install_missing_dir() {
        let tmp = tempdir();
        // Note: do NOT create the directory — it should fail.
        let missing = tmp.join("nonexistent");

        let installer = GrubInstaller::new(&missing);
        let entry = sample_entry_chainload();
        let result = installer.install(&entry);
        assert!(matches!(result, Err(GrubError::InvalidPath(_))));
    }

    #[test]
    fn test_verify_without_install() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        assert!(!installer.verify().expect("verify should not error"));
    }

    // -- GrubDetector EFI edge cases ------------------------------------------

    #[test]
    fn test_detect_efi_via_efi_mount() {
        let tmp = tempdir();
        make_grub_tree(&tmp, "boot/efi/EFI/fedora/grub.cfg", "set timeout=5\n");
        fs::create_dir_all(tmp.join("sys/firmware/efi")).unwrap();

        let detector = GrubDetector::with_root(&tmp);
        let install = detector.detect().expect("should detect via EFI");
        assert!(install.is_efi());
    }

    // -- misc helpers ---------------------------------------------------------

    /// Create a unique temp directory for a test.  Returns a `PathBuf` (not a
    /// guard) because this is test code and cleanup is optional.
    fn tempdir() -> PathBuf {
        let mut base = std::env::temp_dir();
        base.push(format!("slateos_grub_test_{}", std::process::id()));
        // Append a counter to avoid collisions between tests running in the
        // same process.
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        base.push(format!("{}", CTR.fetch_add(1, Ordering::Relaxed)));
        // Ensure a clean slate.
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("failed to create test temp dir");
        base
    }
}
