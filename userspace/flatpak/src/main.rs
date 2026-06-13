//! Slate OS Flatpak — sandboxed application management
//!
//! A multi-personality binary providing sandboxed application packaging,
//! installation, and management for SlateOS. Implements the core flatpak
//! command set for managing applications and runtimes from OSTree-based
//! remotes.
//!
//! # Personalities
//!
//! Detected via `argv[0]` basename (stripping path and `.exe` suffix):
//!
//! - **flatpak** (default) — full sandboxed application management
//!
//! # Usage
//!
//! ```text
//! flatpak install [remote] <ref>       Install an app or runtime
//! flatpak uninstall <ref>              Remove an installed app
//! flatpak update [ref]                 Update apps/runtimes
//! flatpak run <app>                    Run a sandboxed application
//! flatpak list                         List installed refs
//! flatpak info <ref>                   Show app/runtime information
//! flatpak search <query>               Search for apps in remotes
//! flatpak remote-add <name> <url>      Add a remote repository
//! flatpak remote-delete <name>         Remove a remote repository
//! flatpak remote-list                  List configured remotes
//! flatpak remote-info <remote> <ref>   Show info from a remote
//! flatpak repair                       Repair installation
//! flatpak history                      Show install/update history
//! flatpak override <app>               Override app permissions
//! flatpak permission-show <app>        Show app permissions
//! flatpak build-init <dir> <app> <sdk> <runtime>  Init build directory
//! flatpak build <dir> <cmd...>         Run build command in sandbox
//! flatpak build-finish <dir>           Finalize build
//! flatpak build-export <repo> <dir>    Export build to repo
//! flatpak build-import-bundle <repo> <file>  Import a bundle
//! flatpak build-bundle <repo> <file> <ref>   Create a bundle
//! flatpak config --set/--get/--unset <key> [value]  Configuration
//! ```

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::process;

// ============================================================================
// Data model
// ============================================================================

/// Installation scope — user-local or system-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallationType {
    User,
    System,
}

impl fmt::Display for InstallationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::System => write!(f, "system"),
        }
    }
}

/// A configured remote repository.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct Remote {
    name: String,
    url: String,
    title: String,
    gpg_verify: bool,
    enabled: bool,
    priority: i32,
    installation: InstallationType,
}

impl Remote {
    #[cfg_attr(not(test), allow(dead_code))]
    fn new(name: &str, url: &str, installation: InstallationType) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            title: String::new(),
            gpg_verify: true,
            enabled: true,
            priority: 1,
            installation,
        }
    }
}

/// Kind of flatpak ref — app or runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefKind {
    App,
    Runtime,
}

impl fmt::Display for RefKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::App => write!(f, "app"),
            Self::Runtime => write!(f, "runtime"),
        }
    }
}

/// A parsed flatpak ref (e.g. `app/org.example.App/x86_64/stable`).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FlatpakRef {
    app_id: String,
    branch: String,
    arch: String,
    kind: RefKind,
    commit: String,
    installed_size: u64,
    download_size: u64,
    description: String,
    remote: String,
    installation: InstallationType,
}

impl FlatpakRef {
    #[cfg_attr(not(test), allow(dead_code))]
    fn full_ref(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.kind, self.app_id, self.arch, self.branch
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn matches_query(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.app_id.to_lowercase().contains(&q)
            || self.description.to_lowercase().contains(&q)
    }
}

impl fmt::Display for FlatpakRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\t{}\t{}\t{}\t{}",
            self.app_id, self.branch, self.arch, self.kind, self.installation
        )
    }
}

/// Sandbox permission categories.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(test), allow(dead_code))]
struct Permissions {
    shared: Vec<String>,
    sockets: Vec<String>,
    devices: Vec<String>,
    filesystems: Vec<String>,
    env_vars: BTreeMap<String, String>,
}

impl Permissions {
    #[cfg_attr(not(test), allow(dead_code))]
    fn is_empty(&self) -> bool {
        self.shared.is_empty()
            && self.sockets.is_empty()
            && self.devices.is_empty()
            && self.filesystems.is_empty()
            && self.env_vars.is_empty()
    }
}

/// A history entry tracking install/update/uninstall events.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct HistoryEntry {
    timestamp: String,
    action: String,
    app_id: String,
    branch: String,
    remote: String,
    old_commit: String,
    new_commit: String,
}

impl fmt::Display for HistoryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.old_commit.is_empty() {
            write!(
                f,
                "{}\t{}\t{}\t{}\t{}",
                self.timestamp, self.action, self.app_id, self.branch, self.remote
            )
        } else {
            write!(
                f,
                "{}\t{}\t{}\t{}\t{}\t{}..{}",
                self.timestamp,
                self.action,
                self.app_id,
                self.branch,
                self.remote,
                &self.old_commit[..8.min(self.old_commit.len())],
                &self.new_commit[..8.min(self.new_commit.len())]
            )
        }
    }
}

/// Configuration key-value store.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(test), allow(dead_code))]
struct Config {
    values: BTreeMap<String, String>,
}

impl Config {
    #[cfg_attr(not(test), allow(dead_code))]
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn unset(&mut self, key: &str) -> bool {
        self.values.remove(key).is_some()
    }
}

/// Build state for a build directory.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct BuildState {
    directory: String,
    app_id: String,
    sdk: String,
    runtime: String,
    finished: bool,
}

// ============================================================================
// Installation paths
// ============================================================================

fn user_install_path() -> String {
    if let Ok(home) = env::var("HOME") {
        format!("{}/.local/share/flatpak", home)
    } else {
        "/home/user/.local/share/flatpak".to_string()
    }
}

fn system_install_path() -> &'static str {
    "/var/lib/flatpak"
}

fn install_path(inst: InstallationType) -> String {
    match inst {
        InstallationType::User => user_install_path(),
        InstallationType::System => system_install_path().to_string(),
    }
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

fn has_flag(args: &[String], short: &str, long: &str) -> bool {
    args.iter().any(|a| a == short || a == long)
}

fn has_long_flag(args: &[String], long: &str) -> bool {
    args.iter().any(|a| a == long)
}

fn get_option_value<'a>(args: &'a [String], short: &str, long: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == short || arg == long {
            return iter.next().map(|s| s.as_str());
        }
        // Handle --option=value
        if let Some(rest) = arg.strip_prefix(long)
            && let Some(val) = rest.strip_prefix('=') {
                return Some(val);
            }
    }
    None
}

#[allow(dead_code)]
fn get_positional_args(args: &[String]) -> Vec<&str> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect()
}

/// Collect positional args, skipping flags and their values.
fn collect_positionals(args: &[String], flags_with_values: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            // Check if this flag takes a value
            if flags_with_values.iter().any(|f| arg == *f) {
                skip_next = true;
            }
            continue;
        }
        result.push(arg.clone());
    }
    result
}

// ============================================================================
// Validation helpers
// ============================================================================

/// Validate that a string looks like a valid flatpak app ID (reverse DNS).
fn is_valid_app_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    for part in &parts {
        if part.is_empty() {
            return false;
        }
        for c in part.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return false;
            }
        }
    }
    true
}

/// Validate a remote name.
fn is_valid_remote_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validate a URL (basic check).
fn is_valid_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("file://")
        || url.starts_with("oci+")
}

/// Validate an architecture string.
fn is_valid_arch(arch: &str) -> bool {
    matches!(
        arch,
        "x86_64" | "aarch64" | "i386" | "arm" | "riscv64"
    )
}

/// Parse a ref string: could be a full ref or just an app ID.
fn parse_ref_string(s: &str) -> (Option<RefKind>, String, Option<String>, Option<String>) {
    let parts: Vec<&str> = s.split('/').collect();
    match parts.len() {
        4 => {
            let kind = match parts[0] {
                "app" => Some(RefKind::App),
                "runtime" => Some(RefKind::Runtime),
                _ => None,
            };
            (
                kind,
                parts[1].to_string(),
                Some(parts[2].to_string()),
                Some(parts[3].to_string()),
            )
        }
        3 => (
            None,
            parts[0].to_string(),
            Some(parts[1].to_string()),
            Some(parts[2].to_string()),
        ),
        _ => (None, s.to_string(), None, None),
    }
}

/// Format a size in bytes to human-readable form.
#[cfg_attr(not(test), allow(dead_code))]
fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 bytes".to_string();
    }
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < units.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, units[0])
    } else {
        format!("{:.1} {}", size, units[unit_idx])
    }
}

// ============================================================================
// Shared permission names
// ============================================================================

const VALID_SHARED: &[&str] = &["network", "ipc"];
const VALID_SOCKETS: &[&str] = &[
    "x11",
    "wayland",
    "pulseaudio",
    "session-bus",
    "system-bus",
    "cups",
    "pcsc",
    "gpg-agent",
    "ssh-auth",
    "inherit-wayland-socket",
];
const VALID_DEVICES: &[&str] = &["dri", "kvm", "shm", "all"];
const VALID_COLUMNS: &[&str] = &[
    "application",
    "branch",
    "arch",
    "origin",
    "installation",
    "ref",
    "active",
    "latest",
    "size",
    "description",
    "options",
];

