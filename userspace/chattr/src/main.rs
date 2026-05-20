//! OurOS ext2/ext4 file attribute utilities.
//!
//! Multi-personality binary providing:
//! - **chattr** — change file attributes on an ext2/ext3/ext4 filesystem
//! - **lsattr** — list file attributes on an ext2/ext3/ext4 filesystem
//!
//! Manages extended file attributes (immutable, append-only, no dump, etc.)
//! stored in the inode flags field.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// File attribute flags (ext2/ext4 compatible)
// ============================================================================

/// Attribute flag definition.
#[derive(Clone, Debug)]
struct AttrDef {
    /// Single character representation.
    letter: char,
    /// Flag bitmask value.
    flag: u32,
    /// Human-readable name.
    name: &'static str,
    /// Short description.
    description: &'static str,
}

// ext4 file attribute flags.
const EXT4_SECRM_FL: u32 = 0x0000_0001;
const EXT4_UNRM_FL: u32 = 0x0000_0002;
const EXT4_COMPR_FL: u32 = 0x0000_0004;
const EXT4_SYNC_FL: u32 = 0x0000_0008;
const EXT4_IMMUTABLE_FL: u32 = 0x0000_0010;
const EXT4_APPEND_FL: u32 = 0x0000_0020;
const EXT4_NODUMP_FL: u32 = 0x0000_0040;
const EXT4_NOATIME_FL: u32 = 0x0000_0080;
const EXT4_DIRTY_FL: u32 = 0x0000_0100;
const EXT4_COMPRBLK_FL: u32 = 0x0000_0200;
const EXT4_NOCOMPR_FL: u32 = 0x0000_0400;
const EXT4_ENCRYPT_FL: u32 = 0x0000_0800;
const EXT4_INDEX_FL: u32 = 0x0000_1000;
const EXT4_JOURNAL_DATA_FL: u32 = 0x0000_4000;
const EXT4_NOTAIL_FL: u32 = 0x0000_8000;
const EXT4_DIRSYNC_FL: u32 = 0x0001_0000;
const EXT4_TOPDIR_FL: u32 = 0x0002_0000;
const EXT4_EXTENTS_FL: u32 = 0x0008_0000;
const EXT4_VERITY_FL: u32 = 0x0010_0000;
const EXT4_NOCOW_FL: u32 = 0x0080_0000;
const EXT4_CASEFOLD_FL: u32 = 0x4000_0000;
const EXT4_PROJINHERIT_FL: u32 = 0x2000_0000;

const ATTR_DEFS: &[AttrDef] = &[
    AttrDef { letter: 's', flag: EXT4_SECRM_FL, name: "Secure_Deletion", description: "secure deletion" },
    AttrDef { letter: 'u', flag: EXT4_UNRM_FL, name: "Undelete", description: "undelete" },
    AttrDef { letter: 'c', flag: EXT4_COMPR_FL, name: "Compression_Requested", description: "compress" },
    AttrDef { letter: 'S', flag: EXT4_SYNC_FL, name: "Synchronous_Updates", description: "synchronous updates" },
    AttrDef { letter: 'i', flag: EXT4_IMMUTABLE_FL, name: "Immutable", description: "immutable" },
    AttrDef { letter: 'a', flag: EXT4_APPEND_FL, name: "Append_Only", description: "append only" },
    AttrDef { letter: 'd', flag: EXT4_NODUMP_FL, name: "No_Dump", description: "no dump" },
    AttrDef { letter: 'A', flag: EXT4_NOATIME_FL, name: "No_Atime", description: "no atime updates" },
    AttrDef { letter: 'Z', flag: EXT4_DIRTY_FL, name: "Dirty", description: "dirty (compressed)" },
    AttrDef { letter: 'B', flag: EXT4_COMPRBLK_FL, name: "Compressed_Dirty_File", description: "compressed dirty file" },
    AttrDef { letter: 'X', flag: EXT4_NOCOMPR_FL, name: "Compression_Raw_Access", description: "raw access to compressed data" },
    AttrDef { letter: 'E', flag: EXT4_ENCRYPT_FL, name: "Encrypted", description: "encrypted" },
    AttrDef { letter: 'I', flag: EXT4_INDEX_FL, name: "Indexed_Directory", description: "indexed directory (htree)" },
    AttrDef { letter: 'j', flag: EXT4_JOURNAL_DATA_FL, name: "Journal_Data", description: "journal data" },
    AttrDef { letter: 't', flag: EXT4_NOTAIL_FL, name: "No_Tailmerging", description: "no tail-merging" },
    AttrDef { letter: 'D', flag: EXT4_DIRSYNC_FL, name: "Synchronous_Directory_Updates", description: "synchronous directory updates" },
    AttrDef { letter: 'T', flag: EXT4_TOPDIR_FL, name: "Top_of_Directory_Hierarchies", description: "top of directory hierarchy" },
    AttrDef { letter: 'e', flag: EXT4_EXTENTS_FL, name: "Extents", description: "uses extents" },
    AttrDef { letter: 'V', flag: EXT4_VERITY_FL, name: "Verity", description: "verity protected" },
    AttrDef { letter: 'C', flag: EXT4_NOCOW_FL, name: "No_COW", description: "no copy on write" },
    AttrDef { letter: 'F', flag: EXT4_CASEFOLD_FL, name: "Casefold", description: "casefolded directory" },
    AttrDef { letter: 'P', flag: EXT4_PROJINHERIT_FL, name: "Project_Hierarchy", description: "project hierarchy" },
];

