//! Slate OS initial RAM filesystem builder.
//!
//! Multi-personality binary providing:
//! - **mkinitramfs** — create an initramfs image for boot
//! - **update-initramfs** — manage initramfs images (create/update/delete)
//! - **lsinitramfs** — list contents of an initramfs image
//!
//! Builds a compressed cpio archive containing the minimal root filesystem
//! needed to mount the real root and hand off to init.

#![deny(clippy::all)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration
// ============================================================================

#[derive(Clone, Debug)]
struct InitramfsConfig {
    /// Output file path.
    output: PathBuf,
    /// Kernel version (e.g. "6.8.0-generic").
    kernel_version: String,
    /// Modules to include.
    modules: Vec<String>,
    /// Extra files/directories to include.
    extra_files: Vec<PathBuf>,
    /// Hooks/scripts to run. Parsed from /etc/mkinitramfs.conf; the
    /// runner that consumes them is part of the still-pending boot
    /// hook stage.
    #[allow(dead_code)]
    hooks: Vec<String>,
    /// Compression method.
    compression: Compression,
    /// Module directory base.
    modules_dir: PathBuf,
    /// Config directory.
    config_dir: PathBuf,
    /// Whether to include all firmware.
    include_firmware: bool,
    /// Verbose output.
    verbose: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Compression {
    Gzip,
    Xz,
    Lz4,
    Zstd,
    None,
}

impl Compression {
    // Consumed by the future output-naming pass that suffixes the
    // initramfs file with the compression extension.
    #[allow(dead_code)]
    fn extension(&self) -> &str {
        match self {
            Compression::Gzip => ".gz",
            Compression::Xz => ".xz",
            Compression::Lz4 => ".lz4",
            Compression::Zstd => ".zst",
            Compression::None => "",
        }
    }

    fn from_str(s: &str) -> Option<Compression> {
        match s {
            "gzip" | "gz" => Some(Compression::Gzip),
            "xz" => Some(Compression::Xz),
            "lz4" => Some(Compression::Lz4),
            "zstd" | "zst" => Some(Compression::Zstd),
            "none" | "cat" => Some(Compression::None),
            _ => None,
        }
    }
}

impl Default for InitramfsConfig {
    fn default() -> Self {
        Self {
            output: PathBuf::from("/boot/initramfs.img"),
            kernel_version: detect_kernel_version(),
            modules: Vec::new(),
            extra_files: Vec::new(),
            hooks: vec![
                "base".to_string(),
                "udev".to_string(),
                "modconf".to_string(),
                "block".to_string(),
                "filesystems".to_string(),
                "fsck".to_string(),
            ],
            compression: Compression::Gzip,
            modules_dir: PathBuf::from("/lib/modules"),
            config_dir: PathBuf::from("/etc/initramfs-tools"),
            include_firmware: true,
            verbose: false,
        }
    }
}

// ============================================================================
// Kernel version detection
// ============================================================================

fn detect_kernel_version() -> String {
    // Try /proc/version first.
    if let Ok(ver) = fs::read_to_string("/proc/version") {
        let parts: Vec<&str> = ver.split_whitespace().collect();
        if parts.len() >= 3 {
            return parts[2].to_string();
        }
    }
    // Try uname -r equivalent.
    "0.1.0-slateos".to_string()
}

fn find_installed_kernels() -> Vec<String> {
    let mut kernels = Vec::new();
    if let Ok(entries) = fs::read_dir("/lib/modules") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                kernels.push(name.to_string());
            }
        }
    }
    // Fallback if /lib/modules doesn't exist.
    if kernels.is_empty() {
        kernels.push(detect_kernel_version());
    }
    kernels.sort();
    kernels
}

// ============================================================================
// Config file parsing
// ============================================================================

fn read_config(config_dir: &Path) -> InitramfsConfig {
    let mut config = InitramfsConfig::default();

    // Read /etc/initramfs-tools/initramfs.conf
    let conf_path = config_dir.join("initramfs.conf");
    if let Ok(content) = fs::read_to_string(&conf_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "COMPRESS" => {
                        if let Some(c) = Compression::from_str(value) {
                            config.compression = c;
                        }
                    }
                    "MODULES" => match value {
                        "most" => config.include_firmware = true,
                        "dep" => config.include_firmware = false,
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    // Read modules list.
    let modules_path = config_dir.join("modules");
    if let Ok(content) = fs::read_to_string(&modules_path) {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                config.modules.push(line.to_string());
            }
        }
    }

    config
}