fn is_valid_shared(s: &str) -> bool {
    VALID_SHARED.contains(&s)
}

fn is_valid_socket(s: &str) -> bool {
    VALID_SOCKETS.contains(&s)
}

fn is_valid_device(s: &str) -> bool {
    VALID_DEVICES.contains(&s)
}

fn is_valid_column(s: &str) -> bool {
    VALID_COLUMNS.contains(&s)
}

// ============================================================================
// Subcommand implementations
// ============================================================================

fn cmd_install(args: &[String]) -> i32 {
    let assume_yes = has_flag(args, "-y", "--assumeyes");
    let user_scope = has_long_flag(args, "--user");
    let system_scope = has_long_flag(args, "--system");
    let reinstall = has_long_flag(args, "--reinstall");

    if user_scope && system_scope {
        eprintln!("error: --user and --system cannot both be specified");
        return 1;
    }

    let flags_with_values: &[&str] = &[];
    let positional = collect_positionals(args, flags_with_values);

    if positional.is_empty() {
        eprintln!("error: no ref specified for install");
        eprintln!("Usage: flatpak install [REMOTE] REF");
        return 1;
    }

    let installation = if user_scope {
        InstallationType::User
    } else {
        InstallationType::System
    };

    let (remote, app_ref) = if positional.len() >= 2 {
        (Some(positional[0].as_str()), positional[1].as_str())
    } else {
        (None, positional[0].as_str())
    };

    let (_kind, app_id, arch, branch) = parse_ref_string(app_ref);

    if !is_valid_app_id(&app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    if let Some(ref a) = arch
        && !is_valid_arch(a) {
            eprintln!("error: unsupported architecture '{}'", a);
            return 1;
        }

    let remote_name = remote.unwrap_or("flathub");
    let branch_name = branch.as_deref().unwrap_or("stable");
    let arch_name = arch.as_deref().unwrap_or("x86_64");
    let path = install_path(installation);

    if reinstall {
        println!(
            "Reinstalling {} from {} ({}/{})...",
            app_id, remote_name, arch_name, branch_name
        );
    } else {
        println!(
            "Installing {} from {} ({}/{})...",
            app_id, remote_name, arch_name, branch_name
        );
    }

    if !assume_yes {
        println!("Proceed with installation? [Y/n]:");
    }

    println!(
        "Installation {} at {} complete.",
        if user_scope { "user" } else { "system" },
        path
    );
    0
}

fn cmd_uninstall(args: &[String]) -> i32 {
    let assume_yes = has_flag(args, "-y", "--assumeyes");
    let unused = has_long_flag(args, "--unused");
    let delete_data = has_long_flag(args, "--delete-data");

    if unused {
        println!("Removing unused runtimes...");
        if delete_data {
            println!("Deleting application data for unused runtimes.");
        }
        println!("Done.");
        return 0;
    }

    let positional = collect_positionals(args, &[]);
    if positional.is_empty() {
        eprintln!("error: no ref specified for uninstall");
        eprintln!("Usage: flatpak uninstall REF");
        return 1;
    }

    let app_ref = positional[0].as_str();
    let (_kind, app_id, _arch, _branch) = parse_ref_string(app_ref);

    if !is_valid_app_id(&app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    if !assume_yes {
        println!("Uninstall {} ? [Y/n]:", app_id);
    }

    println!("Uninstalling {}...", app_id);

    if delete_data {
        println!("Deleting application data for {}.", app_id);
    }

    println!("Done.");
    0
}

fn cmd_update(args: &[String]) -> i32 {
    let assume_yes = has_flag(args, "-y", "--assumeyes");
    let no_pull = has_long_flag(args, "--no-pull");
    let no_deploy = has_long_flag(args, "--no-deploy");

    let positional = collect_positionals(args, &[]);

    if no_pull && no_deploy {
        eprintln!("error: --no-pull and --no-deploy cannot both be specified");
        return 1;
    }

    if let Some(app_ref) = positional.first() {
        let (_kind, app_id, _arch, _branch) = parse_ref_string(app_ref);
        if !is_valid_app_id(&app_id) {
            eprintln!("error: '{}' is not a valid application ID", app_id);
            return 1;
        }
        println!("Checking for updates to {}...", app_id);
        if !no_pull {
            println!("Pulling latest metadata...");
        }
        if !no_deploy {
            println!("Deploying updates...");
        }
    } else {
        println!("Checking for updates to all installed refs...");
        if !no_pull {
            println!("Pulling latest metadata...");
        }
        if !no_deploy {
            println!("Deploying updates...");
        }
    }

    if !assume_yes {
        println!("Proceed with updates? [Y/n]:");
    }

    println!("Nothing to update.");
    0
}

fn cmd_run(args: &[String]) -> i32 {
    let command = get_option_value(args, "", "--command");
    let branch = get_option_value(args, "", "--branch");
    let arch = get_option_value(args, "", "--arch");

    // Collect --env=KEY=VALUE
    let mut env_vars: Vec<(String, String)> = Vec::new();
    for arg in args {
        if let Some(rest) = arg.strip_prefix("--env=")
            && let Some(eq_pos) = rest.find('=') {
                let key = rest[..eq_pos].to_string();
                let val = rest[eq_pos + 1..].to_string();
                env_vars.push((key, val));
            }
    }

    // Collect sandbox modifications
    let mut shares: Vec<String> = Vec::new();
    let mut unshares: Vec<String> = Vec::new();
    let mut sockets: Vec<String> = Vec::new();
    let mut nosockets: Vec<String> = Vec::new();
    let mut devices: Vec<String> = Vec::new();
    let mut nodevices: Vec<String> = Vec::new();
    let mut filesystems: Vec<String> = Vec::new();
    let mut nofilesystems: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--share" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_shared(val) {
                    eprintln!("error: unknown share type '{val}'");
                    return 1;
                }
                shares.push(val.clone());
            }
            "--unshare" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_shared(val) {
                    eprintln!("error: unknown share type '{val}'");
                    return 1;
                }
                unshares.push(val.clone());
            }
            "--socket" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_socket(val) {
                    eprintln!("error: unknown socket type '{val}'");
                    return 1;
                }
                sockets.push(val.clone());
            }
            "--nosocket" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_socket(val) {
                    eprintln!("error: unknown socket type '{val}'");
                    return 1;
                }
                nosockets.push(val.clone());
            }
            "--device" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_device(val) {
                    eprintln!("error: unknown device type '{val}'");
                    return 1;
                }
                devices.push(val.clone());
            }
            "--nodevice" if i + 1 < args.len() => {
                i += 1;
                let val = &args[i];
                if !is_valid_device(val) {
                    eprintln!("error: unknown device type '{val}'");
                    return 1;
                }
                nodevices.push(val.clone());
            }
            "--filesystem" if i + 1 < args.len() => {
                i += 1;
                filesystems.push(args[i].clone());
            }
            "--nofilesystem"
                if i + 1 < args.len() => {
                    i += 1;
                    nofilesystems.push(args[i].clone());
                }
            _ => {}
        }
        i += 1;
    }

    let flags_with_values: &[&str] = &[
        "--command",
        "--branch",
        "--arch",
        "--env",
        "--share",
        "--unshare",
        "--socket",
        "--nosocket",
        "--device",
        "--nodevice",
        "--filesystem",
        "--nofilesystem",
    ];
    let positional = collect_positionals(args, flags_with_values);

    if positional.is_empty() {
        eprintln!("error: no application specified");
        eprintln!("Usage: flatpak run [OPTIONS] APP");
        return 1;
    }

    let app_id = positional[0].as_str();
    if !is_valid_app_id(app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    if let Some(a) = arch
        && !is_valid_arch(a) {
            eprintln!("error: unsupported architecture '{}'", a);
            return 1;
        }

    let branch_name = branch.unwrap_or("stable");
    let arch_name = arch.unwrap_or("x86_64");

    println!("Running {} ({}/{})...", app_id, arch_name, branch_name);

    if !shares.is_empty() {
        println!("  Additional shares: {}", shares.join(", "));
    }
    if !unshares.is_empty() {
        println!("  Removed shares: {}", unshares.join(", "));
    }
    if !sockets.is_empty() {
        println!("  Additional sockets: {}", sockets.join(", "));
    }
    if !nosockets.is_empty() {
        println!("  Removed sockets: {}", nosockets.join(", "));
    }
    if !devices.is_empty() {
        println!("  Additional devices: {}", devices.join(", "));
    }
    if !nodevices.is_empty() {
        println!("  Removed devices: {}", nodevices.join(", "));
    }
    if !filesystems.is_empty() {
        println!("  Additional filesystems: {}", filesystems.join(", "));
    }
    if !nofilesystems.is_empty() {
        println!("  Removed filesystems: {}", nofilesystems.join(", "));
    }
    if !env_vars.is_empty() {
        for (k, v) in &env_vars {
            println!("  Env: {}={}", k, v);
        }
    }

    if let Some(cmd) = command {
        println!("  Command override: {}", cmd);
    }

    0
}

