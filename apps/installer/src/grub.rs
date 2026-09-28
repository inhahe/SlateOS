//! GRUB integration for dual-boot scenarios.
//!
//! Detects existing GRUB installations and adds/removes/updates a menu entry
//! for Slate OS alongside any existing Linux (or other) entries. The module never
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

use pathtext::ShowPath;
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
    /// An Slate OS entry already exists when trying to install a new one.
    EntryAlreadyExists,
    /// No Slate OS entry exists when trying to update or remove one.
    EntryNotFound,
    /// The custom script's name is taken by a file this installer did not
    /// write -- it lacks the marker -- which is left alone rather than
    /// replaced or deleted. Carries the file's path, as shown.
    NotOurs(String),
    /// Running `update-grub` / `grub2-mkconfig` failed.
    UpdateFailed(String),
    /// A provided path is syntactically or semantically invalid.
    InvalidPath(String),
    /// A field of the [`GrubEntry`] cannot be safely rendered into a
    /// `menuentry` block (see [`GrubEntry::validate`]).
    InvalidEntry(String),
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
            Self::NotOurs(p) => write!(
                f,
                "{p} was not written by the Slate OS installer; leaving it alone"
            ),
            Self::UpdateFailed(msg) => write!(f, "GRUB update failed: {msg}"),
            Self::InvalidPath(p) => write!(f, "invalid path: {p}"),
            Self::InvalidEntry(msg) => write!(f, "invalid GRUB entry: {msg}"),
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
///
/// It held two more fields until 2026-09-25, `grub_cfg_path` and `custom_dir`,
/// each a path flattened to text through `to_string_lossy` and read by
/// nothing at all -- `known-issues.md`
/// `TD-C-THE-INSTALLER-RECORDS-A-GRUB-PATH-NOTHING-EVER-READS`. Deleted rather
/// than made byte-correct: a careful-looking dead field reads as one that
/// matters. The custom-scripts directory as a real path lives on
/// [`GrubInstaller`], which uses it; a consumer of `grub.cfg`'s path should
/// arrive with the field, as a `PathBuf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrubConfig {
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

/// Strategy for booting Slate OS from GRUB.
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

/// Quote a value for use inside a double-quoted GRUB string.
///
/// Everything we interpolate into a `menuentry` block is emitted inside
/// `"…"`.  Within such a string GRUB's lexer treats exactly three bytes
/// specially — `\` (escape), `"` (terminator) and `$` (variable expansion) —
/// so escaping those three with a backslash makes the value inert: it can no
/// longer terminate the string, start a new command, or expand a variable.
/// This mirrors `grub_quote()` in GRUB's own `util/grub-mkconfig_lib.in`.
///
/// Control characters are *not* handled here because they cannot be escaped
/// this way; [`GrubEntry::validate`] rejects them before we get here.
fn grub_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl GrubEntry {
    /// Check that every field can be faithfully rendered into a `menuentry`.
    ///
    /// A GRUB quoted string has no escape for a control character: a raw
    /// newline in a title would end the current command and let the remainder
    /// be parsed as fresh GRUB script, which runs at boot with full firmware
    /// privilege.  Since no legitimate title, path, UUID or kernel parameter
    /// contains one, we reject rather than silently mangle — the caller (an
    /// installer UI, or a title scraped from another OS's `/etc/os-release`)
    /// gets a hard error instead of a corrupted or hostile boot menu.
    pub fn validate(&self) -> Result<(), GrubError> {
        let fields: [(&str, &str); 4] = [
            ("title", &self.title),
            ("kernel_path", &self.kernel_path),
            ("root_partition", &self.root_partition),
            ("uuid", &self.uuid),
        ];
        for (name, value) in fields {
            check_no_control_chars(name, value)?;
        }
        if let Some(initrd) = &self.initrd_path {
            check_no_control_chars("initrd_path", initrd)?;
        }
        for param in &self.kernel_params {
            check_no_control_chars("kernel_params", param)?;
        }
        Ok(())
    }
}