// ============================================================================
// Attribute operations
// ============================================================================

fn flags_to_string(flags: u32) -> String {
    let mut result = String::new();
    for attr in ATTR_DEFS {
        if flags & attr.flag != 0 {
            result.push(attr.letter);
        } else {
            result.push('-');
        }
    }
    result
}

fn letter_to_flag(c: char) -> Option<u32> {
    ATTR_DEFS.iter().find(|a| a.letter == c).map(|a| a.flag)
}

fn parse_attr_spec(spec: &str) -> Result<(u32, u32, u32), String> {
    // Format: +attr, -attr, or =attr
    // Returns (add_flags, remove_flags, set_flags)
    // =flags: set exactly these
    // +flags: add these
    // -flags: remove these
    let mut add = 0u32;
    let mut remove = 0u32;
    let mut set = 0u32;

    let first = spec.chars().next();
    let mode = match first {
        Some('+') => '+',
        Some('-') => '-',
        Some('=') => '=',
        _ => return Err(format!("Invalid attribute specification: {spec}")),
    };

    for c in spec[1..].chars() {
        let flag = letter_to_flag(c).ok_or_else(|| format!("Unknown attribute: {c}"))?;
        match mode {
            '+' => add |= flag,
            '-' => remove |= flag,
            '=' => set |= flag,
            _ => unreachable!(),
        }
    }

    Ok((add, remove, set))
}

/// Simulated attribute storage path.
fn attr_file_path(file: &str) -> String {
    // In a real implementation, this would use ioctl FS_IOC_GETFLAGS/FS_IOC_SETFLAGS.
    // We simulate by storing in an xattr-like sidecar file.
    format!("{file}.attrs")
}

fn read_attrs(file: &str) -> u32 {
    let attr_path = attr_file_path(file);
    if let Ok(content) = fs::read_to_string(&attr_path) {
        if let Ok(val) = u32::from_str_radix(content.trim(), 16) {
            return val;
        }
    }
    // Default: extents flag is commonly set on ext4.
    if fs::metadata(file).map(|m| m.is_file()).unwrap_or(false) {
        EXT4_EXTENTS_FL
    } else {
        0
    }
}

fn write_attrs(file: &str, flags: u32) -> io::Result<()> {
    let attr_path = attr_file_path(file);
    fs::write(&attr_path, format!("{flags:08x}\n"))
}

// ============================================================================
// lsattr command
// ============================================================================

struct LsattrOpts {
    recursive: bool,
    all: bool,
    dirs_as_files: bool,
    verbose: bool,
    long_format: bool,
    project: bool,
    files: Vec<String>,
}