// ============================================================================
// CPIO archive building (newc format)
// ============================================================================

/// CPIO newc header (110 bytes ASCII).
struct CpioEntry {
    name: String,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    mtime: u32,
    data: Vec<u8>,
    dev_major: u32,
    dev_minor: u32,
    rdev_major: u32,
    rdev_minor: u32,
}

impl CpioEntry {
    fn directory(name: &str, mode: u32) -> Self {
        Self {
            name: name.to_string(),
            mode: 0o040000 | mode,
            uid: 0,
            gid: 0,
            nlink: 2,
            mtime: 0,
            data: Vec::new(),
            dev_major: 0,
            dev_minor: 0,
            rdev_major: 0,
            rdev_minor: 0,
        }
    }

    fn file(name: &str, mode: u32, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            mode: 0o100000 | mode,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            data,
            dev_major: 0,
            dev_minor: 0,
            rdev_major: 0,
            rdev_minor: 0,
        }
    }

    fn symlink(name: &str, target: &str) -> Self {
        Self {
            name: name.to_string(),
            mode: 0o120000 | 0o777,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            data: target.as_bytes().to_vec(),
            dev_major: 0,
            dev_minor: 0,
            rdev_major: 0,
            rdev_minor: 0,
        }
    }

    fn _char_device(name: &str, mode: u32, major: u32, minor: u32) -> Self {
        Self {
            name: name.to_string(),
            mode: 0o020000 | mode,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            data: Vec::new(),
            dev_major: 0,
            dev_minor: 0,
            rdev_major: major,
            rdev_minor: minor,
        }
    }

    fn trailer() -> Self {
        Self {
            name: "TRAILER!!!".to_string(),
            mode: 0,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            data: Vec::new(),
            dev_major: 0,
            dev_minor: 0,
            rdev_major: 0,
            rdev_minor: 0,
        }
    }

    /// Serialize to newc format.
    fn serialize(&self, ino: u32) -> Vec<u8> {
        let namesize = self.name.len() + 1; // Include null terminator.
        let filesize = self.data.len();
        let header = format!(
            "070701\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}\
             {:08X}",
            ino,
            self.mode,
            self.uid,
            self.gid,
            self.nlink,
            self.mtime,
            filesize,
            self.dev_major,
            self.dev_minor,
            self.rdev_major,
            self.rdev_minor,
            namesize,
            0u32, // checksum (unused in newc)
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(self.name.as_bytes());
        buf.push(0); // Null terminator.

        // Pad to 4-byte boundary after header + name.
        let header_name_len = 110 + namesize;
        let pad = (4 - (header_name_len % 4)) % 4;
        buf.extend(std::iter::repeat_n(0u8, pad));

        // File data.
        buf.extend_from_slice(&self.data);

        // Pad data to 4-byte boundary.
        let data_pad = (4 - (filesize % 4)) % 4;
        buf.extend(std::iter::repeat_n(0u8, data_pad));

        buf
    }
}

fn build_cpio(entries: &[CpioEntry]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        buf.extend(entry.serialize((i + 1) as u32));
    }
    // Add trailer.
    let trailer = CpioEntry::trailer();
    buf.extend(trailer.serialize(0));

    // Pad to 512-byte boundary (block size).
    let block_pad = (512 - (buf.len() % 512)) % 512;
    buf.extend(std::iter::repeat_n(0u8, block_pad));

    buf
}

// ============================================================================
// initramfs content generation
// ============================================================================

