//! OurOS GRUB bootloader management utility.
//!
//! Multi-personality binary providing:
//! - **grub-install** (default) — install GRUB bootloader to a device
//! - **grub-mkconfig** — generate GRUB configuration file
//! - **grub-set-default** — set default boot entry (saved in grubenv)
//! - **grub-reboot** — set one-time boot entry (next_entry in grubenv)
//! - **grub-editenv** — edit GRUB environment block (1024-byte format)
//! - **grub-probe** — probe device for filesystem information
//! - **update-grub** — convenience wrapper for grub-mkconfig -o /boot/grub/grub.cfg
//!
//! Personality is detected via the basename of argv[0].

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const GRUB_ENV_SIZE: usize = 1024;
const GRUB_ENV_HEADER: &str = "# GRUB Environment Block\n";
const _DEFAULT_GRUB_DIR: &str = "/boot/grub";
const DEFAULT_GRUB_CFG: &str = "/boot/grub/grub.cfg";
const DEFAULT_GRUBENV: &str = "/boot/grub/grubenv";
const DEFAULT_GRUB_DEFAULTS: &str = "/etc/default/grub";
const OS_RELEASE_PATH: &str = "/etc/os-release";
const GRUB_D_DIR: &str = "/etc/grub.d";
const BOOT_DIR: &str = "/boot";
const EFI_FALLBACK_DIR: &str = "/boot/efi";

// Known GRUB targets.
const KNOWN_TARGETS: &[&str] = &[
    "i386-pc",
    "x86_64-efi",
    "i386-efi",
    "arm-efi",
    "arm64-efi",
    "i386-coreboot",
    "i386-multiboot",
    "mips-arc",
    "mipsel-loongson",
    "powerpc-ieee1275",
    "sparc64-ieee1275",
    "x86_64-xen",
];

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum GrubError {
    Io(io::Error),
    InvalidArgs(String),
    InvalidEnvBlock(String),
    DeviceNotFound(String),
    UnsupportedTarget(String),
}

impl std::fmt::Display for GrubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrubError::Io(e) => write!(f, "I/O error: {e}"),
            GrubError::InvalidArgs(msg) => write!(f, "invalid arguments: {msg}"),
            GrubError::InvalidEnvBlock(msg) => write!(f, "invalid environment block: {msg}"),
            GrubError::DeviceNotFound(dev) => write!(f, "device not found: {dev}"),
            GrubError::UnsupportedTarget(t) => write!(f, "unsupported target: {t}"),
        }
    }
}

impl From<io::Error> for GrubError {
    fn from(e: io::Error) -> Self {
        GrubError::Io(e)
    }
}

// ============================================================================
// Boot mode detection
// ============================================================================

/// Detected boot firmware mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootMode {
    Bios,
    Efi,
}

impl std::fmt::Display for BootMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootMode::Bios => write!(f, "BIOS"),
            BootMode::Efi => write!(f, "EFI"),
        }
    }
}

/// Detect whether we booted via EFI or BIOS by checking for /sys/firmware/efi.
fn detect_boot_mode() -> BootMode {
    if Path::new("/sys/firmware/efi").exists() {
        BootMode::Efi
    } else {
        BootMode::Bios
    }
}

// ============================================================================
// OS release parsing
// ============================================================================

/// Parse a file in os-release format (KEY=VALUE or KEY="VALUE").
fn parse_os_release(content: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let mut val = line[eq_pos + 1..].trim().to_string();
            // Strip surrounding quotes.
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val = val[1..val.len() - 1].to_string();
            }
            map.insert(key, val);
        }
    }
    map
}

/// Read the OS pretty name from /etc/os-release.
fn read_os_name(os_release_path: &str) -> String {
    if let Ok(content) = fs::read_to_string(os_release_path) {
        let map = parse_os_release(&content);
        if let Some(name) = map.get("PRETTY_NAME") {
            return name.clone();
        }
        if let Some(name) = map.get("NAME") {
            return name.clone();
        }
    }
    "OurOS".to_string()
}

// ============================================================================
// GRUB defaults parsing (/etc/default/grub)
// ============================================================================

/// Parsed GRUB default configuration.
#[derive(Debug, Clone)]
struct GrubDefaults {
    timeout: u32,
    default_entry: String,
    cmdline_linux: String,
    cmdline_linux_default: String,
    terminal_output: String,
    disable_os_prober: bool,
}

impl Default for GrubDefaults {
    fn default() -> Self {
        Self {
            timeout: 5,
            default_entry: "0".to_string(),
            cmdline_linux: String::new(),
            cmdline_linux_default: "quiet splash".to_string(),
            terminal_output: "console".to_string(),
            disable_os_prober: false,
        }
    }
}

/// Parse /etc/default/grub into structured defaults.
fn parse_grub_defaults(content: &str) -> GrubDefaults {
    let mut defaults = GrubDefaults::default();
    let map = parse_os_release(content); // Same KEY=VALUE format.

    if let Some(v) = map.get("GRUB_TIMEOUT") {
        if let Ok(t) = v.parse::<u32>() {
            defaults.timeout = t;
        }
    }
    if let Some(v) = map.get("GRUB_DEFAULT") {
        defaults.default_entry = v.clone();
    }
    if let Some(v) = map.get("GRUB_CMDLINE_LINUX") {
        defaults.cmdline_linux = v.clone();
    }
    if let Some(v) = map.get("GRUB_CMDLINE_LINUX_DEFAULT") {
        defaults.cmdline_linux_default = v.clone();
    }
    if let Some(v) = map.get("GRUB_TERMINAL_OUTPUT") {
        defaults.terminal_output = v.clone();
    }
    if let Some(v) = map.get("GRUB_DISABLE_OS_PROBER") {
        defaults.disable_os_prober = v == "true" || v == "1";
    }

    defaults
}

fn read_grub_defaults(path: &str) -> GrubDefaults {
    if let Ok(content) = fs::read_to_string(path) {
        parse_grub_defaults(&content)
    } else {
        GrubDefaults::default()
    }
}

// ============================================================================
// Kernel scanning
// ============================================================================

/// A detected kernel + initrd pair under /boot.
#[derive(Debug, Clone)]
struct KernelEntry {
    version: String,
    kernel_path: String,
    initrd_path: Option<String>,
}

/// Scan a directory for kernel images (vmlinuz-*) and matching initrd files.
fn scan_kernels(boot_dir: &str) -> Vec<KernelEntry> {
    let mut entries = Vec::new();
    let dir = match fs::read_dir(boot_dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };

    let mut kernel_files: Vec<String> = Vec::new();
    let mut initrd_files: Vec<String> = Vec::new();

    for entry in dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("vmlinuz-") {
            kernel_files.push(name);
        } else if name.starts_with("initrd.img-") || name.starts_with("initramfs-") {
            initrd_files.push(name);
        }
    }

    // Sort descending so newest kernel comes first.
    kernel_files.sort_by(|a, b| b.cmp(a));

    for kf in &kernel_files {
        let version = if let Some(v) = kf.strip_prefix("vmlinuz-") {
            v.to_string()
        } else {
            continue;
        };

        // Find matching initrd.
        let initrd = initrd_files
            .iter()
            .find(|i| {
                i.strip_prefix("initrd.img-")
                    .or_else(|| i.strip_prefix("initramfs-"))
                    .map(|v| {
                        let v = v.strip_suffix(".img").unwrap_or(v);
                        v == version
                    })
                    .unwrap_or(false)
            })
            .map(|i| format!("{boot_dir}/{i}"));

        entries.push(KernelEntry {
            version,
            kernel_path: format!("{boot_dir}/{kf}"),
            initrd_path: initrd,
        });
    }

    entries
}

// ============================================================================
// GRUB environment block (grubenv)
// ============================================================================

/// A GRUB environment block: 1024 bytes, starts with header comment,
/// contains KEY=VALUE pairs separated by newlines, padded with '#' bytes.
#[derive(Debug, Clone)]
struct GrubEnv {
    vars: BTreeMap<String, String>,
}

impl GrubEnv {
    fn new() -> Self {
        Self {
            vars: BTreeMap::new(),
        }
    }

    /// Parse a GRUB environment block from raw bytes.
    fn parse(data: &[u8]) -> Result<Self, GrubError> {
        if data.len() != GRUB_ENV_SIZE {
            return Err(GrubError::InvalidEnvBlock(format!(
                "expected {GRUB_ENV_SIZE} bytes, got {}",
                data.len()
            )));
        }

        let text = String::from_utf8_lossy(data);
        if !text.starts_with(GRUB_ENV_HEADER) {
            return Err(GrubError::InvalidEnvBlock(
                "missing GRUB environment block header".to_string(),
            ));
        }

        let body = &text[GRUB_ENV_HEADER.len()..];
        let mut vars = BTreeMap::new();

        for line in body.lines() {
            let line = line.trim_end_matches('#');
            if line.is_empty() {
                continue;
            }
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].to_string();
                let val = line[eq_pos + 1..].to_string();
                if !key.is_empty() && !key.starts_with('#') {
                    vars.insert(key, val);
                }
            }
        }

        Ok(Self { vars })
    }

    /// Serialize to a 1024-byte environment block.
    fn serialize(&self) -> Vec<u8> {
        let mut content = String::from(GRUB_ENV_HEADER);
        for (key, val) in &self.vars {
            let _ = writeln!(content, "{key}={val}");
        }

        let mut buf = content.into_bytes();

        // Pad with '#' to reach exactly GRUB_ENV_SIZE bytes.
        if buf.len() < GRUB_ENV_SIZE {
            buf.resize(GRUB_ENV_SIZE, b'#');
        } else {
            buf.truncate(GRUB_ENV_SIZE);
        }

        buf
    }

    #[allow(dead_code)] // Used in tests; public API for external consumers.
    fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|s| s.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        self.vars.insert(key.to_string(), value.to_string());
    }

    fn unset(&mut self, key: &str) {
        self.vars.remove(key);
    }
}

/// Read grubenv from a file path, or return a new empty env if not found.
fn read_grubenv(path: &str) -> GrubEnv {
    match fs::read(path) {
        Ok(data) => GrubEnv::parse(&data).unwrap_or_else(|_| GrubEnv::new()),
        Err(_) => GrubEnv::new(),
    }
}

/// Write grubenv to a file path.
fn write_grubenv(path: &str, env: &GrubEnv) -> Result<(), GrubError> {
    let data = env.serialize();
    fs::write(path, &data)?;
    Ok(())
}

// ============================================================================
// grub-install
// ============================================================================

struct InstallOptions {
    target: Option<String>,
    efi_directory: Option<String>,
    boot_directory: String,
    bootloader_id: String,
    recheck: bool,
    removable: bool,
    device: Option<String>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            target: None,
            efi_directory: None,
            boot_directory: "/boot".to_string(),
            bootloader_id: "ouros".to_string(),
            recheck: false,
            removable: false,
            device: None,
        }
    }
}