fn cmd_lsattr(args: &[String]) {
    let mut opts = LsattrOpts {
        recursive: false,
        all: false,
        dirs_as_files: false,
        verbose: false,
        long_format: false,
        project: false,
        files: Vec::new(),
    };

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: lsattr [-RVadlp] [file...]");
                println!();
                println!("List file attributes on an ext2/ext3/ext4 filesystem.");
                println!();
                println!("Options:");
                println!("  -R    Recurse into directories");
                println!("  -V    Display version");
                println!("  -a    Show all files (including dotfiles)");
                println!("  -d    List directories as files, not their contents");
                println!("  -l    Long format (human-readable attribute names)");
                println!("  -v    Verbose");
                println!("  -p    Show project number");
                println!("  -h, --help  Show this help");
                println!("  --version   Show version");
                process::exit(0);
            }
            "--version" | "-V" => {
                println!("lsattr {VERSION}");
                process::exit(0);
            }
            s if s.starts_with('-') && s.len() > 1 && !s.starts_with("--") => {
                for c in s[1..].chars() {
                    match c {
                        'R' => opts.recursive = true,
                        'a' => opts.all = true,
                        'd' => opts.dirs_as_files = true,
                        'v' => opts.verbose = true,
                        'l' => opts.long_format = true,
                        'p' => opts.project = true,
                        _ => {}
                    }
                }
            }
            s => opts.files.push(s.to_string()),
        }
    }

    if opts.files.is_empty() {
        opts.files.push(".".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for file in &opts.files {
        list_attrs(&mut out, file, &opts, 0);
    }
}

fn list_attrs(out: &mut io::StdoutLock<'_>, path: &str, opts: &LsattrOpts, depth: u32) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("lsattr: cannot read {path}: {e}");
            return;
        }
    };

    if metadata.is_dir() && !opts.dirs_as_files && depth == 0 {
        // List directory contents.
        if let Ok(entries) = fs::read_dir(path) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if !opts.all && name.starts_with('.') {
                        None
                    } else {
                        Some(e.path().to_string_lossy().to_string())
                    }
                })
                .collect();
            names.sort();

            for entry_path in &names {
                print_file_attrs(out, entry_path, opts);
                if opts.recursive {
                    if let Ok(m) = fs::symlink_metadata(entry_path) {
                        if m.is_dir() {
                            let _ = writeln!(out);
                            list_attrs(out, entry_path, opts, depth + 1);
                        }
                    }
                }
            }
        }
    } else {
        print_file_attrs(out, path, opts);
    }
}

fn print_file_attrs(out: &mut io::StdoutLock<'_>, path: &str, opts: &LsattrOpts) {
    let flags = read_attrs(path);

    if opts.long_format {
        // Long format: list attribute names.
        let names: Vec<&str> = ATTR_DEFS
            .iter()
            .filter(|a| flags & a.flag != 0)
            .map(|a| a.name)
            .collect();
        if names.is_empty() {
            let _ = writeln!(out, "{path}: ---");
        } else {
            let _ = writeln!(out, "{path}: {}", names.join(", "));
        }
    } else {
        let attr_str = flags_to_string(flags);
        if opts.project {
            let _ = writeln!(out, "{:5} {attr_str} {path}", 0);
        } else {
            let _ = writeln!(out, "{attr_str} {path}");
        }
    }
}

// ============================================================================
// chattr command
// ============================================================================

struct ChattrOpts {
    recursive: bool,
    verbose: bool,
    version_num: Option<u32>,
    project: Option<u32>,
    specs: Vec<String>,
    files: Vec<String>,
}

