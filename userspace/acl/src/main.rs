//! Slate OS POSIX access control list utility.
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
// `User` and `Group` are constructed only by `parse_acl_entry`, which is the
// extended-entry reader this build cannot reach yet. See its doc comment.
#[allow(dead_code)]
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
    /// Dead for the same reason as [`parse_acl_entry`], which is its only
    /// caller; see there.
    #[allow(dead_code)]
    fn from_rwx(s: &str) -> Self {
        Self {
            read: s.contains('r'),
            write: s.contains('w'),
            execute: s.contains('x'),
        }
    }

    fn to_rwx(self) -> String {
        format!(
            "{}{}{}",
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.execute { 'x' } else { '-' }
        )
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

/// Parse one `user:name:rwx` entry.
///
/// NOT CALLED BY THE PROGRAM, and kept deliberately. It is the other half of a
/// read this build cannot do: extended ACL entries live in extended
/// attributes, `posix` lists every xattr call as a stub, and this is the
/// function that would turn `user:www-data:r--` into an `AclEntry` the moment
/// one could be fetched. Its 18 tests describe the format precisely.
///
/// It went quiet when `setfacl` was deleted -- that command parsed entries in
/// order to apply them, and it applied nothing. Deleting this too would throw
/// away correct, tested code that the working version needs, so it is marked
/// rather than removed.
///
/// TRIGGER: when `getxattr` stops being a stub, call this from `read_file_acl`
/// and drop the allow.
#[allow(dead_code)]
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

/// The file's ownership and mode, or `None` where this build cannot ask.
///
/// Unix only. The host build exists to run the test suite and has no uid, gid
/// or mode to report; returning zeros there would be the same defect this
/// commit removes, one platform over.
#[cfg(unix)]
fn file_ids_and_mode(meta: &fs::Metadata) -> Option<(u32, u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.uid(), meta.gid(), meta.mode()))
}

#[cfg(not(unix))]
fn file_ids_and_mode(_meta: &fs::Metadata) -> Option<(u32, u32, u32)> {
    None
}

/// A uid as a name, or the number when the database cannot say.
///
/// Printing the number is what real getfacl does for an unresolvable id, and
/// it is true: the number was read from the inode.
fn uid_name(uid: u32) -> String {
    userdb::UserDb::load(userdb::DEFAULT_PATH)
        .ok()
        .and_then(|db| db.find_uid(uid).and_then(userdb::Record::username))
        .unwrap_or_else(|| uid.to_string())
}

/// The name of the group `gid`, or the number if nothing names it.
///
/// # What this replaced
///
/// The group field went through [`uid_name`], which reads the *user*
/// database: a file owned by group `adm` (gid 4) was reported as `lp`, the
/// user whose uid is 4. It looks right on any machine where each account has
/// a private group of the same number, which is most of them -- and it is
/// wrong for every system group, which is where ACLs are actually used.
///
/// `userdb` cannot answer this. It is SlateOS's own account store and holds a
/// user's primary gid, but no table of groups, so there is nothing in it to
/// map 4 to a name. Group names live in `/etc/group`, which is `pwdb`'s half
/// of the world, and an ACL group entry is a POSIX group by definition.
fn gid_name(gid: u32) -> String {
    gid_name_in(&pwdb::Db::load(), gid)
}

/// [`gid_name`] against a database the caller supplies, so it can be tested
/// without an `/etc/group` on the machine running the test.
fn gid_name_in(db: &pwdb::Db, gid: u32) -> String {
    db.group_by_gid(gid)
        .and_then(|g| String::from_utf8(g.name.clone()).ok())
        .unwrap_or_else(|| gid.to_string())
}

/// The access ACL of `path`, or `Err` saying why there is none to report.
///
/// # What this replaced
///
/// It called `fs::metadata` and DISCARDED the result -- `if let Ok(_meta)` --
/// then reported owner "root", group "root" and mode 0o755 for every file,
/// readable or not. On the failing branch it called `generate_default_acl`,
/// which added `user:www-data:r--`: a permissions claim naming a real service
/// account, about a file whose ownership had never been read. `getfacl` is
/// what somebody runs to audit who can reach a file.
///
/// # What it can honestly report
///
/// The three base entries -- `user::`, `group::`, `other::` -- are defined by
/// the file mode, which is real and readable. Extended entries live in
/// extended attributes, and `posix` lists every xattr call as a stub, so this
/// build cannot read them and does not pretend to: `has_extended` stays false
/// and the caller prints nothing rather than inventing an entry.
fn read_file_acl(path: &str) -> Result<FileAcl, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    let (uid, gid, mode) = file_ids_and_mode(&meta)
        .ok_or_else(|| format!("{path}: this build cannot read file ownership"))?;

    let access = vec![
        AclEntry {
            tag: AclTag::UserObj,
            perms: Perms::from_mode(mode, 6),
            _default: false,
        },
        AclEntry {
            tag: AclTag::GroupObj,
            perms: Perms::from_mode(mode, 3),
            _default: false,
        },
        AclEntry {
            tag: AclTag::Other,
            perms: Perms::from_mode(mode, 0),
            _default: false,
        },
    ];

    Ok(FileAcl {
        path: path.to_string(),
        owner: uid_name(uid),
        group: gid_name(gid),
        access,
        default: Vec::new(),
    })
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

