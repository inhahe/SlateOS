//! Slate OS User and Group Management Utilities
//!
//! Multi-personality binary providing POSIX-compatible user/group management:
//! useradd, userdel, usermod, groupadd, groupdel, groupmod, newgrp.
//!
//! # Where an account lives, and where a group lives
//!
//! Not in the same place, and the split is deliberate. `design-decisions.md`
//! §353 makes `/etc/users.yaml` the single truth about user accounts, with
//! `/etc/passwd` and `/etc/shadow` *generated* from it on every change for the
//! benefit of ported software that reads the flat files directly. So the three
//! user commands here go through `userdb` and never write those two files.
//!
//! They used to write them by hand, and that was the most damaging of the
//! defects §353 was decided to end: an account `useradd` created existed only
//! in `/etc/passwd`, so the next account change from *any* other tool
//! regenerated that file from a database the account was not in, and the
//! account silently ceased to exist.
//!
//! Groups are **not** part of §353. `/etc/group` and `/etc/gshadow` are still
//! parsed and written here directly, with atomic writes (write-to-temp then
//! rename) and a backup copy. Group *membership* is therefore recorded twice
//! -- in the group's member list and in each account's own `groups` list --
//! so no command below touches either list directly: the `Database` methods
//! that change membership change both. See `todo.txt` and `open-questions.md`
//! for the question of whether the group files should be generated too.
//!
//! # Personality Detection
//!
//! The tool inspects `argv[0]` basename to determine which command to run.
//!
//! # Usage
//!
//! ```text
//! useradd [options] LOGIN       Add a new user
//! userdel [options] LOGIN       Delete a user
//! usermod [options] LOGIN       Modify a user
//! groupadd [options] GROUP      Add a new group
//! groupdel GROUP                Delete a group
//! groupmod [options] GROUP      Modify a group
//! newgrp [GROUP]                Change effective group
//! ```

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Where the account database and the group files live.
///
/// A directory rather than four absolute paths, because every file involved is
/// found relative to it -- including the two `userdb` generates, which it
/// derives from the database's own directory precisely so that a test cannot
/// write over the real `/etc/passwd`.
const ETC_DIR: &str = "/etc";

/// The account database, `§353`'s single truth. Named relative to [`ETC_DIR`];
/// `userdb::DEFAULT_PATH` is the same file spelled absolutely.
const USERS_NAME: &str = "users.yaml";
const GROUP_NAME: &str = "group";
const GSHADOW_NAME: &str = "gshadow";

const SKEL_DIR: &str = "/etc/skel";
const DEFAULT_SHELL: &str = "/bin/sh";
const DEFAULT_HOME_BASE: &str = "/home";

/// System account UID/GID range.
const SYS_ID_MIN: u32 = 100;
const SYS_ID_MAX: u32 = 999;

/// Regular account UID/GID range.
const REG_ID_MIN: u32 = 1000;
const REG_ID_MAX: u32 = 60000;

// ============================================================================
// Data structures
// ============================================================================

// The `PasswdEntry` and `ShadowEntry` structs stood here, with their
// `parse`/`serialize` pairs and `ShadowEntry::new_locked`. They are gone:
// `design-decisions.md` §353 makes `/etc/passwd` and `/etc/shadow` *generated*
// output, so a struct that parsed one was parsing a rendering rather than the
// thing itself, and a struct that serialised one was writing a line the next
// account change would overwrite.
//
// The account a user *is* now lives in `/etc/users.yaml` as a `userdb::Record`,
// and the two flat files are produced from it by `userdb::UserDb::save`. The
// group half of this binary is unaffected: `/etc/group` and `/etc/gshadow` are
// outside §353 and are still parsed and written below.

/// An entry from `/etc/group`.
#[derive(Clone, Debug, PartialEq)]
struct GroupEntry {
    name: String,
    password: String,
    gid: u32,
    members: Vec<String>,
}

impl GroupEntry {
    fn serialize(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.name,
            self.password,
            self.gid,
            self.members.join(",")
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 4 {
            return None;
        }
        let members = if parts[3].is_empty() {
            Vec::new()
        } else {
            parts[3].split(',').map(|s| s.to_string()).collect()
        };
        Some(GroupEntry {
            name: parts[0].to_string(),
            password: parts[1].to_string(),
            gid: parts[2].parse().ok()?,
            members,
        })
    }
}

/// An entry from `/etc/gshadow`.
#[derive(Clone, Debug, PartialEq)]
struct GshadowEntry {
    name: String,
    password: String,
    admins: String,
    members: Vec<String>,
}

impl GshadowEntry {
    fn serialize(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.name,
            self.password,
            self.admins,
            self.members.join(",")
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 4 {
            return None;
        }
        let members = if parts[3].is_empty() {
            Vec::new()
        } else {
            parts[3].split(',').map(|s| s.to_string()).collect()
        };
        Some(GshadowEntry {
            name: parts[0].to_string(),
            password: parts[1].to_string(),
            admins: parts[2].to_string(),
            members,
        })
    }
}

// ============================================================================
// Database: the account database, plus the two group files
// ============================================================================

/// Everything these commands read and write, and the `/etc` it came from.
///
/// Two stores, not one, and the split is §353's: user accounts live in
/// `/etc/users.yaml` and are written through `userdb`, which regenerates
/// `/etc/passwd` and `/etc/shadow` from them; groups live in `/etc/group` and
/// `/etc/gshadow`, which §353 says nothing about and which are still written
/// here directly.
///
/// The directory is a field rather than a set of constants so that the tests
/// can run the real commands against a real `/etc` in a scratch directory.
/// `userdb::UserDb::save` derives the two generated files from the database's
/// own directory for the same reason, and the two must agree about which
/// directory that is -- so there is one.
struct Database {
    users: userdb::UserDb,
    groups: Vec<GroupEntry>,
    gshadow: Vec<GshadowEntry>,
    etc: std::path::PathBuf,
}

impl Database {
    /// Load the system's account database, from `/etc`.
    fn load() -> Self {
        Self::load_in(Path::new(ETC_DIR))
    }

    /// Load from `etc`. A missing file is an empty one -- which is right for
    /// the group files, and is `userdb::UserDb::load`'s own rule for a
    /// database that is not there yet.
    fn load_in(etc: &Path) -> Self {
        Database {
            users: userdb::UserDb::load(etc.join(USERS_NAME)).unwrap_or_default(),
            groups: Self::load_file(&etc.join(GROUP_NAME), GroupEntry::parse),
            gshadow: Self::load_file(&etc.join(GSHADOW_NAME), GshadowEntry::parse),
            etc: etc.to_path_buf(),
        }
    }

    fn load_file<T, F>(path: &Path, parser: F) -> Vec<T>
    where
        F: Fn(&str) -> Option<T>,
    {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        content.lines().filter_map(parser).collect()
    }

    /// Write everything back.
    ///
    /// The group files are written first, and the account database last,
    /// because the reference between the two runs one way: a user record names
    /// a primary gid, and no group names a user's uid. A failure between the
    /// two therefore leaves a group nobody is in -- which is inert -- rather
    /// than an account whose primary group does not exist.
    ///
    /// It is not one atomic operation across both stores. `userdb`'s own save
    /// stages its three files and renames them together; these two are a
    /// write-temp-and-rename each, as they have always been. Making the group
    /// files atomic with the accounts means generating them from a database
    /// too, which is a larger decision than §353 took -- see `todo.txt`.
    fn save(&self) -> Result<(), String> {
        Self::atomic_write(
            &self.etc.join(GROUP_NAME),
            &self.groups,
            GroupEntry::serialize,
        )?;
        Self::atomic_write(
            &self.etc.join(GSHADOW_NAME),
            &self.gshadow,
            GshadowEntry::serialize,
        )?;
        self.users
            .save(self.etc.join(USERS_NAME))
            .map_err(|e| format!("failed to write the account database: {e}"))
    }

    fn atomic_write<T, F>(path: &Path, entries: &[T], serializer: F) -> Result<(), String>
    where
        F: Fn(&T) -> String,
    {
        let tmp_path = path.with_extension("tmp");
        let backup_path = path.with_file_name(format!(
            "{}-",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));

        let mut content = String::new();
        for entry in entries {
            content.push_str(&serializer(entry));
            content.push('\n');
        }

        // Write to temporary file.
        fs::write(&tmp_path, &content)
            .map_err(|e| format!("failed to write {}: {}", tmp_path.display(), e))?;

        // Create backup of existing file (ignore errors if original doesn't exist).
        if path.exists() {
            let _ = fs::copy(path, &backup_path);
        }

        // Atomic rename.
        fs::rename(&tmp_path, path).map_err(|e| {
            format!(
                "failed to rename {} to {}: {}",
                tmp_path.display(),
                path.display(),
                e
            )
        })?;

        Ok(())
    }

    // ---- lookup helpers ----

    fn find_user(&self, name: &str) -> Option<&userdb::Record> {
        self.users.find(name)
    }

    fn find_user_by_uid(&self, uid: u32) -> Option<&userdb::Record> {
        self.users.find_uid(uid)
    }

    fn find_group(&self, name: &str) -> Option<&GroupEntry> {
        self.groups.iter().find(|g| g.name == name)
    }

    fn find_group_by_gid(&self, gid: u32) -> Option<&GroupEntry> {
        self.groups.iter().find(|g| g.gid == gid)
    }

    /// Next free UID in the given range.
    fn next_uid(&self, min: u32, max: u32) -> Option<u32> {
        let used: Vec<u32> = self
            .users
            .records()
            .iter()
            .filter_map(userdb::Record::uid)
            .collect();
        (min..=max).find(|id| !used.contains(id))
    }

    /// Next free GID in the given range.
    fn next_gid(&self, min: u32, max: u32) -> Option<u32> {
        let used: Vec<u32> = self.groups.iter().map(|g| g.gid).collect();
        (min..=max).find(|id| !used.contains(id))
    }

    // ---- Group membership, which is recorded in two places ----
    //
    // `/etc/group` has a member list per group, and the account record has a
    // `groups` list per user. They are the same fact written twice, which is
    // the shape of every defect §330 and §353 were decided to end -- so no
    // command below touches either list directly. Every change goes through
    // one of these, which change both, and a caller therefore cannot update
    // one store and forget the other.

    /// Add `username` to `group`, in both places membership is recorded.
    fn add_to_group(&mut self, username: &str, group: &str) {
        if let Some(ge) = self.groups.iter_mut().find(|g| g.name == group)
            && !ge.members.iter().any(|m| m == username)
        {
            ge.members.push(username.to_string());
        }
        if let Some(gs) = self.gshadow.iter_mut().find(|g| g.name == group)
            && !gs.members.iter().any(|m| m == username)
        {
            gs.members.push(username.to_string());
        }
        if let Some(record) = self.users.find_mut(username) {
            let mut names = record.groups();
            if !names.iter().any(|g| g == group) {
                names.push(group.to_string());
                record.set_groups(&names);
            }
        }
    }

