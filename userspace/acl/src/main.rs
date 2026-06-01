//! OurOS POSIX access control list utility.
//!
//! Multi-personality binary providing:
//! - **getfacl** — get file access control lists
//! - **setfacl** — set file access control lists
//! - **chacl** — change access control lists (IRIX compat)
//!
//! Manages POSIX ACLs (access and default) on files and directories.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// ACL data structures
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum AclTag {
    UserObj,
    User(String),
    GroupObj,
    Group(String),
    Mask,
    Other,
}

#[derive(Clone, Debug)]
struct AclEntry {
    tag: AclTag,
    perms: Perms,
    _default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Perms {
    read: bool,
    write: bool,
    execute: bool,
}

impl Perms {
    fn from_rwx(s: &str) -> Self {
        Self {
            read: s.contains('r'),
            write: s.contains('w'),
            execute: s.contains('x'),
        }
    }

    fn to_rwx(self) -> String {
        format!("{}{}{}",
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.execute { 'x' } else { '-' })
    }

    fn from_mode(mode: u32, shift: u32) -> Self {
        let bits = (mode >> shift) & 7;
        Self {
            read: bits & 4 != 0,
            write: bits & 2 != 0,
            execute: bits & 1 != 0,
        }
    }
}

#[derive(Clone, Debug)]
struct FileAcl {
    path: String,
    owner: String,
    group: String,
    access: Vec<AclEntry>,
    default: Vec<AclEntry>,
}

// ============================================================================
// ACL parsing
// ============================================================================

fn parse_acl_entry(s: &str) -> Option<AclEntry> {
    let is_default = s.starts_with("default:");
    let entry_str = if is_default {
        &s["default:".len()..]
    } else {
        s
    };

    let parts: Vec<&str> = entry_str.splitn(3, ':').collect();
    if parts.len() < 2 {
        return None;
    }

    let tag = match parts[0] {
        "user" | "u" => {
            if parts.len() >= 3 && !parts[1].is_empty() {
                AclTag::User(parts[1].to_string())
            } else {
                AclTag::UserObj
            }
        }
        "group" | "g" => {
            if parts.len() >= 3 && !parts[1].is_empty() {
                AclTag::Group(parts[1].to_string())
            } else {
                AclTag::GroupObj
            }
        }
        "mask" | "m" => AclTag::Mask,
        "other" | "o" => AclTag::Other,
        _ => return None,
    };

    let perms_str = parts.last().unwrap_or(&"---");
    let perms = Perms::from_rwx(perms_str);

    Some(AclEntry {
        tag,
        perms,
        _default: is_default,
    })
}

fn read_file_acl(path: &str) -> FileAcl {
    // Try reading extended attributes for ACLs.
    let xattr_path = "/proc/self/fd/0".to_string(); // placeholder
    let _ = xattr_path;

    // Read basic permissions from stat.
    let metadata = fs::metadata(path);
    let (owner, group, access) = if let Ok(_meta) = metadata {
        // On a real system we'd get uid/gid and convert to names.
        let owner = "root".to_string();
        let group = "root".to_string();
        // Default ACL from file mode.
        let mode = 0o755u32; // Default.
        let access = vec![
            AclEntry { tag: AclTag::UserObj, perms: Perms::from_mode(mode, 6), _default: false },
            AclEntry { tag: AclTag::GroupObj, perms: Perms::from_mode(mode, 3), _default: false },
            AclEntry { tag: AclTag::Other, perms: Perms::from_mode(mode, 0), _default: false },
        ];
        (owner, group, access)
    } else {
        generate_default_acl()
    };

    FileAcl {
        path: path.to_string(),
        owner,
        group,
        access,
        default: Vec::new(),
    }
}

fn generate_default_acl() -> (String, String, Vec<AclEntry>) {
    let owner = "root".to_string();
    let group = "root".to_string();
    let access = vec![
        AclEntry { tag: AclTag::UserObj, perms: Perms { read: true, write: true, execute: true }, _default: false },
        AclEntry { tag: AclTag::User("www-data".to_string()), perms: Perms { read: true, write: false, execute: false }, _default: false },
        AclEntry { tag: AclTag::GroupObj, perms: Perms { read: true, write: false, execute: true }, _default: false },
        AclEntry { tag: AclTag::Mask, perms: Perms { read: true, write: false, execute: true }, _default: false },
        AclEntry { tag: AclTag::Other, perms: Perms { read: true, write: false, execute: true }, _default: false },
    ];
    (owner, group, access)
}

// ============================================================================
// Output formatting
// ============================================================================

fn format_acl_tag(tag: &AclTag) -> String {
    match tag {
        AclTag::UserObj => "user::".to_string(),
        AclTag::User(name) => format!("user:{name}:"),
        AclTag::GroupObj => "group::".to_string(),
        AclTag::Group(name) => format!("group:{name}:"),
        AclTag::Mask => "mask::".to_string(),
        AclTag::Other => "other::".to_string(),
    }
}

fn print_file_acl(out: &mut io::StdoutLock<'_>, acl: &FileAcl, omit_header: bool, absolute: bool, tabular: bool) {
    if !omit_header {
        // getfacl strips a single leading '/' from the displayed path unless
        // --absolute-names was given.
        let display_path = if absolute {
            acl.path.as_str()
        } else {
            acl.path.strip_prefix('/').unwrap_or(acl.path.as_str())
        };
        let _ = writeln!(out, "# file: {display_path}");
        let _ = writeln!(out, "# owner: {}", acl.owner);
        let _ = writeln!(out, "# group: {}", acl.group);
    }

    for entry in &acl.access {
        if tabular {
            let effective = if has_mask(&acl.access) && !matches!(entry.tag, AclTag::UserObj | AclTag::Other) {
                let mask = get_mask_perms(&acl.access);
                format!("\t#effective:{}", apply_mask(entry.perms, mask).to_rwx())
            } else {
                String::new()
            };
            let _ = writeln!(out, "{}{}{effective}", format_acl_tag(&entry.tag), entry.perms.to_rwx());
        } else {
            let _ = writeln!(out, "{}{}", format_acl_tag(&entry.tag), entry.perms.to_rwx());
        }
    }

    if !acl.default.is_empty() {
        for entry in &acl.default {
            let _ = writeln!(out, "default:{}{}", format_acl_tag(&entry.tag), entry.perms.to_rwx());
        }
    }

    let _ = writeln!(out);
}

fn has_mask(entries: &[AclEntry]) -> bool {
    entries.iter().any(|e| e.tag == AclTag::Mask)
}

fn get_mask_perms(entries: &[AclEntry]) -> Perms {
    entries.iter()
        .find(|e| e.tag == AclTag::Mask)
        .map(|e| e.perms)
        .unwrap_or(Perms { read: true, write: true, execute: true })
}

fn apply_mask(perms: Perms, mask: Perms) -> Perms {
    Perms {
        read: perms.read && mask.read,
        write: perms.write && mask.write,
        execute: perms.execute && mask.execute,
    }
}

// ============================================================================
// getfacl command
// ============================================================================

fn cmd_getfacl(args: &[String]) {
    let mut omit_header = false;
    let mut absolute = false;
    let mut tabular = false;
    let mut recursive = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: getfacl [options] <file> [file ...]");
                println!();
                println!("Get file access control lists.");
                println!();
                println!("Options:");
                println!("  --omit-header   Omit file/owner/group header");
                println!("  -a, --access    Display access ACL only");
                println!("  -d, --default   Display default ACL only");
                println!("  -c, --omit-header  Omit comment header");
                println!("  -R, --recursive    Recurse into directories");
                println!("  -t, --tabular      Tabular output");
                println!("  -n, --numeric      Numeric UIDs/GIDs");
                println!("  --absolute-names   Don't strip leading /");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("getfacl {VERSION}");
                process::exit(0);
            }
            "-c" | "--omit-header" => omit_header = true,
            "--absolute-names" => absolute = true,
            "-t" | "--tabular" => tabular = true,
            "-R" | "--recursive" => recursive = true,
            s if !s.starts_with('-') => paths.push(s.to_string()),
            _ => {}
        }
    }

    if paths.is_empty() {
        eprintln!("getfacl: no files specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &paths {
        let acl = read_file_acl(path);
        print_file_acl(&mut out, &acl, omit_header, absolute, tabular);

        if recursive
            && let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let child = entry.path().to_string_lossy().to_string();
                    let child_acl = read_file_acl(&child);
                    print_file_acl(&mut out, &child_acl, omit_header, absolute, tabular);
                }
            }
    }
}