fn generate_init_script(config: &InitramfsConfig) -> String {
    let mut script = String::new();
    script.push_str("#!/bin/sh\n");
    script.push_str("# Slate OS initramfs init script\n");
    script.push_str("# Generated by mkinitramfs\n\n");

    script.push_str("export PATH=/sbin:/bin:/usr/sbin:/usr/bin\n\n");

    // Mount virtual filesystems.
    script.push_str("mount -t proc proc /proc\n");
    script.push_str("mount -t sysfs sysfs /sys\n");
    script.push_str("mount -t devtmpfs devtmpfs /dev\n");
    script.push_str("mkdir -p /dev/pts\n");
    script.push_str("mount -t devpts devpts /dev/pts\n\n");

    // Load modules.
    for module in &config.modules {
        script.push_str(&format!("modprobe {module}\n"));
    }
    if !config.modules.is_empty() {
        script.push('\n');
    }

    // Parse kernel command line.
    script.push_str("# Parse kernel command line\n");
    script.push_str("ROOT=\"\"\n");
    script.push_str("ROOTFSTYPE=\"\"\n");
    script.push_str("ROOTFLAGS=\"\"\n");
    script.push_str("for x in $(cat /proc/cmdline); do\n");
    script.push_str("    case $x in\n");
    script.push_str("        root=*) ROOT=${x#root=} ;;\n");
    script.push_str("        rootfstype=*) ROOTFSTYPE=${x#rootfstype=} ;;\n");
    script.push_str("        rootflags=*) ROOTFLAGS=${x#rootflags=} ;;\n");
    script.push_str("    esac\n");
    script.push_str("done\n\n");

    // Mount root.
    script.push_str("# Mount root filesystem\n");
    script.push_str("mkdir -p /newroot\n");
    script.push_str("if [ -n \"$ROOTFSTYPE\" ]; then\n");
    script.push_str("    mount -t $ROOTFSTYPE ${ROOTFLAGS:+-o $ROOTFLAGS} $ROOT /newroot\n");
    script.push_str("else\n");
    script.push_str("    mount ${ROOTFLAGS:+-o $ROOTFLAGS} $ROOT /newroot\n");
    script.push_str("fi\n\n");

    // Switch root.
    script.push_str("# Switch to real root\n");
    script.push_str("umount /proc /sys /dev/pts /dev 2>/dev/null\n");
    script.push_str("exec switch_root /newroot /sbin/init\n");
    script.push_str("echo \"Failed to switch_root, dropping to shell\"\n");
    script.push_str("exec /bin/sh\n");

    script
}

fn build_initramfs_entries(config: &InitramfsConfig) -> Vec<CpioEntry> {
    let mut entries = Vec::new();

    // Create directory structure.
    let dirs = [
        ".",
        "bin",
        "sbin",
        "etc",
        "lib",
        "lib/modules",
        "lib/firmware",
        "dev",
        "proc",
        "sys",
        "run",
        "tmp",
        "newroot",
        "usr",
        "usr/bin",
        "usr/sbin",
        "usr/lib",
        "var",
        "var/run",
    ];
    for dir in &dirs {
        entries.push(CpioEntry::directory(dir, 0o755));
    }

    // Create /init script.
    let init_script = generate_init_script(config);
    entries.push(CpioEntry::file("init", 0o755, init_script.into_bytes()));

    // Add symlinks.
    entries.push(CpioEntry::symlink("linuxrc", "init"));

    // Gather kernel modules.
    let mod_dir = config.modules_dir.join(&config.kernel_version);
    if mod_dir.is_dir() {
        let module_files = find_modules(&mod_dir, &config.modules);
        for (rel_path, data) in &module_files {
            let cpio_path = format!("lib/modules/{}/{rel_path}", config.kernel_version);
            entries.push(CpioEntry::file(&cpio_path, 0o644, data.clone()));
        }
    }

    // Add extra files.
    for file in &config.extra_files {
        if let Ok(data) = fs::read(file) {
            let name = file.to_string_lossy();
            let name = name.trim_start_matches('/');
            entries.push(CpioEntry::file(name, 0o644, data));
        }
    }

    // Add /etc/fstab stub.
    entries.push(CpioEntry::file(
        "etc/fstab",
        0o644,
        b"# initramfs fstab\n".to_vec(),
    ));

    entries
}

fn find_modules(mod_dir: &Path, requested: &[String]) -> Vec<(String, Vec<u8>)> {
    let mut results = Vec::new();
    let mut seen = BTreeSet::new();

    // If specific modules requested, find them.
    if !requested.is_empty() {
        for modname in requested {
            let ko_name = if modname.ends_with(".ko")
                || modname.ends_with(".ko.zst")
                || modname.ends_with(".ko.xz")
                || modname.ends_with(".ko.gz")
            {
                modname.clone()
            } else {
                format!("{modname}.ko")
            };
            if let Some((path, data)) = find_module_file(mod_dir, &ko_name)
                && seen.insert(path.clone())
            {
                results.push((path, data));
            }
        }
    } else {
        // Include essential modules by default.
        let essential = [
            "ext4",
            "vfat",
            "fat",
            "nls_cp437",
            "nls_utf8",
            "ahci",
            "sd_mod",
            "sr_mod",
            "usb_storage",
            "ehci_hcd",
            "ohci_hcd",
            "uhci_hcd",
            "xhci_hcd",
            "virtio_blk",
            "virtio_pci",
            "virtio_scsi",
        ];
        for name in &essential {
            let ko_name = format!("{name}.ko");
            if let Some((path, data)) = find_module_file(mod_dir, &ko_name)
                && seen.insert(path.clone())
            {
                results.push((path, data));
            }
        }
    }

    results
}

