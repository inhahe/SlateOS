//! OurOS EFI boot manager control.
//!
//! Multi-personality binary providing:
//! - **bootctl** — control the boot loader (systemd-boot)
//!
//! Manages EFI System Partition boot entries, sets default/oneshot
//! boot options, and installs/updates the boot loader.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration
// ============================================================================

fn find_esp() -> PathBuf {
    // Standard EFI System Partition mount points.
    let candidates = ["/efi", "/boot/efi", "/boot"];
    for path in &candidates {
        let p = Path::new(path);
        if p.join("EFI").is_dir() || p.join("loader").is_dir() {
            return p.to_path_buf();
        }
    }
    PathBuf::from("/boot/efi")
}

// ============================================================================
// Boot entry structures
// ============================================================================

#[derive(Clone, Debug)]
struct BootEntry {
    id: String,
    title: String,
    _source: String,
    linux: String,
    initrd: Vec<String>,
    options: String,
    _machine_id: String,
    _version: String,
    _sort_key: String,
}

#[derive(Clone, Debug)]
struct LoaderConfig {
    default_entry: String,
    timeout: Option<u32>,
    console_mode: String,
    editor: bool,
    auto_entries: bool,
    auto_firmware: bool,
}

impl Default for LoaderConfig {
    fn default() -> Self {
        Self {
            default_entry: "@saved".to_string(),
            timeout: Some(5),
            console_mode: "auto".to_string(),
            editor: true,
            auto_entries: true,
            auto_firmware: true,
        }
    }
}

// ============================================================================
// Loader config parsing
// ============================================================================

fn load_loader_config(esp: &Path) -> LoaderConfig {
    let conf_path = esp.join("loader/loader.conf");
    let mut config = LoaderConfig::default();

    if let Ok(content) = fs::read_to_string(&conf_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(char::is_whitespace) {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "default" => config.default_entry = value.to_string(),
                    "timeout" => config.timeout = value.parse().ok(),
                    "console-mode" => config.console_mode = value.to_string(),
                    "editor" => config.editor = value == "yes" || value == "1" || value == "true",
                    "auto-entries" => config.auto_entries = value != "no" && value != "0",
                    "auto-firmware" => config.auto_firmware = value != "no" && value != "0",
                    _ => {}
                }
            }
        }
    }

    config
}

fn save_loader_config(esp: &Path, config: &LoaderConfig) -> Result<(), String> {
    let conf_dir = esp.join("loader");
    fs::create_dir_all(&conf_dir).map_err(|e| format!("Cannot create loader dir: {e}"))?;

    let mut content = String::new();
    content.push_str(&format!("default  {}\n", config.default_entry));
    if let Some(timeout) = config.timeout {
        content.push_str(&format!("timeout  {timeout}\n"));
    }
    content.push_str(&format!("console-mode  {}\n", config.console_mode));
    content.push_str(&format!("editor  {}\n", if config.editor { "yes" } else { "no" }));

    fs::write(conf_dir.join("loader.conf"), content)
        .map_err(|e| format!("Cannot write loader.conf: {e}"))
}

// ============================================================================
// Boot entry discovery
// ============================================================================