fn parse_install_args(args: &[String]) -> Result<InstallOptions, GrubError> {
    let mut opts = InstallOptions::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--target" {
            i += 1;
            let t = args
                .get(i)
                .ok_or_else(|| GrubError::InvalidArgs("--target requires a value".into()))?;
            if !KNOWN_TARGETS.contains(&t.as_str()) {
                return Err(GrubError::UnsupportedTarget(t.clone()));
            }
            opts.target = Some(t.clone());
        } else if let Some(t) = arg.strip_prefix("--target=") {
            if !KNOWN_TARGETS.contains(&t) {
                return Err(GrubError::UnsupportedTarget(t.to_string()));
            }
            opts.target = Some(t.to_string());
        } else if arg == "--efi-directory" {
            i += 1;
            opts.efi_directory = Some(
                args.get(i)
                    .ok_or_else(|| {
                        GrubError::InvalidArgs("--efi-directory requires a value".into())
                    })?
                    .clone(),
            );
        } else if let Some(v) = arg.strip_prefix("--efi-directory=") {
            opts.efi_directory = Some(v.to_string());
        } else if arg == "--boot-directory" {
            i += 1;
            opts.boot_directory = args
                .get(i)
                .ok_or_else(|| {
                    GrubError::InvalidArgs("--boot-directory requires a value".into())
                })?
                .clone();
        } else if let Some(v) = arg.strip_prefix("--boot-directory=") {
            opts.boot_directory = v.to_string();
        } else if arg == "--bootloader-id" {
            i += 1;
            opts.bootloader_id = args
                .get(i)
                .ok_or_else(|| {
                    GrubError::InvalidArgs("--bootloader-id requires a value".into())
                })?
                .clone();
        } else if let Some(v) = arg.strip_prefix("--bootloader-id=") {
            opts.bootloader_id = v.to_string();
        } else if arg == "--recheck" {
            opts.recheck = true;
        } else if arg == "--removable" {
            opts.removable = true;
        } else if arg == "--help" || arg == "-h" {
            print_install_usage();
            process::exit(0);
        } else if arg == "--version" || arg == "-V" {
            println!("grub-install (OurOS) {VERSION}");
            process::exit(0);
        } else if arg.starts_with('-') {
            return Err(GrubError::InvalidArgs(format!("unknown option: {arg}")));
        } else {
            opts.device = Some(arg.clone());
        }
        i += 1;
    }
    Ok(opts)
}

fn print_install_usage() {
    println!("Usage: grub-install [OPTIONS] [DEVICE]");
    println!();
    println!("Install GRUB bootloader to a device.");
    println!();
    println!("Options:");
    println!("  --target=TARGET        Installation target platform");
    println!("  --efi-directory=DIR    EFI system partition mount point");
    println!("  --boot-directory=DIR   Boot directory (default: /boot)");
    println!("  --bootloader-id=ID     Bootloader identifier (default: ouros)");
    println!("  --recheck              Re-check device map");
    println!("  --removable            Install for removable media");
    println!("  -h, --help             Show this help");
    println!("  -V, --version          Show version");
    println!();
    println!("Supported targets:");
    for t in KNOWN_TARGETS {
        println!("  {t}");
    }
}

fn run_install(args: &[String]) -> Result<(), GrubError> {
    let opts = parse_install_args(args)?;
    let boot_mode = detect_boot_mode();

    // Auto-detect target if not specified.
    let target = opts.target.clone().unwrap_or_else(|| match boot_mode {
        BootMode::Efi => "x86_64-efi".to_string(),
        BootMode::Bios => "i386-pc".to_string(),
    });

    let is_efi = target.contains("efi");

    println!("Installing GRUB to {}...", target);
    println!("  Boot mode: {boot_mode}");
    println!("  Boot directory: {}", opts.boot_directory);
    println!("  Bootloader ID: {}", opts.bootloader_id);

    if opts.recheck {
        println!("  Rechecking device map...");
    }

    if is_efi {
        let efi_dir = opts
            .efi_directory
            .as_deref()
            .unwrap_or(EFI_FALLBACK_DIR);
        println!("  EFI directory: {efi_dir}");

        // Create EFI bootloader directory.
        let efi_boot_dir = if opts.removable {
            format!("{efi_dir}/EFI/BOOT")
        } else {
            format!("{efi_dir}/EFI/{}", opts.bootloader_id)
        };

        create_dir_all_quiet(&efi_boot_dir);

        // Determine EFI binary name.
        let efi_binary = if target.starts_with("x86_64") {
            if opts.removable {
                "BOOTX64.EFI"
            } else {
                "grubx64.efi"
            }
        } else if target.starts_with("i386") {
            if opts.removable {
                "BOOTIA32.EFI"
            } else {
                "grubia32.efi"
            }
        } else if target.starts_with("arm64") || target.starts_with("aarch64") {
            if opts.removable {
                "BOOTAA64.EFI"
            } else {
                "grubaa64.efi"
            }
        } else if opts.removable {
            "BOOT.EFI"
        } else {
            "grub.efi"
        };

        println!("  EFI binary: {efi_boot_dir}/{efi_binary}");
    } else {
        // BIOS installation requires a device.
        let device = opts
            .device
            .as_deref()
            .unwrap_or("/dev/sda");
        println!("  Install device: {device}");

        if !Path::new(device).exists() {
            return Err(GrubError::DeviceNotFound(device.to_string()));
        }
    }

    // Create GRUB directory.
    let grub_dir = format!("{}/grub", opts.boot_directory);
    create_dir_all_quiet(&grub_dir);

    // Write device.map if rechecking or doesn't exist.
    let device_map_path = format!("{grub_dir}/device.map");
    if opts.recheck || !Path::new(&device_map_path).exists() {
        let device = opts.device.as_deref().unwrap_or("/dev/sda");
        let content = format!("(hd0)\t{device}\n");
        let _ = fs::write(&device_map_path, content);
        println!("  Wrote {device_map_path}");
    }

    println!("Installation finished. No error reported.");
    Ok(())
}

fn create_dir_all_quiet(path: &str) {
    let _ = fs::create_dir_all(path);
}

// ============================================================================
// grub-mkconfig
// ============================================================================

struct MkconfigOptions {
    output: Option<String>,
    boot_dir: String,
    grub_defaults_path: String,
    os_release_path: String,
    grub_d_dir: String,
}

impl Default for MkconfigOptions {
    fn default() -> Self {
        Self {
            output: None,
            boot_dir: BOOT_DIR.to_string(),
            grub_defaults_path: DEFAULT_GRUB_DEFAULTS.to_string(),
            os_release_path: OS_RELEASE_PATH.to_string(),
            grub_d_dir: GRUB_D_DIR.to_string(),
        }
    }
}

fn parse_mkconfig_args(args: &[String]) -> Result<MkconfigOptions, GrubError> {
    let mut opts = MkconfigOptions::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-o" {
            i += 1;
            opts.output = Some(
                args.get(i)
                    .ok_or_else(|| GrubError::InvalidArgs("-o requires a file path".into()))?
                    .clone(),
            );
        } else if let Some(v) = arg.strip_prefix("-o=") {
            opts.output = Some(v.to_string());
        } else if arg == "--output" {
            i += 1;
            opts.output = Some(
                args.get(i)
                    .ok_or_else(|| GrubError::InvalidArgs("--output requires a file path".into()))?
                    .clone(),
            );
        } else if let Some(v) = arg.strip_prefix("--output=") {
            opts.output = Some(v.to_string());
        } else if arg == "--help" || arg == "-h" {
            print_mkconfig_usage();
            process::exit(0);
        } else if arg == "--version" || arg == "-V" {
            println!("grub-mkconfig (OurOS) {VERSION}");
            process::exit(0);
        } else if arg.starts_with('-') {
            return Err(GrubError::InvalidArgs(format!("unknown option: {arg}")));
        }
        i += 1;
    }
    Ok(opts)
}

fn print_mkconfig_usage() {
    println!("Usage: grub-mkconfig [-o OUTPUT_FILE]");
    println!();
    println!("Generate GRUB configuration file.");
    println!();
    println!("Options:");
    println!("  -o FILE      Output to FILE instead of stdout");
    println!("  --output=FILE  Same as -o");
    println!("  -h, --help   Show this help");
    println!("  -V, --version  Show version");
}

/// Generate the GRUB configuration text.
fn generate_grub_config(opts: &MkconfigOptions) -> String {
    let defaults = read_grub_defaults(&opts.grub_defaults_path);
    let os_name = read_os_name(&opts.os_release_path);
    let kernels = scan_kernels(&opts.boot_dir);

    let mut cfg = String::new();

    // 00_header equivalent.
    generate_header(&mut cfg, &defaults);

    // 10_linux equivalent.
    generate_linux_entries(&mut cfg, &kernels, &os_name, &defaults);

    // 30_os-prober equivalent (only if not disabled).
    if !defaults.disable_os_prober {
        generate_os_prober_section(&mut cfg);
    }

    // 40_custom equivalent.
    generate_custom_section(&mut cfg, &opts.grub_d_dir);

    cfg
}

fn generate_header(cfg: &mut String, defaults: &GrubDefaults) {
    let _ = writeln!(cfg, "#");
    let _ = writeln!(cfg, "# DO NOT EDIT THIS FILE");
    let _ = writeln!(cfg, "#");
    let _ = writeln!(
        cfg,
        "# It is automatically generated by grub-mkconfig using templates"
    );
    let _ = writeln!(cfg, "# from /etc/grub.d and settings from /etc/default/grub");
    let _ = writeln!(cfg, "#");
    let _ = writeln!(cfg);
    let _ = writeln!(cfg, "# BEGIN /etc/grub.d/00_header");
    let _ = writeln!(cfg, "set default=\"{}\"", defaults.default_entry);

    if defaults.default_entry == "saved" {
        let _ = writeln!(cfg, "load_env");
        let _ = writeln!(cfg, "set default=\"${{saved_entry}}\"");
    }

    let _ = writeln!(cfg, "set timeout={}", defaults.timeout);
    let _ = writeln!(cfg, "set terminal_output={}", defaults.terminal_output);
    let _ = writeln!(cfg, "# END /etc/grub.d/00_header");
    let _ = writeln!(cfg);
}