fn find_module_file(dir: &Path, name: &str) -> Option<(String, Vec<u8>)> {
    _scan_dir_for_module(dir, dir, name)
}

fn _scan_dir_for_module(base: &Path, dir: &Path, name: &str) -> Option<(String, Vec<u8>)> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(result) = _scan_dir_for_module(base, &path, name) {
                return Some(result);
            }
        } else if let Some(fname) = path.file_name().and_then(|n| n.to_str())
            && (fname == name || fname.starts_with(name))
        {
            let rel = path.strip_prefix(base).ok()?;
            let data = fs::read(&path).ok()?;
            return Some((rel.to_string_lossy().to_string(), data));
        }
    }
    None
}

// ============================================================================
// Compression stub
// ============================================================================

fn compress_data(data: &[u8], compression: &Compression) -> Vec<u8> {
    // In a real system, invoke gzip/xz/lz4/zstd.
    // For now, return uncompressed with a header marker.
    match compression {
        Compression::None => data.to_vec(),
        _ => {
            // Placeholder: real implementation would call compression library.
            // Return data as-is since we don't have external deps.
            data.to_vec()
        }
    }
}

// ============================================================================
// lsinitramfs: list contents
// ============================================================================

fn list_cpio_contents(data: &[u8]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut offset = 0;

    while offset + 110 <= data.len() {
        // Check magic.
        if &data[offset..offset + 6] != b"070701" {
            break;
        }

        // Parse header fields.
        let namesize = usize::from_str_radix(
            std::str::from_utf8(&data[offset + 94..offset + 102]).unwrap_or("0"),
            16,
        )
        .unwrap_or(0);
        let filesize = usize::from_str_radix(
            std::str::from_utf8(&data[offset + 54..offset + 62]).unwrap_or("0"),
            16,
        )
        .unwrap_or(0);

        let name_start = offset + 110;
        let name_end = name_start + namesize.saturating_sub(1); // Exclude null.
        if name_end > data.len() {
            break;
        }
        let name = String::from_utf8_lossy(&data[name_start..name_end]).to_string();

        if name == "TRAILER!!!" {
            break;
        }

        entries.push(name);

        // Advance past header + name (padded to 4).
        let header_name_total = 110 + namesize;
        let header_pad = (4 - (header_name_total % 4)) % 4;
        let data_start = name_start + namesize + header_pad;

        // Advance past data (padded to 4).
        let data_pad = (4 - (filesize % 4)) % 4;
        offset = data_start + filesize + data_pad;
    }

    entries
}

fn lsinitramfs_main(args: &[String]) -> i32 {
    let mut verbose = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-l" | "--long" => verbose = true,
            "--help" | "-h" => {
                println!("Usage: lsinitramfs [-l] <initramfs-file> ...");
                println!();
                println!("List contents of an initramfs image.");
                println!();
                println!("Options:");
                println!("  -l, --long    Long listing format");
                println!("  -h, --help    Display this help");
                println!("  --version     Display version");
                return 0;
            }
            "--version" => {
                println!("lsinitramfs (Slate OS) {VERSION}");
                return 0;
            }
            s => files.push(s.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!("lsinitramfs: no initramfs file specified");
        return 1;
    }

    for file in &files {
        if files.len() > 1 {
            println!("==> {file} <==");
        }
        match fs::read(file) {
            Ok(data) => {
                let entries = list_cpio_contents(&data);
                for entry in &entries {
                    if verbose {
                        println!("  {entry}");
                    } else {
                        println!("{entry}");
                    }
                }
            }
            Err(e) => {
                eprintln!("lsinitramfs: cannot read '{file}': {e}");
                return 1;
            }
        }
    }

    0
}