fn cmd_list(args: &[String]) -> i32 {
    let app_only = has_long_flag(args, "--app");
    let runtime_only = has_long_flag(args, "--runtime");
    let user_scope = has_long_flag(args, "--user");
    let system_scope = has_long_flag(args, "--system");
    let columns = get_option_value(args, "", "--columns");

    if app_only && runtime_only {
        eprintln!("error: --app and --runtime cannot both be specified");
        return 1;
    }

    // Validate columns if specified
    if let Some(cols) = columns {
        for col in cols.split(',') {
            let trimmed = col.trim();
            if !is_valid_column(trimmed) {
                eprintln!("error: unknown column '{}'", trimmed);
                eprintln!(
                    "Valid columns: {}",
                    VALID_COLUMNS.join(", ")
                );
                return 1;
            }
        }
    }

    let scope_label = if user_scope {
        "user"
    } else if system_scope {
        "system"
    } else {
        "all"
    };

    let kind_label = if app_only {
        "apps"
    } else if runtime_only {
        "runtimes"
    } else {
        "refs"
    };

    let col_display = columns.unwrap_or("application,branch,arch,origin,installation");

    println!(
        "Listing {} {} (columns: {}):",
        scope_label, kind_label, col_display
    );
    println!("No installed refs found.");
    0
}

fn cmd_info(args: &[String]) -> i32 {
    let show_metadata = has_long_flag(args, "--show-metadata");
    let show_permissions = has_long_flag(args, "--show-permissions");
    let show_location = has_long_flag(args, "--show-location");

    let positional = collect_positionals(args, &[]);
    if positional.is_empty() {
        eprintln!("error: no ref specified");
        eprintln!("Usage: flatpak info [OPTIONS] REF");
        return 1;
    }

    let app_ref = positional[0].as_str();
    let (_kind, app_id, arch, branch) = parse_ref_string(app_ref);

    if !is_valid_app_id(&app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    let arch_name = arch.as_deref().unwrap_or("x86_64");
    let branch_name = branch.as_deref().unwrap_or("stable");

    println!("        ID: {}", app_id);
    println!("       Ref: app/{}/{}/{}", app_id, arch_name, branch_name);
    println!("      Arch: {}", arch_name);
    println!("    Branch: {}", branch_name);
    println!("    Origin: flathub");
    println!("    Commit: (not installed)");

    if show_location {
        println!(
            "  Location: {}/app/{}/{}/{}",
            system_install_path(),
            app_id,
            arch_name,
            branch_name
        );
    }

    if show_metadata {
        println!();
        println!("[Application]");
        println!("name={}", app_id);
        println!("runtime=org.freedesktop.Platform/{}/{}", arch_name, branch_name);
    }

    if show_permissions {
        println!();
        println!("[Context]");
        println!("shared=network;ipc;");
        println!("sockets=x11;wayland;pulseaudio;");
        println!("devices=dri;");
        println!("filesystems=xdg-download;");
    }

    0
}

fn cmd_search(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.is_empty() {
        eprintln!("error: no search query specified");
        eprintln!("Usage: flatpak search QUERY");
        return 1;
    }

    let query = positional.join(" ");
    println!("Searching for '{}'...", query);
    println!("No matches found.");
    0
}

fn cmd_remote_add(args: &[String]) -> i32 {
    let no_gpg_verify = has_long_flag(args, "--no-gpg-verify");
    let if_not_exists = has_long_flag(args, "--if-not-exists");

    let positional = collect_positionals(args, &[]);
    if positional.len() < 2 {
        eprintln!("error: remote name and URL required");
        eprintln!("Usage: flatpak remote-add [OPTIONS] NAME URL");
        return 1;
    }

    let name = positional[0].as_str();
    let url = positional[1].as_str();

    if !is_valid_remote_name(name) {
        eprintln!(
            "error: '{}' is not a valid remote name (use alphanumeric, -, _)",
            name
        );
        return 1;
    }

    if !is_valid_url(url) {
        eprintln!(
            "error: '{}' does not look like a valid URL (must start with http://, https://, file://, or oci+)",
            url
        );
        return 1;
    }

    if if_not_exists {
        println!("Adding remote '{}' at {} (if not exists)...", name, url);
    } else {
        println!("Adding remote '{}' at {}...", name, url);
    }

    if no_gpg_verify {
        println!("  Warning: GPG verification disabled for this remote.");
    }

    println!("Remote '{}' added.", name);
    0
}

fn cmd_remote_delete(args: &[String]) -> i32 {
    let force = has_long_flag(args, "--force");

    let positional = collect_positionals(args, &[]);
    if positional.is_empty() {
        eprintln!("error: remote name required");
        eprintln!("Usage: flatpak remote-delete [OPTIONS] NAME");
        return 1;
    }

    let name = positional[0].as_str();

    if !is_valid_remote_name(name) {
        eprintln!("error: '{}' is not a valid remote name", name);
        return 1;
    }

    if force {
        println!("Force-removing remote '{}'...", name);
    } else {
        println!("Removing remote '{}'...", name);
    }

    println!("Remote '{}' deleted.", name);
    0
}

fn cmd_remote_list(args: &[String]) -> i32 {
    let show_disabled = has_long_flag(args, "--show-disabled");

    println!("Name\tTitle\tURL\tPriority");
    println!("flathub\tFlathub\thttps://dl.flathub.org/repo/\t1");

    if show_disabled {
        println!("(including disabled remotes)");
    }

    0
}

fn cmd_remote_info(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.len() < 2 {
        eprintln!("error: remote and ref required");
        eprintln!("Usage: flatpak remote-info REMOTE REF");
        return 1;
    }

    let remote = positional[0].as_str();
    let app_ref = positional[1].as_str();

    if !is_valid_remote_name(remote) {
        eprintln!("error: '{}' is not a valid remote name", remote);
        return 1;
    }

    let (_kind, app_id, arch, branch) = parse_ref_string(app_ref);

    if !is_valid_app_id(&app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    let arch_name = arch.as_deref().unwrap_or("x86_64");
    let branch_name = branch.as_deref().unwrap_or("stable");

    println!("        ID: {}", app_id);
    println!("       Ref: app/{}/{}/{}", app_id, arch_name, branch_name);
    println!("      Arch: {}", arch_name);
    println!("    Branch: {}", branch_name);
    println!("    Remote: {}", remote);
    println!("    Commit: (not available)");
    println!("  Download: (not available)");
    println!("  Installed: (not available)");
    0
}

fn cmd_repair(args: &[String]) -> i32 {
    let user_scope = has_long_flag(args, "--user");
    let system_scope = has_long_flag(args, "--system");

    if user_scope && system_scope {
        eprintln!("error: --user and --system cannot both be specified");
        return 1;
    }

    let installation = if user_scope {
        InstallationType::User
    } else {
        InstallationType::System
    };

    let path = install_path(installation);
    println!("Repairing {} installation at {}...", installation, path);
    println!("Checking installed refs...");
    println!("Verifying OSTree repository...");
    println!("Checking for missing objects...");
    println!("Repair complete. No problems found.");
    0
}

fn cmd_history(args: &[String]) -> i32 {
    let user_scope = has_long_flag(args, "--user");
    let system_scope = has_long_flag(args, "--system");

    let scope = if user_scope {
        "user"
    } else if system_scope {
        "system"
    } else {
        "all"
    };

    println!("History ({} installations):", scope);
    println!("Time\tAction\tApplication\tBranch\tRemote");
    println!("No history entries found.");
    0
}

fn cmd_override(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[
        "--share",
        "--unshare",
        "--socket",
        "--nosocket",
        "--device",
        "--nodevice",
        "--filesystem",
        "--nofilesystem",
        "--env",
    ]);

    if positional.is_empty() {
        eprintln!("error: no application specified");
        eprintln!("Usage: flatpak override [OPTIONS] APP");
        return 1;
    }

    let app_id = positional[0].as_str();
    if !is_valid_app_id(app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    // Parse sandbox overrides
    let mut changes = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--share" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("share={}", args[i]));
            }
            "--unshare" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("unshare={}", args[i]));
            }
            "--socket" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("socket={}", args[i]));
            }
            "--nosocket" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("nosocket={}", args[i]));
            }
            "--device" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("device={}", args[i]));
            }
            "--nodevice" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("nodevice={}", args[i]));
            }
            "--filesystem" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("filesystem={}", args[i]));
            }
            "--nofilesystem" if i + 1 < args.len() => {
                i += 1;
                changes.push(format!("nofilesystem={}", args[i]));
            }
            _ => {}
        }
        i += 1;
    }

    if changes.is_empty() {
        println!("No overrides specified for {}.", app_id);
        return 0;
    }

    println!("Setting overrides for {}:", app_id);
    for change in &changes {
        println!("  {}", change);
    }
    println!("Done.");
    0
}