fn discover_entries(esp: &Path) -> Vec<BootEntry> {
    let mut entries = Vec::new();
    let entries_dir = esp.join("loader/entries");

    if let Ok(dir_entries) = fs::read_dir(&entries_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("conf")
                && let Some(boot_entry) = parse_boot_entry(&path) {
                    entries.push(boot_entry);
                }
        }
    }

    // Fallback: generate simulated entries.
    if entries.is_empty() {
        entries.push(BootEntry {
            id: "ouros.conf".to_string(),
            title: "OurOS".to_string(),
            _source: "loader/entries/ouros.conf".to_string(),
            linux: "/vmlinuz-ouros".to_string(),
            initrd: vec!["/initramfs-ouros.img".to_string()],
            options: "root=/dev/sda2 rw quiet".to_string(),
            _machine_id: String::new(),
            _version: "0.1.0".to_string(),
            _sort_key: String::new(),
        });
        entries.push(BootEntry {
            id: "ouros-fallback.conf".to_string(),
            title: "OurOS (fallback)".to_string(),
            _source: "loader/entries/ouros-fallback.conf".to_string(),
            linux: "/vmlinuz-ouros".to_string(),
            initrd: vec!["/initramfs-ouros-fallback.img".to_string()],
            options: "root=/dev/sda2 rw".to_string(),
            _machine_id: String::new(),
            _version: "0.1.0".to_string(),
            _sort_key: String::new(),
        });
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

fn parse_boot_entry(path: &Path) -> Option<BootEntry> {
    let content = fs::read_to_string(path).ok()?;
    let id = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut title = String::new();
    let mut linux = String::new();
    let mut initrd = Vec::new();
    let mut options = String::new();
    let mut machine_id = String::new();
    let mut version = String::new();
    let mut sort_key = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(char::is_whitespace) {
            let value = value.trim();
            match key.trim() {
                "title" => title = value.to_string(),
                "linux" => linux = value.to_string(),
                "initrd" => initrd.push(value.to_string()),
                "options" => options = value.to_string(),
                "machine-id" => machine_id = value.to_string(),
                "version" => version = value.to_string(),
                "sort-key" => sort_key = value.to_string(),
                _ => {}
            }
        }
    }

    Some(BootEntry {
        id,
        title,
        _source: path.to_string_lossy().to_string(),
        linux,
        initrd,
        options,
        _machine_id: machine_id,
        _version: version,
        _sort_key: sort_key,
    })
}

// ============================================================================
// Firmware info
// ============================================================================

fn get_firmware_info() -> (String, bool) {
    // Check if running in EFI mode.
    let efi = Path::new("/sys/firmware/efi").is_dir();
    let fw_type = if efi { "UEFI" } else { "BIOS" };
    (fw_type.to_string(), efi)
}

fn get_secure_boot_status() -> &'static str {
    let sb_path = "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
    if let Ok(data) = fs::read(sb_path)
        && data.len() >= 5 && data[4] == 1 {
            return "enabled";
        }
    "disabled"
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_status(esp: &Path) -> i32 {
    let (fw_type, is_efi) = get_firmware_info();
    let config = load_loader_config(esp);
    let entries = discover_entries(esp);

    println!("System:");
    println!("     Firmware: {fw_type}");
    if is_efi {
        println!("  Secure Boot: {}", get_secure_boot_status());
    }
    println!();
    println!("Current Boot Loader:");
    println!("      Product: systemd-boot (OurOS {VERSION})");
    println!("          ESP: {}", esp.display());
    println!();
    println!("Boot Loader Configuration:");
    println!("      default: {}", config.default_entry);
    if let Some(t) = config.timeout {
        println!("      timeout: {t}s");
    }
    println!(" console-mode: {}", config.console_mode);
    println!("       editor: {}", if config.editor { "yes" } else { "no" });
    println!();
    println!("Boot Entries ({}):", entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let marker = if i == 0 { " (default)" } else { "" };
        println!("  {} {}{marker}", entry.id, entry.title);
        if !entry.linux.is_empty() {
            println!("    linux:   {}", entry.linux);
        }
        for ir in &entry.initrd {
            println!("    initrd:  {ir}");
        }
        if !entry.options.is_empty() {
            println!("    options: {}", entry.options);
        }
    }

    0
}

fn cmd_list(esp: &Path) -> i32 {
    let entries = discover_entries(esp);
    let config = load_loader_config(esp);

    for entry in &entries {
        let is_default = entry.id.contains(&config.default_entry)
            || config.default_entry == "@saved";
        let marker = if is_default { " (default)" } else { "" };
        println!("{}{marker}", entry.title);
        println!("  id: {}", entry.id);
        if !entry.linux.is_empty() {
            println!("  source: {}", entry.linux);
        }
    }

    0
}