/// Compute the path getfacl prints in the `# file:` header.
///
/// getfacl strips a single leading '/' from the displayed path so that
/// absolute paths render relative, unless `--absolute-names` was given (in
/// which case the path is shown verbatim). Paths without a leading '/' are
/// returned unchanged in both modes.
fn display_acl_path(path: &str, absolute: bool) -> &str {
    if absolute {
        path
    } else {
        path.strip_prefix('/').unwrap_or(path)
    }
}

fn print_file_acl(
    out: &mut io::StdoutLock<'_>,
    acl: &FileAcl,
    omit_header: bool,
    absolute: bool,
    tabular: bool,
) {
    if !omit_header {
        let display_path = display_acl_path(acl.path.as_str(), absolute);
        let _ = writeln!(out, "# file: {display_path}");
        let _ = writeln!(out, "# owner: {}", acl.owner);
        let _ = writeln!(out, "# group: {}", acl.group);
    }

    for entry in &acl.access {
        if tabular {
            let effective =
                if has_mask(&acl.access) && !matches!(entry.tag, AclTag::UserObj | AclTag::Other) {
                    let mask = get_mask_perms(&acl.access);
                    format!("\t#effective:{}", apply_mask(entry.perms, mask).to_rwx())
                } else {
                    String::new()
                };
            let _ = writeln!(
                out,
                "{}{}{effective}",
                format_acl_tag(&entry.tag),
                entry.perms.to_rwx()
            );
        } else {
            let _ = writeln!(
                out,
                "{}{}",
                format_acl_tag(&entry.tag),
                entry.perms.to_rwx()
            );
        }
    }

    if !acl.default.is_empty() {
        for entry in &acl.default {
            let _ = writeln!(
                out,
                "default:{}{}",
                format_acl_tag(&entry.tag),
                entry.perms.to_rwx()
            );
        }
    }

    let _ = writeln!(out);
}

fn has_mask(entries: &[AclEntry]) -> bool {
    entries.iter().any(|e| e.tag == AclTag::Mask)
}

fn get_mask_perms(entries: &[AclEntry]) -> Perms {
    entries
        .iter()
        .find(|e| e.tag == AclTag::Mask)
        .map(|e| e.perms)
        .unwrap_or(Perms {
            read: true,
            write: true,
            execute: true,
        })
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
    let mut failed = false;

    // A file whose ownership cannot be read has no ACL to report, and saying
    // so per file is what real getfacl does -- it continues to the next
    // operand and exits non-zero at the end.
    let show =
        |out: &mut io::StdoutLock<'_>, path: &str, failed: &mut bool| match read_file_acl(path) {
            Ok(acl) => print_file_acl(out, &acl, omit_header, absolute, tabular),
            Err(e) => {
                eprintln!("getfacl: {e}");
                *failed = true;
            }
        };

    for path in &paths {
        show(&mut out, path, &mut failed);

        if recursive && let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let child = entry.path().to_string_lossy().to_string();
                show(&mut out, &child, &mut failed);
            }
        }
    }

    if failed {
        process::exit(1);
    }
}