    /// Remove a user from every group, in both places.
    fn remove_user_from_groups(&mut self, username: &str) {
        for group in &mut self.groups {
            group.members.retain(|m| m != username);
        }
        for gs in &mut self.gshadow {
            gs.members.retain(|m| m != username);
        }
        if let Some(record) = self.users.find_mut(username) {
            record.set_groups(&[]);
        }
    }

    /// Rename a user wherever it is recorded as a member.
    ///
    /// The account record's own `groups` list needs no rename -- it holds
    /// group names, not the user's -- but the group files hold the user's name
    /// in every group it belongs to.
    fn rename_user_in_groups(&mut self, old_name: &str, new_name: &str) {
        for group in &mut self.groups {
            for m in &mut group.members {
                if m == old_name {
                    *m = new_name.to_string();
                }
            }
        }
        for gs in &mut self.gshadow {
            for m in &mut gs.members {
                if m == old_name {
                    *m = new_name.to_string();
                }
            }
        }
    }

    /// Rename a group wherever it is recorded, including in every account that
    /// belongs to it. A rename that missed the accounts would leave them
    /// naming a group that no longer exists.
    fn rename_group_everywhere(&mut self, old_name: &str, new_name: &str) {
        if let Some(gs) = self.gshadow.iter_mut().find(|g| g.name == old_name) {
            gs.name = new_name.to_string();
        }
        for record in self.users.records_mut() {
            let mut names = record.groups();
            if names.iter().any(|g| g == old_name) {
                for name in &mut names {
                    if name == old_name {
                        *name = new_name.to_string();
                    }
                }
                record.set_groups(&names);
            }
        }
    }

    /// Forget a group everywhere it is recorded as a membership.
    fn forget_group(&mut self, group: &str) {
        self.groups.retain(|g| g.name != group);
        self.gshadow.retain(|g| g.name != group);
        for record in self.users.records_mut() {
            let names = record.groups();
            if names.iter().any(|g| g == group) {
                let kept: Vec<String> = names.into_iter().filter(|g| g != group).collect();
                record.set_groups(&kept);
            }
        }
    }
}

/// The group a record counts as being in primarily.
///
/// A record with no `gid` is generated into `/etc/passwd` with its uid in the
/// gid column -- the user-private-group convention -- so that is the group it
/// is in, and a question about primary groups has to ask it the same way the
/// generated file answers it. Asking `Record::gid` alone would let `groupdel`
/// delete a group that is somebody's primary one.
fn primary_gid(record: &userdb::Record) -> Option<u32> {
    record.gid().or_else(|| record.uid())
}

// ============================================================================
// Account expiry dates
// ============================================================================

/// Parse a `-e` argument into days since the Unix epoch.
///
/// `/etc/shadow`'s eighth field is a *number* of days. The old code copied the
/// `-e` argument into it verbatim, so `useradd -e 2027-01-01` wrote the text
/// `2027-01-01` where a number belongs: glibc reads that as no expiry at all,
/// and the account the administrator meant to time-limit never expired. Both
/// spellings shadow-utils accepts are taken here and both are converted.
///
/// An empty argument clears the field, which is how `usermod -e ""` spells
/// "never expires". So does a negative number, for the reason the aging
/// commands take `-1`: a literal `-1` left in the file is read as a date one
/// day *before* the epoch, which is the opposite of never.
///
/// # Errors
///
/// A string that is neither a number nor a valid `YYYY-MM-DD` date, since the
/// alternative is storing something that will be read as a date nobody chose.
fn parse_expire_date(text: &str) -> Result<Option<i64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Ok(days) = text.parse::<i64>() {
        return Ok(if days < 0 { None } else { Some(days) });
    }
    userdb::days_from_date(text)
        .map(Some)
        .ok_or_else(|| format!("invalid date `{text}': expected YYYY-MM-DD or a number of days"))
}

// ============================================================================
// Validation helpers
// ============================================================================

fn validate_username(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if name.len() > 32 {
        return Err("username too long (max 32 characters)".to_string());
    }
    let first = name.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first == b'_') {
        return Err("username must start with a lowercase letter or underscore".to_string());
    }
    for ch in name.chars() {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' || ch == '.')
        {
            return Err(format!(
                "invalid character '{}' in username (allowed: a-z, 0-9, _, -, .)",
                ch
            ));
        }
    }
    Ok(())
}

fn validate_groupname(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("group name cannot be empty".to_string());
    }
    if name.len() > 32 {
        return Err("group name too long (max 32 characters)".to_string());
    }
    for ch in name.chars() {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' || ch == '.')
        {
            return Err(format!(
                "invalid character '{}' in group name (allowed: a-z, 0-9, _, -, .)",
                ch
            ));
        }
    }
    Ok(())
}

// ============================================================================
// Home directory / skeleton management
// ============================================================================

fn create_home_dir(home: &str, skel: &str, uid: u32, gid: u32) -> Result<(), String> {
    // Create the home directory.
    fs::create_dir_all(home).map_err(|e| format!("failed to create {}: {}", home, e))?;

    // Copy skeleton contents if skeleton dir exists.
    if Path::new(skel).is_dir() {
        copy_dir_recursive(skel, home)
            .map_err(|e| format!("failed to copy skel {}: {}", skel, e))?;
    }

    // Set ownership via syscall (chown equivalent).
    set_ownership(home, uid, gid);
    Ok(())
}

fn copy_dir_recursive(src: &str, dst: &str) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let file_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let dst_path = format!("{}/{}", dst, file_name);

        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path.to_string_lossy(), &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn remove_home_dir(home: &str) -> Result<(), String> {
    if Path::new(home).exists() {
        fs::remove_dir_all(home).map_err(|e| format!("failed to remove {}: {}", home, e))?;
    }
    Ok(())
}