fn check_no_control_chars(field: &str, value: &str) -> Result<(), GrubError> {
    if let Some(c) = value.chars().find(|c| c.is_control()) {
        return Err(GrubError::InvalidEntry(format!(
            "{field} contains a control character (U+{:04X})",
            c as u32
        )));
    }
    Ok(())
}

/// Generate the GRUB `menuentry` text for the given [`GrubEntry`].
///
/// The returned string is a complete, self-contained `menuentry` block ready to
/// be embedded in a script that writes to stdout (the `/etc/grub.d/` pattern).
///
/// # Errors
///
/// Returns [`GrubError::InvalidEntry`] if any field contains a control
/// character; see [`GrubEntry::validate`].
pub fn generate_entry(entry: &GrubEntry) -> Result<String, GrubError> {
    entry.validate()?;
    Ok(match entry.entry_type {
        GrubEntryType::Chainload => generate_chainload_entry(entry),
        GrubEntryType::Direct => generate_direct_entry(entry),
    })
}

/// Emit the `search`/`set root` line that selects the boot partition.
fn push_root_selection(out: &mut String, entry: &GrubEntry) {
    // Prefer UUID-based search when a UUID is provided, fall back to device.
    if entry.uuid.is_empty() {
        out.push_str(&format!(
            "    set root=\"{}\"\n",
            grub_quote(&entry.root_partition)
        ));
    } else {
        out.push_str(&format!(
            "    search --no-floppy --fs-uuid --set=root \"{}\"\n",
            grub_quote(&entry.uuid)
        ));
    }
}

fn generate_chainload_entry(entry: &GrubEntry) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(&format!("menuentry \"{}\" {{\n", grub_quote(&entry.title)));
    out.push_str("    insmod part_gpt\n");
    out.push_str("    insmod chain\n");
    out.push_str("    insmod fat\n");

    push_root_selection(&mut out, entry);

    out.push_str(&format!(
        "    chainloader \"{}\"\n",
        grub_quote(&entry.kernel_path)
    ));
    out.push_str("}\n");
    out
}

fn generate_direct_entry(entry: &GrubEntry) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(&format!("menuentry \"{}\" {{\n", grub_quote(&entry.title)));
    out.push_str("    insmod part_gpt\n");
    out.push_str("    insmod multiboot2\n");

    push_root_selection(&mut out, entry);

    // Each parameter is quoted individually: they are separate arguments, and
    // quoting the joined string would collapse them into one.  Quoting them
    // individually also keeps a parameter that itself contains a space intact,
    // which the previous bare join silently split in two.
    out.push_str(&format!(
        "    multiboot2 \"{}\"",
        grub_quote(&entry.kernel_path)
    ));
    for param in &entry.kernel_params {
        out.push_str(&format!(" \"{}\"", grub_quote(param)));
    }
    out.push('\n');

    if let Some(ref initrd) = entry.initrd_path {
        out.push_str(&format!("    module2 \"{}\"\n", grub_quote(initrd)));
    }

    out.push_str("}\n");
    out
}

/// Generate the full `/etc/grub.d/40_slateos` script content for the given entry.
///
/// The script is a standard GRUB custom-entry executable: it prints the menu
/// entry to stdout so that `update-grub` / `grub2-mkconfig` can incorporate it.
///
/// # Errors
///
/// Propagates [`GrubError::InvalidEntry`] from [`generate_entry`].
pub fn generate_custom_script(entry: &GrubEntry) -> Result<String, GrubError> {
    let menu_entry = generate_entry(entry)?;
    let mut script = String::with_capacity(512);
    script.push_str("#!/bin/sh\n");
    script.push_str(&format!("{SLATEOS_MARKER}\n"));
    script.push_str("exec tail -n +3 \"$0\"\n");
    script.push_str(&menu_entry);
    Ok(script)
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
        if let Some(cand) = i.checked_add(36).and_then(|e| text.get(i..e))
            && is_valid_uuid(cand)
        {
            return Some(cand);
        }
    }
    // Try short-form.
    for (i, _) in text.match_indices(|c: char| c.is_ascii_hexdigit()) {
        if let Some(cand) = i.checked_add(9).and_then(|e| text.get(i..e))
            && is_valid_uuid(cand)
        {
            return Some(cand);
        }
    }
    None
}