// ============================================================================
// setfacl command
// ============================================================================

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    // `setfacl` and `chacl` ARE GONE, under design-decisions 1006: a command
    // that does not work is deleted, not kept as a stub. They printed
    // "setfacl: removing all ACL entries from <path>", "modifying ...",
    // "recursing into <path>" -- and this crate contains no write of any kind.
    // Zero fs::write, File::create, set_permissions or setxattr in 793 lines.
    // Announcing a modification that did not happen is worse than refusing,
    // because the output is the only evidence anybody has.
    //
    // They cannot be implemented here yet either: ACL entries live in extended
    // attributes and `posix` lists every xattr call as a stub. The names come
    // back when there is something behind them.
    cmd_getfacl(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Naming a gid ──

    /// The exact confusion the group field used to make. On a real Debian
    /// system uid 4 is `lp` and gid 4 is `adm`; the two databases are
    /// unrelated and coincide only for private user groups.
    #[test]
    fn a_gid_is_named_from_the_group_file_not_the_user_file() {
        let passwd = b"root:x:0:0::/root:/bin/sh\nlp:x:4:7::/var/spool/lpd:/bin/false\n";
        let group = b"root:x:0:\nadm:x:4:alice\n";
        let db = pwdb::Db::from_bytes(passwd, group);
        assert_eq!(gid_name_in(&db, 4), "adm");
        // And the wrong answer is a different string, so this would have
        // failed against the old code rather than passing by coincidence.
        assert_ne!(gid_name_in(&db, 4), "lp");
    }

    #[test]
    fn an_unnamed_gid_reads_back_as_its_number() {
        let db = pwdb::Db::from_bytes(b"", b"root:x:0:\n");
        assert_eq!(gid_name_in(&db, 1234), "1234");
    }

    /// A group name is bytes and need not be UTF-8. Printing the number is
    /// honest; `from_utf8_lossy` would invent characters nobody chose.
    #[test]
    fn a_non_utf8_group_name_falls_back_to_the_number() {
        let db = pwdb::Db::from_bytes(b"", b"\xff\xfe:x:9:\n");
        assert_eq!(gid_name_in(&db, 9), "9");
    }

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
        let p = Perms {
            read: true,
            write: false,
            execute: true,
        };
        assert_eq!(p.to_rwx(), "r-x");
    }

    #[test]
    fn test_perms_to_rwx_all() {
        let p = Perms {
            read: true,
            write: true,
            execute: true,
        };
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
        assert_eq!(
            format_acl_tag(&AclTag::User("bob".to_string())),
            "user:bob:"
        );
        assert_eq!(format_acl_tag(&AclTag::GroupObj), "group::");
        assert_eq!(format_acl_tag(&AclTag::Mask), "mask::");
        assert_eq!(format_acl_tag(&AclTag::Other), "other::");
    }

    #[test]
    fn test_apply_mask() {
        let perms = Perms {
            read: true,
            write: true,
            execute: true,
        };
        let mask = Perms {
            read: true,
            write: false,
            execute: true,
        };
        let result = apply_mask(perms, mask);
        assert_eq!(result.to_rwx(), "r-x");
    }

    #[test]
    fn test_has_mask() {
        let entries = vec![
            AclEntry {
                tag: AclTag::UserObj,
                perms: Perms::from_rwx("rwx"),
                _default: false,
            },
            AclEntry {
                tag: AclTag::Mask,
                perms: Perms::from_rwx("r-x"),
                _default: false,
            },
        ];
        assert!(has_mask(&entries));
    }

    #[test]
    fn test_has_no_mask() {
        let entries = vec![AclEntry {
            tag: AclTag::UserObj,
            perms: Perms::from_rwx("rwx"),
            _default: false,
        }];
        assert!(!has_mask(&entries));
    }

    #[test]
    fn test_get_mask_perms() {
        let entries = vec![AclEntry {
            tag: AclTag::Mask,
            perms: Perms::from_rwx("r-x"),
            _default: false,
        }];
        let mask = get_mask_perms(&entries);
        assert_eq!(mask.to_rwx(), "r-x");
    }

    #[test]
    fn test_file_acl_clone() {
        let acl = FileAcl {
            path: "/test".to_string(),
            owner: "root".to_string(),
            group: "root".to_string(),
            access: vec![AclEntry {
                tag: AclTag::UserObj,
                perms: Perms::from_rwx("rwx"),
                _default: false,
            }],
            default: Vec::new(),
        };
        let c = acl.clone();
        assert_eq!(c.path, "/test");
    }

    #[test]
    fn test_perms_equality() {
        let a = Perms {
            read: true,
            write: false,
            execute: true,
        };
        let b = Perms {
            read: true,
            write: false,
            execute: true,
        };
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

    #[test]
    fn test_display_acl_path_strips_leading_slash() {
        // Default getfacl behavior: a single leading '/' is stripped.
        assert_eq!(display_acl_path("/etc/passwd", false), "etc/passwd");
        assert_eq!(display_acl_path("/", false), "");
    }

    #[test]
    fn test_display_acl_path_absolute_keeps_slash() {
        // --absolute-names: path shown verbatim.
        assert_eq!(display_acl_path("/etc/passwd", true), "/etc/passwd");
        assert_eq!(display_acl_path("/", true), "/");
    }

    #[test]
    fn test_display_acl_path_relative_unchanged() {
        // No leading '/': unchanged in both modes.
        assert_eq!(display_acl_path("etc/passwd", false), "etc/passwd");
        assert_eq!(display_acl_path("etc/passwd", true), "etc/passwd");
    }

    #[test]
    fn test_display_acl_path_strips_only_one_slash() {
        // Only the first leading '/' is removed (matches getfacl).
        assert_eq!(display_acl_path("//srv", false), "/srv");
        assert_eq!(display_acl_path("//srv", true), "//srv");
    }
}