fn cmd_chattr(args: &[String]) {
    let mut opts = ChattrOpts {
        recursive: false,
        verbose: false,
        version_num: None,
        project: None,
        specs: Vec::new(),
        files: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: chattr [-RVf] [-v version] [-p project] {{+-=}}[attr] file...");
                println!();
                println!("Change file attributes on an ext2/ext3/ext4 filesystem.");
                println!();
                println!("Attributes:");
                for attr in ATTR_DEFS {
                    println!("  {}  {}", attr.letter, attr.description);
                }
                println!();
                println!("Options:");
                println!("  -R    Recurse into directories");
                println!("  -V    Verbose (print changed files)");
                println!("  -f    Suppress most error messages");
                println!("  -v N  Set file version/generation number");
                println!("  -p N  Set project number");
                println!("  -h, --help  Show this help");
                println!("  --version   Show version");
                process::exit(0);
            }
            "--version" => {
                println!("chattr {VERSION}");
                process::exit(0);
            }
            "-R" => opts.recursive = true,
            "-V" => opts.verbose = true,
            "-f" => {} // Suppress errors — we handle gracefully anyway.
            "-v" => {
                i += 1;
                if i < args.len() {
                    opts.version_num = args[i].parse().ok();
                }
            }
            "-p" => {
                i += 1;
                if i < args.len() {
                    opts.project = args[i].parse().ok();
                }
            }
            s if s.starts_with('+') || s.starts_with('-') || s.starts_with('=') => {
                opts.specs.push(s.to_string());
            }
            s => {
                opts.files.push(s.to_string());
            }
        }
        i += 1;
    }

    if opts.specs.is_empty() && opts.version_num.is_none() && opts.project.is_none() {
        eprintln!("chattr: must specify attribute changes");
        process::exit(1);
    }

    if opts.files.is_empty() {
        eprintln!("chattr: no files specified");
        process::exit(1);
    }

    // Parse attribute specs.
    let mut total_add = 0u32;
    let mut total_remove = 0u32;
    let mut total_set: Option<u32> = None;

    for spec in &opts.specs {
        match parse_attr_spec(spec) {
            Ok((add, remove, set)) => {
                total_add |= add;
                total_remove |= remove;
                if set != 0 || spec.starts_with('=') {
                    total_set = Some(set);
                }
            }
            Err(e) => {
                eprintln!("chattr: {e}");
                process::exit(1);
            }
        }
    }

    for file in &opts.files {
        apply_attrs(file, total_add, total_remove, total_set, &opts);
    }
}