// ============================================================================
// GrubInstaller — entry lifecycle management
// ============================================================================

/// What the custom script's place in the scripts directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// Nothing: there is no Slate OS entry.
    Absent,
    /// The script this installer writes, marker and all.
    Ours,
    /// A file of the same name that this installer did not write.
    Foreign,
}

/// Manages the lifecycle of the Slate OS GRUB menu entry.
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
    /// Fails with [`GrubError::EntryAlreadyExists`] if our script is already
    /// there, and [`GrubError::NotOurs`] if a file of its name is someone
    /// else's -- which is not written over.
    pub fn install(&self, entry: &GrubEntry) -> Result<(), GrubError> {
        // Reject an unrenderable entry before touching the filesystem.
        entry.validate()?;

        if !self.custom_dir.is_dir() {
            return Err(GrubError::InvalidPath(self.custom_dir.shown().to_string()));
        }

        let path = self.script_path();
        match self.state()? {
            EntryState::Absent => {}
            EntryState::Ours => return Err(GrubError::EntryAlreadyExists),
            EntryState::Foreign => return Err(GrubError::NotOurs(path.shown().to_string())),
        }

        let script = generate_custom_script(entry)?;
        // Crash-safe: `fs::write` truncates first, so an interrupted write
        // leaves a *partial shell script* in the custom-script directory —
        // and `grub-mkconfig` executes whatever is there. A half-written
        // script does not fail loudly; it silently contributes a malformed
        // `grub.cfg`, which is not discovered until the next boot.
        safeio::write_str_atomically(&path, &script)?;

        // Make the script executable (Unix).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Remove the Slate OS GRUB entry.
    ///
    /// Fails with [`GrubError::EntryNotFound`] if the script does not exist.
    ///
    /// A file of that name without the marker is someone else's and is not
    /// deleted ([`GrubError::NotOurs`]).
    pub fn uninstall(&self) -> Result<(), GrubError> {
        let path = self.script_path();
        self.expect_ours(&path)?;
        fs::remove_file(&path)?;
        Ok(())
    }

    /// Update an existing Slate OS entry with new parameters.
    ///
    /// Fails with [`GrubError::EntryNotFound`] if the script does not exist,
    /// and [`GrubError::NotOurs`] if a file of its name is not ours -- which
    /// is not overwritten.
    pub fn update(&self, entry: &GrubEntry) -> Result<(), GrubError> {
        // Reject an unrenderable entry before touching the filesystem.
        entry.validate()?;

        let path = self.script_path();
        self.expect_ours(&path)?;

        let script = generate_custom_script(entry)?;
        // Crash-safe, and more important here than in `install`: this path
        // overwrites an entry that is *already working*. A truncated write
        // would replace a boot entry that boots with one that does not, which
        // is a machine the user cannot start to fix it from.
        safeio::write_str_atomically(&path, &script)?;
        Ok(())
    }

    /// Check whether our custom-script file exists and contains the Slate OS
    /// marker.
    pub fn verify(&self) -> Result<bool, GrubError> {
        Ok(self.state()? == EntryState::Ours)
    }

    /// What the custom script's place holds: nothing, our script, or a file
    /// of the same name that is someone else's.
    ///
    /// Read as bytes: a file that is not text is not ours, and saying so is
    /// the answer, not an error.
    pub fn state(&self) -> Result<EntryState, GrubError> {
        let bytes = match fs::read(self.script_path()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(EntryState::Absent),
            Err(e) => return Err(e.into()),
        };
        let marker = SLATEOS_MARKER.as_bytes();
        Ok(if bytes.windows(marker.len()).any(|w| w == marker) {
            EntryState::Ours
        } else {
            EntryState::Foreign
        })
    }

    /// `Ok` when the script at `path` is ours; the error to give when it is
    /// missing or someone else's.
    fn expect_ours(&self, path: &Path) -> Result<(), GrubError> {
        match self.state()? {
            EntryState::Ours => Ok(()),
            EntryState::Absent => Err(GrubError::EntryNotFound),
            EntryState::Foreign => Err(GrubError::NotOurs(path.shown().to_string())),
        }
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

/// Where `program` is on `PATH`, as a shell would find it.
///
/// Searched here rather than by running `which`, which minimal installs of
/// Fedora and Arch do not ship: asking it there reported GRUB's tools
/// missing when they were not.
fn find_program(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_in(std::env::split_paths(&path), program)
}

/// The first of `dirs` holding an executable `program`, searching only the
/// [`searchable`] ones.
fn find_in(dirs: impl IntoIterator<Item = PathBuf>, program: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .filter(|dir| searchable(dir))
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

/// Whether a directory on `PATH` is searched for GRUB's tools: only an
/// absolute one. An empty entry, which POSIX reads as the current directory,
/// `.`, or any other relative path is passed over: these tools run as root,
/// and a `PATH` that reaches into wherever the command was started is a
/// classic way to run someone else's `update-grub` instead.
fn searchable(dir: &Path) -> bool {
    dir.is_absolute()
}

/// A file that can be run: on Unix, a regular file with an execute bit.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// A file that can be run: elsewhere, a regular file.
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

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
    /// Returns the command's name on success, [`GrubError::UpdateFailed`] if
    /// it exits with a non-zero status, or [`GrubError::GrubNotFound`] if no
    /// known command could be found.
    pub fn update_grub(&self) -> Result<&'static str, GrubError> {
        for cmd_args in UPDATE_COMMANDS {
            // An entry in the table with no program is a table bug, not a
            // system state — skip it rather than reporting a GRUB failure the
            // user could act on.
            let Some(program) = cmd_args.first().copied() else {
                continue;
            };

            // Run what was found, by its full path: a second lookup by name
            // could find something else.
            let Some(found) = find_program(program) else {
                continue;
            };

            let mut cmd = Command::new(found);
            if cmd_args.len() > 1 {
                if let Some(ref out_path) = self.output_path {
                    // Use the caller-provided output path instead of the
                    // default baked into the candidate list.
                    cmd.arg("-o").arg(out_path);
                } else {
                    for arg in cmd_args.get(1..).unwrap_or_default() {
                        cmd.arg(arg);
                    }
                }
            }

            let output = cmd
                .output()
                .map_err(|e| GrubError::UpdateFailed(format!("failed to run {program}: {e}")))?;

            if output.status.success() {
                return Ok(program);
            }

            // A diagnostic, decoded where it is written: what a failing
            // tool printed is read by a person, and lines stay lines.
            return Err(GrubError::UpdateFailed(format!(
                "{program} exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Err(GrubError::GrubNotFound)
    }

    /// Detect which update command is available without running it.
    pub fn detect_command() -> Option<&'static str> {
        for cmd_args in UPDATE_COMMANDS {
            // An entry in the table with no program is a table bug, not a
            // system state — skip it rather than reporting a GRUB failure the
            // user could act on.
            let Some(program) = cmd_args.first().copied() else {
                continue;
            };
            if find_program(program).is_some() {
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
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(text.contains("menuentry \"Slate OS 1.0\""));
        assert!(text.contains("insmod chain"));
        assert!(text.contains("insmod part_gpt"));
        assert!(text.contains("insmod fat"));
        assert!(text.contains("search --no-floppy --fs-uuid --set=root \"ABCD-1234\""));
        assert!(text.contains("chainloader \"/EFI/slateos/limine.efi\""));
        assert!(text.starts_with("menuentry"));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn test_generate_chainload_entry_without_uuid() {
        let mut entry = sample_entry_chainload();
        entry.uuid = String::new();
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(text.contains("set root=\"(hd0,gpt1)\""));
        assert!(!text.contains("search"));
    }

    // -- entry generation (direct) --------------------------------------------

    #[test]
    fn test_generate_direct_entry_with_uuid_and_params() {
        let entry = sample_entry_direct();
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(text.contains("menuentry \"Slate OS 1.0\""));
        assert!(text.contains("insmod multiboot2"));
        assert!(text.contains(
            "search --no-floppy --fs-uuid --set=root \"a1b2c3d4-e5f6-7890-abcd-ef1234567890\""
        ));
        assert!(text.contains("multiboot2 \"/boot/kernel.elf\" \"console=ttyS0\" \"debug\""));
        assert!(text.contains("module2 \"/boot/initrd.img\""));
    }

    #[test]
    fn test_generate_direct_entry_without_initrd() {
        let mut entry = sample_entry_direct();
        entry.initrd_path = None;
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(!text.contains("module2"));
    }

    #[test]
    fn test_generate_direct_entry_no_params() {
        let mut entry = sample_entry_direct();
        entry.kernel_params.clear();
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(text.contains("multiboot2 \"/boot/kernel.elf\"\n"));
    }

    #[test]
    fn test_generate_direct_entry_without_uuid() {
        let mut entry = sample_entry_direct();
        entry.uuid = String::new();
        let text = generate_entry(&entry).expect("sample entry is renderable");

        assert!(text.contains("set root=\"(hd0,gpt3)\""));
    }

    // -- custom script generation ---------------------------------------------

    #[test]
    fn test_generate_custom_script_has_shebang_and_marker() {
        let entry = sample_entry_chainload();
        let script = generate_custom_script(&entry).expect("sample entry is renderable");

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(SLATEOS_MARKER));
        assert!(script.contains("exec tail"));
        assert!(script.contains("menuentry"));
    }

    #[test]
    fn test_custom_script_contains_full_entry() {
        let entry = sample_entry_direct();
        let script = generate_custom_script(&entry).expect("sample entry is renderable");
        let plain = generate_entry(&entry).expect("sample entry is renderable");

        assert!(script.contains(&plain));
    }

    // -- injection resistance --------------------------------------------------
    //
    // The generated block is fed to GRUB, which executes it at boot with full
    // firmware privilege — before any OS, and so before any OS-level security
    // boundary exists.  A title is not necessarily typed by the person at the
    // keyboard: `os-prober` scrapes one out of another partition's
    // `/etc/os-release`, so it can be attacker-controlled on a multi-boot
    // machine.  Every field must therefore be inert.

    /// A `"` in a title must not be able to close the `menuentry` string and
    /// let the rest of the title be parsed as GRUB script.
    #[test]
    fn a_quote_in_the_title_cannot_open_a_second_menuentry() {
        let mut entry = sample_entry_chainload();
        entry.title = r#"Slate" {} menuentry "Backdoor"#.into();
        let text = generate_entry(&entry).expect("quotes are escapable, not rejected");

        // Exactly one menuentry *command* was emitted.  A bare substring count
        // would be 2 here and still be correct: the escaped title legitimately
        // contains the text `menuentry `.  What matters is that only one line
        // *begins* a menuentry command — the other occurrence is inert payload
        // inside a quoted string.
        assert_eq!(
            text.lines()
                .filter(|l| l.trim_start().starts_with("menuentry "))
                .count(),
            1,
            "a second menuentry was injected:\n{text}"
        );
        // The quotes survive, escaped, so the title still round-trips visually.
        assert!(
            text.starts_with(r#"menuentry "Slate\" {} menuentry \"Backdoor" {"#),
            "unexpected rendering:\n{text}"
        );
    }

    /// `$` must not expand: a title must not be able to read GRUB variables.
    #[test]
    fn a_dollar_sign_in_the_title_is_not_expanded() {
        let mut entry = sample_entry_chainload();
        entry.title = "Slate $root $(cmd)".into();
        let text = generate_entry(&entry).expect("dollar signs are escapable");

        assert!(
            text.contains(r#"menuentry "Slate \$root \$(cmd)""#),
            "{text}"
        );
    }

    /// A trailing `\` must not escape the closing quote of the string.
    #[test]
    fn a_backslash_cannot_escape_the_closing_quote() {
        let mut entry = sample_entry_chainload();
        entry.title = r"Slate\".into();
        let text = generate_entry(&entry).expect("backslashes are escapable");

        assert!(text.starts_with(r#"menuentry "Slate\\" {"#), "{text}");
    }

    /// A newline cannot be escaped inside a GRUB string, so it must be
    /// rejected outright rather than emitted and treated as a command
    /// separator.
    #[test]
    fn a_newline_in_a_field_is_rejected_rather_than_injected() {
        for (name, mut entry) in [
            ("title", sample_entry_direct()),
            ("kernel_path", sample_entry_direct()),
            ("root_partition", sample_entry_direct()),
            ("uuid", sample_entry_direct()),
            ("initrd_path", sample_entry_direct()),
            ("kernel_params", sample_entry_direct()),
        ] {
            let hostile = "x\nlinux /evil\n".to_string();
            match name {
                "title" => entry.title = hostile,
                "kernel_path" => entry.kernel_path = hostile,
                "root_partition" => entry.root_partition = hostile,
                "uuid" => entry.uuid = hostile,
                "initrd_path" => entry.initrd_path = Some(hostile),
                _ => entry.kernel_params = vec![hostile],
            }
            let err =
                generate_entry(&entry).expect_err(&format!("a newline in {name} must be rejected"));
            assert!(
                matches!(err, GrubError::InvalidEntry(ref m) if m.contains(name)),
                "{name}: wrong error {err:?}"
            );
        }
    }

    /// `install` must refuse an unrenderable entry *before* writing anything.
    #[test]
    fn install_rejects_a_hostile_entry_without_writing_a_file() {
        let dir = tempdir();
        let installer = GrubInstaller::new(&dir);

        let mut entry = sample_entry_chainload();
        entry.title = "Slate\nlinux /evil".into();

        assert!(matches!(
            installer.install(&entry),
            Err(GrubError::InvalidEntry(_))
        ));
        assert!(
            !dir.join(CUSTOM_SCRIPT_NAME).exists(),
            "a rejected entry must leave no script behind"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// A kernel parameter containing a space is one argument, not two.
    #[test]
    fn a_kernel_parameter_containing_a_space_stays_one_argument() {
        let mut entry = sample_entry_direct();
        entry.kernel_params = vec!["init=/bin/my init".into()];
        let text = generate_entry(&entry).expect("spaces are fine");

        assert!(text.contains(r#""init=/bin/my init""#), "{text}");
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
        assert!(contents.contains("chainloader \"/EFI/slateos/limine.efi\""));
    }

    /// Both GRUB writes go through `safeio`, not `std::fs::write`.
    ///
    /// A successful atomic write and a successful truncating write leave
    /// identical bytes at an identical path; they differ only when the write
    /// is interrupted, which no portable test can stage. So the routing itself
    /// is asserted, via `safeio`'s `audit` counters.
    ///
    /// The stakes here are the highest of any adopter. A half-written file in
    /// the custom-script directory is a *partial shell script*, and
    /// `grub-mkconfig` executes whatever it finds there — it does not fail
    /// loudly, it silently contributes a malformed `grub.cfg`. For `update`
    /// the entry being overwritten is one that already boots, so a truncated
    /// write replaces a working boot entry with a broken one: a machine the
    /// user cannot start in order to fix it.
    ///
    /// `install` and `update` are asserted separately because they are
    /// separate call sites; fixing one and missing the other is the likely
    /// regression.
    ///
    /// The counters are process-global and tests run in parallel, so each
    /// check compares a before and after reading rather than an absolute.
    #[test]
    fn both_grub_writes_go_through_safeio() {
        let tmp = tempdir();
        fs::create_dir_all(&tmp).unwrap();

        let installer = GrubInstaller::new(&tmp);
        let entry = sample_entry_chainload();

        let before = safeio::writes_performed();
        installer.install(&entry).expect("install");
        assert!(
            safeio::writes_performed() > before,
            "install did not go through safeio -- it must not use std::fs::write"
        );

        let mut updated = sample_entry_chainload();
        updated.title = "Slate OS 2.0".into();
        let before = safeio::writes_performed();
        installer.update(&updated).expect("update");
        assert!(
            safeio::writes_performed() > before,
            "update did not go through safeio -- it must not use std::fs::write"
        );

        // The script is still the one GRUB should run, not merely present.
        let contents = fs::read_to_string(installer.script_path()).unwrap();
        assert!(contents.contains(SLATEOS_MARKER));
        assert!(contents.contains("Slate OS 2.0"));
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

    #[test]
    fn a_script_of_our_name_that_is_not_ours_is_left_alone() {
        // `40_slateos` written by something else -- another tool, or a user's
        // own entry -- has no marker. Replacing it or deleting it would
        // destroy what someone else put there; adding over it would too.
        let tmp = tempdir();
        let installer = GrubInstaller::new(&tmp);
        let theirs = "#!/bin/sh\necho 'menuentry \"Mine\" {}'\n";
        fs::write(installer.script_path(), theirs).unwrap();

        assert_eq!(installer.state().unwrap(), EntryState::Foreign);
        assert!(!installer.verify().unwrap());
        assert!(matches!(
            installer.install(&sample_entry_chainload()),
            Err(GrubError::NotOurs(_))
        ));
        assert!(matches!(
            installer.update(&sample_entry_chainload()),
            Err(GrubError::NotOurs(_))
        ));
        assert!(matches!(installer.uninstall(), Err(GrubError::NotOurs(_))));
        assert_eq!(fs::read_to_string(installer.script_path()).unwrap(), theirs);
    }

    #[test]
    fn a_script_that_is_not_text_is_someone_elses_rather_than_an_error() {
        let tmp = tempdir();
        let installer = GrubInstaller::new(&tmp);
        fs::write(installer.script_path(), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert_eq!(installer.state().unwrap(), EntryState::Foreign);
    }

    #[test]
    fn the_entrys_state_follows_its_lifecycle() {
        let tmp = tempdir();
        let installer = GrubInstaller::new(&tmp);
        assert_eq!(installer.state().unwrap(), EntryState::Absent);
        installer.install(&sample_entry_chainload()).unwrap();
        assert_eq!(installer.state().unwrap(), EntryState::Ours);
        installer.uninstall().unwrap();
        assert_eq!(installer.state().unwrap(), EntryState::Absent);
    }

    // -- finding GRUB's tools -------------------------------------------------

    /// A file that `is_executable` accepts on this host.
    fn make_program(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn a_program_is_found_in_the_first_directory_that_has_it() {
        let tmp = tempdir();
        let (a, b, c) = (tmp.join("a"), tmp.join("b"), tmp.join("c"));
        fs::create_dir_all(&a).unwrap();
        let in_b = make_program(&b, "update-grub");
        make_program(&c, "update-grub");
        assert_eq!(
            find_in([a.clone(), b, c], "update-grub"),
            Some(in_b),
            "the search runs in PATH's order and stops at the first"
        );
        assert_eq!(find_in([a], "update-grub"), None);
    }

    #[test]
    fn only_an_absolute_directory_on_path_is_searched() {
        // An empty PATH entry means the current directory to POSIX, and so
        // does `.`; running GRUB's tools as root from wherever the command was
        // started is how someone else's `update-grub` gets run.
        for relative in ["", ".", "bin", "../sbin"] {
            assert!(
                !searchable(Path::new(relative)),
                "{relative:?} was searched"
            );
        }
        assert!(searchable(&tempdir()));
    }

    #[test]
    fn a_directory_on_path_that_is_not_absolute_is_passed_over() {
        // cargo runs a crate's tests in the crate's own directory, whose
        // `Cargo.toml` is there to be found through a relative entry -- on a
        // host where any file can be run -- if the search looked in one.
        assert!(
            Path::new("Cargo.toml").is_file(),
            "the tests run in the crate's directory"
        );
        for relative in [PathBuf::new(), PathBuf::from("."), PathBuf::from("src/..")] {
            assert_eq!(
                find_in([relative.clone()], "Cargo.toml"),
                None,
                "{relative:?}"
            );
        }
        let tmp = tempdir();
        make_program(&tmp, "update-grub");
        assert_eq!(
            find_in([PathBuf::new(), tmp.clone()], "update-grub"),
            Some(tmp.join("update-grub"))
        );
    }

    #[test]
    fn a_directory_of_the_programs_name_is_not_the_program() {
        let tmp = tempdir();
        fs::create_dir_all(tmp.join("update-grub")).unwrap();
        assert_eq!(find_in([tmp], "update-grub"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_an_execute_bit_is_not_the_program() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir();
        let path = make_program(&tmp, "update-grub");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(find_in([tmp], "update-grub"), None);
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