/// Set ownership on a path. On our OS this would be a syscall; here we attempt
/// a chown-like call. Non-fatal on failure so tools don't break in test envs.
fn set_ownership(_path: &str, _uid: u32, _gid: u32) {
    // On Slate OS, this would invoke SYS_CHOWN. In the current build
    // environment we skip actual chown since it requires kernel support.
    // The directory was already created with the process's credentials.
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

struct Args {
    args: Vec<String>,
    pos: usize,
}

impl Args {
    fn new(args: Vec<String>) -> Self {
        Args { args, pos: 0 }
    }

    /// Get the next argument value for a flag (e.g., -d /home/foo).
    fn next_value(&mut self) -> Option<String> {
        self.pos += 1;
        if self.pos < self.args.len() {
            Some(self.args[self.pos].clone())
        } else {
            None
        }
    }

    fn current(&self) -> Option<&str> {
        if self.pos < self.args.len() {
            Some(&self.args[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

// ============================================================================
// Output helpers
// ============================================================================

fn write_stdout(msg: &str) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(msg.as_bytes());
    let _ = handle.write_all(b"\n");
}

fn write_stderr(msg: &str) {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(msg.as_bytes());
    let _ = handle.write_all(b"\n");
}

fn die(prog: &str, msg: &str) -> ! {
    write_stderr(&format!("{}: {}", prog, msg));
    process::exit(1);
}

// ============================================================================
// useradd
// ============================================================================

struct UseraddOpts {
    create_home: bool,
    home_dir: Option<String>,
    shell: Option<String>,
    primary_group: Option<String>,
    supp_groups: Vec<String>,
    uid: Option<u32>,
    comment: Option<String>,
    expire_date: Option<String>,
    password: Option<String>,
    system_account: bool,
    skel_dir: Option<String>,
    username: Option<String>,
}

impl UseraddOpts {
    fn new() -> Self {
        UseraddOpts {
            create_home: false,
            home_dir: None,
            shell: None,
            primary_group: None,
            supp_groups: Vec::new(),
            uid: None,
            comment: None,
            expire_date: None,
            password: None,
            system_account: false,
            skel_dir: None,
            username: None,
        }
    }
}

fn parse_useradd_args(argv: &[String]) -> UseraddOpts {
    let mut opts = UseraddOpts::new();
    let mut args = Args::new(argv.to_vec());

    while let Some(arg) = args.current() {
        match arg {
            "-m" => opts.create_home = true,
            "-r" => opts.system_account = true,
            "-d" => {
                opts.home_dir = args.next_value();
                if opts.home_dir.is_none() {
                    die("useradd", "option -d requires an argument");
                }
            }
            "-s" => {
                opts.shell = args.next_value();
                if opts.shell.is_none() {
                    die("useradd", "option -s requires an argument");
                }
            }
            "-g" => {
                opts.primary_group = args.next_value();
                if opts.primary_group.is_none() {
                    die("useradd", "option -g requires an argument");
                }
            }
            "-G" => {
                let val = args.next_value();
                match val {
                    Some(v) => {
                        opts.supp_groups = v.split(',').map(|s| s.to_string()).collect();
                    }
                    None => die("useradd", "option -G requires an argument"),
                }
            }
            "-u" => {
                let val = args.next_value();
                match val {
                    Some(v) => match v.parse::<u32>() {
                        Ok(id) => opts.uid = Some(id),
                        Err(_) => die("useradd", &format!("invalid UID: {}", v)),
                    },
                    None => die("useradd", "option -u requires an argument"),
                }
            }
            "-c" => {
                opts.comment = args.next_value();
                if opts.comment.is_none() {
                    die("useradd", "option -c requires an argument");
                }
            }
            "-e" => {
                opts.expire_date = args.next_value();
                if opts.expire_date.is_none() {
                    die("useradd", "option -e requires an argument");
                }
            }
            "-p" => {
                opts.password = args.next_value();
                if opts.password.is_none() {
                    die("useradd", "option -p requires an argument");
                }
            }
            "-k" => {
                opts.skel_dir = args.next_value();
                if opts.skel_dir.is_none() {
                    die("useradd", "option -k requires an argument");
                }
            }
            _ => {
                if arg.starts_with('-') {
                    die("useradd", &format!("unknown option: {}", arg));
                }
                opts.username = Some(arg.to_string());
            }
        }
        args.advance();
    }
    opts
}

fn cmd_useradd(argv: &[String]) -> i32 {
    let opts = parse_useradd_args(argv);

    let username = match &opts.username {
        Some(u) => u.clone(),
        None => {
            write_stderr("useradd: missing username");
            write_stderr("Usage: useradd [options] LOGIN");
            return 1;
        }
    };

    if let Err(e) = validate_username(&username) {
        write_stderr(&format!("useradd: {}", e));
        return 1;
    }

    // Parsed before anything is built, so a malformed date fails the command
    // rather than half of it.
    let expires = match opts.expire_date.as_deref().map(parse_expire_date) {
        Some(Ok(days)) => days,
        Some(Err(e)) => {
            write_stderr(&format!("useradd: {}", e));
            return 1;
        }
        None => None,
    };

    let mut db = Database::load();

    // Check for duplicates.
    if db.find_user(&username).is_some() {
        write_stderr(&format!("useradd: user '{}' already exists", username));
        return 1;
    }

    // Determine UID.
    let (id_min, id_max) = if opts.system_account {
        (SYS_ID_MIN, SYS_ID_MAX)
    } else {
        (REG_ID_MIN, REG_ID_MAX)
    };

    let uid = match opts.uid {
        Some(u) => {
            if db.find_user_by_uid(u).is_some() {
                write_stderr(&format!("useradd: UID {} already in use", u));
                return 1;
            }
            u
        }
        None => match db.next_uid(id_min, id_max) {
            Some(u) => u,
            None => {
                write_stderr("useradd: no available UIDs");
                return 1;
            }
        },
    };

    // Determine primary group.
    let gid = match &opts.primary_group {
        Some(g) => {
            // Group specified by name or GID.
            match g.parse::<u32>() {
                Ok(id) => {
                    if db.find_group_by_gid(id).is_none() {
                        write_stderr(&format!("useradd: group GID {} does not exist", id));
                        return 1;
                    }
                    id
                }
                Err(_) => match db.find_group(g) {
                    Some(ge) => ge.gid,
                    None => {
                        write_stderr(&format!("useradd: group '{}' does not exist", g));
                        return 1;
                    }
                },
            }
        }
        None => {
            // Create a group with the same name as the user (User Private Group scheme).
            let new_gid = match db.next_gid(id_min, id_max) {
                Some(g) => g,
                None => {
                    write_stderr("useradd: no available GIDs");
                    return 1;
                }
            };
            db.groups.push(GroupEntry {
                name: username.clone(),
                password: "x".to_string(),
                gid: new_gid,
                members: Vec::new(),
            });
            db.gshadow.push(GshadowEntry {
                name: username.clone(),
                password: "!".to_string(),
                admins: String::new(),
                members: Vec::new(),
            });
            new_gid
        }
    };

    let home = opts
        .home_dir
        .unwrap_or_else(|| format!("{}/{}", DEFAULT_HOME_BASE, username));
    let shell = opts.shell.unwrap_or_else(|| DEFAULT_SHELL.to_string());
    let gecos = opts.comment.unwrap_or_default();

    // Build the account record.
    let mut record = userdb::Record::new();
    record.set(userdb::field::USERNAME, &username);
    record.set_uid(uid);
    record.set_gid(gid);
    // Only when there is one: an empty `display_name` and an absent one both
    // generate an empty GECOS column, so writing the field would be a line in
    // the file that says nothing.
    if !gecos.is_empty() {
        record.set(userdb::field::DISPLAY_NAME, &gecos);
    }
    record.set_home(&home);
    record.set(userdb::field::SHELL, &shell);

    match &opts.password {
        // `-p` takes an already-encrypted entry, so it is stored as given --
        // this is the one path that must *not* hash, because hashing a hash
        // produces an account whose password is a string nobody typed. The
        // change is dated today for the reason `userdb::Record::set_password`
        // dates its own: a password whose age is unknown is one no aging
        // policy can ever act on.
        Some(pw) => {
            record.set(userdb::field::PASSWORD_HASH, pw);
            let aging = userdb::Aging {
                changed: userdb::today(),
                ..userdb::Aging::default()
            };
            record.set_aging(&aging);
        }
        // No password given: the account is locked until someone sets one,
        // which is what `useradd` has always done and what stops a new
        // account being one that logs in without being asked for anything.
        None => record.set_locked(true),
    }

    // No aging policy is written beyond that. The `0:99999:7` the old code
    // wrote here was shadow-utils' `/etc/login.defs` defaults, which this
    // system does not have -- and they mean exactly what leaving the fields
    // empty means: no minimum age, no expiry, and a warning period that only
    // matters if there were an expiry. Writing them would present an
    // invention as a policy an administrator had chosen.

    if let Some(days) = expires {
        let aging = userdb::Aging {
            expires: Some(days),
            ..record.aging()
        };
        record.set_aging(&aging);
    }

    db.users.push(record);

    // Add user to supplementary groups.
    for gname in &opts.supp_groups {
        if db.find_group(gname).is_none() {
            write_stderr(&format!("useradd: group '{}' does not exist", gname));
            return 1;
        }
        db.add_to_group(&username, gname);
    }

    // Save database.
    if let Err(e) = db.save() {
        write_stderr(&format!("useradd: {}", e));
        return 1;
    }

    // Create home directory if requested.
    if opts.create_home {
        let skel = opts.skel_dir.unwrap_or_else(|| SKEL_DIR.to_string());
        if let Err(e) = create_home_dir(&home, &skel, uid, gid) {
            write_stderr(&format!("useradd: {}", e));
            return 1;
        }
    }

    0
}

// ============================================================================
// userdel
// ============================================================================

struct UserdelOpts {
    remove_home: bool,
    force: bool,
    username: Option<String>,
}

fn parse_userdel_args(argv: &[String]) -> UserdelOpts {
    let mut opts = UserdelOpts {
        remove_home: false,
        force: false,
        username: None,
    };
    let mut args = Args::new(argv.to_vec());

    while let Some(arg) = args.current() {
        match arg {
            "-r" => opts.remove_home = true,
            "-f" => opts.force = true,
            _ => {
                if arg.starts_with('-') {
                    die("userdel", &format!("unknown option: {}", arg));
                }
                opts.username = Some(arg.to_string());
            }
        }
        args.advance();
    }
    opts
}

fn cmd_userdel(argv: &[String]) -> i32 {
    let opts = parse_userdel_args(argv);

    let username = match &opts.username {
        Some(u) => u.clone(),
        None => {
            write_stderr("userdel: missing username");
            write_stderr("Usage: userdel [options] LOGIN");
            return 1;
        }
    };

    let mut db = Database::load();

    // The home directory is read before the record goes, because removing it
    // is the last thing this command does and by then there is nothing left to
    // ask.
    let home = match db.find_user(&username) {
        Some(record) => record.home().unwrap_or_default(),
        None => {
            if opts.force {
                return 0;
            }
            write_stderr(&format!("userdel: user '{}' does not exist", username));
            return 1;
        }
    };

    // Remove from every group first, while the record is still there to have
    // its own membership list cleared, and only then remove the account.
    db.remove_user_from_groups(&username);
    db.users.remove(&username);

    // Remove the user's private group if it exists and has no other members.
    let private_group_empty = db
        .groups
        .iter()
        .find(|g| g.name == username)
        .map(|g| g.members.is_empty())
        .unwrap_or(false);
    if private_group_empty {
        db.forget_group(&username);
    }

    if let Err(e) = db.save() {
        write_stderr(&format!("userdel: {}", e));
        return 1;
    }

    // Remove home directory if requested.
    if opts.remove_home
        && !home.is_empty()
        && let Err(e) = remove_home_dir(&home)
    {
        write_stderr(&format!("userdel: warning: {}", e));
        // Not fatal, user was already deleted.
    }

    0
}

// ============================================================================
// usermod
// ============================================================================

struct UsermodOpts {
    new_login: Option<String>,
    home_dir: Option<String>,
    move_home: bool,
    shell: Option<String>,
    primary_group: Option<String>,
    supp_groups: Vec<String>,
    supp_groups_set: bool, // true if -G was given at all
    append_groups: bool,
    lock: bool,
    unlock: bool,
    expire_date: Option<String>,
    comment: Option<String>,
    username: Option<String>,
}

impl UsermodOpts {
    fn new() -> Self {
        UsermodOpts {
            new_login: None,
            home_dir: None,
            move_home: false,
            shell: None,
            primary_group: None,
            supp_groups: Vec::new(),
            supp_groups_set: false,
            append_groups: false,
            lock: false,
            unlock: false,
            expire_date: None,
            comment: None,
            username: None,
        }
    }
}

fn parse_usermod_args(argv: &[String]) -> UsermodOpts {
    let mut opts = UsermodOpts::new();
    let mut args = Args::new(argv.to_vec());

    while let Some(arg) = args.current() {
        match arg {
            "-m" => opts.move_home = true,
            "-a" => opts.append_groups = true,
            "-L" => opts.lock = true,
            "-U" => opts.unlock = true,
            "-l" => {
                opts.new_login = args.next_value();
                if opts.new_login.is_none() {
                    die("usermod", "option -l requires an argument");
                }
            }
            "-d" => {
                opts.home_dir = args.next_value();
                if opts.home_dir.is_none() {
                    die("usermod", "option -d requires an argument");
                }
            }
            "-s" => {
                opts.shell = args.next_value();
                if opts.shell.is_none() {
                    die("usermod", "option -s requires an argument");
                }
            }
            "-g" => {
                opts.primary_group = args.next_value();
                if opts.primary_group.is_none() {
                    die("usermod", "option -g requires an argument");
                }
            }
            "-G" => {
                let val = args.next_value();
                match val {
                    Some(v) => {
                        opts.supp_groups = v.split(',').map(|s| s.to_string()).collect();
                        opts.supp_groups_set = true;
                    }
                    None => die("usermod", "option -G requires an argument"),
                }
            }
            "-e" => {
                opts.expire_date = args.next_value();
                if opts.expire_date.is_none() {
                    die("usermod", "option -e requires an argument");
                }
            }
            "-c" => {
                opts.comment = args.next_value();
                if opts.comment.is_none() {
                    die("usermod", "option -c requires an argument");
                }
            }
            _ => {
                if arg.starts_with('-') {
                    die("usermod", &format!("unknown option: {}", arg));
                }
                opts.username = Some(arg.to_string());
            }
        }
        args.advance();
    }
    opts
}

fn cmd_usermod(argv: &[String]) -> i32 {
    let opts = parse_usermod_args(argv);

    let username = match &opts.username {
        Some(u) => u.clone(),
        None => {
            write_stderr("usermod: missing username");
            write_stderr("Usage: usermod [options] LOGIN");
            return 1;
        }
    };

    // Asking for both is a contradiction, and doing the last one asked would
    // be picking a winner the caller did not name. shadow-utils refuses it too.
    if opts.lock && opts.unlock {
        write_stderr("usermod: -L and -U cannot be given together");
        return 1;
    }

    // Parsed before anything is changed, so a malformed date fails the command
    // rather than half of it.
    let expires = match opts.expire_date.as_deref().map(parse_expire_date) {
        Some(Ok(days)) => Some(days),
        Some(Err(e)) => {
            write_stderr(&format!("usermod: {}", e));
            return 1;
        }
        None => None,
    };

    let mut db = Database::load();

    // Find the user.
    if db.find_user(&username).is_none() {
        write_stderr(&format!("usermod: user '{}' does not exist", username));
        return 1;
    }

    // Validate new login name if provided.
    if let Some(ref new_name) = opts.new_login {
        if let Err(e) = validate_username(new_name) {
            write_stderr(&format!("usermod: {}", e));
            return 1;
        }
        if new_name != &username && db.find_user(new_name).is_some() {
            write_stderr(&format!("usermod: user '{}' already exists", new_name));
            return 1;
        }
    }

    // Resolve the new primary group before any change is applied, for the same
    // reason the date is parsed early: a `-g` naming a group that does not
    // exist must leave the account exactly as it was, not renamed and then
    // refused.
    let new_gid = match &opts.primary_group {
        Some(g) => match g.parse::<u32>() {
            Ok(id) => {
                if db.find_group_by_gid(id).is_none() {
                    write_stderr(&format!("usermod: group GID {} does not exist", id));
                    return 1;
                }
                Some(id)
            }
            Err(_) => match db.find_group(g) {
                Some(ge) => Some(ge.gid),
                None => {
                    write_stderr(&format!("usermod: group '{}' does not exist", g));
                    return 1;
                }
            },
        },
        None => None,
    };

    // Every supplementary group must exist before any of them is joined.
    if opts.supp_groups_set {
        for gname in &opts.supp_groups {
            if db.find_group(gname).is_none() {
                write_stderr(&format!("usermod: group '{}' does not exist", gname));
                return 1;
            }
        }
    }

    let old_home = db
        .find_user(&username)
        .and_then(userdb::Record::home)
        .unwrap_or_default();

    // Renaming touches the group files as well as the record, so it is done
    // through the pair that changes both.
    if let Some(ref new_name) = opts.new_login {
        db.rename_user_in_groups(&username, new_name);
    }

    let Some(record) = db.users.find_mut(&username) else {
        write_stderr(&format!("usermod: user '{}' does not exist", username));
        return 1;
    };

    if let Some(ref new_name) = opts.new_login {
        record.set(userdb::field::USERNAME, new_name);
    }
    if let Some(ref home) = opts.home_dir {
        record.set_home(home);
    }
    if let Some(ref shell) = opts.shell {
        record.set(userdb::field::SHELL, shell);
    }
    if let Some(ref comment) = opts.comment {
        record.set(userdb::field::DISPLAY_NAME, comment);
    }
    if let Some(gid) = new_gid {
        record.set_gid(gid);
    }

    // Supplementary groups.
    let effective_username = opts.new_login.as_deref().unwrap_or(&username).to_string();

    if opts.supp_groups_set {
        // Replace mode empties the memberships first; append mode keeps them.
        // Both then join the listed groups, which were checked to exist above.
        if !opts.append_groups {
            db.remove_user_from_groups(&effective_username);
        }
        for gname in &opts.supp_groups {
            db.add_to_group(&effective_username, gname);
        }
    }

    let Some(record) = db.users.find_mut(&effective_username) else {
        write_stderr(&format!(
            "usermod: user '{}' does not exist",
            effective_username
        ));
        return 1;
    };

    // Lock and unlock. `-L` and `-U` are `set_locked`'s two arguments, which
    // is also what makes `-U` on an account with no password underneath leave
    // it locked rather than passwordless -- see `userdb::Record::set_locked`.
    if opts.lock {
        record.set_locked(true);
    }
    if opts.unlock {
        record.set_locked(false);
    }

    // Expire date. `Some(None)` is `-e ""`, which clears it.
    if let Some(days) = expires {
        let aging = userdb::Aging {
            expires: days,
            ..record.aging()
        };
        record.set_aging(&aging);
    }

    if let Err(e) = db.save() {
        write_stderr(&format!("usermod: {}", e));
        return 1;
    }

    // Move home directory if requested.
    if opts.move_home
        && let Some(ref new_home) = opts.home_dir
        && Path::new(&old_home).exists()
        && old_home != *new_home
        && let Err(e) = fs::rename(&old_home, new_home)
    {
        write_stderr(&format!(
            "usermod: failed to move {} to {}: {}",
            old_home, new_home, e
        ));
        return 1;
    }

    0
}

// ============================================================================
// groupadd
// ============================================================================

struct GroupaddOpts {
    gid: Option<u32>,
    system_group: bool,
    force: bool,
    groupname: Option<String>,
}

fn parse_groupadd_args(argv: &[String]) -> GroupaddOpts {
    let mut opts = GroupaddOpts {
        gid: None,
        system_group: false,
        force: false,
        groupname: None,
    };
    let mut args = Args::new(argv.to_vec());

    while let Some(arg) = args.current() {
        match arg {
            "-r" => opts.system_group = true,
            "-f" => opts.force = true,
            "-g" => {
                let val = args.next_value();
                match val {
                    Some(v) => match v.parse::<u32>() {
                        Ok(id) => opts.gid = Some(id),
                        Err(_) => die("groupadd", &format!("invalid GID: {}", v)),
                    },
                    None => die("groupadd", "option -g requires an argument"),
                }
            }
            _ => {
                if arg.starts_with('-') {
                    die("groupadd", &format!("unknown option: {}", arg));
                }
                opts.groupname = Some(arg.to_string());
            }
        }
        args.advance();
    }
    opts
}

fn cmd_groupadd(argv: &[String]) -> i32 {
    let opts = parse_groupadd_args(argv);

    let groupname = match &opts.groupname {
        Some(g) => g.clone(),
        None => {
            write_stderr("groupadd: missing group name");
            write_stderr("Usage: groupadd [options] GROUP");
            return 1;
        }
    };

    if let Err(e) = validate_groupname(&groupname) {
        write_stderr(&format!("groupadd: {}", e));
        return 1;
    }

    let mut db = Database::load();

    // Check for duplicate.
    if db.find_group(&groupname).is_some() {
        if opts.force {
            return 0;
        }
        write_stderr(&format!("groupadd: group '{}' already exists", groupname));
        return 1;
    }

    // Determine GID.
    let (id_min, id_max) = if opts.system_group {
        (SYS_ID_MIN, SYS_ID_MAX)
    } else {
        (REG_ID_MIN, REG_ID_MAX)
    };

    let gid = match opts.gid {
        Some(g) => {
            if db.find_group_by_gid(g).is_some() {
                if opts.force {
                    // Force: find next available.
                    match db.next_gid(id_min, id_max) {
                        Some(ng) => ng,
                        None => {
                            write_stderr("groupadd: no available GIDs");
                            return 1;
                        }
                    }
                } else {
                    write_stderr(&format!("groupadd: GID {} already in use", g));
                    return 1;
                }
            } else {
                g
            }
        }
        None => match db.next_gid(id_min, id_max) {
            Some(g) => g,
            None => {
                write_stderr("groupadd: no available GIDs");
                return 1;
            }
        },
    };

    db.groups.push(GroupEntry {
        name: groupname.clone(),
        password: "x".to_string(),
        gid,
        members: Vec::new(),
    });

    db.gshadow.push(GshadowEntry {
        name: groupname.clone(),
        password: "!".to_string(),
        admins: String::new(),
        members: Vec::new(),
    });

    if let Err(e) = db.save() {
        write_stderr(&format!("groupadd: {}", e));
        return 1;
    }

    0
}

// ============================================================================
// groupdel
// ============================================================================

fn cmd_groupdel(argv: &[String]) -> i32 {
    let groupname = match argv.first() {
        Some(g) => g.clone(),
        None => {
            write_stderr("groupdel: missing group name");
            write_stderr("Usage: groupdel GROUP");
            return 1;
        }
    };

    let mut db = Database::load();

    // Check group exists.
    let group = match db.find_group(&groupname) {
        Some(g) => g.clone(),
        None => {
            write_stderr(&format!("groupdel: group '{}' does not exist", groupname));
            return 1;
        }
    };

    // Cannot remove a group that is the primary group of any user.
    let primary_users: Vec<String> = db
        .users
        .records()
        .iter()
        .filter(|r| primary_gid(r) == Some(group.gid))
        .filter_map(userdb::Record::username)
        .collect();

    if !primary_users.is_empty() {
        write_stderr(&format!(
            "groupdel: cannot remove group '{}': primary group of user(s): {}",
            groupname,
            primary_users.join(", ")
        ));
        return 1;
    }

    db.forget_group(&groupname);

    if let Err(e) = db.save() {
        write_stderr(&format!("groupdel: {}", e));
        return 1;
    }

    0
}

// ============================================================================
// groupmod
// ============================================================================

struct GroupmodOpts {
    new_name: Option<String>,
    new_gid: Option<u32>,
    groupname: Option<String>,
}

fn parse_groupmod_args(argv: &[String]) -> GroupmodOpts {
    let mut opts = GroupmodOpts {
        new_name: None,
        new_gid: None,
        groupname: None,
    };
    let mut args = Args::new(argv.to_vec());

    while let Some(arg) = args.current() {
        match arg {
            "-n" => {
                opts.new_name = args.next_value();
                if opts.new_name.is_none() {
                    die("groupmod", "option -n requires an argument");
                }
            }
            "-g" => {
                let val = args.next_value();
                match val {
                    Some(v) => match v.parse::<u32>() {
                        Ok(id) => opts.new_gid = Some(id),
                        Err(_) => die("groupmod", &format!("invalid GID: {}", v)),
                    },
                    None => die("groupmod", "option -g requires an argument"),
                }
            }
            _ => {
                if arg.starts_with('-') {
                    die("groupmod", &format!("unknown option: {}", arg));
                }
                opts.groupname = Some(arg.to_string());
            }
        }
        args.advance();
    }
    opts
}

fn cmd_groupmod(argv: &[String]) -> i32 {
    let opts = parse_groupmod_args(argv);

    let groupname = match &opts.groupname {
        Some(g) => g.clone(),
        None => {
            write_stderr("groupmod: missing group name");
            write_stderr("Usage: groupmod [options] GROUP");
            return 1;
        }
    };

    let mut db = Database::load();

    let group_idx = match db.groups.iter().position(|g| g.name == groupname) {
        Some(i) => i,
        None => {
            write_stderr(&format!("groupmod: group '{}' does not exist", groupname));
            return 1;
        }
    };

    // Validate new name.
    if let Some(ref new_name) = opts.new_name {
        if let Err(e) = validate_groupname(new_name) {
            write_stderr(&format!("groupmod: {}", e));
            return 1;
        }
        if new_name != &groupname && db.find_group(new_name).is_some() {
            write_stderr(&format!("groupmod: group '{}' already exists", new_name));
            return 1;
        }
    }

    // Validate new GID.
    if let Some(new_gid) = opts.new_gid
        && let Some(existing) = db.find_group_by_gid(new_gid)
        && existing.name != groupname
    {
        write_stderr(&format!("groupmod: GID {} already in use", new_gid));
        return 1;
    }

    let old_gid = db.groups[group_idx].gid;

    // Apply GID change.
    if let Some(new_gid) = opts.new_gid {
        db.groups[group_idx].gid = new_gid;
        // Update all users whose primary GID matches. A record with no `gid`
        // of its own is in the group numbered after its uid, so it is caught
        // here too and gains an explicit one -- the alternative is a user
        // whose primary group silently stops being the group it was in.
        for record in db.users.records_mut() {
            if primary_gid(record) == Some(old_gid) {
                record.set_gid(new_gid);
            }
        }
    }

    // Apply name change, in the group files and in every account that names
    // the group.
    if let Some(ref new_name) = opts.new_name {
        let old_name = db.groups[group_idx].name.clone();
        db.groups[group_idx].name = new_name.clone();
        db.rename_group_everywhere(&old_name, new_name);
    }

    if let Err(e) = db.save() {
        write_stderr(&format!("groupmod: {}", e));
        return 1;
    }

    0
}

// ============================================================================
// newgrp
// ============================================================================

fn cmd_newgrp(argv: &[String]) -> i32 {
    let groupname = match argv.first() {
        Some(g) => g.clone(),
        None => {
            // No group specified: reset to user's default group.
            write_stdout("newgrp: resetting to default group");
            return 0;
        }
    };

    let db = Database::load();

    // Validate the group exists.
    let group = match db.find_group(&groupname) {
        Some(g) => g,
        None => {
            write_stderr(&format!("newgrp: group '{}' does not exist", groupname));
            return 1;
        }
    };

    // In a full implementation, we would use SYS_SETGID to change the
    // effective group ID and potentially SYS_INITGROUPS to initialize
    // the supplementary group list, then exec a new shell. For now,
    // report what would happen.
    write_stdout(&format!(
        "newgrp: switching to group '{}' (gid={})",
        group.name, group.gid
    ));

    // On Slate OS, we would invoke:
    //   syscall(SYS_SETGID, group.gid)
    //   syscall(SYS_EXEC, shell_path, ...)
    // For now, just exit success indicating the group was validated.
    0
}

// ============================================================================
// Personality detection and dispatch
// ============================================================================

/// Extract the basename of argv[0], stripping path separators and .exe suffix.
fn detect_personality(argv0: &str) -> &str {
    let name = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    name.strip_suffix(".exe").unwrap_or(name)
}

fn usage_all() {
    write_stderr("Slate OS User/Group Management Tools");
    write_stderr("");
    write_stderr("This binary responds to its invocation name:");
    write_stderr("  useradd  - add a user account");
    write_stderr("  userdel  - delete a user account");
    write_stderr("  usermod  - modify a user account");
    write_stderr("  groupadd - add a group");
    write_stderr("  groupdel - delete a group");
    write_stderr("  groupmod - modify a group");
    write_stderr("  newgrp   - change effective group");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("useradd");
    let personality = detect_personality(argv0);
    let rest: Vec<String> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        Vec::new()
    };

    let exit_code = match personality {
        "useradd" => cmd_useradd(&rest),
        "userdel" => cmd_userdel(&rest),
        "usermod" => cmd_usermod(&rest),
        "groupadd" => cmd_groupadd(&rest),
        "groupdel" => cmd_groupdel(&rest),
        "groupmod" => cmd_groupmod(&rest),
        "newgrp" => cmd_newgrp(&rest),
        _ => {
            write_stderr(&format!("unknown personality: {}", personality));
            usage_all();
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
    use scratchdir::ScratchDir;

    /// A private directory for one test, holding that test's `/etc/passwd`,
    /// `/etc/group` and `/etc/shadow` stand-ins.
    ///
    /// The uniqueness comes from `ScratchDir`, which draws on the pid *and* a
    /// process-wide counter. This used to be a counter alone, which looks
    /// sufficient and is not: a counter distinguishes the threads within one
    /// run, but it restarts at 0 in every run, so two concurrent runs walk
    /// `useradd_test_0`, `_1`, `_2` … together. `new` opened by deleting the
    /// directory, so each run wiped the other's shadow file part-way through a
    /// test about whether a password is accepted.
    struct TestEnv {
        dir: ScratchDir,
    }

    impl TestEnv {
        fn new() -> Self {
            TestEnv {
                dir: ScratchDir::new("useradd_test"),
            }
        }

        fn path(&self, name: &str) -> String {
            self.dir.path(name).to_string_lossy().to_string()
        }

        // Reserved for use in future tests that round-trip files through
        // useradd's read/write paths. Whatever they write is removed with the
        // `ScratchDir` above, including on an unwind out of a failed assertion.
        #[allow(dead_code)]
        fn write_file(&self, name: &str, content: &str) {
            fs::write(self.dir.path(name), content).expect("write test file");
        }

        #[allow(dead_code)]
        fn read_file(&self, name: &str) -> String {
            fs::read_to_string(self.dir.path(name)).unwrap_or_default()
        }
    }

    // ---- The account record these commands build ----
    //
    // The `PasswdEntry` and `ShadowEntry` parse/serialize tests stood here.
    // They tested two parsers for two files that are now *generated*, so what
    // they asserted is asserted by `userdb`'s own tests, next to the code that
    // produces those files. What is still this crate's to keep is the record
    // it builds -- and, below, the one option whose argument it must convert
    // rather than copy.

    /// A record shaped like the ones these commands make.
    fn record(name: &str, uid: u32, gid: u32) -> userdb::Record {
        let mut record = userdb::Record::new();
        record.set(userdb::field::USERNAME, name);
        record.set_uid(uid);
        record.set_gid(gid);
        record
    }

    // ---- `-e`: the one argument that is converted rather than stored ----

    /// `/etc/shadow`'s expiry column is a number of days. `useradd -e` takes a
    /// date, and the old code copied the text of it straight into that column,
    /// so the account never expired at all -- glibc reads `2027-01-01` as no
    /// expiry. The two dates checked against known day numbers are the same
    /// ones `passwd`'s date *printer* is tested with, so the two directions
    /// are pinned to the same answers.
    #[test]
    fn an_expiry_date_is_converted_to_days_rather_than_copied() {
        assert_eq!(parse_expire_date("1970-01-01"), Ok(Some(0)));
        assert_eq!(parse_expire_date("2024-01-01"), Ok(Some(19723)));
        assert_eq!(parse_expire_date("2000-03-01"), Ok(Some(11017)));
    }

    /// A bare number is already what the column holds, and shadow-utils
    /// accepts it, so it is taken as given.
    #[test]
    fn a_bare_number_of_days_is_taken_as_it_stands() {
        assert_eq!(parse_expire_date("20000"), Ok(Some(20000)));
        assert_eq!(parse_expire_date("  20000  "), Ok(Some(20000)));
    }

    /// Both spellings of "never": an empty argument, which is how
    /// `usermod -e ""` clears the field, and a negative number, which must not
    /// reach the file -- a literal `-1` there is read as a date one day
    /// *before* the epoch, i.e. expired, the opposite of what was asked.
    #[test]
    fn never_expires_is_an_absent_field_and_never_a_negative_number() {
        assert_eq!(parse_expire_date(""), Ok(None));
        assert_eq!(parse_expire_date("   "), Ok(None));
        assert_eq!(parse_expire_date("-1"), Ok(None));
        assert_eq!(parse_expire_date("-99999"), Ok(None));
    }

    /// A date that is not a date is refused rather than stored. Storing it
    /// would put a value in the column that some later reader takes for a day
    /// number nobody chose.
    #[test]
    fn a_date_that_is_not_a_date_is_refused() {
        for bad in [
            "2024-13-01",
            "2024-00-10",
            "2024-02-30",
            "2023-02-29",
            "2024-01-00",
            "2024-01",
            "2024-01-01-01",
            "next tuesday",
            "20x4-01-01",
        ] {
            assert!(parse_expire_date(bad).is_err(), "accepted {bad:?}");
        }
    }

    /// The leap-year rule, at the three dates that distinguish the three
    /// clauses of it.
    #[test]
    fn the_leap_day_exists_in_the_years_it_exists_in() {
        assert_eq!(parse_expire_date("2024-02-29"), Ok(Some(19782)));
        assert_eq!(parse_expire_date("2000-02-29"), Ok(Some(11016)));
        assert!(parse_expire_date("1900-02-29").is_err());
    }

    // ---- GroupEntry tests ----

    #[test]
    fn test_group_parse_valid() {
        let line = "staff:x:100:alice,bob,carol";
        let entry = GroupEntry::parse(line).expect("should parse");
        assert_eq!(entry.name, "staff");
        assert_eq!(entry.gid, 100);
        assert_eq!(entry.members, vec!["alice", "bob", "carol"]);
    }

    #[test]
    fn test_group_parse_no_members() {
        let line = "wheel:x:10:";
        let entry = GroupEntry::parse(line).expect("should parse");
        assert!(entry.members.is_empty());
    }

    #[test]
    fn test_group_parse_single_member() {
        let line = "admin:x:4:root";
        let entry = GroupEntry::parse(line).expect("should parse");
        assert_eq!(entry.members, vec!["root"]);
    }

    #[test]
    fn test_group_parse_invalid() {
        assert!(GroupEntry::parse("short:x").is_none());
    }

    #[test]
    fn test_group_serialize_roundtrip() {
        let entry = GroupEntry {
            name: "devs".to_string(),
            password: "x".to_string(),
            gid: 2000,
            members: vec!["alice".to_string(), "bob".to_string()],
        };
        let serialized = entry.serialize();
        let parsed = GroupEntry::parse(&serialized).expect("roundtrip");
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_group_serialize_empty_members() {
        let entry = GroupEntry {
            name: "empty".to_string(),
            password: "x".to_string(),
            gid: 3000,
            members: Vec::new(),
        };
        assert_eq!(entry.serialize(), "empty:x:3000:");
    }

    // ---- GshadowEntry tests ----

    #[test]
    fn test_gshadow_parse_valid() {
        let line = "staff:!::alice,bob";
        let entry = GshadowEntry::parse(line).expect("should parse");
        assert_eq!(entry.name, "staff");
        assert_eq!(entry.password, "!");
        assert_eq!(entry.members, vec!["alice", "bob"]);
    }

    #[test]
    fn test_gshadow_parse_empty_members() {
        let line = "wheel:!::";
        let entry = GshadowEntry::parse(line).expect("should parse");
        assert!(entry.members.is_empty());
    }

    #[test]
    fn test_gshadow_serialize_roundtrip() {
        let entry = GshadowEntry {
            name: "test".to_string(),
            password: "!".to_string(),
            admins: "root".to_string(),
            members: vec!["u1".to_string()],
        };
        let serialized = entry.serialize();
        let parsed = GshadowEntry::parse(&serialized).expect("roundtrip");
        assert_eq!(parsed, entry);
    }

    // ---- Validation tests ----

    #[test]
    fn test_validate_username_valid() {
        assert!(validate_username("john").is_ok());
        assert!(validate_username("_svc").is_ok());
        assert!(validate_username("user.name").is_ok());
        assert!(validate_username("user-name").is_ok());
        assert!(validate_username("user123").is_ok());
    }

    #[test]
    fn test_validate_username_empty() {
        assert!(validate_username("").is_err());
    }

    #[test]
    fn test_validate_username_too_long() {
        let long = "a".repeat(33);
        assert!(validate_username(&long).is_err());
    }

    #[test]
    fn test_validate_username_starts_with_digit() {
        assert!(validate_username("1user").is_err());
    }

    #[test]
    fn test_validate_username_starts_with_dash() {
        assert!(validate_username("-user").is_err());
    }

    #[test]
    fn test_validate_username_uppercase() {
        assert!(validate_username("User").is_err());
    }

    #[test]
    fn test_validate_username_space() {
        assert!(validate_username("us er").is_err());
    }

    #[test]
    fn test_validate_username_special_chars() {
        assert!(validate_username("user@host").is_err());
        assert!(validate_username("user:name").is_err());
    }

    #[test]
    fn test_validate_groupname_valid() {
        assert!(validate_groupname("staff").is_ok());
        assert!(validate_groupname("dev-team").is_ok());
        assert!(validate_groupname("group.1").is_ok());
    }

    #[test]
    fn test_validate_groupname_empty() {
        assert!(validate_groupname("").is_err());
    }

    #[test]
    fn test_validate_groupname_too_long() {
        let long = "g".repeat(33);
        assert!(validate_groupname(&long).is_err());
    }

    #[test]
    fn test_validate_groupname_invalid_chars() {
        assert!(validate_groupname("Group").is_err());
        assert!(validate_groupname("grp name").is_err());
    }

    // ---- Personality detection tests ----

    #[test]
    fn test_detect_personality_plain() {
        assert_eq!(detect_personality("useradd"), "useradd");
        assert_eq!(detect_personality("userdel"), "userdel");
        assert_eq!(detect_personality("groupadd"), "groupadd");
        assert_eq!(detect_personality("newgrp"), "newgrp");
    }

    #[test]
    fn test_detect_personality_with_path() {
        assert_eq!(detect_personality("/usr/sbin/useradd"), "useradd");
        assert_eq!(detect_personality("/bin/groupdel"), "groupdel");
    }

    #[test]
    fn test_detect_personality_with_exe() {
        assert_eq!(detect_personality("useradd.exe"), "useradd");
        assert_eq!(detect_personality("/usr/bin/usermod.exe"), "usermod");
    }

    #[test]
    fn test_detect_personality_windows_path() {
        assert_eq!(
            detect_personality("C:\\Program Files\\useradd.exe"),
            "useradd"
        );
        assert_eq!(detect_personality("D:\\bin\\groupmod.exe"), "groupmod");
    }

    #[test]
    fn test_detect_personality_unknown() {
        assert_eq!(detect_personality("something_else"), "something_else");
    }

    // ---- Database tests ----

    /// An empty database. Its directory is `/etc` and nothing here saves; the
    /// tests that do save go through `TestEnv` and a scratch directory.
    fn empty_db() -> Database {
        Database {
            users: userdb::UserDb::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
            etc: std::path::PathBuf::from(ETC_DIR),
        }
    }

    /// A database holding one account and its user-private group, as
    /// `useradd` with no `-g` would have left it.
    fn db_with_user(name: &str, uid: u32) -> Database {
        let mut db = empty_db();
        let mut user = record(name, uid, uid);
        user.set_home(&format!("/home/{name}"));
        user.set(userdb::field::SHELL, "/bin/sh");
        user.set_locked(true);
        db.users.push(user);
        db.groups.push(group(name, uid));
        db.gshadow.push(GshadowEntry {
            name: name.to_string(),
            password: "!".to_string(),
            admins: String::new(),
            members: Vec::new(),
        });
        db
    }

    fn group(name: &str, gid: u32) -> GroupEntry {
        GroupEntry {
            name: name.to_string(),
            password: "x".to_string(),
            gid,
            members: Vec::new(),
        }
    }

    #[test]
    fn test_database_next_uid_empty() {
        assert_eq!(empty_db().next_uid(1000, 60000), Some(1000));
    }

    #[test]
    fn test_database_next_uid_with_existing() {
        let mut db = empty_db();
        db.users.push(record("u1", 1000, 1000));
        assert_eq!(db.next_uid(1000, 60000), Some(1001));
    }

    #[test]
    fn test_database_next_uid_exhausted() {
        let mut db = empty_db();
        db.users.push(record("u1", 100, 100));
        db.users.push(record("u2", 101, 101));
        assert_eq!(db.next_uid(100, 101), None);
    }

    #[test]
    fn test_database_next_gid_empty() {
        assert_eq!(empty_db().next_gid(1000, 60000), Some(1000));
    }

    #[test]
    fn test_database_next_gid_skips_used() {
        let mut db = empty_db();
        db.groups.push(group("g1", 1000));
        assert_eq!(db.next_gid(1000, 60000), Some(1001));
    }

    #[test]
    fn test_database_find_user() {
        let db = db_with_user("alice", 1000);
        assert!(db.find_user("alice").is_some());
        assert!(db.find_user("bob").is_none());
    }

    #[test]
    fn test_database_find_user_by_uid() {
        let db = db_with_user("alice", 1000);
        assert_eq!(
            db.find_user_by_uid(1000).and_then(userdb::Record::username),
            Some("alice".to_string())
        );
        assert!(db.find_user_by_uid(1234).is_none());
    }

    #[test]
    fn test_database_find_group() {
        let mut db = empty_db();
        db.groups.push(group("devs", 2000));
        assert_eq!(db.find_group("devs").map(|g| g.gid), Some(2000));
        assert!(db.find_group("nobody").is_none());
    }

    #[test]
    fn test_database_find_group_by_gid() {
        let mut db = empty_db();
        db.groups.push(group("devs", 2000));
        assert_eq!(
            db.find_group_by_gid(2000).map(|g| g.name.clone()),
            Some("devs".to_string())
        );
        assert!(db.find_group_by_gid(1).is_none());
    }

    /// A record with no `gid` of its own is in the group numbered after its
    /// uid, because that is the group the generated `/etc/passwd` puts it in.
    /// Asking `Record::gid` alone would let `groupdel` remove a group that is
    /// somebody's primary one.
    #[test]
    fn a_record_with_no_gid_is_in_the_group_numbered_after_its_uid() {
        let mut bare = userdb::Record::new();
        bare.set(userdb::field::USERNAME, "alice");
        bare.set_uid(1000);
        assert_eq!(bare.gid(), None);
        assert_eq!(primary_gid(&bare), Some(1000));

        assert_eq!(primary_gid(&record("bob", 1001, 50)), Some(50));
    }

    // ---- Atomic write tests ----

    #[test]
    fn test_atomic_write_creates_file() {
        let env = TestEnv::new();
        let path = env.dir.path("group");
        Database::atomic_write(&path, &[group("staff", 100)], GroupEntry::serialize)
            .expect("write should succeed");
        let content = fs::read_to_string(&path).expect("read");
        assert!(content.contains("staff:x:100:"), "{content}");
    }

    #[test]
    fn test_atomic_write_creates_backup() {
        let env = TestEnv::new();
        let path = env.dir.path("group");
        let backup = env.dir.path("group-");

        // Write initial content.
        fs::write(&path, "original\n").expect("write original");

        Database::atomic_write(&path, &[group("new", 1)], GroupEntry::serialize).expect("write");

        let backup_content = fs::read_to_string(&backup).expect("read backup");
        assert_eq!(backup_content, "original\n");
    }

    #[test]
    fn test_atomic_write_multiple_entries() {
        let env = TestEnv::new();
        let path = env.dir.path("group");
        let entries = vec![
            GroupEntry {
                name: "g1".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: vec!["a".to_string()],
            },
            group("g2", 200),
        ];
        Database::atomic_write(&path, &entries, GroupEntry::serialize).expect("write");
        let content = fs::read_to_string(&path).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("g1:"));
        assert!(lines[1].starts_with("g2:"));
    }

    // ---- Home directory tests ----

    #[test]
    fn test_create_home_dir_basic() {
        let env = TestEnv::new();
        let home = env.path("newhome");
        let skel = env.path("skel");
        // No skel dir — should still create home.
        create_home_dir(&home, &skel, 1000, 1000).expect("create");
        assert!(Path::new(&home).is_dir());
    }

    #[test]
    fn test_create_home_dir_with_skel() {
        let env = TestEnv::new();
        let skel = env.path("skel");
        fs::create_dir_all(&skel).expect("mkdir skel");
        fs::write(format!("{}/profile", skel), "# profile\n").expect("write");
        fs::write(format!("{}/rc", skel), "# rc\n").expect("write");

        let home = env.path("userhome");
        create_home_dir(&home, &skel, 1000, 1000).expect("create");

        assert!(Path::new(&format!("{}/profile", home)).exists());
        assert!(Path::new(&format!("{}/rc", home)).exists());
    }

    #[test]
    fn test_create_home_dir_with_skel_subdir() {
        let env = TestEnv::new();
        let skel = env.path("skel");
        fs::create_dir_all(format!("{}/subdir", skel)).expect("mkdir");
        fs::write(format!("{}/subdir/file.txt", skel), "content").expect("write");

        let home = env.path("userhome2");
        create_home_dir(&home, &skel, 1000, 1000).expect("create");

        assert!(Path::new(&format!("{}/subdir/file.txt", home)).exists());
    }

    #[test]
    fn test_remove_home_dir() {
        let env = TestEnv::new();
        let home = env.path("todelete");
        fs::create_dir_all(&home).expect("mkdir");
        fs::write(format!("{}/file", home), "data").expect("write");

        remove_home_dir(&home).expect("remove");
        assert!(!Path::new(&home).exists());
    }

    #[test]
    fn test_remove_home_dir_nonexistent() {
        let env = TestEnv::new();
        let home = env.path("nosuchdir");
        // Should succeed without error.
        remove_home_dir(&home).expect("remove nonexistent");
    }

    // ---- Argument parsing tests ----

    #[test]
    fn test_parse_useradd_basic() {
        let args = vec!["testuser".to_string()];
        let opts = parse_useradd_args(&args);
        assert_eq!(opts.username.as_deref(), Some("testuser"));
        assert!(!opts.create_home);
        assert!(!opts.system_account);
    }

    #[test]
    fn test_parse_useradd_all_flags() {
        let args = vec![
            "-m".to_string(),
            "-r".to_string(),
            "-d".to_string(),
            "/home/custom".to_string(),
            "-s".to_string(),
            "/bin/zsh".to_string(),
            "-g".to_string(),
            "staff".to_string(),
            "-G".to_string(),
            "wheel,audio".to_string(),
            "-u".to_string(),
            "5000".to_string(),
            "-c".to_string(),
            "Test User".to_string(),
            "-e".to_string(),
            "2025-12-31".to_string(),
            "-p".to_string(),
            "$6$hash".to_string(),
            "-k".to_string(),
            "/etc/skel2".to_string(),
            "myuser".to_string(),
        ];
        let opts = parse_useradd_args(&args);
        assert!(opts.create_home);
        assert!(opts.system_account);
        assert_eq!(opts.home_dir.as_deref(), Some("/home/custom"));
        assert_eq!(opts.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(opts.primary_group.as_deref(), Some("staff"));
        assert_eq!(opts.supp_groups, vec!["wheel", "audio"]);
        assert_eq!(opts.uid, Some(5000));
        assert_eq!(opts.comment.as_deref(), Some("Test User"));
        assert_eq!(opts.expire_date.as_deref(), Some("2025-12-31"));
        assert_eq!(opts.password.as_deref(), Some("$6$hash"));
        assert_eq!(opts.skel_dir.as_deref(), Some("/etc/skel2"));
        assert_eq!(opts.username.as_deref(), Some("myuser"));
    }

    #[test]
    fn test_parse_userdel_basic() {
        let args = vec!["bob".to_string()];
        let opts = parse_userdel_args(&args);
        assert_eq!(opts.username.as_deref(), Some("bob"));
        assert!(!opts.remove_home);
        assert!(!opts.force);
    }

    #[test]
    fn test_parse_userdel_with_flags() {
        let args = vec!["-r".to_string(), "-f".to_string(), "bob".to_string()];
        let opts = parse_userdel_args(&args);
        assert!(opts.remove_home);
        assert!(opts.force);
        assert_eq!(opts.username.as_deref(), Some("bob"));
    }

    #[test]
    fn test_parse_usermod_login_rename() {
        let args = vec![
            "-l".to_string(),
            "newname".to_string(),
            "oldname".to_string(),
        ];
        let opts = parse_usermod_args(&args);
        assert_eq!(opts.new_login.as_deref(), Some("newname"));
        assert_eq!(opts.username.as_deref(), Some("oldname"));
    }

    #[test]
    fn test_parse_usermod_append_groups() {
        let args = vec![
            "-a".to_string(),
            "-G".to_string(),
            "audio,video".to_string(),
            "user1".to_string(),
        ];
        let opts = parse_usermod_args(&args);
        assert!(opts.append_groups);
        assert!(opts.supp_groups_set);
        assert_eq!(opts.supp_groups, vec!["audio", "video"]);
    }

    #[test]
    fn test_parse_usermod_lock_unlock() {
        let lock_args = vec!["-L".to_string(), "user1".to_string()];
        let lock_opts = parse_usermod_args(&lock_args);
        assert!(lock_opts.lock);
        assert!(!lock_opts.unlock);

        let unlock_args = vec!["-U".to_string(), "user1".to_string()];
        let unlock_opts = parse_usermod_args(&unlock_args);
        assert!(!unlock_opts.lock);
        assert!(unlock_opts.unlock);
    }

    #[test]
    fn test_parse_groupadd_basic() {
        let args = vec!["newgroup".to_string()];
        let opts = parse_groupadd_args(&args);
        assert_eq!(opts.groupname.as_deref(), Some("newgroup"));
        assert!(!opts.system_group);
        assert!(!opts.force);
    }

    #[test]
    fn test_parse_groupadd_system() {
        let args = vec![
            "-r".to_string(),
            "-g".to_string(),
            "500".to_string(),
            "sysgroup".to_string(),
        ];
        let opts = parse_groupadd_args(&args);
        assert!(opts.system_group);
        assert_eq!(opts.gid, Some(500));
        assert_eq!(opts.groupname.as_deref(), Some("sysgroup"));
    }

    #[test]
    fn test_parse_groupmod_rename() {
        let args = vec![
            "-n".to_string(),
            "newname".to_string(),
            "oldname".to_string(),
        ];
        let opts = parse_groupmod_args(&args);
        assert_eq!(opts.new_name.as_deref(), Some("newname"));
        assert_eq!(opts.groupname.as_deref(), Some("oldname"));
    }

    #[test]
    fn test_parse_groupmod_change_gid() {
        let args = vec!["-g".to_string(), "5000".to_string(), "mygroup".to_string()];
        let opts = parse_groupmod_args(&args);
        assert_eq!(opts.new_gid, Some(5000));
        assert_eq!(opts.groupname.as_deref(), Some("mygroup"));
    }

    // ---- Integration-style tests using an in-memory Database ----

    #[test]
    fn test_db_add_and_find_user() {
        let mut db = empty_db();
        assert!(db.find_user("alice").is_none());

        db.users.push(record("alice", 1000, 1000));

        assert_eq!(
            db.find_user("alice").and_then(userdb::Record::uid),
            Some(1000)
        );
    }

    #[test]
    fn test_db_duplicate_user_detection() {
        let db = db_with_user("alice", 1000);
        assert!(db.find_user("alice").is_some());
        assert!(db.find_user_by_uid(1000).is_some());
    }

    #[test]
    fn test_db_add_and_find_group() {
        let mut db = empty_db();
        db.groups.push(GroupEntry {
            name: "devs".to_string(),
            password: "x".to_string(),
            gid: 2000,
            members: vec!["alice".to_string()],
        });
        assert_eq!(db.find_group("devs").map(|g| g.gid), Some(2000));
    }

    #[test]
    fn test_db_delete_user() {
        let mut db = db_with_user("bob", 1001);
        assert!(db.find_user("bob").is_some());

        assert!(db.users.remove("bob"));

        assert!(db.find_user("bob").is_none());
    }

    #[test]
    fn test_db_delete_group() {
        let mut db = empty_db();
        db.groups.push(group("temp", 5000));
        assert!(db.find_group("temp").is_some());

        db.forget_group("temp");
        assert!(db.find_group("temp").is_none());
    }

    #[test]
    fn test_uid_auto_assignment_system_range() {
        assert_eq!(
            empty_db().next_uid(SYS_ID_MIN, SYS_ID_MAX),
            Some(SYS_ID_MIN)
        );
    }

    #[test]
    fn test_uid_auto_assignment_regular_range() {
        assert_eq!(
            empty_db().next_uid(REG_ID_MIN, REG_ID_MAX),
            Some(REG_ID_MIN)
        );
    }

    #[test]
    fn test_gid_auto_assignment_gaps() {
        let mut db = empty_db();
        // Create groups at 1000 and 1002, leaving 1001 free.
        db.groups.push(group("g1", 1000));
        db.groups.push(group("g2", 1002));
        assert_eq!(db.next_gid(1000, 60000), Some(1001));
    }

    /// `usermod -L` then `-U` gives back the password the lock was laid over.
    ///
    /// This used to be a test of string surgery on a `!` prefix, performed by
    /// the test itself rather than by the code under test -- so it asserted
    /// that the *test* could prepend and strip a character. It now runs the
    /// operation the commands run, which is the one place the rule lives.
    #[test]
    fn locking_and_unlocking_an_account_restores_the_password_underneath() {
        let mut user = record("test", 1000, 1000);
        user.set(userdb::field::PASSWORD_HASH, "$6$salt$realhash");
        assert!(!user.is_locked());

        user.set_locked(true);
        assert!(user.is_locked());
        assert_eq!(
            user.check_password("anything"),
            userdb::Auth::Locked,
            "a locked account accepts nothing"
        );

        user.set_locked(false);
        assert!(!user.is_locked());
        assert_eq!(
            user.get(userdb::field::PASSWORD_HASH),
            Some("$6$salt$realhash".to_string())
        );
    }

    /// A membership change reaches both places membership is recorded. The
    /// account's own `groups` list and `/etc/group`'s member list are the same
    /// fact written twice, and a change that reached only one of them is the
    /// disagreement `design-decisions.md` §330 and §353 exist to end.
    #[test]
    fn test_supplementary_group_management() {
        let mut db = db_with_user("alice", 1000);
        db.groups.push(group("audio", 100));
        db.groups.push(group("video", 101));

        db.add_to_group("alice", "audio");
        db.add_to_group("alice", "video");

        assert_eq!(db.find_group("audio").map(|g| g.members.len()), Some(1));
        assert_eq!(db.find_group("video").map(|g| g.members.len()), Some(1));
        assert_eq!(
            db.find_user("alice").map(userdb::Record::groups),
            Some(vec!["audio".to_string(), "video".to_string()]),
            "the account has to know its own memberships too"
        );

        db.remove_user_from_groups("alice");
        assert!(db.find_group("audio").is_some_and(|g| g.members.is_empty()));
        assert!(db.find_group("video").is_some_and(|g| g.members.is_empty()));
        assert_eq!(
            db.find_user("alice").map(userdb::Record::groups),
            Some(Vec::new())
        );
    }

    /// Renaming a group renames it in the accounts that belong to it, not only
    /// in the group files -- otherwise those accounts name a group that no
    /// longer exists.
    #[test]
    fn renaming_a_group_renames_it_in_the_accounts_that_are_in_it() {
        let mut db = db_with_user("alice", 1000);
        db.groups.push(group("audio", 100));
        db.add_to_group("alice", "audio");

        db.rename_group_everywhere("audio", "sound");

        assert_eq!(
            db.find_user("alice").map(userdb::Record::groups),
            Some(vec!["sound".to_string()])
        );
    }

    /// Deleting a group removes it from the accounts that were in it.
    #[test]
    fn deleting_a_group_removes_it_from_the_accounts_that_were_in_it() {
        let mut db = db_with_user("alice", 1000);
        db.groups.push(group("audio", 100));
        db.add_to_group("alice", "audio");

        db.forget_group("audio");

        assert!(db.find_group("audio").is_none());
        assert_eq!(
            db.find_user("alice").map(userdb::Record::groups),
            Some(Vec::new())
        );
    }

    #[test]
    fn test_groupmod_gid_change_updates_users() {
        let mut db = db_with_user("alice", 1000);
        assert_eq!(
            primary_gid(db.find_user("alice").expect("alice")),
            Some(1000)
        );

        // Change group GID from 1000 to 2000, as `groupmod -g` does.
        let old_gid = db.groups[0].gid;
        db.groups[0].gid = 2000;
        for user in db.users.records_mut() {
            if primary_gid(user) == Some(old_gid) {
                user.set_gid(2000);
            }
        }

        assert_eq!(
            primary_gid(db.find_user("alice").expect("alice")),
            Some(2000)
        );
        assert_eq!(db.groups[0].gid, 2000);
    }

    #[test]
    fn test_private_group_cleanup_on_userdel() {
        let mut db = db_with_user("bob", 1001);
        // bob has a private group "bob" with no other members.
        let private_empty = db
            .find_group("bob")
            .map(|g| g.members.is_empty())
            .unwrap_or(false);
        assert!(private_empty);

        db.remove_user_from_groups("bob");
        db.users.remove("bob");
        if private_empty {
            db.forget_group("bob");
        }
        assert!(db.find_group("bob").is_none());
    }

    #[test]
    fn test_private_group_preserved_if_has_members() {
        let mut db = db_with_user("carol", 1002);
        // Add another member to carol's group.
        db.add_to_group("dave", "carol");

        assert!(
            db.find_group("carol")
                .is_some_and(|g| !g.members.is_empty())
        );

        // Delete user carol but keep the group, since dave is still in it.
        db.remove_user_from_groups("carol");
        db.users.remove("carol");
        assert!(db.find_group("carol").is_some());
    }

    #[test]
    fn test_copy_dir_recursive_basic() {
        let env = TestEnv::new();
        let src = env.path("src_dir");
        let dst = env.path("dst_dir");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::create_dir_all(&dst).expect("mkdir dst");
        fs::write(format!("{}/a.txt", src), "aaa").expect("write");
        fs::create_dir(format!("{}/sub", src)).expect("mkdir sub");
        fs::write(format!("{}/sub/b.txt", src), "bbb").expect("write");

        copy_dir_recursive(&src, &dst).expect("copy");

        assert_eq!(
            fs::read_to_string(format!("{}/a.txt", dst)).expect("read a"),
            "aaa"
        );
        assert_eq!(
            fs::read_to_string(format!("{}/sub/b.txt", dst)).expect("read b"),
            "bbb"
        );
    }

    // ---- File-based roundtrip tests ----

    /// The whole point of the move: an account this binary saves is in the
    /// database *and* in the two files generated from it.
    ///
    /// Before this, `useradd` wrote `/etc/passwd` and `/etc/shadow` and never
    /// touched the database -- so the next save from any other tool
    /// regenerated both files from a database the account was not in, and the
    /// account silently ceased to exist.
    #[test]
    fn an_account_saved_here_reaches_the_database_and_both_generated_files() {
        let env = TestEnv::new();
        let mut db = Database::load_in(env.dir.dir());
        db.groups.push(group("staff", 100));
        let mut user = record("alice", 1000, 100);
        user.set_home("/home/alice");
        user.set(userdb::field::SHELL, "/bin/sh");
        // A real entry, not a plausible-looking string: `userdb` generates a
        // stored entry it cannot recompute as `*`, so a fake one would be
        // written as `*` and the test would be checking nothing.
        user.set_password_with_salt("correct horse", "abcdef0123456789")
            .expect("the pinned salt is one crypt can carry");
        db.users.push(user);
        db.save().expect("save");

        // The database itself.
        let reloaded = Database::load_in(env.dir.dir());
        assert_eq!(
            reloaded.find_user("alice").and_then(userdb::Record::uid),
            Some(1000)
        );
        assert!(reloaded.find_group("staff").is_some());

        // ...and the two files generated beside it.
        let passwd = env.read_file("passwd");
        assert!(
            passwd.lines().any(|l| l.starts_with("alice:x:1000:100:")),
            "{passwd}"
        );
        let shadow = env.read_file("shadow");
        assert!(
            shadow
                .lines()
                .any(|l| l.starts_with("alice:$6$abcdef0123456789$")),
            "{shadow}"
        );
    }

    /// Saving writes the group files as well, and a reload sees both stores.
    #[test]
    fn a_saved_database_reloads_with_both_stores_intact() {
        let env = TestEnv::new();
        let mut db = Database::load_in(env.dir.dir());
        db.groups.push(group("wheel", 10));
        db.gshadow.push(GshadowEntry {
            name: "wheel".to_string(),
            password: "!".to_string(),
            admins: "root".to_string(),
            members: Vec::new(),
        });
        db.users.push(record("root", 0, 0));
        db.add_to_group("root", "wheel");
        db.save().expect("save");

        let reloaded = Database::load_in(env.dir.dir());
        assert_eq!(
            reloaded.find_group("wheel").map(|g| g.members.clone()),
            Some(vec!["root".to_string()])
        );
        assert_eq!(
            reloaded.gshadow.first().map(|g| g.admins.clone()),
            Some("root".to_string())
        );
        assert_eq!(
            reloaded.find_user("root").map(userdb::Record::groups),
            Some(vec!["wheel".to_string()])
        );
    }

    #[test]
    fn test_file_roundtrip_group() {
        let env = TestEnv::new();
        let path = env.dir.path("group");
        let entries = vec![
            group("root", 0),
            GroupEntry {
                name: "staff".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: vec!["alice".to_string(), "bob".to_string()],
            },
        ];
        Database::atomic_write(&path, &entries, GroupEntry::serialize).expect("write");
        let loaded: Vec<GroupEntry> = Database::load_file(&path, GroupEntry::parse);
        assert_eq!(loaded.len(), 2);
        assert!(loaded[0].members.is_empty());
        assert_eq!(loaded[1].members, vec!["alice", "bob"]);
    }

    #[test]
    fn test_file_roundtrip_gshadow() {
        let env = TestEnv::new();
        let path = env.dir.path("gshadow");
        let entries = vec![GshadowEntry {
            name: "wheel".to_string(),
            password: "!".to_string(),
            admins: "root".to_string(),
            members: vec!["admin".to_string()],
        }];
        Database::atomic_write(&path, &entries, GshadowEntry::serialize).expect("write");
        let loaded: Vec<GshadowEntry> = Database::load_file(&path, GshadowEntry::parse);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].admins, "root");
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let entries: Vec<GroupEntry> =
            Database::load_file(Path::new("/nonexistent/path/file"), GroupEntry::parse);
        assert!(entries.is_empty());
    }

    // ---- Edge case tests ----

    /// A display name holding a colon is refused at generation rather than
    /// written, because a colon there shifts every later field -- it would
    /// move the home directory into the shell column. The old test asserted
    /// the opposite property, that a *parser* split such a line correctly,
    /// which is the wrong end: nothing should be producing the line.
    #[test]
    fn a_display_name_that_cannot_be_written_fails_the_save_rather_than_shifting_fields() {
        let env = TestEnv::new();
        let mut db = Database::load_in(env.dir.dir());
        let mut user = record("alice", 1000, 1000);
        user.set(userdb::field::DISPLAY_NAME, "Alice:Smith");
        db.users.push(user);

        assert!(db.save().is_err());
    }

    #[test]
    fn test_validate_username_max_length() {
        let name = "a".repeat(32);
        assert!(validate_username(&name).is_ok());

        let long = "a".repeat(33);
        assert!(validate_username(&long).is_err());
    }

    #[test]
    fn test_validate_username_underscore_start() {
        assert!(validate_username("_system").is_ok());
    }

    #[test]
    fn test_validate_username_dot_and_dash() {
        assert!(validate_username("user.name-test").is_ok());
    }

    #[test]
    fn test_next_uid_contiguous_fill() {
        let mut db = empty_db();
        for i in 100..110u32 {
            db.users.push(record(&format!("u{i}"), i, i));
        }
        assert_eq!(db.next_uid(100, 999), Some(110));
    }

    #[test]
    fn test_next_gid_contiguous_fill() {
        let mut db = empty_db();
        for i in 1000..1005u32 {
            db.groups.push(group(&format!("g{i}"), i));
        }
        assert_eq!(db.next_gid(1000, 60000), Some(1005));
    }

    #[test]
    fn test_rename_user_preserves_other_members() {
        let mut db = empty_db();
        db.groups.push(GroupEntry {
            name: "team".to_string(),
            password: "x".to_string(),
            gid: 100,
            members: vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
        });
        db.gshadow.push(GshadowEntry {
            name: "team".to_string(),
            password: "!".to_string(),
            admins: String::new(),
            members: vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
        });
        db.rename_user_in_groups("bob", "robert");
        assert_eq!(db.groups[0].members, vec!["alice", "robert", "carol"]);
    }

    #[test]
    fn test_remove_user_from_groups_no_match() {
        let mut db = empty_db();
        db.groups.push(GroupEntry {
            name: "team".to_string(),
            password: "x".to_string(),
            gid: 100,
            members: vec!["alice".to_string()],
        });
        // Removing nonexistent user should be a no-op.
        db.remove_user_from_groups("zzzz");
        assert_eq!(db.groups[0].members, vec!["alice"]);
    }

    #[test]
    fn test_group_parse_invalid_gid() {
        assert!(GroupEntry::parse("name:x:abc:").is_none());
    }

    #[test]
    fn test_gshadow_parse_with_admins() {
        let line = "wheel:!:root,admin:user1,user2";
        let entry = GshadowEntry::parse(line).expect("parse");
        assert_eq!(entry.admins, "root,admin");
        assert_eq!(entry.members, vec!["user1", "user2"]);
    }

    #[test]
    fn test_detect_personality_trailing_slash() {
        // Edge case: path ending with separator should yield empty, but
        // let's verify behavior.
        let result = detect_personality("useradd/");
        // After splitting on /, last component is "", strip_suffix returns ""
        assert_eq!(result, "");
    }

    #[test]
    fn test_args_iterator_empty() {
        let args = Args::new(Vec::new());
        assert!(args.current().is_none());
    }

    #[test]
    fn test_args_iterator_single() {
        let args = Args::new(vec!["hello".to_string()]);
        assert_eq!(args.current(), Some("hello"));
    }

    #[test]
    fn test_args_next_value_at_end() {
        let mut args = Args::new(vec!["-d".to_string()]);
        args.pos = 0;
        // next_value increments pos to 1, which is past the end.
        assert!(args.next_value().is_none());
    }

    #[test]
    fn test_group_parse_empty_string() {
        assert!(GroupEntry::parse("").is_none());
    }

    #[test]
    fn test_gshadow_parse_empty_string() {
        assert!(GshadowEntry::parse("").is_none());
    }
}