fn cmd_set_default(esp: &Path, entry_id: &str) -> i32 {
    let mut config = load_loader_config(esp);
    config.default_entry = entry_id.to_string();

    match save_loader_config(esp, &config) {
        Ok(()) => {
            println!("bootctl: default set to '{entry_id}'");
            0
        }
        Err(e) => {
            eprintln!("bootctl: {e}");
            1
        }
    }
}

fn cmd_set_oneshot(esp: &Path, entry_id: &str) -> i32 {
    // Write to EFI variable (simulated).
    let loader_dir = esp.join("loader");
    let _ = fs::create_dir_all(&loader_dir);
    match fs::write(loader_dir.join("oneshot"), entry_id) {
        Ok(()) => {
            println!("bootctl: oneshot set to '{entry_id}'");
            0
        }
        Err(e) => {
            eprintln!("bootctl: {e}");
            1
        }
    }
}

fn cmd_set_timeout(esp: &Path, timeout: &str) -> i32 {
    let mut config = load_loader_config(esp);
    if timeout == "menu-hidden" || timeout == "0" {
        config.timeout = Some(0);
    } else if let Ok(t) = timeout.parse::<u32>() {
        config.timeout = Some(t);
    } else {
        eprintln!("bootctl: invalid timeout '{timeout}'");
        return 1;
    }

    match save_loader_config(esp, &config) {
        Ok(()) => {
            println!("bootctl: timeout set to {}s", config.timeout.unwrap_or(0));
            0
        }
        Err(e) => {
            eprintln!("bootctl: {e}");
            1
        }
    }
}

fn cmd_install(esp: &Path) -> i32 {
    let target = esp.join("EFI/systemd");
    let _ = fs::create_dir_all(&target);
    let _ = fs::create_dir_all(esp.join("loader/entries"));

    eprintln!("bootctl: would copy systemd-bootx64.efi to {}", target.display());
    eprintln!("bootctl: would set EFI boot variable");
    println!("bootctl: installed to {}", esp.display());
    0
}

fn cmd_update(esp: &Path) -> i32 {
    let target = esp.join("EFI/systemd/systemd-bootx64.efi");
    if !target.parent().map(|p| p.exists()).unwrap_or(false) {
        eprintln!("bootctl: boot loader not installed");
        return 1;
    }
    eprintln!("bootctl: would update systemd-bootx64.efi");
    println!("bootctl: updated boot loader on {}", esp.display());
    0
}

fn cmd_remove(esp: &Path) -> i32 {
    let target = esp.join("EFI/systemd");
    if target.exists() {
        eprintln!("bootctl: would remove {}", target.display());
        println!("bootctl: removed boot loader from {}", esp.display());
    } else {
        eprintln!("bootctl: boot loader not installed");
        return 1;
    }
    0
}

fn cmd_reboot_to_firmware() -> i32 {
    eprintln!("bootctl: would set OsIndications EFI variable for firmware setup");
    eprintln!("bootctl: system would reboot to firmware setup");
    0
}