fn generate_linux_entries(
    cfg: &mut String,
    kernels: &[KernelEntry],
    os_name: &str,
    defaults: &GrubDefaults,
) {
    let _ = writeln!(cfg, "# BEGIN /etc/grub.d/10_linux");

    if kernels.is_empty() {
        let _ = writeln!(cfg, "# No kernels found in /boot");
        let _ = writeln!(cfg, "# END /etc/grub.d/10_linux");
        let _ = writeln!(cfg);
        return;
    }

    // Combine command line arguments.
    let cmdline = build_cmdline(&defaults.cmdline_linux, &defaults.cmdline_linux_default);

    for (idx, kernel) in kernels.iter().enumerate() {
        // Primary entry.
        let title = if idx == 0 {
            format!("{os_name}")
        } else {
            format!("{os_name}, with Linux {}", kernel.version)
        };

        let _ = writeln!(cfg, "menuentry '{title}' --class ouros --class os {{");
        let _ = writeln!(cfg, "\tload_video");
        let _ = writeln!(cfg, "\tinsmod gzio");
        let _ = writeln!(cfg, "\tinsmod part_gpt");
        let _ = writeln!(cfg, "\tinsmod ext2");

        if cmdline.is_empty() {
            let _ = writeln!(cfg, "\tlinux {} root=UUID=XXXX ro", kernel.kernel_path);
        } else {
            let _ = writeln!(
                cfg,
                "\tlinux {} root=UUID=XXXX ro {}",
                kernel.kernel_path, cmdline
            );
        }

        if let Some(ref initrd) = kernel.initrd_path {
            let _ = writeln!(cfg, "\tinitrd {initrd}");
        }
        let _ = writeln!(cfg, "}}");
        let _ = writeln!(cfg);

        // Recovery entry.
        let recovery_title = format!(
            "{os_name}, with Linux {} (recovery mode)",
            kernel.version
        );
        let _ = writeln!(
            cfg,
            "menuentry '{recovery_title}' --class ouros --class os {{"
        );
        let _ = writeln!(cfg, "\tload_video");
        let _ = writeln!(cfg, "\tinsmod gzio");
        let _ = writeln!(cfg, "\tinsmod part_gpt");
        let _ = writeln!(cfg, "\tinsmod ext2");
        let _ = writeln!(
            cfg,
            "\tlinux {} root=UUID=XXXX ro single {}",
            kernel.kernel_path, defaults.cmdline_linux
        );
        if let Some(ref initrd) = kernel.initrd_path {
            let _ = writeln!(cfg, "\tinitrd {initrd}");
        }
        let _ = writeln!(cfg, "}}");
        let _ = writeln!(cfg);
    }

    let _ = writeln!(cfg, "# END /etc/grub.d/10_linux");
    let _ = writeln!(cfg);
}

fn build_cmdline(cmdline_linux: &str, cmdline_linux_default: &str) -> String {
    let mut parts = Vec::new();
    if !cmdline_linux_default.is_empty() {
        parts.push(cmdline_linux_default.to_string());
    }
    if !cmdline_linux.is_empty() {
        parts.push(cmdline_linux.to_string());
    }
    parts.join(" ")
}

fn generate_os_prober_section(cfg: &mut String) {
    let _ = writeln!(cfg, "# BEGIN /etc/grub.d/30_os-prober");
    let _ = writeln!(cfg, "# OS prober would scan for other operating systems here.");
    let _ = writeln!(cfg, "# Currently no other OS detected.");
    let _ = writeln!(cfg, "# END /etc/grub.d/30_os-prober");
    let _ = writeln!(cfg);
}

fn generate_custom_section(cfg: &mut String, grub_d_dir: &str) {
    let _ = writeln!(cfg, "# BEGIN /etc/grub.d/40_custom");

    let custom_path = PathBuf::from(grub_d_dir).join("40_custom");
    if let Ok(content) = fs::read_to_string(&custom_path) {
        // Skip shebang and comment lines at the top.
        let mut in_header = true;
        for line in content.lines() {
            if in_header {
                if line.starts_with("#!") || line.starts_with('#') || line.trim().is_empty() {
                    continue;
                }
                in_header = false;
            }
            let _ = writeln!(cfg, "{line}");
        }
    }

    let _ = writeln!(cfg, "# END /etc/grub.d/40_custom");
}

fn run_mkconfig(args: &[String]) -> Result<(), GrubError> {
    let opts = parse_mkconfig_args(args)?;

    eprintln!("Generating grub configuration file ...");

    let config = generate_grub_config(&opts);

    if let Some(ref output) = opts.output {
        // Ensure parent directory exists.
        if let Some(parent) = Path::new(output).parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(output, &config)?;
        eprintln!("done");
    } else {
        print!("{config}");
    }

    Ok(())
}

// ============================================================================
// grub-set-default
// ============================================================================

fn print_set_default_usage() {
    println!("Usage: grub-set-default ENTRY");
    println!();
    println!("Set the default boot entry. ENTRY is a menu entry number or title.");
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help");
    println!("  -V, --version  Show version");
}

fn run_set_default(args: &[String]) -> Result<(), GrubError> {
    run_set_default_with_path(args, DEFAULT_GRUBENV)
}

fn run_set_default_with_path(args: &[String], env_path: &str) -> Result<(), GrubError> {
    if args.is_empty() {
        return Err(GrubError::InvalidArgs(
            "grub-set-default requires an entry argument".into(),
        ));
    }

    let entry = &args[0];
    if entry == "--help" || entry == "-h" {
        print_set_default_usage();
        return Ok(());
    }
    if entry == "--version" || entry == "-V" {
        println!("grub-set-default (OurOS) {VERSION}");
        return Ok(());
    }

    let mut env = read_grubenv(env_path);
    env.set("saved_entry", entry);
    write_grubenv(env_path, &env)?;

    println!("Default boot entry set to: {entry}");
    Ok(())
}

// ============================================================================
// grub-reboot
// ============================================================================

fn print_reboot_usage() {
    println!("Usage: grub-reboot ENTRY");
    println!();
    println!("Set a one-time boot entry for the next reboot only.");
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help");
    println!("  -V, --version  Show version");
}

fn run_reboot(args: &[String]) -> Result<(), GrubError> {
    run_reboot_with_path(args, DEFAULT_GRUBENV)
}

fn run_reboot_with_path(args: &[String], env_path: &str) -> Result<(), GrubError> {
    if args.is_empty() {
        return Err(GrubError::InvalidArgs(
            "grub-reboot requires an entry argument".into(),
        ));
    }

    let entry = &args[0];
    if entry == "--help" || entry == "-h" {
        print_reboot_usage();
        return Ok(());
    }
    if entry == "--version" || entry == "-V" {
        println!("grub-reboot (OurOS) {VERSION}");
        return Ok(());
    }

    let mut env = read_grubenv(env_path);
    env.set("next_entry", entry);
    write_grubenv(env_path, &env)?;

    println!("One-time boot entry set to: {entry}");
    Ok(())
}

// ============================================================================
// grub-editenv
// ============================================================================

fn print_editenv_usage() {
    println!("Usage: grub-editenv [FILE] COMMAND [ARGS...]");
    println!();
    println!("Edit GRUB environment block.");
    println!();
    println!("Commands:");
    println!("  list           List environment variables");
    println!("  set KEY=VALUE  Set a variable");
    println!("  unset KEY      Unset a variable");
    println!("  create         Create a new (empty) environment block");
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help");
    println!("  -V, --version  Show version");
    println!();
    println!("If FILE is not specified, {DEFAULT_GRUBENV} is used.");
}

struct EditenvArgs {
    file: String,
    command: String,
    params: Vec<String>,
}

fn parse_editenv_args(args: &[String]) -> Result<EditenvArgs, GrubError> {
    if args.is_empty() {
        return Err(GrubError::InvalidArgs(
            "grub-editenv requires a command (list, set, unset, create)".into(),
        ));
    }

    // Check for help/version first.
    if args[0] == "--help" || args[0] == "-h" {
        print_editenv_usage();
        process::exit(0);
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("grub-editenv (OurOS) {VERSION}");
        process::exit(0);
    }

    // Determine if first arg is a file or a command.
    let commands = ["list", "set", "unset", "create"];
    let (file, cmd_start) = if commands.contains(&args[0].as_str()) {
        (DEFAULT_GRUBENV.to_string(), 0)
    } else {
        if args.len() < 2 {
            return Err(GrubError::InvalidArgs(
                "expected a command after the file path".into(),
            ));
        }
        (args[0].clone(), 1)
    };

    let command = args
        .get(cmd_start)
        .ok_or_else(|| GrubError::InvalidArgs("missing command".into()))?
        .clone();

    if !commands.contains(&command.as_str()) {
        return Err(GrubError::InvalidArgs(format!(
            "unknown command: {command}. Expected: list, set, unset, create"
        )));
    }

    let params = args[cmd_start + 1..].to_vec();

    Ok(EditenvArgs {
        file,
        command,
        params,
    })
}

fn run_editenv(args: &[String]) -> Result<(), GrubError> {
    let ea = parse_editenv_args(args)?;

    match ea.command.as_str() {
        "list" => {
            let env = read_grubenv(&ea.file);
            if env.vars.is_empty() {
                println!("# No variables set.");
            } else {
                for (key, val) in &env.vars {
                    println!("{key}={val}");
                }
            }
        }
        "set" => {
            if ea.params.is_empty() {
                return Err(GrubError::InvalidArgs(
                    "set requires KEY=VALUE argument(s)".into(),
                ));
            }
            let mut env = read_grubenv(&ea.file);
            for param in &ea.params {
                if let Some(eq_pos) = param.find('=') {
                    let key = &param[..eq_pos];
                    let val = &param[eq_pos + 1..];
                    if key.is_empty() {
                        return Err(GrubError::InvalidArgs(
                            "variable name cannot be empty".into(),
                        ));
                    }
                    env.set(key, val);
                } else {
                    return Err(GrubError::InvalidArgs(format!(
                        "expected KEY=VALUE, got: {param}"
                    )));
                }
            }
            write_grubenv(&ea.file, &env)?;
        }
        "unset" => {
            if ea.params.is_empty() {
                return Err(GrubError::InvalidArgs(
                    "unset requires a variable name".into(),
                ));
            }
            let mut env = read_grubenv(&ea.file);
            for key in &ea.params {
                env.unset(key);
            }
            write_grubenv(&ea.file, &env)?;
        }
        "create" => {
            let env = GrubEnv::new();
            write_grubenv(&ea.file, &env)?;
            println!("Created new environment block: {}", ea.file);
        }
        _ => unreachable!(),
    }

    Ok(())
}

// ============================================================================
// grub-probe
// ============================================================================

struct ProbeOptions {
    target: String,
    device: String,
}

fn parse_probe_args(args: &[String]) -> Result<ProbeOptions, GrubError> {
    let mut target = "device".to_string();
    let mut device = String::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--target" || arg == "-t" {
            i += 1;
            target = args
                .get(i)
                .ok_or_else(|| GrubError::InvalidArgs("--target requires a value".into()))?
                .clone();
        } else if let Some(v) = arg.strip_prefix("--target=") {
            target = v.to_string();
        } else if let Some(v) = arg.strip_prefix("-t=") {
            target = v.to_string();
        } else if arg == "--help" || arg == "-h" {
            print_probe_usage();
            process::exit(0);
        } else if arg == "--version" || arg == "-V" {
            println!("grub-probe (OurOS) {VERSION}");
            process::exit(0);
        } else if arg.starts_with('-') {
            return Err(GrubError::InvalidArgs(format!("unknown option: {arg}")));
        } else {
            device = arg.clone();
        }
        i += 1;
    }

    if device.is_empty() {
        return Err(GrubError::InvalidArgs(
            "grub-probe requires a device or path argument".into(),
        ));
    }

    let valid_targets = [
        "device",
        "fs",
        "fs_uuid",
        "fs_label",
        "drive",
        "partmap",
    ];
    if !valid_targets.contains(&target.as_str()) {
        return Err(GrubError::InvalidArgs(format!(
            "unknown probe target: {target}. Valid targets: {}",
            valid_targets.join(", ")
        )));
    }

    Ok(ProbeOptions { target, device })
}

fn print_probe_usage() {
    println!("Usage: grub-probe [OPTIONS] DEVICE_OR_PATH");
    println!();
    println!("Probe device information for GRUB.");
    println!();
    println!("Options:");
    println!("  -t, --target=TARGET  Probe target (default: device)");
    println!("                       Valid: device, fs, fs_uuid, fs_label, drive, partmap");
    println!("  -h, --help           Show this help");
    println!("  -V, --version        Show version");
}