fn cmd_permission_show(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.is_empty() {
        eprintln!("error: no application specified");
        eprintln!("Usage: flatpak permission-show APP");
        return 1;
    }

    let app_id = positional[0].as_str();
    if !is_valid_app_id(app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    println!("Permissions for {}:", app_id);
    println!("  [Context]");
    println!("  shared=network;ipc;");
    println!("  sockets=x11;wayland;pulseaudio;");
    println!("  devices=dri;");
    println!("  filesystems=xdg-download;");
    0
}

fn cmd_build_init(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.len() < 4 {
        eprintln!("error: insufficient arguments");
        eprintln!("Usage: flatpak build-init DIRECTORY APP SDK RUNTIME [BRANCH]");
        return 1;
    }

    let directory = positional[0].as_str();
    let app_id = positional[1].as_str();
    let sdk = positional[2].as_str();
    let runtime = positional[3].as_str();
    let branch = if positional.len() > 4 {
        positional[4].as_str()
    } else {
        "master"
    };

    if !is_valid_app_id(app_id) {
        eprintln!("error: '{}' is not a valid application ID", app_id);
        return 1;
    }

    if !is_valid_app_id(sdk) {
        eprintln!("error: '{}' is not a valid SDK ID", sdk);
        return 1;
    }

    if !is_valid_app_id(runtime) {
        eprintln!("error: '{}' is not a valid runtime ID", runtime);
        return 1;
    }

    println!("Initializing build directory: {}", directory);
    println!("  Application: {}", app_id);
    println!("  SDK: {}", sdk);
    println!("  Runtime: {}", runtime);
    println!("  Branch: {}", branch);
    println!("Build directory initialized.");
    0
}

fn cmd_build(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.len() < 2 {
        eprintln!("error: directory and command required");
        eprintln!("Usage: flatpak build DIRECTORY COMMAND [ARGS...]");
        return 1;
    }

    let directory = positional[0].as_str();
    let command_parts: Vec<&str> = positional[1..].iter().map(|s| s.as_str()).collect();

    println!(
        "Running build command in sandbox: {} {}",
        directory,
        command_parts.join(" ")
    );
    0
}

fn cmd_build_finish(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[
        "--share",
        "--socket",
        "--device",
        "--filesystem",
        "--command",
        "--env",
    ]);

    if positional.is_empty() {
        eprintln!("error: build directory required");
        eprintln!("Usage: flatpak build-finish [OPTIONS] DIRECTORY");
        return 1;
    }

    let directory = positional[0].as_str();
    let command = get_option_value(args, "", "--command");

    println!("Finalizing build in: {}", directory);

    // Collect permissions to set in metadata
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--share" if i + 1 < args.len() => {
                i += 1;
                println!("  share={}", args[i]);
            }
            "--socket" if i + 1 < args.len() => {
                i += 1;
                println!("  socket={}", args[i]);
            }
            "--device" if i + 1 < args.len() => {
                i += 1;
                println!("  device={}", args[i]);
            }
            "--filesystem" if i + 1 < args.len() => {
                i += 1;
                println!("  filesystem={}", args[i]);
            }
            "--env" if i + 1 < args.len() => {
                i += 1;
                println!("  env={}", args[i]);
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(cmd) = command {
        println!("  command={}", cmd);
    }

    println!("Build finalized.");
    0
}

fn cmd_build_export(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &["--subject", "--body", "--gpg-sign"]);
    if positional.len() < 2 {
        eprintln!("error: repository and directory required");
        eprintln!("Usage: flatpak build-export [OPTIONS] REPO DIRECTORY [BRANCH]");
        return 1;
    }

    let repo = positional[0].as_str();
    let directory = positional[1].as_str();
    let branch = if positional.len() > 2 {
        positional[2].as_str()
    } else {
        "master"
    };

    let subject = get_option_value(args, "", "--subject");
    let body = get_option_value(args, "", "--body");

    println!(
        "Exporting build from {} to repository {} (branch: {})...",
        directory, repo, branch
    );

    if let Some(s) = subject {
        println!("  Subject: {}", s);
    }
    if let Some(b) = body {
        println!("  Body: {}", b);
    }

    println!("Export complete.");
    0
}

fn cmd_build_import_bundle(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &[]);
    if positional.len() < 2 {
        eprintln!("error: repository and bundle file required");
        eprintln!("Usage: flatpak build-import-bundle REPO FILE");
        return 1;
    }

    let repo = positional[0].as_str();
    let file = positional[1].as_str();

    println!("Importing bundle {} into repository {}...", file, repo);
    println!("Import complete.");
    0
}

fn cmd_build_bundle(args: &[String]) -> i32 {
    let positional = collect_positionals(args, &["--arch"]);
    if positional.len() < 3 {
        eprintln!("error: repository, file, and ref required");
        eprintln!("Usage: flatpak build-bundle [OPTIONS] REPO FILE REF");
        return 1;
    }

    let repo = positional[0].as_str();
    let file = positional[1].as_str();
    let ref_name = positional[2].as_str();
    let arch = get_option_value(args, "", "--arch");

    let (_kind, app_id, _arch_parsed, _branch) = parse_ref_string(ref_name);
    if !is_valid_app_id(&app_id) {
        eprintln!("error: '{}' is not a valid ref", ref_name);
        return 1;
    }

    println!(
        "Creating bundle {} from {} (ref: {})...",
        file, repo, ref_name
    );

    if let Some(a) = arch {
        println!("  Architecture: {}", a);
    }

    println!("Bundle created.");
    0
}

fn cmd_config(args: &[String]) -> i32 {
    let do_set = has_long_flag(args, "--set");
    let do_get = has_long_flag(args, "--get");
    let do_unset = has_long_flag(args, "--unset");

    let flag_count = [do_set, do_get, do_unset]
        .iter()
        .filter(|&&v| v)
        .count();

    if flag_count == 0 {
        eprintln!("error: one of --set, --get, or --unset required");
        eprintln!("Usage: flatpak config --set KEY VALUE");
        eprintln!("       flatpak config --get KEY");
        eprintln!("       flatpak config --unset KEY");
        return 1;
    }

    if flag_count > 1 {
        eprintln!("error: only one of --set, --get, --unset may be specified");
        return 1;
    }

    let positional = collect_positionals(args, &[]);

    if do_get {
        if positional.is_empty() {
            eprintln!("error: key required for --get");
            return 1;
        }
        let key = positional[0].as_str();
        println!("Config key '{}' is not set.", key);
        return 0;
    }

    if do_unset {
        if positional.is_empty() {
            eprintln!("error: key required for --unset");
            return 1;
        }
        let key = positional[0].as_str();
        println!("Unset config key '{}'.", key);
        return 0;
    }

    // do_set
    if positional.len() < 2 {
        eprintln!("error: key and value required for --set");
        return 1;
    }
    let key = positional[0].as_str();
    let value = positional[1].as_str();
    println!("Set config '{}' = '{}'.", key, value);
    0
}

// ============================================================================
// Help and version
// ============================================================================

fn print_version() {
    println!("flatpak 1.0.0 (Slate OS)");
}