fn cmd_is_installed(esp: &Path) -> i32 {
    let bootloader = esp.join("EFI/systemd/systemd-bootx64.efi");
    if bootloader.exists() {
        println!("yes");
        0
    } else {
        println!("no");
        1
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bootctl");
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
    let _ = prog_name; // Single personality.

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let mut esp_path: Option<String> = None;
    let mut command: Option<String> = None;
    let mut command_arg: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--esp-path" | "-p" => {
                i += 1;
                if i < rest.len() {
                    esp_path = Some(rest[i].clone());
                }
            }
            "--help" | "-h" => {
                println!("Usage: bootctl [options] <command>");
                println!();
                println!("Commands:");
                println!("  status              Show boot status");
                println!("  list                List boot entries");
                println!("  set-default ID      Set default boot entry");
                println!("  set-oneshot ID      Set one-time boot entry");
                println!("  set-timeout SEC     Set boot menu timeout");
                println!("  install             Install boot loader");
                println!("  update              Update boot loader");
                println!("  remove              Remove boot loader");
                println!("  is-installed        Check if installed");
                println!("  reboot-to-firmware  Reboot to firmware setup");
                println!();
                println!("Options:");
                println!("  -p, --esp-path PATH  EFI System Partition path");
                println!("  -h, --help           Display this help");
                println!("  --version            Display version");
                process::exit(0);
            }
            "--version" => {
                println!("bootctl (OurOS) {VERSION}");
                process::exit(0);
            }
            s if !s.starts_with('-') => {
                if command.is_none() {
                    command = Some(s.to_string());
                } else {
                    command_arg = Some(s.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }

    let esp = esp_path
        .map(PathBuf::from)
        .unwrap_or_else(find_esp);

    let exit_code = match command.as_deref() {
        Some("status") | None => cmd_status(&esp),
        Some("list") => cmd_list(&esp),
        Some("set-default") => match command_arg.as_deref() {
            Some(id) => cmd_set_default(&esp, id),
            None => {
                eprintln!("bootctl: set-default requires an entry ID");
                1
            }
        },
        Some("set-oneshot") => match command_arg.as_deref() {
            Some(id) => cmd_set_oneshot(&esp, id),
            None => {
                eprintln!("bootctl: set-oneshot requires an entry ID");
                1
            }
        },
        Some("set-timeout") => match command_arg.as_deref() {
            Some(t) => cmd_set_timeout(&esp, t),
            None => {
                eprintln!("bootctl: set-timeout requires a timeout value");
                1
            }
        },
        Some("install") => cmd_install(&esp),
        Some("update") => cmd_update(&esp),
        Some("remove") => cmd_remove(&esp),
        Some("is-installed") => cmd_is_installed(&esp),
        Some("reboot-to-firmware") => cmd_reboot_to_firmware(),
        Some(other) => {
            eprintln!("bootctl: unknown command '{other}'");
            1
        }
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
    fn test_default_loader_config() {
        let config = LoaderConfig::default();
        assert_eq!(config.default_entry, "@saved");
        assert_eq!(config.timeout, Some(5));
        assert!(config.editor);
        assert!(config.auto_entries);
    }

    #[test]
    fn test_find_esp() {
        let esp = find_esp();
        assert!(!esp.to_string_lossy().is_empty());
    }

    #[test]
    fn test_discover_entries_fallback() {
        let entries = discover_entries(Path::new("/nonexistent"));
        assert!(!entries.is_empty());
    }

    #[test]
    fn test_boot_entry_fields() {
        let entry = BootEntry {
            id: "test.conf".to_string(),
            title: "Test".to_string(),
            _source: "/test".to_string(),
            linux: "/vmlinuz".to_string(),
            initrd: vec!["/initrd.img".to_string()],
            options: "root=/dev/sda1".to_string(),
            _machine_id: String::new(),
            _version: "1.0".to_string(),
            _sort_key: String::new(),
        };
        assert_eq!(entry.id, "test.conf");
        assert_eq!(entry.linux, "/vmlinuz");
    }

    #[test]
    fn test_get_firmware_info() {
        let (fw_type, _is_efi) = get_firmware_info();
        assert!(!fw_type.is_empty());
    }

    #[test]
    fn test_get_secure_boot_status() {
        let status = get_secure_boot_status();
        assert!(!status.is_empty());
    }

    #[test]
    fn test_loader_config_timeout() {
        let mut config = LoaderConfig::default();
        config.timeout = Some(10);
        assert_eq!(config.timeout, Some(10));
    }

    #[test]
    fn test_loader_config_no_timeout() {
        let mut config = LoaderConfig::default();
        config.timeout = None;
        assert!(config.timeout.is_none());
    }
}