// ============================================================================
// mkinitramfs personality
// ============================================================================

fn mkinitramfs_main(args: &[String]) -> i32 {
    let mut config = InitramfsConfig::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    config.output = PathBuf::from(&args[i]);
                }
            }
            "-k" | "--kernel" => {
                i += 1;
                if i < args.len() {
                    config.kernel_version = args[i].clone();
                }
            }
            "-m" | "--module" => {
                i += 1;
                if i < args.len() {
                    config.modules.push(args[i].clone());
                }
            }
            "-c" | "--compress" => {
                i += 1;
                if i < args.len() {
                    if let Some(c) = Compression::from_str(&args[i]) {
                        config.compression = c;
                    } else {
                        eprintln!("mkinitramfs: unknown compression '{}'", args[i]);
                        return 1;
                    }
                }
            }
            "-d" | "--config-dir" => {
                i += 1;
                if i < args.len() {
                    config.config_dir = PathBuf::from(&args[i]);
                }
            }
            "-v" | "--verbose" => config.verbose = true,
            "--help" | "-h" => {
                println!("Usage: mkinitramfs [options] [-o outfile] [version]");
                println!();
                println!("Create an initramfs image.");
                println!();
                println!("Options:");
                println!("  -o, --output FILE    Output file (default: /boot/initramfs.img)");
                println!("  -k, --kernel VER     Kernel version");
                println!("  -m, --module MOD     Include module");
                println!("  -c, --compress TYPE  Compression: gzip, xz, lz4, zstd, none");
                println!("  -d, --config-dir DIR Config directory");
                println!("  -v, --verbose        Verbose output");
                println!("  -h, --help           Display this help");
                println!("  --version            Display version");
                return 0;
            }
            "--version" => {
                println!("mkinitramfs (Slate OS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                config.kernel_version = s.to_string();
            }
            other => {
                eprintln!("mkinitramfs: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    // Read config files.
    let file_config = read_config(&config.config_dir);
    if config.modules.is_empty() {
        config.modules = file_config.modules;
    }
    if config.compression == Compression::Gzip && file_config.compression != Compression::Gzip {
        config.compression = file_config.compression;
    }

    if config.verbose {
        eprintln!("mkinitramfs: building for kernel {}", config.kernel_version);
        eprintln!("mkinitramfs: output: {}", config.output.display());
        eprintln!("mkinitramfs: compression: {:?}", config.compression);
        eprintln!("mkinitramfs: modules: {:?}", config.modules);
    }

    // Build the initramfs.
    let entries = build_initramfs_entries(&config);
    if config.verbose {
        eprintln!("mkinitramfs: {} entries", entries.len());
        for entry in &entries {
            eprintln!("  {}", entry.name);
        }
    }

    let cpio_data = build_cpio(&entries);
    let compressed = compress_data(&cpio_data, &config.compression);

    // Write output.
    match fs::write(&config.output, &compressed) {
        Ok(()) => {
            if config.verbose {
                eprintln!(
                    "mkinitramfs: wrote {} bytes to {}",
                    compressed.len(),
                    config.output.display()
                );
            }
            0
        }
        Err(e) => {
            eprintln!(
                "mkinitramfs: failed to write '{}': {e}",
                config.output.display()
            );
            1
        }
    }
}

// ============================================================================
// update-initramfs personality
// ============================================================================

fn update_initramfs_main(args: &[String]) -> i32 {
    let mut action = "update"; // create, update, delete
    let mut kernel_version: Option<String> = None;
    let mut verbose = false;
    let mut boot_dir = PathBuf::from("/boot");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => action = "create",
            "-u" => action = "update",
            "-d" => action = "delete",
            "-k" => {
                i += 1;
                if i < args.len() {
                    if args[i] == "all" {
                        kernel_version = None; // All kernels.
                    } else {
                        kernel_version = Some(args[i].clone());
                    }
                }
            }
            "-b" => {
                i += 1;
                if i < args.len() {
                    boot_dir = PathBuf::from(&args[i]);
                }
            }
            "-v" => verbose = true,
            "--help" | "-h" => {
                println!("Usage: update-initramfs [-c|-u|-d] [-k version|all] [-b bootdir]");
                println!();
                println!("Manage initramfs images.");
                println!();
                println!("Actions:");
                println!("  -c    Create a new initramfs");
                println!("  -u    Update an existing initramfs");
                println!("  -d    Delete an initramfs");
                println!();
                println!("Options:");
                println!("  -k VERSION  Kernel version (default: current, 'all' for all)");
                println!("  -b DIR      Boot directory (default: /boot)");
                println!("  -v          Verbose output");
                println!("  -h, --help  Display this help");
                println!("  --version   Display version");
                return 0;
            }
            "--version" => {
                println!("update-initramfs (Slate OS) {VERSION}");
                return 0;
            }
            other => {
                eprintln!("update-initramfs: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    let versions = match &kernel_version {
        Some(v) => vec![v.clone()],
        None => find_installed_kernels(),
    };

    for ver in &versions {
        let img_name = format!("initramfs-{ver}.img");
        let img_path = boot_dir.join(&img_name);

        match action {
            "create" | "update" => {
                if verbose {
                    eprintln!("update-initramfs: {action}ing {}", img_path.display());
                }
                let config = InitramfsConfig {
                    kernel_version: ver.clone(),
                    output: img_path,
                    verbose,
                    ..InitramfsConfig::default()
                };

                let entries = build_initramfs_entries(&config);
                let cpio_data = build_cpio(&entries);
                let compressed = compress_data(&cpio_data, &config.compression);

                match fs::write(&config.output, &compressed) {
                    Ok(()) => {
                        println!("update-initramfs: Generating {}", config.output.display());
                    }
                    Err(e) => {
                        eprintln!("update-initramfs: failed: {e}");
                        return 1;
                    }
                }
            }
            "delete" => {
                if verbose {
                    eprintln!("update-initramfs: deleting {}", img_path.display());
                }
                if img_path.exists() {
                    if let Err(e) = fs::remove_file(&img_path) {
                        eprintln!("update-initramfs: failed to remove: {e}");
                        return 1;
                    }
                    println!("update-initramfs: Deleting {}", img_path.display());
                } else {
                    eprintln!("update-initramfs: {} does not exist", img_path.display());
                }
            }
            _ => unreachable!(),
        }
    }

    0
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("mkinitramfs");
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

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "update-initramfs" => update_initramfs_main(&rest),
        "lsinitramfs" => lsinitramfs_main(&rest),
        _ => mkinitramfs_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_from_str() {
        assert_eq!(Compression::from_str("gzip"), Some(Compression::Gzip));
        assert_eq!(Compression::from_str("gz"), Some(Compression::Gzip));
        assert_eq!(Compression::from_str("xz"), Some(Compression::Xz));
        assert_eq!(Compression::from_str("lz4"), Some(Compression::Lz4));
        assert_eq!(Compression::from_str("zstd"), Some(Compression::Zstd));
        assert_eq!(Compression::from_str("zst"), Some(Compression::Zstd));
        assert_eq!(Compression::from_str("none"), Some(Compression::None));
        assert_eq!(Compression::from_str("cat"), Some(Compression::None));
        assert_eq!(Compression::from_str("bzip2"), None);
    }

    #[test]
    fn test_compression_extension() {
        assert_eq!(Compression::Gzip.extension(), ".gz");
        assert_eq!(Compression::Xz.extension(), ".xz");
        assert_eq!(Compression::None.extension(), "");
    }

    #[test]
    fn test_cpio_entry_directory() {
        let entry = CpioEntry::directory("test_dir", 0o755);
        assert_eq!(entry.name, "test_dir");
        assert_eq!(entry.mode, 0o040755);
        assert_eq!(entry.nlink, 2);
        assert!(entry.data.is_empty());
    }

    #[test]
    fn test_cpio_entry_file() {
        let data = b"hello world".to_vec();
        let entry = CpioEntry::file("test.txt", 0o644, data.clone());
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.mode, 0o100644);
        assert_eq!(entry.data, data);
    }

    #[test]
    fn test_cpio_entry_symlink() {
        let entry = CpioEntry::symlink("link", "target");
        assert_eq!(entry.name, "link");
        assert_eq!(entry.mode, 0o120777);
        assert_eq!(entry.data, b"target");
    }

    #[test]
    fn test_cpio_serialize_roundtrip() {
        let entries = vec![
            CpioEntry::directory(".", 0o755),
            CpioEntry::directory("bin", 0o755),
            CpioEntry::file("bin/test", 0o755, b"#!/bin/sh\necho hello\n".to_vec()),
        ];
        let cpio_data = build_cpio(&entries);

        // Should start with the magic number.
        assert_eq!(&cpio_data[0..6], b"070701");

        // Should be able to list contents.
        let listed = list_cpio_contents(&cpio_data);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0], ".");
        assert_eq!(listed[1], "bin");
        assert_eq!(listed[2], "bin/test");
    }

    #[test]
    fn test_cpio_empty() {
        let entries: Vec<CpioEntry> = vec![];
        let cpio_data = build_cpio(&entries);
        let listed = list_cpio_contents(&cpio_data);
        assert!(listed.is_empty());
    }

    #[test]
    fn test_cpio_large_file() {
        let data = vec![0x42u8; 4096];
        let entries = vec![CpioEntry::file("bigfile", 0o644, data)];
        let cpio_data = build_cpio(&entries);
        let listed = list_cpio_contents(&cpio_data);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], "bigfile");
    }

    #[test]
    fn test_generate_init_script_basic() {
        let config = InitramfsConfig::default();
        let script = generate_init_script(&config);
        assert!(script.contains("#!/bin/sh"));
        assert!(script.contains("mount -t proc"));
        assert!(script.contains("switch_root"));
    }

    #[test]
    fn test_generate_init_script_with_modules() {
        let config = InitramfsConfig {
            modules: vec!["ext4".to_string(), "ahci".to_string()],
            ..InitramfsConfig::default()
        };
        let script = generate_init_script(&config);
        assert!(script.contains("modprobe ext4"));
        assert!(script.contains("modprobe ahci"));
    }

    #[test]
    fn test_build_initramfs_entries_has_init() {
        let config = InitramfsConfig::default();
        let entries = build_initramfs_entries(&config);
        assert!(entries.iter().any(|e| e.name == "init"));
        assert!(entries.iter().any(|e| e.name == "."));
        assert!(entries.iter().any(|e| e.name == "bin"));
    }

    #[test]
    fn test_build_initramfs_entries_has_directories() {
        let config = InitramfsConfig::default();
        let entries = build_initramfs_entries(&config);
        let dir_names: Vec<&str> = entries
            .iter()
            .filter(|e| e.mode & 0o040000 != 0)
            .map(|e| e.name.as_str())
            .collect();
        assert!(dir_names.contains(&"proc"));
        assert!(dir_names.contains(&"sys"));
        assert!(dir_names.contains(&"dev"));
    }

    #[test]
    fn test_default_config() {
        let config = InitramfsConfig::default();
        assert_eq!(config.compression, Compression::Gzip);
        assert!(config.include_firmware);
        assert!(!config.verbose);
        assert_eq!(config.hooks.len(), 6);
    }

    #[test]
    fn test_detect_kernel_version() {
        let ver = detect_kernel_version();
        assert!(!ver.is_empty());
    }

    #[test]
    fn test_list_cpio_contents_invalid() {
        let data = b"not a cpio file";
        let listed = list_cpio_contents(data);
        assert!(listed.is_empty());
    }

    #[test]
    fn test_list_cpio_contents_truncated() {
        let data = b"070701";
        let listed = list_cpio_contents(data);
        assert!(listed.is_empty());
    }

    #[test]
    fn test_compress_data_none() {
        let data = b"test data";
        let compressed = compress_data(data, &Compression::None);
        assert_eq!(compressed, data);
    }

    #[test]
    fn test_cpio_trailer() {
        let trailer = CpioEntry::trailer();
        assert_eq!(trailer.name, "TRAILER!!!");
        assert_eq!(trailer.mode, 0);
    }

    #[test]
    fn test_find_installed_kernels() {
        let kernels = find_installed_kernels();
        assert!(!kernels.is_empty());
    }

    #[test]
    fn test_cpio_alignment() {
        // Verify that odd-length filenames are properly padded.
        let entry = CpioEntry::file("a", 0o644, vec![1, 2, 3]);
        let serialized = entry.serialize(1);
        // Total must be aligned to 4.
        assert_eq!(serialized.len() % 4, 0);
    }

    #[test]
    fn test_cpio_entry_char_device() {
        let entry = CpioEntry::_char_device("dev/null", 0o666, 1, 3);
        assert_eq!(entry.mode, 0o020666);
        assert_eq!(entry.rdev_major, 1);
        assert_eq!(entry.rdev_minor, 3);
    }
}