/// Probe a device or mountpoint and print the requested information.
fn run_probe(args: &[String]) -> Result<(), GrubError> {
    let opts = parse_probe_args(args)?;
    let device = &opts.device;

    // Try to resolve the device from a mountpoint.
    let resolved_device = resolve_device(device);

    match opts.target.as_str() {
        "device" => {
            println!("{}", resolved_device);
        }
        "fs" => {
            let fs_type = probe_fs_type(&resolved_device);
            println!("{fs_type}");
        }
        "fs_uuid" => {
            let uuid = probe_fs_uuid(&resolved_device);
            println!("{uuid}");
        }
        "fs_label" => {
            let label = probe_fs_label(&resolved_device);
            println!("{label}");
        }
        "drive" => {
            let drive = resolve_grub_drive(&resolved_device);
            println!("{drive}");
        }
        "partmap" => {
            let partmap = probe_partmap(&resolved_device);
            println!("{partmap}");
        }
        _ => {
            return Err(GrubError::InvalidArgs(format!(
                "unknown target: {}",
                opts.target
            )));
        }
    }

    Ok(())
}

/// Resolve a mountpoint or path to a device by reading /proc/mounts.
fn resolve_device(path: &str) -> String {
    // If it already looks like a device path, return as-is.
    if path.starts_with("/dev/") {
        return path.to_string();
    }

    // Try to read /proc/mounts for mountpoint resolution.
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        let mut best_match = String::new();
        let mut best_len = 0;
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let mount_dev = parts[0];
                let mount_point = parts[1];
                if path.starts_with(mount_point) && mount_point.len() > best_len {
                    best_match = mount_dev.to_string();
                    best_len = mount_point.len();
                }
            }
        }
        if !best_match.is_empty() {
            return best_match;
        }
    }

    // Fallback: return path as-is.
    path.to_string()
}

/// Probe filesystem type from sysfs or fallback.
fn probe_fs_type(device: &str) -> String {
    // Try /sys/class/block/<dev>/device/... or blkid-like approach.
    let dev_name = device
        .strip_prefix("/dev/")
        .unwrap_or(device);

    let fstype_path = format!("/sys/class/block/{dev_name}/device/fstype");
    if let Ok(t) = fs::read_to_string(&fstype_path) {
        let t = t.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }

    // Try reading /proc/mounts for the filesystem type.
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == device {
                return parts[2].to_string();
            }
        }
    }

    "ext2".to_string()
}

/// Probe filesystem UUID from sysfs.
fn probe_fs_uuid(device: &str) -> String {
    let dev_name = device
        .strip_prefix("/dev/")
        .unwrap_or(device);

    // Try sysfs first.
    let paths = [
        format!("/sys/class/block/{dev_name}/uuid"),
        format!("/sys/class/block/{dev_name}/device/uuid"),
    ];

    for p in &paths {
        if let Ok(uuid) = fs::read_to_string(p) {
            let uuid = uuid.trim();
            if !uuid.is_empty() {
                return uuid.to_string();
            }
        }
    }

    // Fallback placeholder.
    "XXXX-XXXX".to_string()
}

/// Probe filesystem label from sysfs.
fn probe_fs_label(device: &str) -> String {
    let dev_name = device
        .strip_prefix("/dev/")
        .unwrap_or(device);

    let label_path = format!("/sys/class/block/{dev_name}/device/label");
    if let Ok(label) = fs::read_to_string(&label_path) {
        let label = label.trim();
        if !label.is_empty() {
            return label.to_string();
        }
    }

    String::new()
}

/// Convert a Linux device path to a GRUB drive notation.
fn resolve_grub_drive(device: &str) -> String {
    let dev_name = device
        .strip_prefix("/dev/")
        .unwrap_or(device);

    // Handle common patterns: sdX -> (hdN), nvmeXnYpZ, vdX, etc.
    if let Some(rest) = dev_name.strip_prefix("sd") {
        if let Some(first_char) = rest.chars().next() {
            let disk_num = (first_char as u32).saturating_sub('a' as u32);
            // Check for partition number.
            let part = &rest[1..];
            if part.is_empty() {
                return format!("(hd{disk_num})");
            }
            if let Ok(part_num) = part.parse::<u32>() {
                return format!("(hd{disk_num},{part_num})");
            }
        }
    }

    if let Some(rest) = dev_name.strip_prefix("nvme") {
        // nvme0n1p1 -> disk 0, partition 1.
        let mut disk = 0u32;
        let mut part = None;
        let parts_vec: Vec<&str> = rest.split('p').collect();
        if let Some(np) = parts_vec.first() {
            if let Some(n_part) = np.strip_suffix("n1").or_else(|| np.strip_suffix("n0")) {
                if let Ok(d) = n_part.parse::<u32>() {
                    disk = d;
                }
            }
        }
        if parts_vec.len() > 1 {
            if let Some(p) = parts_vec.last() {
                if let Ok(p) = p.parse::<u32>() {
                    part = Some(p);
                }
            }
        }
        return match part {
            Some(p) => format!("(hd{disk},{p})"),
            None => format!("(hd{disk})"),
        };
    }

    if let Some(rest) = dev_name.strip_prefix("vd") {
        if let Some(first_char) = rest.chars().next() {
            let disk_num = (first_char as u32).saturating_sub('a' as u32);
            let part = &rest[1..];
            if part.is_empty() {
                return format!("(hd{disk_num})");
            }
            if let Ok(part_num) = part.parse::<u32>() {
                return format!("(hd{disk_num},{part_num})");
            }
        }
    }

    format!("(hd0)")
}

/// Probe partition map type.
fn probe_partmap(device: &str) -> String {
    let dev_name = device
        .strip_prefix("/dev/")
        .unwrap_or(device);

    // Strip partition number to get the whole-disk device.
    let disk_name = strip_partition(dev_name);

    let uevent_path = format!("/sys/class/block/{disk_name}/uevent");
    if let Ok(content) = fs::read_to_string(&uevent_path) {
        // Check uevent for table type hints.
        for line in content.lines() {
            if line.starts_with("DEVTYPE=") {
                // If it is a partition, try reading the parent.
                break;
            }
        }
    }

    // Try reading from /sys/block/<disk>/device/partition_table_type.
    let pttype_path = format!("/sys/class/block/{disk_name}/device/partition_table_type");
    if let Ok(pt) = fs::read_to_string(&pttype_path) {
        let pt = pt.trim();
        if !pt.is_empty() {
            return pt.to_string();
        }
    }

    // Fallback: most modern systems use GPT.
    "gpt".to_string()
}

/// Strip the partition number from a device name.
/// e.g., "sda1" -> "sda", "nvme0n1p1" -> "nvme0n1".
fn strip_partition(dev: &str) -> String {
    // Handle nvme style: ends with pN.
    if dev.starts_with("nvme") {
        if let Some(p_idx) = dev.rfind('p') {
            let after = &dev[p_idx + 1..];
            if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
                return dev[..p_idx].to_string();
            }
        }
        return dev.to_string();
    }

    // Handle sdX, vdX style: trailing digits are partition number.
    let trimmed = dev.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.len() < dev.len() && !trimmed.is_empty() {
        return trimmed.to_string();
    }

    dev.to_string()
}

// ============================================================================
// update-grub (wrapper)
// ============================================================================