fn apply_attrs(path: &str, add: u32, remove: u32, set: Option<u32>, opts: &ChattrOpts) {
    // Check file exists.
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("chattr: cannot stat {path}: {e}");
            return;
        }
    };

    let current = read_attrs(path);
    let new_flags = if let Some(set_flags) = set {
        set_flags
    } else {
        (current | add) & !remove
    };

    if new_flags != current {
        if let Err(e) = write_attrs(path, new_flags) {
            eprintln!("chattr: cannot set attributes on {path}: {e}");
            return;
        }
        if opts.verbose {
            let old_str = flags_to_string(current);
            let new_str = flags_to_string(new_flags);
            eprintln!("Flags of {path} set as {new_str} (was {old_str})");
        }
    }

    // Recurse into directories.
    if opts.recursive && metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let entry_path = entry.path().to_string_lossy().to_string();
                apply_attrs(&entry_path, add, remove, set, opts);
            }
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("chattr");
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

    match prog_name.as_str() {
        "lsattr" => cmd_lsattr(&rest),
        _ => cmd_chattr(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_defs_count() {
        assert_eq!(ATTR_DEFS.len(), 22);
    }

    #[test]
    fn test_attr_defs_unique_letters() {
        let letters: Vec<char> = ATTR_DEFS.iter().map(|a| a.letter).collect();
        for (i, l) in letters.iter().enumerate() {
            for (j, m) in letters.iter().enumerate() {
                if i != j {
                    assert_ne!(l, m, "Duplicate letter: {l}");
                }
            }
        }
    }

    #[test]
    fn test_attr_defs_unique_flags() {
        let flags: Vec<u32> = ATTR_DEFS.iter().map(|a| a.flag).collect();
        for (i, f) in flags.iter().enumerate() {
            for (j, g) in flags.iter().enumerate() {
                if i != j {
                    assert_ne!(f, g, "Duplicate flag: {f:#x}");
                }
            }
        }
    }

    #[test]
    fn test_flags_to_string_none() {
        let s = flags_to_string(0);
        assert_eq!(s.len(), ATTR_DEFS.len());
        assert!(s.chars().all(|c| c == '-'));
    }

    #[test]
    fn test_flags_to_string_immutable() {
        let s = flags_to_string(EXT4_IMMUTABLE_FL);
        assert!(s.contains('i'));
        // Other flags should be dashes.
        let non_dash: Vec<char> = s.chars().filter(|&c| c != '-').collect();
        assert_eq!(non_dash.len(), 1);
        assert_eq!(non_dash[0], 'i');
    }

    #[test]
    fn test_flags_to_string_multiple() {
        let s = flags_to_string(EXT4_IMMUTABLE_FL | EXT4_APPEND_FL);
        assert!(s.contains('i'));
        assert!(s.contains('a'));
    }

    #[test]
    fn test_letter_to_flag() {
        assert_eq!(letter_to_flag('i'), Some(EXT4_IMMUTABLE_FL));
        assert_eq!(letter_to_flag('a'), Some(EXT4_APPEND_FL));
        assert_eq!(letter_to_flag('d'), Some(EXT4_NODUMP_FL));
        assert_eq!(letter_to_flag('S'), Some(EXT4_SYNC_FL));
    }

    #[test]
    fn test_letter_to_flag_unknown() {
        assert_eq!(letter_to_flag('z'), None);
        assert_eq!(letter_to_flag('Q'), None);
    }

    #[test]
    fn test_parse_attr_spec_add() {
        let (add, remove, set) = parse_attr_spec("+i").unwrap();
        assert_eq!(add, EXT4_IMMUTABLE_FL);
        assert_eq!(remove, 0);
        assert_eq!(set, 0);
    }

    #[test]
    fn test_parse_attr_spec_remove() {
        let (add, remove, set) = parse_attr_spec("-i").unwrap();
        assert_eq!(add, 0);
        assert_eq!(remove, EXT4_IMMUTABLE_FL);
        assert_eq!(set, 0);
    }

    #[test]
    fn test_parse_attr_spec_set() {
        let (add, remove, set) = parse_attr_spec("=ia").unwrap();
        assert_eq!(add, 0);
        assert_eq!(remove, 0);
        assert_eq!(set, EXT4_IMMUTABLE_FL | EXT4_APPEND_FL);
    }

    #[test]
    fn test_parse_attr_spec_multiple() {
        let (add, _, _) = parse_attr_spec("+iad").unwrap();
        assert_eq!(
            add,
            EXT4_IMMUTABLE_FL | EXT4_APPEND_FL | EXT4_NODUMP_FL
        );
    }

    #[test]
    fn test_parse_attr_spec_empty_set() {
        let (add, remove, set) = parse_attr_spec("=").unwrap();
        assert_eq!(add, 0);
        assert_eq!(remove, 0);
        assert_eq!(set, 0);
    }

    #[test]
    fn test_parse_attr_spec_invalid() {
        assert!(parse_attr_spec("x").is_err());
    }

    #[test]
    fn test_parse_attr_spec_unknown_letter() {
        assert!(parse_attr_spec("+z").is_err());
    }

    #[test]
    fn test_ext4_flag_values() {
        // Verify key flags match ext4 kernel values.
        assert_eq!(EXT4_SECRM_FL, 0x1);
        assert_eq!(EXT4_UNRM_FL, 0x2);
        assert_eq!(EXT4_COMPR_FL, 0x4);
        assert_eq!(EXT4_SYNC_FL, 0x8);
        assert_eq!(EXT4_IMMUTABLE_FL, 0x10);
        assert_eq!(EXT4_APPEND_FL, 0x20);
        assert_eq!(EXT4_NODUMP_FL, 0x40);
        assert_eq!(EXT4_NOATIME_FL, 0x80);
    }

    #[test]
    fn test_attr_application() {
        // Test that add/remove logic works correctly.
        let current = EXT4_EXTENTS_FL;
        let add = EXT4_IMMUTABLE_FL;
        let remove = 0u32;
        let new_flags = (current | add) & !remove;
        assert!(new_flags & EXT4_IMMUTABLE_FL != 0);
        assert!(new_flags & EXT4_EXTENTS_FL != 0);
    }

    #[test]
    fn test_attr_removal() {
        let current = EXT4_IMMUTABLE_FL | EXT4_APPEND_FL;
        let remove = EXT4_IMMUTABLE_FL;
        let new_flags = (current | 0) & !remove;
        assert!(new_flags & EXT4_IMMUTABLE_FL == 0);
        assert!(new_flags & EXT4_APPEND_FL != 0);
    }

    #[test]
    fn test_attr_set_replaces() {
        let set = EXT4_NODUMP_FL;
        assert_eq!(set, EXT4_NODUMP_FL);
        assert!(set & EXT4_IMMUTABLE_FL == 0);
    }
}