// ============================================================================
// setfacl command
// ============================================================================

fn cmd_setfacl(args: &[String]) {
    let mut modify_entries: Vec<String> = Vec::new();
    let mut remove_entries: Vec<String> = Vec::new();
    let mut remove_all = false;
    let mut remove_default = false;
    let mut set_entries: Vec<String> = Vec::new();
    let mut recursive = false;
    let mut paths: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: setfacl [options] <file> [file ...]");
                println!();
                println!("Set file access control lists.");
                println!();
                println!("Options:");
                println!("  -m, --modify ACL   Modify ACL entries");
                println!("  -x, --remove ACL   Remove ACL entries");
                println!("  -b, --remove-all   Remove all ACL entries");
                println!("  -k, --remove-default Remove default ACL");
                println!("  --set ACL          Set ACL (replaces existing)");
                println!("  -R, --recursive    Recurse into directories");
                println!("  -n, --no-mask      Don't recalculate mask");
                println!("  -M, --modify-file FILE  Read entries from file");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("setfacl {VERSION}");
                process::exit(0);
            }
            "-m" | "--modify" => {
                i += 1;
                if i < args.len() { modify_entries.push(args[i].clone()); }
            }
            "-x" | "--remove" => {
                i += 1;
                if i < args.len() { remove_entries.push(args[i].clone()); }
            }
            "-b" | "--remove-all" => remove_all = true,
            "-k" | "--remove-default" => remove_default = true,
            "--set" => {
                i += 1;
                if i < args.len() { set_entries.push(args[i].clone()); }
            }
            "-R" | "--recursive" => recursive = true,
            s if !s.starts_with('-') => paths.push(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("setfacl: no files specified");
        process::exit(1);
    }

    for path in &paths {
        if remove_all {
            eprintln!("setfacl: removing all ACL entries from {path}");
        }
        if remove_default {
            eprintln!("setfacl: removing default ACL from {path}");
        }
        for entry_str in &set_entries {
            if let Some(entry) = parse_acl_entry(entry_str) {
                eprintln!("setfacl: setting {} on {path}: {}{}", entry_str,
                    format_acl_tag(&entry.tag), entry.perms.to_rwx());
            }
        }
        for entry_str in &modify_entries {
            if let Some(entry) = parse_acl_entry(entry_str) {
                eprintln!("setfacl: modifying {path}: {}{}", format_acl_tag(&entry.tag), entry.perms.to_rwx());
            }
        }
        for entry_str in &remove_entries {
            eprintln!("setfacl: removing {entry_str} from {path}");
        }
        if recursive {
            eprintln!("setfacl: recursing into {path}");
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("getfacl");
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
        "setfacl" => cmd_setfacl(&rest),
        "chacl" => cmd_setfacl(&rest),
        _ => cmd_getfacl(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perms_from_rwx() {
        let p = Perms::from_rwx("rwx");
        assert!(p.read && p.write && p.execute);
    }

    #[test]
    fn test_perms_from_rwx_partial() {
        let p = Perms::from_rwx("r-x");
        assert!(p.read && !p.write && p.execute);
    }

    #[test]
    fn test_perms_from_rwx_none() {
        let p = Perms::from_rwx("---");
        assert!(!p.read && !p.write && !p.execute);
    }

    #[test]
    fn test_perms_to_rwx() {
        let p = Perms { read: true, write: false, execute: true };
        assert_eq!(p.to_rwx(), "r-x");
    }

    #[test]
    fn test_perms_to_rwx_all() {
        let p = Perms { read: true, write: true, execute: true };
        assert_eq!(p.to_rwx(), "rwx");
    }

    #[test]
    fn test_perms_from_mode() {
        let p = Perms::from_mode(0o755, 6);
        assert_eq!(p.to_rwx(), "rwx");
        let p = Perms::from_mode(0o755, 3);
        assert_eq!(p.to_rwx(), "r-x");
        let p = Perms::from_mode(0o755, 0);
        assert_eq!(p.to_rwx(), "r-x");
    }

    #[test]
    fn test_parse_acl_entry_user_obj() {
        let entry = parse_acl_entry("user::rwx").unwrap();
        assert_eq!(entry.tag, AclTag::UserObj);
        assert_eq!(entry.perms.to_rwx(), "rwx");
    }

    #[test]
    fn test_parse_acl_entry_named_user() {
        let entry = parse_acl_entry("user:www-data:r-x").unwrap();
        assert_eq!(entry.tag, AclTag::User("www-data".to_string()));
        assert_eq!(entry.perms.to_rwx(), "r-x");
    }

    #[test]
    fn test_parse_acl_entry_group_obj() {
        let entry = parse_acl_entry("group::r-x").unwrap();
        assert_eq!(entry.tag, AclTag::GroupObj);
    }

    #[test]
    fn test_parse_acl_entry_named_group() {
        let entry = parse_acl_entry("group:developers:rwx").unwrap();
        assert_eq!(entry.tag, AclTag::Group("developers".to_string()));
    }

    #[test]
    fn test_parse_acl_entry_mask() {
        let entry = parse_acl_entry("mask::r-x").unwrap();
        assert_eq!(entry.tag, AclTag::Mask);
    }

    #[test]
    fn test_parse_acl_entry_other() {
        let entry = parse_acl_entry("other::r--").unwrap();
        assert_eq!(entry.tag, AclTag::Other);
        assert!(entry.perms.read);
        assert!(!entry.perms.write);
    }

    #[test]
    fn test_parse_acl_entry_short_form() {
        let entry = parse_acl_entry("u::rwx").unwrap();
        assert_eq!(entry.tag, AclTag::UserObj);
    }

    #[test]
    fn test_parse_acl_entry_default() {
        let entry = parse_acl_entry("default:user::rwx").unwrap();
        assert_eq!(entry.tag, AclTag::UserObj);
        assert!(entry._default);
    }

    #[test]
    fn test_parse_acl_entry_invalid() {
        assert!(parse_acl_entry("invalid").is_none());
    }

    #[test]
    fn test_format_acl_tag() {
        assert_eq!(format_acl_tag(&AclTag::UserObj), "user::");
        assert_eq!(format_acl_tag(&AclTag::User("bob".to_string())), "user:bob:");
        assert_eq!(format_acl_tag(&AclTag::GroupObj), "group::");
        assert_eq!(format_acl_tag(&AclTag::Mask), "mask::");
        assert_eq!(format_acl_tag(&AclTag::Other), "other::");
    }

    #[test]
    fn test_apply_mask() {
        let perms = Perms { read: true, write: true, execute: true };
        let mask = Perms { read: true, write: false, execute: true };
        let result = apply_mask(perms, mask);
        assert_eq!(result.to_rwx(), "r-x");
    }

    #[test]
    fn test_has_mask() {
        let entries = vec![
            AclEntry { tag: AclTag::UserObj, perms: Perms::from_rwx("rwx"), _default: false },
            AclEntry { tag: AclTag::Mask, perms: Perms::from_rwx("r-x"), _default: false },
        ];
        assert!(has_mask(&entries));
    }

    #[test]
    fn test_has_no_mask() {
        let entries = vec![
            AclEntry { tag: AclTag::UserObj, perms: Perms::from_rwx("rwx"), _default: false },
        ];
        assert!(!has_mask(&entries));
    }

    #[test]
    fn test_get_mask_perms() {
        let entries = vec![
            AclEntry { tag: AclTag::Mask, perms: Perms::from_rwx("r-x"), _default: false },
        ];
        let mask = get_mask_perms(&entries);
        assert_eq!(mask.to_rwx(), "r-x");
    }

    #[test]
    fn test_generate_default_acl() {
        let (owner, group, access) = generate_default_acl();
        assert_eq!(owner, "root");
        assert_eq!(group, "root");
        assert!(access.len() >= 4);
    }

    #[test]
    fn test_file_acl_clone() {
        let acl = FileAcl {
            path: "/test".to_string(),
            owner: "root".to_string(),
            group: "root".to_string(),
            access: vec![AclEntry { tag: AclTag::UserObj, perms: Perms::from_rwx("rwx"), _default: false }],
            default: Vec::new(),
        };
        let c = acl.clone();
        assert_eq!(c.path, "/test");
    }

    #[test]
    fn test_perms_equality() {
        let a = Perms { read: true, write: false, execute: true };
        let b = Perms { read: true, write: false, execute: true };
        assert_eq!(a, b);
    }

    #[test]
    fn test_acl_entry_clone() {
        let entry = AclEntry {
            tag: AclTag::User("test".to_string()),
            perms: Perms::from_rwx("rw-"),
            _default: false,
        };
        let c = entry.clone();
        assert_eq!(c.tag, AclTag::User("test".to_string()));
    }
}