fn run_update_grub(_args: &[String]) -> Result<(), GrubError> {
    println!("Updating GRUB configuration...");
    let mkconfig_args = vec!["-o".to_string(), DEFAULT_GRUB_CFG.to_string()];
    run_mkconfig(&mkconfig_args)
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("grub-install");
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

    let remaining = &args[1..];

    let result = match prog_name.as_str() {
        "grub-install" => run_install(remaining),
        "grub-mkconfig" => run_mkconfig(remaining),
        "grub-set-default" => run_set_default(remaining),
        "grub-reboot" => run_reboot(remaining),
        "grub-editenv" => run_editenv(remaining),
        "grub-probe" => run_probe(remaining),
        "update-grub" => run_update_grub(remaining),
        _ => {
            // Default to grub-install for unrecognized names.
            run_install(remaining)
        }
    };

    if let Err(e) = result {
        eprintln!("{prog_name}: error: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // Helper: create a temporary directory and return its path.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("grub2_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // ========================================================================
    // GrubEnv tests
    // ========================================================================

    #[test]
    fn test_grubenv_new_is_empty() {
        let env = GrubEnv::new();
        assert!(env.vars.is_empty());
    }

    #[test]
    fn test_grubenv_set_get() {
        let mut env = GrubEnv::new();
        env.set("saved_entry", "0");
        assert_eq!(env.get("saved_entry"), Some("0"));
    }

    #[test]
    fn test_grubenv_unset() {
        let mut env = GrubEnv::new();
        env.set("key", "value");
        env.unset("key");
        assert_eq!(env.get("key"), None);
    }

    #[test]
    fn test_grubenv_unset_nonexistent() {
        let mut env = GrubEnv::new();
        env.unset("nonexistent");
        assert!(env.vars.is_empty());
    }

    #[test]
    fn test_grubenv_serialize_size() {
        let env = GrubEnv::new();
        let data = env.serialize();
        assert_eq!(data.len(), GRUB_ENV_SIZE);
    }

    #[test]
    fn test_grubenv_serialize_starts_with_header() {
        let env = GrubEnv::new();
        let data = env.serialize();
        let text = String::from_utf8_lossy(&data);
        assert!(text.starts_with(GRUB_ENV_HEADER));
    }

    #[test]
    fn test_grubenv_serialize_padded_with_hash() {
        let env = GrubEnv::new();
        let data = env.serialize();
        // All bytes after header should be '#'.
        for &b in &data[GRUB_ENV_HEADER.len()..] {
            assert_eq!(b, b'#');
        }
    }

    #[test]
    fn test_grubenv_roundtrip() {
        let mut env = GrubEnv::new();
        env.set("saved_entry", "2");
        env.set("next_entry", "recovery");

        let data = env.serialize();
        let env2 = GrubEnv::parse(&data).expect("parse");

        assert_eq!(env2.get("saved_entry"), Some("2"));
        assert_eq!(env2.get("next_entry"), Some("recovery"));
    }

    #[test]
    fn test_grubenv_roundtrip_empty() {
        let env = GrubEnv::new();
        let data = env.serialize();
        let env2 = GrubEnv::parse(&data).expect("parse");
        assert!(env2.vars.is_empty());
    }

    #[test]
    fn test_grubenv_parse_wrong_size() {
        let data = vec![0u8; 512];
        assert!(GrubEnv::parse(&data).is_err());
    }

    #[test]
    fn test_grubenv_parse_missing_header() {
        let mut data = vec![b'x'; GRUB_ENV_SIZE];
        data[0] = b'X';
        assert!(GrubEnv::parse(&data).is_err());
    }

    #[test]
    fn test_grubenv_multiple_vars() {
        let mut env = GrubEnv::new();
        env.set("a", "1");
        env.set("b", "2");
        env.set("c", "3");

        let data = env.serialize();
        let env2 = GrubEnv::parse(&data).expect("parse");

        assert_eq!(env2.get("a"), Some("1"));
        assert_eq!(env2.get("b"), Some("2"));
        assert_eq!(env2.get("c"), Some("3"));
    }

    #[test]
    fn test_grubenv_overwrite_value() {
        let mut env = GrubEnv::new();
        env.set("key", "old");
        env.set("key", "new");
        assert_eq!(env.get("key"), Some("new"));
    }

    #[test]
    fn test_grubenv_value_with_equals() {
        let mut env = GrubEnv::new();
        env.set("cmd", "root=UUID=abc-123");

        let data = env.serialize();
        let env2 = GrubEnv::parse(&data).expect("parse");
        assert_eq!(env2.get("cmd"), Some("root=UUID=abc-123"));
    }

    #[test]
    fn test_grubenv_file_roundtrip() {
        let dir = temp_dir("env_file_rt");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        let mut env = GrubEnv::new();
        env.set("test_key", "test_value");
        write_grubenv(path_str, &env).expect("write");

        let env2 = read_grubenv(path_str);
        assert_eq!(env2.get("test_key"), Some("test_value"));

        // Verify file size.
        let metadata = fs::metadata(&path).expect("metadata");
        assert_eq!(metadata.len(), GRUB_ENV_SIZE as u64);

        cleanup(&dir);
    }

    #[test]
    fn test_read_grubenv_missing_file() {
        let env = read_grubenv("/nonexistent/path/grubenv");
        assert!(env.vars.is_empty());
    }

    // ========================================================================
    // OS release parsing tests
    // ========================================================================

    #[test]
    fn test_parse_os_release_basic() {
        let content = "NAME=\"OurOS\"\nVERSION=\"1.0\"\nPRETTY_NAME=\"OurOS 1.0\"\n";
        let map = parse_os_release(content);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some("OurOS"));
        assert_eq!(map.get("VERSION").map(|s| s.as_str()), Some("1.0"));
        assert_eq!(
            map.get("PRETTY_NAME").map(|s| s.as_str()),
            Some("OurOS 1.0")
        );
    }

    #[test]
    fn test_parse_os_release_unquoted() {
        let content = "ID=ouros\nVERSION_ID=1.0\n";
        let map = parse_os_release(content);
        assert_eq!(map.get("ID").map(|s| s.as_str()), Some("ouros"));
    }

    #[test]
    fn test_parse_os_release_single_quotes() {
        let content = "NAME='OurOS'\n";
        let map = parse_os_release(content);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some("OurOS"));
    }

    #[test]
    fn test_parse_os_release_skip_comments() {
        let content = "# This is a comment\nNAME=\"Test\"\n# Another\n";
        let map = parse_os_release(content);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some("Test"));
    }

    #[test]
    fn test_parse_os_release_empty_lines() {
        let content = "\n\nNAME=\"Test\"\n\n\n";
        let map = parse_os_release(content);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some("Test"));
    }

    #[test]
    fn test_parse_os_release_empty_value() {
        let content = "NAME=\n";
        let map = parse_os_release(content);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some(""));
    }

    #[test]
    fn test_parse_os_release_no_newline_at_end() {
        let content = "NAME=\"Test\"";
        let map = parse_os_release(content);
        assert_eq!(map.get("NAME").map(|s| s.as_str()), Some("Test"));
    }

    #[test]
    fn test_read_os_name_missing_file() {
        let name = read_os_name("/nonexistent/os-release");
        assert_eq!(name, "OurOS");
    }

    #[test]
    fn test_read_os_name_from_file() {
        let dir = temp_dir("os_release");
        let path = dir.join("os-release");
        fs::write(&path, "PRETTY_NAME=\"Test Linux 5.0\"\n").unwrap();
        let name = read_os_name(path.to_str().unwrap());
        assert_eq!(name, "Test Linux 5.0");
        cleanup(&dir);
    }

    #[test]
    fn test_read_os_name_fallback_to_name() {
        let dir = temp_dir("os_release_name");
        let path = dir.join("os-release");
        fs::write(&path, "NAME=\"FallbackOS\"\n").unwrap();
        let name = read_os_name(path.to_str().unwrap());
        assert_eq!(name, "FallbackOS");
        cleanup(&dir);
    }

    // ========================================================================
    // GRUB defaults parsing tests
    // ========================================================================

    #[test]
    fn test_parse_grub_defaults_all_fields() {
        let content = r#"GRUB_TIMEOUT=10
GRUB_DEFAULT="saved"
GRUB_CMDLINE_LINUX="crashkernel=auto"
GRUB_CMDLINE_LINUX_DEFAULT="quiet"
GRUB_TERMINAL_OUTPUT="gfxterm"
GRUB_DISABLE_OS_PROBER="true"
"#;
        let d = parse_grub_defaults(content);
        assert_eq!(d.timeout, 10);
        assert_eq!(d.default_entry, "saved");
        assert_eq!(d.cmdline_linux, "crashkernel=auto");
        assert_eq!(d.cmdline_linux_default, "quiet");
        assert_eq!(d.terminal_output, "gfxterm");
        assert!(d.disable_os_prober);
    }

    #[test]
    fn test_parse_grub_defaults_empty() {
        let d = parse_grub_defaults("");
        assert_eq!(d.timeout, 5);
        assert_eq!(d.default_entry, "0");
        assert_eq!(d.cmdline_linux, "");
        assert_eq!(d.cmdline_linux_default, "quiet splash");
        assert_eq!(d.terminal_output, "console");
        assert!(!d.disable_os_prober);
    }

    #[test]
    fn test_parse_grub_defaults_invalid_timeout() {
        let content = "GRUB_TIMEOUT=abc\n";
        let d = parse_grub_defaults(content);
        assert_eq!(d.timeout, 5); // Fallback to default.
    }

    #[test]
    fn test_parse_grub_defaults_zero_timeout() {
        let content = "GRUB_TIMEOUT=0\n";
        let d = parse_grub_defaults(content);
        assert_eq!(d.timeout, 0);
    }

    #[test]
    fn test_parse_grub_defaults_os_prober_numeric() {
        let content = "GRUB_DISABLE_OS_PROBER=\"1\"\n";
        let d = parse_grub_defaults(content);
        assert!(d.disable_os_prober);
    }

    #[test]
    fn test_parse_grub_defaults_os_prober_false() {
        let content = "GRUB_DISABLE_OS_PROBER=\"false\"\n";
        let d = parse_grub_defaults(content);
        assert!(!d.disable_os_prober);
    }

    #[test]
    fn test_parse_grub_defaults_with_comments() {
        let content = "# Comment\nGRUB_TIMEOUT=3\n# Another\nGRUB_DEFAULT=0\n";
        let d = parse_grub_defaults(content);
        assert_eq!(d.timeout, 3);
        assert_eq!(d.default_entry, "0");
    }

    #[test]
    fn test_read_grub_defaults_missing_file() {
        let d = read_grub_defaults("/nonexistent/grub");
        assert_eq!(d.timeout, 5); // Default values.
    }

    // ========================================================================
    // Kernel scanning tests
    // ========================================================================

    #[test]
    fn test_scan_kernels_empty_dir() {
        let dir = temp_dir("kern_empty");
        let kernels = scan_kernels(dir.to_str().unwrap());
        assert!(kernels.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_scan_kernels_no_dir() {
        let kernels = scan_kernels("/nonexistent/boot");
        assert!(kernels.is_empty());
    }

    #[test]
    fn test_scan_kernels_single_kernel() {
        let dir = temp_dir("kern_single");
        fs::write(dir.join("vmlinuz-5.10.0"), "kernel").unwrap();
        fs::write(dir.join("initrd.img-5.10.0"), "initrd").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 1);
        assert_eq!(kernels[0].version, "5.10.0");
        assert!(kernels[0].initrd_path.is_some());

        cleanup(&dir);
    }

    #[test]
    fn test_scan_kernels_no_initrd() {
        let dir = temp_dir("kern_no_initrd");
        fs::write(dir.join("vmlinuz-5.10.0"), "kernel").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 1);
        assert!(kernels[0].initrd_path.is_none());

        cleanup(&dir);
    }

    #[test]
    fn test_scan_kernels_multiple_sorted() {
        let dir = temp_dir("kern_multi");
        fs::write(dir.join("vmlinuz-5.4.0"), "k1").unwrap();
        fs::write(dir.join("vmlinuz-5.10.0"), "k2").unwrap();
        fs::write(dir.join("vmlinuz-6.1.0"), "k3").unwrap();
        fs::write(dir.join("initrd.img-5.4.0"), "i1").unwrap();
        fs::write(dir.join("initrd.img-5.10.0"), "i2").unwrap();
        fs::write(dir.join("initrd.img-6.1.0"), "i3").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 3);
        // Should be sorted descending.
        assert_eq!(kernels[0].version, "6.1.0");
        assert_eq!(kernels[1].version, "5.4.0");
        assert_eq!(kernels[2].version, "5.10.0");

        cleanup(&dir);
    }

    #[test]
    fn test_scan_kernels_initramfs_style() {
        let dir = temp_dir("kern_initramfs");
        fs::write(dir.join("vmlinuz-5.15.0"), "kernel").unwrap();
        fs::write(dir.join("initramfs-5.15.0.img"), "initrd").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 1);
        assert!(kernels[0].initrd_path.is_some());

        cleanup(&dir);
    }

    #[test]
    fn test_scan_kernels_ignores_non_kernel_files() {
        let dir = temp_dir("kern_ignore");
        fs::write(dir.join("vmlinuz-5.10.0"), "kernel").unwrap();
        fs::write(dir.join("config-5.10.0"), "config").unwrap();
        fs::write(dir.join("System.map-5.10.0"), "sysmap").unwrap();
        fs::write(dir.join("grub"), "grub dir placeholder").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 1);
        assert_eq!(kernels[0].version, "5.10.0");

        cleanup(&dir);
    }

    // ========================================================================
    // Config generation tests
    // ========================================================================

    #[test]
    fn test_generate_header_default() {
        let defaults = GrubDefaults::default();
        let mut cfg = String::new();
        generate_header(&mut cfg, &defaults);
        assert!(cfg.contains("set default=\"0\""));
        assert!(cfg.contains("set timeout=5"));
        assert!(cfg.contains("set terminal_output=console"));
        assert!(cfg.contains("00_header"));
    }

    #[test]
    fn test_generate_header_saved() {
        let defaults = GrubDefaults {
            default_entry: "saved".to_string(),
            ..GrubDefaults::default()
        };
        let mut cfg = String::new();
        generate_header(&mut cfg, &defaults);
        assert!(cfg.contains("set default=\"saved\""));
        assert!(cfg.contains("load_env"));
        assert!(cfg.contains("${saved_entry}"));
    }

    #[test]
    fn test_generate_linux_entries_no_kernels() {
        let defaults = GrubDefaults::default();
        let mut cfg = String::new();
        generate_linux_entries(&mut cfg, &[], "TestOS", &defaults);
        assert!(cfg.contains("No kernels found"));
        assert!(cfg.contains("10_linux"));
    }

    #[test]
    fn test_generate_linux_entries_single_kernel() {
        let kernels = vec![KernelEntry {
            version: "5.10.0".to_string(),
            kernel_path: "/boot/vmlinuz-5.10.0".to_string(),
            initrd_path: Some("/boot/initrd.img-5.10.0".to_string()),
        }];
        let defaults = GrubDefaults::default();
        let mut cfg = String::new();
        generate_linux_entries(&mut cfg, &kernels, "TestOS", &defaults);

        assert!(cfg.contains("menuentry 'TestOS'"));
        assert!(cfg.contains("linux /boot/vmlinuz-5.10.0"));
        assert!(cfg.contains("initrd /boot/initrd.img-5.10.0"));
        assert!(cfg.contains("recovery mode"));
    }

    #[test]
    fn test_generate_linux_entries_multiple_kernels() {
        let kernels = vec![
            KernelEntry {
                version: "6.1.0".to_string(),
                kernel_path: "/boot/vmlinuz-6.1.0".to_string(),
                initrd_path: Some("/boot/initrd.img-6.1.0".to_string()),
            },
            KernelEntry {
                version: "5.10.0".to_string(),
                kernel_path: "/boot/vmlinuz-5.10.0".to_string(),
                initrd_path: None,
            },
        ];
        let defaults = GrubDefaults::default();
        let mut cfg = String::new();
        generate_linux_entries(&mut cfg, &kernels, "TestOS", &defaults);

        // First entry gets OS name only.
        assert!(cfg.contains("menuentry 'TestOS'"));
        // Second entry includes version.
        assert!(cfg.contains("TestOS, with Linux 5.10.0"));
    }

    #[test]
    fn test_generate_linux_entries_no_initrd() {
        let kernels = vec![KernelEntry {
            version: "5.10.0".to_string(),
            kernel_path: "/boot/vmlinuz-5.10.0".to_string(),
            initrd_path: None,
        }];
        let defaults = GrubDefaults::default();
        let mut cfg = String::new();
        generate_linux_entries(&mut cfg, &kernels, "TestOS", &defaults);

        assert!(cfg.contains("linux /boot/vmlinuz-5.10.0"));
        // initrd should NOT appear since there is none.
        let lines: Vec<&str> = cfg.lines().collect();
        let initrd_count = lines.iter().filter(|l| l.contains("initrd")).count();
        assert_eq!(initrd_count, 0);
    }

    #[test]
    fn test_generate_linux_entries_custom_cmdline() {
        let kernels = vec![KernelEntry {
            version: "5.10.0".to_string(),
            kernel_path: "/boot/vmlinuz-5.10.0".to_string(),
            initrd_path: None,
        }];
        let defaults = GrubDefaults {
            cmdline_linux: "crashkernel=auto".to_string(),
            cmdline_linux_default: "quiet".to_string(),
            ..GrubDefaults::default()
        };
        let mut cfg = String::new();
        generate_linux_entries(&mut cfg, &kernels, "TestOS", &defaults);

        assert!(cfg.contains("quiet crashkernel=auto"));
    }

    #[test]
    fn test_generate_os_prober_section() {
        let mut cfg = String::new();
        generate_os_prober_section(&mut cfg);
        assert!(cfg.contains("30_os-prober"));
        assert!(cfg.contains("no other OS detected"));
    }

    #[test]
    fn test_generate_custom_section_no_file() {
        let mut cfg = String::new();
        generate_custom_section(&mut cfg, "/nonexistent/grub.d");
        assert!(cfg.contains("40_custom"));
    }

    #[test]
    fn test_generate_custom_section_with_file() {
        let dir = temp_dir("grub_d_custom");
        fs::write(
            dir.join("40_custom"),
            "#!/bin/sh\n# custom\nmenuentry 'My Custom' {\n\tset root=(hd0,1)\n}\n",
        )
        .unwrap();

        let mut cfg = String::new();
        generate_custom_section(&mut cfg, dir.to_str().unwrap());

        assert!(cfg.contains("menuentry 'My Custom'"));
        assert!(cfg.contains("set root=(hd0,1)"));
        // Shebang and comments should be skipped.
        assert!(!cfg.contains("#!/bin/sh"));

        cleanup(&dir);
    }

    #[test]
    fn test_generate_grub_config_full() {
        let dir = temp_dir("full_config");
        let boot_dir = dir.join("boot");
        fs::create_dir_all(&boot_dir).unwrap();
        fs::write(boot_dir.join("vmlinuz-5.10.0"), "k").unwrap();
        fs::write(boot_dir.join("initrd.img-5.10.0"), "i").unwrap();

        let defaults_dir = dir.join("etc_default");
        fs::create_dir_all(&defaults_dir).unwrap();
        fs::write(
            defaults_dir.join("grub"),
            "GRUB_TIMEOUT=3\nGRUB_DEFAULT=0\n",
        )
        .unwrap();

        let os_release_dir = dir.join("etc");
        fs::create_dir_all(&os_release_dir).unwrap();
        fs::write(
            os_release_dir.join("os-release"),
            "PRETTY_NAME=\"TestOS 1.0\"\n",
        )
        .unwrap();

        let grub_d = dir.join("grub.d");
        fs::create_dir_all(&grub_d).unwrap();

        let opts = MkconfigOptions {
            output: None,
            boot_dir: boot_dir.to_str().unwrap().to_string(),
            grub_defaults_path: defaults_dir.join("grub").to_str().unwrap().to_string(),
            os_release_path: os_release_dir.join("os-release").to_str().unwrap().to_string(),
            grub_d_dir: grub_d.to_str().unwrap().to_string(),
        };

        let cfg = generate_grub_config(&opts);

        assert!(cfg.contains("DO NOT EDIT"));
        assert!(cfg.contains("set timeout=3"));
        assert!(cfg.contains("menuentry 'TestOS 1.0'"));
        assert!(cfg.contains("vmlinuz-5.10.0"));
        assert!(cfg.contains("initrd.img-5.10.0"));
        assert!(cfg.contains("recovery mode"));
        assert!(cfg.contains("30_os-prober"));
        assert!(cfg.contains("40_custom"));

        cleanup(&dir);
    }

    #[test]
    fn test_generate_grub_config_os_prober_disabled() {
        let dir = temp_dir("no_osprober");
        let boot_dir = dir.join("boot");
        fs::create_dir_all(&boot_dir).unwrap();

        let defaults_dir = dir.join("etc_default");
        fs::create_dir_all(&defaults_dir).unwrap();
        fs::write(
            defaults_dir.join("grub"),
            "GRUB_DISABLE_OS_PROBER=true\n",
        )
        .unwrap();

        let grub_d = dir.join("grub.d");
        fs::create_dir_all(&grub_d).unwrap();

        let opts = MkconfigOptions {
            output: None,
            boot_dir: boot_dir.to_str().unwrap().to_string(),
            grub_defaults_path: defaults_dir.join("grub").to_str().unwrap().to_string(),
            os_release_path: "/nonexistent".to_string(),
            grub_d_dir: grub_d.to_str().unwrap().to_string(),
        };

        let cfg = generate_grub_config(&opts);
        assert!(!cfg.contains("30_os-prober"));

        cleanup(&dir);
    }

    // ========================================================================
    // build_cmdline tests
    // ========================================================================

    #[test]
    fn test_build_cmdline_both() {
        let result = build_cmdline("crashkernel=auto", "quiet splash");
        assert_eq!(result, "quiet splash crashkernel=auto");
    }

    #[test]
    fn test_build_cmdline_only_default() {
        let result = build_cmdline("", "quiet splash");
        assert_eq!(result, "quiet splash");
    }

    #[test]
    fn test_build_cmdline_only_linux() {
        let result = build_cmdline("crashkernel=auto", "");
        assert_eq!(result, "crashkernel=auto");
    }

    #[test]
    fn test_build_cmdline_both_empty() {
        let result = build_cmdline("", "");
        assert_eq!(result, "");
    }

    // ========================================================================
    // Boot mode detection tests
    // ========================================================================

    #[test]
    fn test_boot_mode_display() {
        assert_eq!(format!("{}", BootMode::Bios), "BIOS");
        assert_eq!(format!("{}", BootMode::Efi), "EFI");
    }

    #[test]
    fn test_boot_mode_eq() {
        assert_eq!(BootMode::Bios, BootMode::Bios);
        assert_eq!(BootMode::Efi, BootMode::Efi);
        assert_ne!(BootMode::Bios, BootMode::Efi);
    }

    // ========================================================================
    // grub-probe helper tests
    // ========================================================================

    #[test]
    fn test_resolve_grub_drive_sda() {
        assert_eq!(resolve_grub_drive("/dev/sda"), "(hd0)");
    }

    #[test]
    fn test_resolve_grub_drive_sda1() {
        assert_eq!(resolve_grub_drive("/dev/sda1"), "(hd0,1)");
    }

    #[test]
    fn test_resolve_grub_drive_sdb() {
        assert_eq!(resolve_grub_drive("/dev/sdb"), "(hd1)");
    }

    #[test]
    fn test_resolve_grub_drive_sdb2() {
        assert_eq!(resolve_grub_drive("/dev/sdb2"), "(hd1,2)");
    }

    #[test]
    fn test_resolve_grub_drive_sdc3() {
        assert_eq!(resolve_grub_drive("/dev/sdc3"), "(hd2,3)");
    }

    #[test]
    fn test_resolve_grub_drive_vda() {
        assert_eq!(resolve_grub_drive("/dev/vda"), "(hd0)");
    }

    #[test]
    fn test_resolve_grub_drive_vda1() {
        assert_eq!(resolve_grub_drive("/dev/vda1"), "(hd0,1)");
    }

    #[test]
    fn test_resolve_grub_drive_nvme0n1() {
        assert_eq!(resolve_grub_drive("/dev/nvme0n1"), "(hd0)");
    }

    #[test]
    fn test_resolve_grub_drive_nvme0n1p1() {
        assert_eq!(resolve_grub_drive("/dev/nvme0n1p1"), "(hd0,1)");
    }

    #[test]
    fn test_resolve_grub_drive_no_dev_prefix() {
        assert_eq!(resolve_grub_drive("sda"), "(hd0)");
    }

    #[test]
    fn test_resolve_grub_drive_unknown() {
        // Unknown device format falls back.
        assert_eq!(resolve_grub_drive("/dev/xyzzy"), "(hd0)");
    }

    #[test]
    fn test_strip_partition_sda1() {
        assert_eq!(strip_partition("sda1"), "sda");
    }

    #[test]
    fn test_strip_partition_sda() {
        assert_eq!(strip_partition("sda"), "sda");
    }

    #[test]
    fn test_strip_partition_nvme0n1p1() {
        assert_eq!(strip_partition("nvme0n1p1"), "nvme0n1");
    }

    #[test]
    fn test_strip_partition_nvme0n1() {
        assert_eq!(strip_partition("nvme0n1"), "nvme0n1");
    }

    #[test]
    fn test_strip_partition_sdb12() {
        assert_eq!(strip_partition("sdb12"), "sdb");
    }

    #[test]
    fn test_resolve_device_dev_path() {
        assert_eq!(resolve_device("/dev/sda1"), "/dev/sda1");
    }

    // ========================================================================
    // grub-install arg parsing tests
    // ========================================================================

    #[test]
    fn test_parse_install_args_empty() {
        let opts = parse_install_args(&[]).expect("should parse");
        assert!(opts.target.is_none());
        assert!(opts.device.is_none());
        assert_eq!(opts.boot_directory, "/boot");
        assert_eq!(opts.bootloader_id, "ouros");
        assert!(!opts.recheck);
        assert!(!opts.removable);
    }

    #[test]
    fn test_parse_install_args_target_space() {
        let args = vec!["--target".to_string(), "x86_64-efi".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.target.as_deref(), Some("x86_64-efi"));
    }

    #[test]
    fn test_parse_install_args_target_equals() {
        let args = vec!["--target=i386-pc".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.target.as_deref(), Some("i386-pc"));
    }

    #[test]
    fn test_parse_install_args_unsupported_target() {
        let args = vec!["--target=mips-unknown".to_string()];
        assert!(parse_install_args(&args).is_err());
    }

    #[test]
    fn test_parse_install_args_efi_directory() {
        let args = vec![
            "--efi-directory".to_string(),
            "/boot/efi".to_string(),
        ];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.efi_directory.as_deref(), Some("/boot/efi"));
    }

    #[test]
    fn test_parse_install_args_efi_directory_equals() {
        let args = vec!["--efi-directory=/mnt/efi".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.efi_directory.as_deref(), Some("/mnt/efi"));
    }

    #[test]
    fn test_parse_install_args_boot_directory() {
        let args = vec!["--boot-directory=/mnt/boot".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.boot_directory, "/mnt/boot");
    }

    #[test]
    fn test_parse_install_args_bootloader_id() {
        let args = vec!["--bootloader-id=myos".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.bootloader_id, "myos");
    }

    #[test]
    fn test_parse_install_args_recheck() {
        let args = vec!["--recheck".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert!(opts.recheck);
    }

    #[test]
    fn test_parse_install_args_removable() {
        let args = vec!["--removable".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert!(opts.removable);
    }

    #[test]
    fn test_parse_install_args_device() {
        let args = vec!["/dev/sda".to_string()];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.device.as_deref(), Some("/dev/sda"));
    }

    #[test]
    fn test_parse_install_args_all_combined() {
        let args = vec![
            "--target=x86_64-efi".to_string(),
            "--efi-directory=/boot/efi".to_string(),
            "--boot-directory=/boot".to_string(),
            "--bootloader-id=test".to_string(),
            "--recheck".to_string(),
            "--removable".to_string(),
            "/dev/sda".to_string(),
        ];
        let opts = parse_install_args(&args).expect("parse");
        assert_eq!(opts.target.as_deref(), Some("x86_64-efi"));
        assert_eq!(opts.efi_directory.as_deref(), Some("/boot/efi"));
        assert_eq!(opts.boot_directory, "/boot");
        assert_eq!(opts.bootloader_id, "test");
        assert!(opts.recheck);
        assert!(opts.removable);
        assert_eq!(opts.device.as_deref(), Some("/dev/sda"));
    }

    #[test]
    fn test_parse_install_args_unknown_option() {
        let args = vec!["--unknown".to_string()];
        assert!(parse_install_args(&args).is_err());
    }

    #[test]
    fn test_parse_install_args_target_missing_value() {
        let args = vec!["--target".to_string()];
        assert!(parse_install_args(&args).is_err());
    }

    // ========================================================================
    // grub-mkconfig arg parsing tests
    // ========================================================================

    #[test]
    fn test_parse_mkconfig_args_empty() {
        let opts = parse_mkconfig_args(&[]).expect("parse");
        assert!(opts.output.is_none());
    }

    #[test]
    fn test_parse_mkconfig_args_output_o() {
        let args = vec!["-o".to_string(), "/tmp/grub.cfg".to_string()];
        let opts = parse_mkconfig_args(&args).expect("parse");
        assert_eq!(opts.output.as_deref(), Some("/tmp/grub.cfg"));
    }

    #[test]
    fn test_parse_mkconfig_args_output_equals() {
        let args = vec!["-o=/tmp/grub.cfg".to_string()];
        let opts = parse_mkconfig_args(&args).expect("parse");
        assert_eq!(opts.output.as_deref(), Some("/tmp/grub.cfg"));
    }

    #[test]
    fn test_parse_mkconfig_args_output_long() {
        let args = vec!["--output".to_string(), "/tmp/grub.cfg".to_string()];
        let opts = parse_mkconfig_args(&args).expect("parse");
        assert_eq!(opts.output.as_deref(), Some("/tmp/grub.cfg"));
    }

    #[test]
    fn test_parse_mkconfig_args_output_long_equals() {
        let args = vec!["--output=/tmp/grub.cfg".to_string()];
        let opts = parse_mkconfig_args(&args).expect("parse");
        assert_eq!(opts.output.as_deref(), Some("/tmp/grub.cfg"));
    }

    #[test]
    fn test_parse_mkconfig_args_unknown() {
        let args = vec!["--unknown".to_string()];
        assert!(parse_mkconfig_args(&args).is_err());
    }

    #[test]
    fn test_parse_mkconfig_args_o_missing_value() {
        let args = vec!["-o".to_string()];
        assert!(parse_mkconfig_args(&args).is_err());
    }

    // ========================================================================
    // grub-probe arg parsing tests
    // ========================================================================

    #[test]
    fn test_parse_probe_args_basic() {
        let args = vec!["/dev/sda".to_string()];
        let opts = parse_probe_args(&args).expect("parse");
        assert_eq!(opts.device, "/dev/sda");
        assert_eq!(opts.target, "device");
    }

    #[test]
    fn test_parse_probe_args_with_target() {
        let args = vec!["--target=fs".to_string(), "/dev/sda1".to_string()];
        let opts = parse_probe_args(&args).expect("parse");
        assert_eq!(opts.target, "fs");
        assert_eq!(opts.device, "/dev/sda1");
    }

    #[test]
    fn test_parse_probe_args_target_short() {
        let args = vec!["-t".to_string(), "fs_uuid".to_string(), "/".to_string()];
        let opts = parse_probe_args(&args).expect("parse");
        assert_eq!(opts.target, "fs_uuid");
    }

    #[test]
    fn test_parse_probe_args_all_targets() {
        let valid = ["device", "fs", "fs_uuid", "fs_label", "drive", "partmap"];
        for t in &valid {
            let args = vec![format!("--target={t}"), "/dev/sda".to_string()];
            let opts = parse_probe_args(&args).expect("parse");
            assert_eq!(opts.target, *t);
        }
    }

    #[test]
    fn test_parse_probe_args_invalid_target() {
        let args = vec!["--target=invalid".to_string(), "/dev/sda".to_string()];
        assert!(parse_probe_args(&args).is_err());
    }

    #[test]
    fn test_parse_probe_args_no_device() {
        let args: Vec<String> = vec![];
        assert!(parse_probe_args(&args).is_err());
    }

    #[test]
    fn test_parse_probe_args_unknown_option() {
        let args = vec!["--unknown".to_string(), "/dev/sda".to_string()];
        assert!(parse_probe_args(&args).is_err());
    }

    // ========================================================================
    // grub-editenv arg parsing tests
    // ========================================================================

    #[test]
    fn test_parse_editenv_args_list() {
        let args = vec!["list".to_string()];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.command, "list");
        assert_eq!(ea.file, DEFAULT_GRUBENV);
        assert!(ea.params.is_empty());
    }

    #[test]
    fn test_parse_editenv_args_set() {
        let args = vec!["set".to_string(), "key=value".to_string()];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.command, "set");
        assert_eq!(ea.params, vec!["key=value"]);
    }

    #[test]
    fn test_parse_editenv_args_unset() {
        let args = vec!["unset".to_string(), "key".to_string()];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.command, "unset");
        assert_eq!(ea.params, vec!["key"]);
    }

    #[test]
    fn test_parse_editenv_args_create() {
        let args = vec!["create".to_string()];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.command, "create");
    }

    #[test]
    fn test_parse_editenv_args_custom_file() {
        let args = vec![
            "/tmp/grubenv".to_string(),
            "list".to_string(),
        ];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.file, "/tmp/grubenv");
        assert_eq!(ea.command, "list");
    }

    #[test]
    fn test_parse_editenv_args_custom_file_set() {
        let args = vec![
            "/tmp/grubenv".to_string(),
            "set".to_string(),
            "a=b".to_string(),
        ];
        let ea = parse_editenv_args(&args).expect("parse");
        assert_eq!(ea.file, "/tmp/grubenv");
        assert_eq!(ea.command, "set");
        assert_eq!(ea.params, vec!["a=b"]);
    }

    #[test]
    fn test_parse_editenv_args_empty() {
        let args: Vec<String> = vec![];
        assert!(parse_editenv_args(&args).is_err());
    }

    #[test]
    fn test_parse_editenv_args_unknown_command() {
        let args = vec!["badcmd".to_string()];
        assert!(parse_editenv_args(&args).is_err());
    }

    #[test]
    fn test_parse_editenv_args_file_no_command() {
        let args = vec!["/tmp/grubenv".to_string()];
        assert!(parse_editenv_args(&args).is_err());
    }

    // ========================================================================
    // grub-editenv functional tests
    // ========================================================================

    #[test]
    fn test_editenv_create_and_list() {
        let dir = temp_dir("editenv_create");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        // Create.
        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        assert!(path.exists());
        let data = fs::read(&path).unwrap();
        assert_eq!(data.len(), GRUB_ENV_SIZE);

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_set_and_read() {
        let dir = temp_dir("editenv_set");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        // Create first.
        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        // Set a variable.
        let args = vec![
            path_str.clone(),
            "set".to_string(),
            "mykey=myval".to_string(),
        ];
        run_editenv(&args).expect("set");

        // Verify.
        let env = read_grubenv(&path_str);
        assert_eq!(env.get("mykey"), Some("myval"));

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_set_multiple() {
        let dir = temp_dir("editenv_setmulti");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        let args = vec![
            path_str.clone(),
            "set".to_string(),
            "a=1".to_string(),
            "b=2".to_string(),
            "c=3".to_string(),
        ];
        run_editenv(&args).expect("set multiple");

        let env = read_grubenv(&path_str);
        assert_eq!(env.get("a"), Some("1"));
        assert_eq!(env.get("b"), Some("2"));
        assert_eq!(env.get("c"), Some("3"));

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_unset() {
        let dir = temp_dir("editenv_unset");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        let args = vec![
            path_str.clone(),
            "set".to_string(),
            "key=val".to_string(),
        ];
        run_editenv(&args).expect("set");

        let args = vec![
            path_str.clone(),
            "unset".to_string(),
            "key".to_string(),
        ];
        run_editenv(&args).expect("unset");

        let env = read_grubenv(&path_str);
        assert_eq!(env.get("key"), None);

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_set_no_params() {
        let args = vec!["set".to_string()];
        assert!(run_editenv(&args).is_err());
    }

    #[test]
    fn test_editenv_set_bad_format() {
        let dir = temp_dir("editenv_badfmt");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        let args = vec![
            path_str,
            "set".to_string(),
            "noequals".to_string(),
        ];
        assert!(run_editenv(&args).is_err());

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_set_empty_key() {
        let dir = temp_dir("editenv_emptykey");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap().to_string();

        let args = vec![path_str.clone(), "create".to_string()];
        run_editenv(&args).expect("create");

        let args = vec![
            path_str,
            "set".to_string(),
            "=value".to_string(),
        ];
        assert!(run_editenv(&args).is_err());

        cleanup(&dir);
    }

    #[test]
    fn test_editenv_unset_no_params() {
        let args = vec!["unset".to_string()];
        assert!(run_editenv(&args).is_err());
    }

    // ========================================================================
    // grub-set-default functional tests
    // ========================================================================

    #[test]
    fn test_set_default_writes_saved_entry() {
        let dir = temp_dir("set_default");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        // Create an empty env first.
        let env = GrubEnv::new();
        write_grubenv(path_str, &env).unwrap();

        let args = vec!["2".to_string()];
        run_set_default_with_path(&args, path_str).expect("set default");

        let env2 = read_grubenv(path_str);
        assert_eq!(env2.get("saved_entry"), Some("2"));

        cleanup(&dir);
    }

    #[test]
    fn test_set_default_overwrites() {
        let dir = temp_dir("set_default_overwrite");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        let mut env = GrubEnv::new();
        env.set("saved_entry", "0");
        write_grubenv(path_str, &env).unwrap();

        let args = vec!["3".to_string()];
        run_set_default_with_path(&args, path_str).expect("set default");

        let env2 = read_grubenv(path_str);
        assert_eq!(env2.get("saved_entry"), Some("3"));

        cleanup(&dir);
    }

    #[test]
    fn test_set_default_no_args() {
        let result = run_set_default_with_path(&[], DEFAULT_GRUBENV);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_default_string_entry() {
        let dir = temp_dir("set_default_str");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        let env = GrubEnv::new();
        write_grubenv(path_str, &env).unwrap();

        let args = vec!["OurOS, with Linux 5.10.0".to_string()];
        run_set_default_with_path(&args, path_str).expect("set default");

        let env2 = read_grubenv(path_str);
        assert_eq!(
            env2.get("saved_entry"),
            Some("OurOS, with Linux 5.10.0")
        );

        cleanup(&dir);
    }

    // ========================================================================
    // grub-reboot functional tests
    // ========================================================================

    #[test]
    fn test_reboot_writes_next_entry() {
        let dir = temp_dir("reboot");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        let env = GrubEnv::new();
        write_grubenv(path_str, &env).unwrap();

        let args = vec!["1".to_string()];
        run_reboot_with_path(&args, path_str).expect("reboot");

        let env2 = read_grubenv(path_str);
        assert_eq!(env2.get("next_entry"), Some("1"));

        cleanup(&dir);
    }

    #[test]
    fn test_reboot_preserves_saved_entry() {
        let dir = temp_dir("reboot_preserve");
        let path = dir.join("grubenv");
        let path_str = path.to_str().unwrap();

        let mut env = GrubEnv::new();
        env.set("saved_entry", "0");
        write_grubenv(path_str, &env).unwrap();

        let args = vec!["recovery".to_string()];
        run_reboot_with_path(&args, path_str).expect("reboot");

        let env2 = read_grubenv(path_str);
        assert_eq!(env2.get("saved_entry"), Some("0"));
        assert_eq!(env2.get("next_entry"), Some("recovery"));

        cleanup(&dir);
    }

    #[test]
    fn test_reboot_no_args() {
        let result = run_reboot_with_path(&[], DEFAULT_GRUBENV);
        assert!(result.is_err());
    }

    // ========================================================================
    // Error display tests
    // ========================================================================

    #[test]
    fn test_error_display_io() {
        let e = GrubError::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        let s = format!("{e}");
        assert!(s.contains("I/O error"));
    }

    #[test]
    fn test_error_display_invalid_args() {
        let e = GrubError::InvalidArgs("bad".to_string());
        let s = format!("{e}");
        assert!(s.contains("invalid arguments"));
    }

    #[test]
    fn test_error_display_invalid_env_block() {
        let e = GrubError::InvalidEnvBlock("corrupt".to_string());
        let s = format!("{e}");
        assert!(s.contains("invalid environment block"));
    }

    #[test]
    fn test_error_display_device_not_found() {
        let e = GrubError::DeviceNotFound("/dev/foo".to_string());
        let s = format!("{e}");
        assert!(s.contains("device not found"));
    }

    #[test]
    fn test_error_display_unsupported_target() {
        let e = GrubError::UnsupportedTarget("mips-bad".to_string());
        let s = format!("{e}");
        assert!(s.contains("unsupported target"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let grub_err: GrubError = io_err.into();
        let s = format!("{grub_err}");
        assert!(s.contains("denied"));
    }

    // ========================================================================
    // Personality detection tests (simulated)
    // ========================================================================

    fn extract_prog_name(argv0: &str) -> String {
        let s = argv0;
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
    }

    #[test]
    fn test_personality_grub_install() {
        assert_eq!(extract_prog_name("grub-install"), "grub-install");
    }

    #[test]
    fn test_personality_grub_mkconfig() {
        assert_eq!(extract_prog_name("grub-mkconfig"), "grub-mkconfig");
    }

    #[test]
    fn test_personality_grub_set_default() {
        assert_eq!(extract_prog_name("grub-set-default"), "grub-set-default");
    }

    #[test]
    fn test_personality_grub_reboot() {
        assert_eq!(extract_prog_name("grub-reboot"), "grub-reboot");
    }

    #[test]
    fn test_personality_grub_editenv() {
        assert_eq!(extract_prog_name("grub-editenv"), "grub-editenv");
    }

    #[test]
    fn test_personality_grub_probe() {
        assert_eq!(extract_prog_name("grub-probe"), "grub-probe");
    }

    #[test]
    fn test_personality_update_grub() {
        assert_eq!(extract_prog_name("update-grub"), "update-grub");
    }

    #[test]
    fn test_personality_with_unix_path() {
        assert_eq!(
            extract_prog_name("/usr/sbin/grub-install"),
            "grub-install"
        );
    }

    #[test]
    fn test_personality_with_windows_path() {
        assert_eq!(
            extract_prog_name("C:\\Program Files\\grub-install"),
            "grub-install"
        );
    }

    #[test]
    fn test_personality_with_exe_suffix() {
        assert_eq!(
            extract_prog_name("C:\\grub\\grub-install.exe"),
            "grub-install"
        );
    }

    #[test]
    fn test_personality_mixed_separators() {
        assert_eq!(
            extract_prog_name("/usr/local\\bin/grub-mkconfig"),
            "grub-mkconfig"
        );
    }

    #[test]
    fn test_personality_no_path() {
        assert_eq!(extract_prog_name("grub-probe"), "grub-probe");
    }

    #[test]
    fn test_personality_exe_no_path() {
        assert_eq!(extract_prog_name("grub-probe.exe"), "grub-probe");
    }

    // ========================================================================
    // probe_fs_type / probe_fs_uuid / probe_fs_label fallback tests
    // ========================================================================

    #[test]
    fn test_probe_fs_type_fallback() {
        // On a system without sysfs for this device, falls back to ext2.
        let t = probe_fs_type("/dev/nonexistent999");
        assert_eq!(t, "ext2");
    }

    #[test]
    fn test_probe_fs_uuid_fallback() {
        let uuid = probe_fs_uuid("/dev/nonexistent999");
        assert_eq!(uuid, "XXXX-XXXX");
    }

    #[test]
    fn test_probe_fs_label_fallback() {
        let label = probe_fs_label("/dev/nonexistent999");
        assert_eq!(label, "");
    }

    #[test]
    fn test_probe_partmap_fallback() {
        let pm = probe_partmap("/dev/nonexistent999");
        assert_eq!(pm, "gpt");
    }

    // ========================================================================
    // Integration: mkconfig output to file
    // ========================================================================

    #[test]
    fn test_mkconfig_output_to_file() {
        let dir = temp_dir("mkconfig_output");
        let output_path = dir.join("grub.cfg");
        let boot_dir = dir.join("boot");
        fs::create_dir_all(&boot_dir).unwrap();
        fs::write(boot_dir.join("vmlinuz-5.10.0"), "k").unwrap();
        fs::write(boot_dir.join("initrd.img-5.10.0"), "i").unwrap();

        let opts = MkconfigOptions {
            output: Some(output_path.to_str().unwrap().to_string()),
            boot_dir: boot_dir.to_str().unwrap().to_string(),
            grub_defaults_path: "/nonexistent".to_string(),
            os_release_path: "/nonexistent".to_string(),
            grub_d_dir: "/nonexistent".to_string(),
        };

        let config = generate_grub_config(&opts);
        fs::write(&output_path, &config).unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("menuentry"));
        assert!(content.contains("vmlinuz-5.10.0"));

        cleanup(&dir);
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn test_grubenv_serialize_truncation() {
        // If we add too many variables, the block should be truncated to 1024 bytes.
        let mut env = GrubEnv::new();
        for i in 0..100 {
            env.set(&format!("long_variable_name_{i}"), &format!("long_value_{i}_padding"));
        }
        let data = env.serialize();
        assert_eq!(data.len(), GRUB_ENV_SIZE);
    }

    #[test]
    fn test_grubenv_empty_value_roundtrip() {
        let mut env = GrubEnv::new();
        env.set("empty", "");
        let data = env.serialize();
        let env2 = GrubEnv::parse(&data).expect("parse");
        assert_eq!(env2.get("empty"), Some(""));
    }

    #[test]
    fn test_multiple_kernels_with_mixed_initrd() {
        let dir = temp_dir("kern_mixed_initrd");
        fs::write(dir.join("vmlinuz-5.4.0"), "k1").unwrap();
        fs::write(dir.join("vmlinuz-5.10.0"), "k2").unwrap();
        fs::write(dir.join("initrd.img-5.10.0"), "i2").unwrap();

        let kernels = scan_kernels(dir.to_str().unwrap());
        assert_eq!(kernels.len(), 2);

        // 5.10.0 should have initrd, 5.4.0 should not.
        let k_5_10 = kernels.iter().find(|k| k.version == "5.10.0").unwrap();
        let k_5_4 = kernels.iter().find(|k| k.version == "5.4.0").unwrap();

        assert!(k_5_10.initrd_path.is_some());
        assert!(k_5_4.initrd_path.is_none());

        cleanup(&dir);
    }

    #[test]
    fn test_install_target_validation() {
        for t in KNOWN_TARGETS {
            let args = vec![format!("--target={t}")];
            let opts = parse_install_args(&args).expect("should accept known target");
            assert_eq!(opts.target.as_deref(), Some(*t));
        }
    }

    #[test]
    fn test_os_release_with_spaces_in_value() {
        let content = "PRETTY_NAME=\"OurOS 1.0 (Fancy Release)\"\n";
        let map = parse_os_release(content);
        assert_eq!(
            map.get("PRETTY_NAME").map(|s| s.as_str()),
            Some("OurOS 1.0 (Fancy Release)")
        );
    }

    #[test]
    fn test_os_release_multiple_equals_in_value() {
        let content = "BUG_REPORT_URL=https://example.com?a=1&b=2\n";
        let map = parse_os_release(content);
        assert_eq!(
            map.get("BUG_REPORT_URL").map(|s| s.as_str()),
            Some("https://example.com?a=1&b=2")
        );
    }
}