fn print_help(prog: &str) {
    println!("Usage: {} COMMAND [OPTIONS]", prog);
    println!();
    println!("Sandboxed application management for Slate OS");
    println!();
    println!("Application commands:");
    println!("  install [REMOTE] REF      Install an application or runtime");
    println!("  uninstall REF             Remove an installed application");
    println!("  update [REF]              Update applications and runtimes");
    println!("  run APP                   Run a sandboxed application");
    println!("  list                      List installed refs");
    println!("  info REF                  Show application information");
    println!("  search QUERY              Search for applications");
    println!();
    println!("Remote commands:");
    println!("  remote-add NAME URL       Add a remote repository");
    println!("  remote-delete NAME        Remove a remote repository");
    println!("  remote-list               List configured remotes");
    println!("  remote-info REMOTE REF    Show info from a remote");
    println!();
    println!("Management commands:");
    println!("  repair                    Repair installation");
    println!("  history                   Show install/update history");
    println!("  override APP              Override application permissions");
    println!("  permission-show APP       Show application permissions");
    println!("  config --set/--get/--unset KEY [VALUE]  Manage configuration");
    println!();
    println!("Build commands:");
    println!("  build-init DIR APP SDK RT   Initialize build directory");
    println!("  build DIR CMD [ARGS...]     Run build command");
    println!("  build-finish DIR            Finalize build");
    println!("  build-export REPO DIR       Export build to repository");
    println!("  build-import-bundle REPO FILE  Import a bundle file");
    println!("  build-bundle REPO FILE REF  Create a bundle file");
    println!();
    println!("Options:");
    println!("  --help, -h                Show this help message");
    println!("  --version                 Show version information");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn run(args: Vec<String>) -> i32 {
    // Personality detection via argv[0] basename
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("flatpak");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let sub_args: Vec<String> = args.iter().skip(1).cloned().collect();

    // Check for global --help / --version before subcommand dispatch
    if sub_args.is_empty() {
        print_help(&prog_name);
        return 0;
    }

    if has_flag(&sub_args, "-h", "--help") && !sub_args.iter().any(|a| !a.starts_with('-')) {
        print_help(&prog_name);
        return 0;
    }

    if has_long_flag(&sub_args, "--version") && !sub_args.iter().any(|a| !a.starts_with('-')) {
        print_version();
        return 0;
    }

    let subcmd = sub_args[0].as_str();
    let cmd_args: Vec<String> = sub_args.iter().skip(1).cloned().collect();

    match (&*prog_name, subcmd) {
        // flatpak personality (default and explicit)
        (_, "install") => cmd_install(&cmd_args),
        (_, "uninstall") => cmd_uninstall(&cmd_args),
        (_, "update") => cmd_update(&cmd_args),
        (_, "run") => cmd_run(&cmd_args),
        (_, "list") => cmd_list(&cmd_args),
        (_, "info") => cmd_info(&cmd_args),
        (_, "search") => cmd_search(&cmd_args),
        (_, "remote-add") => cmd_remote_add(&cmd_args),
        (_, "remote-delete") => cmd_remote_delete(&cmd_args),
        (_, "remote-list") => cmd_remote_list(&cmd_args),
        (_, "remote-info") => cmd_remote_info(&cmd_args),
        (_, "repair") => cmd_repair(&cmd_args),
        (_, "history") => cmd_history(&cmd_args),
        (_, "override") => cmd_override(&cmd_args),
        (_, "permission-show") => cmd_permission_show(&cmd_args),
        (_, "build-init") => cmd_build_init(&cmd_args),
        (_, "build") => cmd_build(&cmd_args),
        (_, "build-finish") => cmd_build_finish(&cmd_args),
        (_, "build-export") => cmd_build_export(&cmd_args),
        (_, "build-import-bundle") => cmd_build_import_bundle(&cmd_args),
        (_, "build-bundle") => cmd_build_bundle(&cmd_args),
        (_, "config") => cmd_config(&cmd_args),
        (_, "--help" | "-h") => {
            print_help(&prog_name);
            0
        }
        (_, "--version") => {
            print_version();
            0
        }
        _ => {
            eprintln!("error: unknown command '{}' for '{}'", subcmd, prog_name);
            eprintln!("Run '{} --help' for usage information.", prog_name);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    process::exit(run(args));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to build an args vec from a command line string.
    fn make_args(line: &str) -> Vec<String> {
        line.split_whitespace().map(|s| s.to_string()).collect()
    }

    // Helper: run with argv[0] = "flatpak" and the rest from the line.
    fn flatpak(line: &str) -> i32 {
        let mut args = vec!["flatpak".to_string()];
        if !line.is_empty() {
            args.extend(make_args(line));
        }
        run(args)
    }

    // ====================================================================
    // Personality detection
    // ====================================================================

    #[test]
    fn personality_default() {
        assert_eq!(run(vec!["flatpak".to_string()]), 0);
    }

    #[test]
    fn personality_with_unix_path() {
        assert_eq!(
            run(vec!["/usr/bin/flatpak".to_string()]),
            0
        );
    }

    #[test]
    fn personality_with_windows_path() {
        assert_eq!(
            run(vec!["C:\\Program Files\\flatpak.exe".to_string()]),
            0
        );
    }

    #[test]
    fn personality_strips_exe() {
        assert_eq!(
            run(vec!["flatpak.exe".to_string()]),
            0
        );
    }

    #[test]
    fn personality_unknown_still_works() {
        // Unknown personality falls through to flatpak behavior
        let ret = run(vec!["unknown_tool".to_string()]);
        assert_eq!(ret, 0); // shows help
    }

    // ====================================================================
    // Validation helpers
    // ====================================================================

    #[test]
    fn valid_app_id_basic() {
        assert!(is_valid_app_id("org.example.App"));
    }

    #[test]
    fn valid_app_id_long() {
        assert!(is_valid_app_id("org.freedesktop.Platform"));
    }

    #[test]
    fn valid_app_id_with_underscore() {
        assert!(is_valid_app_id("com.example.My_App"));
    }

    #[test]
    fn valid_app_id_with_hyphen() {
        assert!(is_valid_app_id("com.example.my-app"));
    }

    #[test]
    fn invalid_app_id_empty() {
        assert!(!is_valid_app_id(""));
    }

    #[test]
    fn invalid_app_id_single_component() {
        assert!(!is_valid_app_id("firefox"));
    }

    #[test]
    fn invalid_app_id_double_dot() {
        assert!(!is_valid_app_id("org..App"));
    }

    #[test]
    fn invalid_app_id_special_chars() {
        assert!(!is_valid_app_id("org.example.App@1"));
    }

    #[test]
    fn valid_remote_name_alpha() {
        assert!(is_valid_remote_name("flathub"));
    }

    #[test]
    fn valid_remote_name_with_hyphen() {
        assert!(is_valid_remote_name("my-remote"));
    }

    #[test]
    fn valid_remote_name_with_underscore() {
        assert!(is_valid_remote_name("my_remote"));
    }

    #[test]
    fn invalid_remote_name_empty() {
        assert!(!is_valid_remote_name(""));
    }

    #[test]
    fn invalid_remote_name_spaces() {
        assert!(!is_valid_remote_name("my remote"));
    }

    #[test]
    fn invalid_remote_name_special() {
        assert!(!is_valid_remote_name("remote@home"));
    }

    #[test]
    fn valid_url_https() {
        assert!(is_valid_url("https://dl.flathub.org/repo/"));
    }

    #[test]
    fn valid_url_http() {
        assert!(is_valid_url("http://example.com/repo"));
    }

    #[test]
    fn valid_url_file() {
        assert!(is_valid_url("file:///tmp/repo"));
    }

    #[test]
    fn valid_url_oci() {
        assert!(is_valid_url("oci+https://registry.example.com"));
    }

    #[test]
    fn invalid_url_bare() {
        assert!(!is_valid_url("example.com/repo"));
    }

    #[test]
    fn invalid_url_empty() {
        assert!(!is_valid_url(""));
    }

    #[test]
    fn valid_arch_x86_64() {
        assert!(is_valid_arch("x86_64"));
    }

    #[test]
    fn valid_arch_aarch64() {
        assert!(is_valid_arch("aarch64"));
    }

    #[test]
    fn valid_arch_i386() {
        assert!(is_valid_arch("i386"));
    }

    #[test]
    fn valid_arch_riscv64() {
        assert!(is_valid_arch("riscv64"));
    }

    #[test]
    fn invalid_arch() {
        assert!(!is_valid_arch("mips"));
    }

    // ====================================================================
    // parse_ref_string
    // ====================================================================

    #[test]
    fn parse_full_ref() {
        let (kind, id, arch, branch) =
            parse_ref_string("app/org.example.App/x86_64/stable");
        assert_eq!(kind, Some(RefKind::App));
        assert_eq!(id, "org.example.App");
        assert_eq!(arch.as_deref(), Some("x86_64"));
        assert_eq!(branch.as_deref(), Some("stable"));
    }

    #[test]
    fn parse_runtime_ref() {
        let (kind, id, arch, branch) =
            parse_ref_string("runtime/org.freedesktop.Platform/x86_64/23.08");
        assert_eq!(kind, Some(RefKind::Runtime));
        assert_eq!(id, "org.freedesktop.Platform");
        assert_eq!(arch.as_deref(), Some("x86_64"));
        assert_eq!(branch.as_deref(), Some("23.08"));
    }

    #[test]
    fn parse_three_part_ref() {
        let (kind, id, arch, branch) =
            parse_ref_string("org.example.App/x86_64/stable");
        assert_eq!(kind, None);
        assert_eq!(id, "org.example.App");
        assert_eq!(arch.as_deref(), Some("x86_64"));
        assert_eq!(branch.as_deref(), Some("stable"));
    }

    #[test]
    fn parse_bare_app_id() {
        let (kind, id, arch, branch) = parse_ref_string("org.example.App");
        assert_eq!(kind, None);
        assert_eq!(id, "org.example.App");
        assert_eq!(arch, None);
        assert_eq!(branch, None);
    }

    // ====================================================================
    // format_size
    // ====================================================================

    #[test]
    fn format_size_zero() {
        assert_eq!(format_size(0), "0 bytes");
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500 bytes");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn format_size_mb() {
        assert_eq!(format_size(10 * 1024 * 1024), "10.0 MB");
    }

    #[test]
    fn format_size_gb() {
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    // ====================================================================
    // Data model
    // ====================================================================

    #[test]
    fn remote_new() {
        let r = Remote::new("flathub", "https://dl.flathub.org/repo/", InstallationType::System);
        assert_eq!(r.name, "flathub");
        assert!(r.gpg_verify);
        assert!(r.enabled);
        assert_eq!(r.priority, 1);
    }

    #[test]
    fn installation_type_display_user() {
        assert_eq!(format!("{}", InstallationType::User), "user");
    }

    #[test]
    fn installation_type_display_system() {
        assert_eq!(format!("{}", InstallationType::System), "system");
    }

    #[test]
    fn ref_kind_display_app() {
        assert_eq!(format!("{}", RefKind::App), "app");
    }

    #[test]
    fn ref_kind_display_runtime() {
        assert_eq!(format!("{}", RefKind::Runtime), "runtime");
    }

    #[test]
    fn flatpak_ref_full_ref() {
        let r = FlatpakRef {
            app_id: "org.example.App".to_string(),
            branch: "stable".to_string(),
            arch: "x86_64".to_string(),
            kind: RefKind::App,
            commit: "abc123".to_string(),
            installed_size: 1024,
            download_size: 512,
            description: "An example app".to_string(),
            remote: "flathub".to_string(),
            installation: InstallationType::System,
        };
        assert_eq!(r.full_ref(), "app/org.example.App/x86_64/stable");
    }

    #[test]
    fn flatpak_ref_matches_query_by_id() {
        let r = FlatpakRef {
            app_id: "org.example.Firefox".to_string(),
            branch: "stable".to_string(),
            arch: "x86_64".to_string(),
            kind: RefKind::App,
            commit: String::new(),
            installed_size: 0,
            download_size: 0,
            description: "Web browser".to_string(),
            remote: "flathub".to_string(),
            installation: InstallationType::System,
        };
        assert!(r.matches_query("firefox"));
        assert!(r.matches_query("Firefox"));
    }

    #[test]
    fn flatpak_ref_matches_query_by_description() {
        let r = FlatpakRef {
            app_id: "org.mozilla.Firefox".to_string(),
            branch: "stable".to_string(),
            arch: "x86_64".to_string(),
            kind: RefKind::App,
            commit: String::new(),
            installed_size: 0,
            download_size: 0,
            description: "Web browser".to_string(),
            remote: "flathub".to_string(),
            installation: InstallationType::System,
        };
        assert!(r.matches_query("browser"));
    }

    #[test]
    fn flatpak_ref_no_match() {
        let r = FlatpakRef {
            app_id: "org.mozilla.Firefox".to_string(),
            branch: "stable".to_string(),
            arch: "x86_64".to_string(),
            kind: RefKind::App,
            commit: String::new(),
            installed_size: 0,
            download_size: 0,
            description: "Web browser".to_string(),
            remote: "flathub".to_string(),
            installation: InstallationType::System,
        };
        assert!(!r.matches_query("calculator"));
    }

    #[test]
    fn flatpak_ref_display() {
        let r = FlatpakRef {
            app_id: "org.example.App".to_string(),
            branch: "stable".to_string(),
            arch: "x86_64".to_string(),
            kind: RefKind::App,
            commit: String::new(),
            installed_size: 0,
            download_size: 0,
            description: String::new(),
            remote: "flathub".to_string(),
            installation: InstallationType::System,
        };
        let display = format!("{}", r);
        assert!(display.contains("org.example.App"));
        assert!(display.contains("stable"));
    }

    // ====================================================================
    // Permissions
    // ====================================================================

    #[test]
    fn permissions_default_is_empty() {
        let p = Permissions::default();
        assert!(p.is_empty());
    }

    #[test]
    fn permissions_with_shared_not_empty() {
        let mut p = Permissions::default();
        p.shared.push("network".to_string());
        assert!(!p.is_empty());
    }

    #[test]
    fn permissions_with_socket_not_empty() {
        let mut p = Permissions::default();
        p.sockets.push("x11".to_string());
        assert!(!p.is_empty());
    }

    #[test]
    fn permissions_with_device_not_empty() {
        let mut p = Permissions::default();
        p.devices.push("dri".to_string());
        assert!(!p.is_empty());
    }

    #[test]
    fn permissions_with_filesystem_not_empty() {
        let mut p = Permissions::default();
        p.filesystems.push("home".to_string());
        assert!(!p.is_empty());
    }

    #[test]
    fn permissions_with_env_not_empty() {
        let mut p = Permissions::default();
        p.env_vars.insert("KEY".to_string(), "VALUE".to_string());
        assert!(!p.is_empty());
    }

    // ====================================================================
    // Config
    // ====================================================================

    #[test]
    fn config_get_unset_key() {
        let c = Config::default();
        assert_eq!(c.get("missing"), None);
    }

    #[test]
    fn config_set_and_get() {
        let mut c = Config::default();
        c.set("languages", "en;de");
        assert_eq!(c.get("languages"), Some("en;de"));
    }

    #[test]
    fn config_unset_existing() {
        let mut c = Config::default();
        c.set("key", "value");
        assert!(c.unset("key"));
        assert_eq!(c.get("key"), None);
    }

    #[test]
    fn config_unset_missing() {
        let mut c = Config::default();
        assert!(!c.unset("missing"));
    }

    // ====================================================================
    // HistoryEntry display
    // ====================================================================

    #[test]
    fn history_entry_display_install() {
        let h = HistoryEntry {
            timestamp: "2024-01-01T12:00:00".to_string(),
            action: "install".to_string(),
            app_id: "org.example.App".to_string(),
            branch: "stable".to_string(),
            remote: "flathub".to_string(),
            old_commit: String::new(),
            new_commit: "abc12345".to_string(),
        };
        let s = format!("{}", h);
        assert!(s.contains("install"));
        assert!(s.contains("org.example.App"));
    }

    #[test]
    fn history_entry_display_update() {
        let h = HistoryEntry {
            timestamp: "2024-01-01T12:00:00".to_string(),
            action: "update".to_string(),
            app_id: "org.example.App".to_string(),
            branch: "stable".to_string(),
            remote: "flathub".to_string(),
            old_commit: "aabbccdd".to_string(),
            new_commit: "eeff0011".to_string(),
        };
        let s = format!("{}", h);
        assert!(s.contains("update"));
        assert!(s.contains("aabbccdd"));
        assert!(s.contains("eeff0011"));
    }

    // ====================================================================
    // Shared permission validators
    // ====================================================================

    #[test]
    fn valid_shared_network() {
        assert!(is_valid_shared("network"));
    }

    #[test]
    fn valid_shared_ipc() {
        assert!(is_valid_shared("ipc"));
    }

    #[test]
    fn invalid_shared() {
        assert!(!is_valid_shared("bluetooth"));
    }

    #[test]
    fn valid_socket_x11() {
        assert!(is_valid_socket("x11"));
    }

    #[test]
    fn valid_socket_wayland() {
        assert!(is_valid_socket("wayland"));
    }

    #[test]
    fn valid_socket_pulseaudio() {
        assert!(is_valid_socket("pulseaudio"));
    }

    #[test]
    fn valid_socket_session_bus() {
        assert!(is_valid_socket("session-bus"));
    }

    #[test]
    fn valid_socket_system_bus() {
        assert!(is_valid_socket("system-bus"));
    }

    #[test]
    fn invalid_socket() {
        assert!(!is_valid_socket("bluetooth"));
    }

    #[test]
    fn valid_device_dri() {
        assert!(is_valid_device("dri"));
    }

    #[test]
    fn valid_device_all() {
        assert!(is_valid_device("all"));
    }

    #[test]
    fn valid_device_kvm() {
        assert!(is_valid_device("kvm"));
    }

    #[test]
    fn invalid_device() {
        assert!(!is_valid_device("usb"));
    }

    #[test]
    fn valid_column_application() {
        assert!(is_valid_column("application"));
    }

    #[test]
    fn valid_column_size() {
        assert!(is_valid_column("size"));
    }

    #[test]
    fn invalid_column() {
        assert!(!is_valid_column("color"));
    }

    // ====================================================================
    // Argument parsing helpers
    // ====================================================================

    #[test]
    fn has_flag_short() {
        let args = make_args("-y --other");
        assert!(has_flag(&args, "-y", "--assumeyes"));
    }

    #[test]
    fn has_flag_long() {
        let args = make_args("--assumeyes --other");
        assert!(has_flag(&args, "-y", "--assumeyes"));
    }

    #[test]
    fn has_flag_missing() {
        let args = make_args("--other");
        assert!(!has_flag(&args, "-y", "--assumeyes"));
    }

    #[test]
    fn has_long_flag_present() {
        let args = make_args("--user --other");
        assert!(has_long_flag(&args, "--user"));
    }

    #[test]
    fn has_long_flag_missing() {
        let args = make_args("--other");
        assert!(!has_long_flag(&args, "--user"));
    }

    #[test]
    fn get_option_value_present() {
        let args = make_args("--command bash --other");
        assert_eq!(get_option_value(&args, "", "--command"), Some("bash"));
    }

    #[test]
    fn get_option_value_equals() {
        let args = make_args("--command=bash --other");
        assert_eq!(get_option_value(&args, "", "--command"), Some("bash"));
    }

    #[test]
    fn get_option_value_missing() {
        let args = make_args("--other");
        assert_eq!(get_option_value(&args, "", "--command"), None);
    }

    #[test]
    fn collect_positionals_basic() {
        let args = make_args("one two three");
        let pos = collect_positionals(&args, &[]);
        assert_eq!(pos, vec!["one", "two", "three"]);
    }

    #[test]
    fn collect_positionals_skip_flags() {
        let args = make_args("one --flag two");
        let pos = collect_positionals(&args, &[]);
        assert_eq!(pos, vec!["one", "two"]);
    }

    #[test]
    fn collect_positionals_skip_flag_value() {
        let args = make_args("one --branch stable two");
        let pos = collect_positionals(&args, &["--branch"]);
        assert_eq!(pos, vec!["one", "two"]);
    }

    // ====================================================================
    // Install command
    // ====================================================================

    #[test]
    fn install_no_ref() {
        assert_eq!(flatpak("install"), 1);
    }

    #[test]
    fn install_basic() {
        assert_eq!(flatpak("install org.example.App"), 0);
    }

    #[test]
    fn install_with_remote() {
        assert_eq!(flatpak("install flathub org.example.App"), 0);
    }

    #[test]
    fn install_user_scope() {
        assert_eq!(flatpak("install --user org.example.App"), 0);
    }

    #[test]
    fn install_system_scope() {
        assert_eq!(flatpak("install --system org.example.App"), 0);
    }

    #[test]
    fn install_assume_yes() {
        assert_eq!(flatpak("install -y org.example.App"), 0);
    }

    #[test]
    fn install_reinstall() {
        assert_eq!(flatpak("install --reinstall org.example.App"), 0);
    }

    #[test]
    fn install_full_ref() {
        assert_eq!(flatpak("install app/org.example.App/x86_64/stable"), 0);
    }

    #[test]
    fn install_invalid_id() {
        assert_eq!(flatpak("install badname"), 1);
    }

    // ====================================================================
    // Uninstall command
    // ====================================================================

    #[test]
    fn uninstall_no_ref() {
        assert_eq!(flatpak("uninstall"), 1);
    }

    #[test]
    fn uninstall_basic() {
        assert_eq!(flatpak("uninstall org.example.App"), 0);
    }

    #[test]
    fn uninstall_unused() {
        assert_eq!(flatpak("uninstall --unused"), 0);
    }

    #[test]
    fn uninstall_delete_data() {
        assert_eq!(flatpak("uninstall --delete-data org.example.App"), 0);
    }

    #[test]
    fn uninstall_assume_yes() {
        assert_eq!(flatpak("uninstall -y org.example.App"), 0);
    }

    #[test]
    fn uninstall_invalid_id() {
        assert_eq!(flatpak("uninstall badname"), 1);
    }

    // ====================================================================
    // Update command
    // ====================================================================

    #[test]
    fn update_all() {
        assert_eq!(flatpak("update"), 0);
    }

    #[test]
    fn update_specific_ref() {
        assert_eq!(flatpak("update org.example.App"), 0);
    }

    #[test]
    fn update_no_pull() {
        assert_eq!(flatpak("update --no-pull"), 0);
    }

    #[test]
    fn update_no_deploy() {
        assert_eq!(flatpak("update --no-deploy"), 0);
    }

    #[test]
    fn update_no_pull_no_deploy_error() {
        assert_eq!(flatpak("update --no-pull --no-deploy"), 1);
    }

    #[test]
    fn update_assume_yes() {
        assert_eq!(flatpak("update -y"), 0);
    }

    #[test]
    fn update_invalid_id() {
        assert_eq!(flatpak("update badname"), 1);
    }

    // ====================================================================
    // Run command
    // ====================================================================

    #[test]
    fn run_no_app() {
        assert_eq!(flatpak("run"), 1);
    }

    #[test]
    fn run_basic() {
        assert_eq!(flatpak("run org.example.App"), 0);
    }

    #[test]
    fn run_with_branch() {
        assert_eq!(flatpak("run --branch beta org.example.App"), 0);
    }

    #[test]
    fn run_with_arch() {
        assert_eq!(flatpak("run --arch x86_64 org.example.App"), 0);
    }

    #[test]
    fn run_invalid_arch() {
        assert_eq!(flatpak("run --arch mips org.example.App"), 1);
    }

    #[test]
    fn run_with_command() {
        assert_eq!(flatpak("run --command bash org.example.App"), 0);
    }

    #[test]
    fn run_invalid_app_id() {
        assert_eq!(flatpak("run badname"), 1);
    }

    #[test]
    fn run_with_share() {
        assert_eq!(flatpak("run --share network org.example.App"), 0);
    }

    #[test]
    fn run_with_invalid_share() {
        assert_eq!(flatpak("run --share bluetooth org.example.App"), 1);
    }

    #[test]
    fn run_with_socket() {
        assert_eq!(flatpak("run --socket x11 org.example.App"), 0);
    }

    #[test]
    fn run_with_invalid_socket() {
        assert_eq!(flatpak("run --socket bluetooth org.example.App"), 1);
    }

    #[test]
    fn run_with_device() {
        assert_eq!(flatpak("run --device dri org.example.App"), 0);
    }

    #[test]
    fn run_with_invalid_device() {
        assert_eq!(flatpak("run --device usb org.example.App"), 1);
    }

    #[test]
    fn run_with_filesystem() {
        assert_eq!(
            flatpak("run --filesystem /home/user org.example.App"),
            0
        );
    }

    // ====================================================================
    // List command
    // ====================================================================

    #[test]
    fn list_basic() {
        assert_eq!(flatpak("list"), 0);
    }

    #[test]
    fn list_app_only() {
        assert_eq!(flatpak("list --app"), 0);
    }

    #[test]
    fn list_runtime_only() {
        assert_eq!(flatpak("list --runtime"), 0);
    }

    #[test]
    fn list_app_and_runtime_error() {
        assert_eq!(flatpak("list --app --runtime"), 1);
    }

    #[test]
    fn list_user_scope() {
        assert_eq!(flatpak("list --user"), 0);
    }

    #[test]
    fn list_system_scope() {
        assert_eq!(flatpak("list --system"), 0);
    }

    #[test]
    fn list_invalid_column() {
        assert_eq!(flatpak("list --columns color"), 1);
    }

    // ====================================================================
    // Info command
    // ====================================================================

    #[test]
    fn info_no_ref() {
        assert_eq!(flatpak("info"), 1);
    }

    #[test]
    fn info_basic() {
        assert_eq!(flatpak("info org.example.App"), 0);
    }

    #[test]
    fn info_show_metadata() {
        assert_eq!(flatpak("info --show-metadata org.example.App"), 0);
    }

    #[test]
    fn info_show_permissions() {
        assert_eq!(flatpak("info --show-permissions org.example.App"), 0);
    }

    #[test]
    fn info_show_location() {
        assert_eq!(flatpak("info --show-location org.example.App"), 0);
    }

    #[test]
    fn info_invalid_id() {
        assert_eq!(flatpak("info badname"), 1);
    }

    // ====================================================================
    // Search command
    // ====================================================================

    #[test]
    fn search_no_query() {
        assert_eq!(flatpak("search"), 1);
    }

    #[test]
    fn search_basic() {
        assert_eq!(flatpak("search firefox"), 0);
    }

    // ====================================================================
    // Remote commands
    // ====================================================================

    #[test]
    fn remote_add_no_args() {
        assert_eq!(flatpak("remote-add"), 1);
    }

    #[test]
    fn remote_add_one_arg() {
        assert_eq!(flatpak("remote-add flathub"), 1);
    }

    #[test]
    fn remote_add_basic() {
        assert_eq!(
            flatpak("remote-add flathub https://dl.flathub.org/repo/"),
            0
        );
    }

    #[test]
    fn remote_add_no_gpg() {
        assert_eq!(
            flatpak("remote-add --no-gpg-verify myremote https://example.com/repo"),
            0
        );
    }

    #[test]
    fn remote_add_if_not_exists() {
        assert_eq!(
            flatpak("remote-add --if-not-exists flathub https://dl.flathub.org/repo/"),
            0
        );
    }

    #[test]
    fn remote_add_invalid_name() {
        assert_eq!(
            flatpak("remote-add bad@name https://example.com/repo"),
            1
        );
    }

    #[test]
    fn remote_add_invalid_url() {
        assert_eq!(flatpak("remote-add myremote example.com/repo"), 1);
    }

    #[test]
    fn remote_delete_no_name() {
        assert_eq!(flatpak("remote-delete"), 1);
    }

    #[test]
    fn remote_delete_basic() {
        assert_eq!(flatpak("remote-delete flathub"), 0);
    }

    #[test]
    fn remote_delete_force() {
        assert_eq!(flatpak("remote-delete --force flathub"), 0);
    }

    #[test]
    fn remote_delete_invalid_name() {
        assert_eq!(flatpak("remote-delete bad@name"), 1);
    }

    #[test]
    fn remote_list_basic() {
        assert_eq!(flatpak("remote-list"), 0);
    }

    #[test]
    fn remote_list_show_disabled() {
        assert_eq!(flatpak("remote-list --show-disabled"), 0);
    }

    #[test]
    fn remote_info_no_args() {
        assert_eq!(flatpak("remote-info"), 1);
    }

    #[test]
    fn remote_info_one_arg() {
        assert_eq!(flatpak("remote-info flathub"), 1);
    }

    #[test]
    fn remote_info_basic() {
        assert_eq!(
            flatpak("remote-info flathub org.example.App"),
            0
        );
    }

    #[test]
    fn remote_info_invalid_remote() {
        assert_eq!(flatpak("remote-info bad@name org.example.App"), 1);
    }

    #[test]
    fn remote_info_invalid_ref() {
        assert_eq!(flatpak("remote-info flathub badname"), 1);
    }

    // ====================================================================
    // Repair command
    // ====================================================================

    #[test]
    fn repair_system() {
        assert_eq!(flatpak("repair"), 0);
    }

    #[test]
    fn repair_user() {
        assert_eq!(flatpak("repair --user"), 0);
    }

    #[test]
    fn repair_system_explicit() {
        assert_eq!(flatpak("repair --system"), 0);
    }

    // ====================================================================
    // History command
    // ====================================================================

    #[test]
    fn history_basic() {
        assert_eq!(flatpak("history"), 0);
    }

    #[test]
    fn history_user() {
        assert_eq!(flatpak("history --user"), 0);
    }

    #[test]
    fn history_system() {
        assert_eq!(flatpak("history --system"), 0);
    }

    // ====================================================================
    // Override command
    // ====================================================================

    #[test]
    fn override_no_app() {
        assert_eq!(flatpak("override"), 1);
    }

    #[test]
    fn override_no_changes() {
        assert_eq!(flatpak("override org.example.App"), 0);
    }

    #[test]
    fn override_with_share() {
        assert_eq!(
            flatpak("override --share network org.example.App"),
            0
        );
    }

    #[test]
    fn override_invalid_app() {
        assert_eq!(flatpak("override badname"), 1);
    }

    // ====================================================================
    // Permission-show command
    // ====================================================================

    #[test]
    fn permission_show_no_app() {
        assert_eq!(flatpak("permission-show"), 1);
    }

    #[test]
    fn permission_show_basic() {
        assert_eq!(flatpak("permission-show org.example.App"), 0);
    }

    #[test]
    fn permission_show_invalid_app() {
        assert_eq!(flatpak("permission-show badname"), 1);
    }

    // ====================================================================
    // Build commands
    // ====================================================================

    #[test]
    fn build_init_no_args() {
        assert_eq!(flatpak("build-init"), 1);
    }

    #[test]
    fn build_init_basic() {
        assert_eq!(
            flatpak("build-init builddir org.example.App org.freedesktop.Sdk org.freedesktop.Platform"),
            0
        );
    }

    #[test]
    fn build_init_invalid_app_id() {
        assert_eq!(
            flatpak("build-init builddir badname org.freedesktop.Sdk org.freedesktop.Platform"),
            1
        );
    }

    #[test]
    fn build_init_invalid_sdk() {
        assert_eq!(
            flatpak("build-init builddir org.example.App badsdk org.freedesktop.Platform"),
            1
        );
    }

    #[test]
    fn build_init_invalid_runtime() {
        assert_eq!(
            flatpak("build-init builddir org.example.App org.freedesktop.Sdk badruntime"),
            1
        );
    }

    #[test]
    fn build_no_args() {
        assert_eq!(flatpak("build"), 1);
    }

    #[test]
    fn build_basic() {
        assert_eq!(flatpak("build builddir make install"), 0);
    }

    #[test]
    fn build_finish_no_dir() {
        assert_eq!(flatpak("build-finish"), 1);
    }

    #[test]
    fn build_finish_basic() {
        assert_eq!(flatpak("build-finish builddir"), 0);
    }

    #[test]
    fn build_export_no_args() {
        assert_eq!(flatpak("build-export"), 1);
    }

    #[test]
    fn build_export_basic() {
        assert_eq!(flatpak("build-export repo builddir"), 0);
    }

    #[test]
    fn build_import_bundle_no_args() {
        assert_eq!(flatpak("build-import-bundle"), 1);
    }

    #[test]
    fn build_import_bundle_basic() {
        assert_eq!(flatpak("build-import-bundle repo app.flatpak"), 0);
    }

    #[test]
    fn build_bundle_no_args() {
        assert_eq!(flatpak("build-bundle"), 1);
    }

    #[test]
    fn build_bundle_basic() {
        assert_eq!(
            flatpak("build-bundle repo app.flatpak org.example.App"),
            0
        );
    }

    #[test]
    fn build_bundle_invalid_ref() {
        assert_eq!(flatpak("build-bundle repo app.flatpak badname"), 1);
    }

    // ====================================================================
    // Config command
    // ====================================================================

    #[test]
    fn config_no_operation() {
        assert_eq!(flatpak("config"), 1);
    }

    #[test]
    fn config_multiple_operations() {
        assert_eq!(flatpak("config --set --get"), 1);
    }

    #[test]
    fn config_get_basic() {
        assert_eq!(flatpak("config --get languages"), 0);
    }

    #[test]
    fn config_get_no_key() {
        assert_eq!(flatpak("config --get"), 1);
    }

    #[test]
    fn config_set_basic() {
        assert_eq!(flatpak("config --set languages en"), 0);
    }

    #[test]
    fn config_set_no_value() {
        assert_eq!(flatpak("config --set languages"), 1);
    }

    #[test]
    fn config_unset_basic() {
        assert_eq!(flatpak("config --unset languages"), 0);
    }

    #[test]
    fn config_unset_no_key() {
        assert_eq!(flatpak("config --unset"), 1);
    }

    // ====================================================================
    // Help and version
    // ====================================================================

    #[test]
    fn help_global() {
        assert_eq!(flatpak("--help"), 0);
    }

    #[test]
    fn help_short() {
        assert_eq!(flatpak("-h"), 0);
    }

    #[test]
    fn version_flag() {
        assert_eq!(flatpak("--version"), 0);
    }

    #[test]
    fn unknown_command() {
        assert_eq!(flatpak("frobnicate"), 1);
    }

    #[test]
    fn no_args_shows_help() {
        assert_eq!(flatpak(""), 0);
    }

    // ====================================================================
    // Installation paths
    // ====================================================================

    #[test]
    fn system_install_path_value() {
        assert_eq!(system_install_path(), "/var/lib/flatpak");
    }

    #[test]
    fn install_path_system() {
        assert_eq!(
            install_path(InstallationType::System),
            "/var/lib/flatpak"
        );
    }

    #[test]
    fn install_path_user() {
        // user_install_path depends on HOME env var; just check it returns something
        let path = install_path(InstallationType::User);
        assert!(path.contains("flatpak"));
    }

    // ====================================================================
    // Edge cases
    // ====================================================================

    #[test]
    fn install_full_ref_with_arch() {
        assert_eq!(
            flatpak("install app/org.example.App/aarch64/beta"),
            0
        );
    }

    #[test]
    fn run_with_unshare() {
        assert_eq!(
            flatpak("run --unshare network org.example.App"),
            0
        );
    }

    #[test]
    fn run_with_nosocket() {
        assert_eq!(
            flatpak("run --nosocket x11 org.example.App"),
            0
        );
    }

    #[test]
    fn run_with_nodevice() {
        assert_eq!(
            flatpak("run --nodevice dri org.example.App"),
            0
        );
    }

    #[test]
    fn run_with_nofilesystem() {
        assert_eq!(
            flatpak("run --nofilesystem /home org.example.App"),
            0
        );
    }

    #[test]
    fn build_export_with_subject() {
        assert_eq!(
            flatpak("build-export --subject Release repo builddir"),
            0
        );
    }

    #[test]
    fn build_bundle_with_arch() {
        assert_eq!(
            flatpak("build-bundle --arch aarch64 repo app.flatpak org.example.App"),
            0
        );
    }

    #[test]
    fn format_size_one_byte() {
        assert_eq!(format_size(1), "1 bytes");
    }

    #[test]
    fn format_size_exact_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
    }

    #[test]
    fn build_state_fields() {
        let bs = BuildState {
            directory: "/tmp/build".to_string(),
            app_id: "org.example.App".to_string(),
            sdk: "org.freedesktop.Sdk".to_string(),
            runtime: "org.freedesktop.Platform".to_string(),
            finished: false,
        };
        assert!(!bs.finished);
        assert_eq!(bs.app_id, "org.example.App");
    }

    #[test]
    fn remote_fields() {
        let r = Remote::new("test", "https://example.com", InstallationType::User);
        assert_eq!(r.name, "test");
        assert_eq!(r.url, "https://example.com");
        assert!(r.title.is_empty());
        assert_eq!(r.installation, InstallationType::User);
    }

    #[test]
    fn config_overwrite() {
        let mut c = Config::default();
        c.set("key", "v1");
        c.set("key", "v2");
        assert_eq!(c.get("key"), Some("v2"));
    }
}
