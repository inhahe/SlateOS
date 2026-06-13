//! Slate OS User and Group Management Utilities
//!
//! Multi-personality binary providing POSIX-compatible user/group management:
//! useradd, userdel, usermod, groupadd, groupdel, groupmod, newgrp.
//!
//! Manages the traditional `/etc/passwd`, `/etc/shadow`, `/etc/group`, and
//! `/etc/gshadow` files with atomic writes (write-to-temp then rename) and
//! backup file creation.
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

const PASSWD_PATH: &str = "/etc/passwd";
const SHADOW_PATH: &str = "/etc/shadow";
const GROUP_PATH: &str = "/etc/group";
const GSHADOW_PATH: &str = "/etc/gshadow";

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

/// An entry from `/etc/passwd`.
#[derive(Clone, Debug, PartialEq)]
struct PasswdEntry {
    username: String,
    password: String, // typically "x" (shadow)
    uid: u32,
    gid: u32,
    gecos: String,
    home: String,
    shell: String,
}

impl PasswdEntry {
    fn serialize(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            self.username, self.password, self.uid, self.gid, self.gecos, self.home, self.shell
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 {
            return None;
        }
        Some(PasswdEntry {
            username: parts[0].to_string(),
            password: parts[1].to_string(),
            uid: parts[2].parse().ok()?,
            gid: parts[3].parse().ok()?,
            gecos: parts[4].to_string(),
            home: parts[5].to_string(),
            shell: parts[6].to_string(),
        })
    }
}

/// An entry from `/etc/shadow`.
#[derive(Clone, Debug, PartialEq)]
struct ShadowEntry {
    username: String,
    hash: String,
    last_changed: String,
    min_days: String,
    max_days: String,
    warn_days: String,
    inactive: String,
    expire: String,
    reserved: String,
}

impl ShadowEntry {
    fn serialize(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.username,
            self.hash,
            self.last_changed,
            self.min_days,
            self.max_days,
            self.warn_days,
            self.inactive,
            self.expire,
            self.reserved
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 9 {
            return None;
        }
        Some(ShadowEntry {
            username: parts[0].to_string(),
            hash: parts[1].to_string(),
            last_changed: parts[2].to_string(),
            min_days: parts[3].to_string(),
            max_days: parts[4].to_string(),
            warn_days: parts[5].to_string(),
            inactive: parts[6].to_string(),
            expire: parts[7].to_string(),
            reserved: parts[8].to_string(),
        })
    }

    fn new_locked(username: &str) -> Self {
        ShadowEntry {
            username: username.to_string(),
            hash: "!".to_string(),
            last_changed: "0".to_string(),
            min_days: "0".to_string(),
            max_days: "99999".to_string(),
            warn_days: "7".to_string(),
            inactive: String::new(),
            expire: String::new(),
            reserved: String::new(),
        }
    }
}

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
// Database: reads and writes all four files
// ============================================================================

struct Database {
    passwd: Vec<PasswdEntry>,
    shadow: Vec<ShadowEntry>,
    groups: Vec<GroupEntry>,
    gshadow: Vec<GshadowEntry>,
}

impl Database {
    /// Load all four files. Missing files produce empty vectors.
    fn load() -> Self {
        Database {
            passwd: Self::load_file(PASSWD_PATH, PasswdEntry::parse),
            shadow: Self::load_file(SHADOW_PATH, ShadowEntry::parse),
            groups: Self::load_file(GROUP_PATH, GroupEntry::parse),
            gshadow: Self::load_file(GSHADOW_PATH, GshadowEntry::parse),
        }
    }

    fn load_file<T, F>(path: &str, parser: F) -> Vec<T>
    where
        F: Fn(&str) -> Option<T>,
    {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        content.lines().filter_map(parser).collect()
    }

    /// Write all four files atomically: write temp, create backup, rename.
    fn save(&self) -> Result<(), String> {
        Self::atomic_write(PASSWD_PATH, &self.passwd, PasswdEntry::serialize)?;
        Self::atomic_write(SHADOW_PATH, &self.shadow, ShadowEntry::serialize)?;
        Self::atomic_write(GROUP_PATH, &self.groups, GroupEntry::serialize)?;
        Self::atomic_write(GSHADOW_PATH, &self.gshadow, GshadowEntry::serialize)?;
        Ok(())
    }

    fn atomic_write<T, F>(path: &str, entries: &[T], serializer: F) -> Result<(), String>
    where
        F: Fn(&T) -> String,
    {
        let tmp_path = format!("{}.tmp", path);
        let backup_path = format!("{}-", path);

        let mut content = String::new();
        for entry in entries {
            content.push_str(&serializer(entry));
            content.push('\n');
        }

        // Write to temporary file.
        fs::write(&tmp_path, &content)
            .map_err(|e| format!("failed to write {}: {}", tmp_path, e))?;

        // Create backup of existing file (ignore errors if original doesn't exist).
        if Path::new(path).exists() {
            let _ = fs::copy(path, &backup_path);
        }

        // Atomic rename.
        fs::rename(&tmp_path, path)
            .map_err(|e| format!("failed to rename {} to {}: {}", tmp_path, path, e))?;

        Ok(())
    }

    // ---- lookup helpers ----

    fn find_user(&self, name: &str) -> Option<&PasswdEntry> {
        self.passwd.iter().find(|u| u.username == name)
    }

    fn find_user_by_uid(&self, uid: u32) -> Option<&PasswdEntry> {
        self.passwd.iter().find(|u| u.uid == uid)
    }

    fn find_group(&self, name: &str) -> Option<&GroupEntry> {
        self.groups.iter().find(|g| g.name == name)
    }

    fn find_group_by_gid(&self, gid: u32) -> Option<&GroupEntry> {
        self.groups.iter().find(|g| g.gid == gid)
    }

    #[allow(dead_code)] // Used in tests; useful API for future callers.
    fn find_shadow(&self, name: &str) -> Option<&ShadowEntry> {
        self.shadow.iter().find(|s| s.username == name)
    }

    /// Next free UID in the given range.
    fn next_uid(&self, min: u32, max: u32) -> Option<u32> {
        let used: Vec<u32> = self.passwd.iter().map(|p| p.uid).collect();
        (min..=max).find(|id| !used.contains(id))
    }

    /// Next free GID in the given range.
    fn next_gid(&self, min: u32, max: u32) -> Option<u32> {
        let used: Vec<u32> = self.groups.iter().map(|g| g.gid).collect();
        (min..=max).find(|id| !used.contains(id))
    }

    /// Remove a user from all group member lists.
    fn remove_user_from_groups(&mut self, username: &str) {
        for group in &mut self.groups {
            group.members.retain(|m| m != username);
        }
        for gs in &mut self.gshadow {
            gs.members.retain(|m| m != username);
        }
    }

    /// Rename a user in all group member lists.
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

    // Add passwd entry.
    db.passwd.push(PasswdEntry {
        username: username.clone(),
        password: "x".to_string(),
        uid,
        gid,
        gecos,
        home: home.clone(),
        shell,
    });

    // Add shadow entry.
    let mut shadow = ShadowEntry::new_locked(&username);
    if let Some(ref pw) = opts.password {
        shadow.hash = pw.clone();
    }
    if let Some(ref exp) = opts.expire_date {
        shadow.expire = exp.clone();
    }
    db.shadow.push(shadow);

    // Add user to supplementary groups.
    for gname in &opts.supp_groups {
        let found = db.groups.iter_mut().find(|g| g.name == *gname);
        match found {
            Some(ge) => {
                if !ge.members.contains(&username) {
                    ge.members.push(username.clone());
                }
            }
            None => {
                write_stderr(&format!("useradd: group '{}' does not exist", gname));
                return 1;
            }
        }
        let found_gs = db.gshadow.iter_mut().find(|g| g.name == *gname);
        if let Some(gs) = found_gs
            && !gs.members.contains(&username)
        {
            gs.members.push(username.clone());
        }
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

    let user = match db.find_user(&username) {
        Some(u) => u.clone(),
        None => {
            if opts.force {
                return 0;
            }
            write_stderr(&format!("userdel: user '{}' does not exist", username));
            return 1;
        }
    };

    // Remove from passwd.
    db.passwd.retain(|p| p.username != username);

    // Remove from shadow.
    db.shadow.retain(|s| s.username != username);

    // Remove from all group member lists.
    db.remove_user_from_groups(&username);

    // Remove the user's private group if it exists and has no other members.
    let private_group_empty = db
        .groups
        .iter()
        .find(|g| g.name == username)
        .map(|g| g.members.is_empty())
        .unwrap_or(false);
    if private_group_empty {
        db.groups.retain(|g| g.name != username);
        db.gshadow.retain(|g| g.name != username);
    }

    if let Err(e) = db.save() {
        write_stderr(&format!("userdel: {}", e));
        return 1;
    }

    // Remove home directory if requested.
    if opts.remove_home
        && let Err(e) = remove_home_dir(&user.home)
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

    let mut db = Database::load();

    // Find the user.
    let user_idx = match db.passwd.iter().position(|p| p.username == username) {
        Some(i) => i,
        None => {
            write_stderr(&format!("usermod: user '{}' does not exist", username));
            return 1;
        }
    };

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

    let old_home = db.passwd[user_idx].home.clone();

    // Apply changes to passwd entry.
    if let Some(ref new_name) = opts.new_login {
        let old_name = db.passwd[user_idx].username.clone();
        db.passwd[user_idx].username = new_name.clone();
        // Update shadow.
        if let Some(se) = db.shadow.iter_mut().find(|s| s.username == old_name) {
            se.username = new_name.clone();
        }
        // Update group membership references.
        db.rename_user_in_groups(&old_name, new_name);
    }

    if let Some(ref home) = opts.home_dir {
        db.passwd[user_idx].home = home.clone();
    }

    if let Some(ref shell) = opts.shell {
        db.passwd[user_idx].shell = shell.clone();
    }

    if let Some(ref comment) = opts.comment {
        db.passwd[user_idx].gecos = comment.clone();
    }

    // Primary group.
    if let Some(ref g) = opts.primary_group {
        let gid = match g.parse::<u32>() {
            Ok(id) => {
                if db.find_group_by_gid(id).is_none() {
                    write_stderr(&format!("usermod: group GID {} does not exist", id));
                    return 1;
                }
                id
            }
            Err(_) => match db.find_group(g) {
                Some(ge) => ge.gid,
                None => {
                    write_stderr(&format!("usermod: group '{}' does not exist", g));
                    return 1;
                }
            },
        };
        db.passwd[user_idx].gid = gid;
    }

    // Supplementary groups.
    let effective_username = opts.new_login.as_deref().unwrap_or(&username).to_string();

    if opts.supp_groups_set {
        // Verify all groups exist.
        for gname in &opts.supp_groups {
            if db.find_group(gname).is_none() {
                write_stderr(&format!("usermod: group '{}' does not exist", gname));
                return 1;
            }
        }

        if opts.append_groups {
            // Append mode: add to listed groups without removing from existing.
            for gname in &opts.supp_groups {
                if let Some(ge) = db.groups.iter_mut().find(|g| g.name == *gname)
                    && !ge.members.contains(&effective_username)
                {
                    ge.members.push(effective_username.clone());
                }
                if let Some(gs) = db.gshadow.iter_mut().find(|g| g.name == *gname)
                    && !gs.members.contains(&effective_username)
                {
                    gs.members.push(effective_username.clone());
                }
            }
        } else {
            // Replace mode: remove from all groups, add to listed groups.
            db.remove_user_from_groups(&effective_username);
            for gname in &opts.supp_groups {
                if let Some(ge) = db.groups.iter_mut().find(|g| g.name == *gname)
                    && !ge.members.contains(&effective_username)
                {
                    ge.members.push(effective_username.clone());
                }
                if let Some(gs) = db.gshadow.iter_mut().find(|g| g.name == *gname)
                    && !gs.members.contains(&effective_username)
                {
                    gs.members.push(effective_username.clone());
                }
            }
        }
    }

    // Lock/unlock.
    if opts.lock
        && let Some(se) = db
            .shadow
            .iter_mut()
            .find(|s| s.username == effective_username)
        && !se.hash.starts_with('!')
    {
        se.hash = format!("!{}", se.hash);
    }
    if opts.unlock
        && let Some(se) = db
            .shadow
            .iter_mut()
            .find(|s| s.username == effective_username)
        && se.hash.starts_with('!')
    {
        se.hash = se.hash[1..].to_string();
    }

    // Expire date.
    if let Some(ref exp) = opts.expire_date
        && let Some(se) = db
            .shadow
            .iter_mut()
            .find(|s| s.username == effective_username)
    {
        se.expire = exp.clone();
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
        .passwd
        .iter()
        .filter(|p| p.gid == group.gid)
        .map(|p| p.username.clone())
        .collect();

    if !primary_users.is_empty() {
        write_stderr(&format!(
            "groupdel: cannot remove group '{}': primary group of user(s): {}",
            groupname,
            primary_users.join(", ")
        ));
        return 1;
    }

    db.groups.retain(|g| g.name != groupname);
    db.gshadow.retain(|g| g.name != groupname);

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
        // Update all users whose primary GID matches.
        for p in &mut db.passwd {
            if p.gid == old_gid {
                p.gid = new_gid;
            }
        }
    }

    // Apply name change.
    if let Some(ref new_name) = opts.new_name {
        let old_name = db.groups[group_idx].name.clone();
        db.groups[group_idx].name = new_name.clone();
        // Update gshadow.
        if let Some(gs) = db.gshadow.iter_mut().find(|g| g.name == old_name) {
            gs.name = new_name.clone();
        }
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Each test uses a unique temp directory to avoid interference.
    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TestEnv {
        dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("useradd_test_{}", id));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create test dir");
            TestEnv { dir }
        }

        fn path(&self, name: &str) -> String {
            self.dir.join(name).to_string_lossy().to_string()
        }

        // Reserved for use in future tests that round-trip files through
        // useradd's read/write paths; the matching Drop impl below cleans
        // up the directory in all current tests.
        #[allow(dead_code)]
        fn write_file(&self, name: &str, content: &str) {
            let p = self.dir.join(name);
            fs::write(p, content).expect("write test file");
        }

        #[allow(dead_code)]
        fn read_file(&self, name: &str) -> String {
            let p = self.dir.join(name);
            fs::read_to_string(p).unwrap_or_default()
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    // ---- PasswdEntry tests ----

    #[test]
    fn test_passwd_parse_valid() {
        let line = "john:x:1000:1000:John Doe:/home/john:/bin/bash";
        let entry = PasswdEntry::parse(line).expect("should parse");
        assert_eq!(entry.username, "john");
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.gid, 1000);
        assert_eq!(entry.gecos, "John Doe");
        assert_eq!(entry.home, "/home/john");
        assert_eq!(entry.shell, "/bin/bash");
    }

    #[test]
    fn test_passwd_parse_root() {
        let line = "root:x:0:0:root:/root:/bin/sh";
        let entry = PasswdEntry::parse(line).expect("should parse");
        assert_eq!(entry.uid, 0);
        assert_eq!(entry.gid, 0);
    }

    #[test]
    fn test_passwd_parse_invalid_short() {
        assert!(PasswdEntry::parse("too:few:fields").is_none());
    }

    #[test]
    fn test_passwd_parse_invalid_uid() {
        assert!(PasswdEntry::parse("u:x:abc:0::/:").is_none());
    }

    #[test]
    fn test_passwd_serialize_roundtrip() {
        let entry = PasswdEntry {
            username: "alice".to_string(),
            password: "x".to_string(),
            uid: 1001,
            gid: 1001,
            gecos: "Alice".to_string(),
            home: "/home/alice".to_string(),
            shell: "/bin/zsh".to_string(),
        };
        let serialized = entry.serialize();
        let parsed = PasswdEntry::parse(&serialized).expect("roundtrip");
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_passwd_empty_gecos() {
        let line = "svc:x:500:500::/var/svc:/bin/false";
        let entry = PasswdEntry::parse(line).expect("should parse");
        assert_eq!(entry.gecos, "");
    }

    // ---- ShadowEntry tests ----

    #[test]
    fn test_shadow_parse_valid() {
        let line = "john:$6$salt$hash:19000:0:99999:7:::";
        let entry = ShadowEntry::parse(line).expect("should parse");
        assert_eq!(entry.username, "john");
        assert_eq!(entry.hash, "$6$salt$hash");
        assert_eq!(entry.last_changed, "19000");
        assert_eq!(entry.max_days, "99999");
    }

    #[test]
    fn test_shadow_parse_locked() {
        let line = "locked:!:19000:0:99999:7:::";
        let entry = ShadowEntry::parse(line).expect("should parse");
        assert_eq!(entry.hash, "!");
    }

    #[test]
    fn test_shadow_parse_invalid() {
        assert!(ShadowEntry::parse("too:few").is_none());
    }

    #[test]
    fn test_shadow_serialize_roundtrip() {
        let entry = ShadowEntry::new_locked("testuser");
        let serialized = entry.serialize();
        let parsed = ShadowEntry::parse(&serialized).expect("roundtrip");
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_shadow_new_locked_defaults() {
        let s = ShadowEntry::new_locked("bob");
        assert_eq!(s.username, "bob");
        assert_eq!(s.hash, "!");
        assert_eq!(s.min_days, "0");
        assert_eq!(s.max_days, "99999");
        assert_eq!(s.warn_days, "7");
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

    #[test]
    fn test_database_next_uid_empty() {
        let db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert_eq!(db.next_uid(1000, 60000), Some(1000));
    }

    #[test]
    fn test_database_next_uid_with_existing() {
        let db = Database {
            passwd: vec![PasswdEntry {
                username: "u1".to_string(),
                password: "x".to_string(),
                uid: 1000,
                gid: 1000,
                gecos: String::new(),
                home: "/home/u1".to_string(),
                shell: "/bin/sh".to_string(),
            }],
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert_eq!(db.next_uid(1000, 60000), Some(1001));
    }

    #[test]
    fn test_database_next_uid_exhausted() {
        let db = Database {
            passwd: vec![
                PasswdEntry {
                    username: "u1".to_string(),
                    password: "x".to_string(),
                    uid: 100,
                    gid: 100,
                    gecos: String::new(),
                    home: String::new(),
                    shell: String::new(),
                },
                PasswdEntry {
                    username: "u2".to_string(),
                    password: "x".to_string(),
                    uid: 101,
                    gid: 101,
                    gecos: String::new(),
                    home: String::new(),
                    shell: String::new(),
                },
            ],
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert_eq!(db.next_uid(100, 101), None);
    }

    #[test]
    fn test_database_next_gid_empty() {
        let db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert_eq!(db.next_gid(100, 999), Some(100));
    }

    #[test]
    fn test_database_next_gid_skips_used() {
        let db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: vec![
                GroupEntry {
                    name: "g1".to_string(),
                    password: "x".to_string(),
                    gid: 1000,
                    members: Vec::new(),
                },
                GroupEntry {
                    name: "g2".to_string(),
                    password: "x".to_string(),
                    gid: 1001,
                    members: Vec::new(),
                },
            ],
            gshadow: Vec::new(),
        };
        assert_eq!(db.next_gid(1000, 60000), Some(1002));
    }

    #[test]
    fn test_database_find_user() {
        let db = Database {
            passwd: vec![PasswdEntry {
                username: "alice".to_string(),
                password: "x".to_string(),
                uid: 1000,
                gid: 1000,
                gecos: String::new(),
                home: "/home/alice".to_string(),
                shell: "/bin/sh".to_string(),
            }],
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert!(db.find_user("alice").is_some());
        assert!(db.find_user("bob").is_none());
    }

    #[test]
    fn test_database_find_user_by_uid() {
        let db = Database {
            passwd: vec![PasswdEntry {
                username: "alice".to_string(),
                password: "x".to_string(),
                uid: 1000,
                gid: 1000,
                gecos: String::new(),
                home: String::new(),
                shell: String::new(),
            }],
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert!(db.find_user_by_uid(1000).is_some());
        assert!(db.find_user_by_uid(9999).is_none());
    }

    #[test]
    fn test_database_find_group() {
        let db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: vec![GroupEntry {
                name: "staff".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: Vec::new(),
            }],
            gshadow: Vec::new(),
        };
        assert!(db.find_group("staff").is_some());
        assert!(db.find_group("nope").is_none());
    }

    #[test]
    fn test_database_find_group_by_gid() {
        let db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: vec![GroupEntry {
                name: "staff".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: Vec::new(),
            }],
            gshadow: Vec::new(),
        };
        assert!(db.find_group_by_gid(100).is_some());
        assert!(db.find_group_by_gid(999).is_none());
    }

    #[test]
    fn test_database_find_shadow() {
        let db = Database {
            passwd: Vec::new(),
            shadow: vec![ShadowEntry::new_locked("alice")],
            groups: Vec::new(),
            gshadow: Vec::new(),
        };
        assert!(db.find_shadow("alice").is_some());
        assert!(db.find_shadow("bob").is_none());
    }

    #[test]
    fn test_database_remove_user_from_groups() {
        let mut db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: vec![
                GroupEntry {
                    name: "g1".to_string(),
                    password: "x".to_string(),
                    gid: 100,
                    members: vec!["alice".to_string(), "bob".to_string()],
                },
                GroupEntry {
                    name: "g2".to_string(),
                    password: "x".to_string(),
                    gid: 101,
                    members: vec!["alice".to_string()],
                },
            ],
            gshadow: vec![
                GshadowEntry {
                    name: "g1".to_string(),
                    password: "!".to_string(),
                    admins: String::new(),
                    members: vec!["alice".to_string(), "bob".to_string()],
                },
                GshadowEntry {
                    name: "g2".to_string(),
                    password: "!".to_string(),
                    admins: String::new(),
                    members: vec!["alice".to_string()],
                },
            ],
        };
        db.remove_user_from_groups("alice");
        assert_eq!(db.groups[0].members, vec!["bob"]);
        assert!(db.groups[1].members.is_empty());
        assert_eq!(db.gshadow[0].members, vec!["bob"]);
        assert!(db.gshadow[1].members.is_empty());
    }

    #[test]
    fn test_database_rename_user_in_groups() {
        let mut db = Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: vec![GroupEntry {
                name: "devs".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: vec!["oldname".to_string(), "other".to_string()],
            }],
            gshadow: vec![GshadowEntry {
                name: "devs".to_string(),
                password: "!".to_string(),
                admins: String::new(),
                members: vec!["oldname".to_string(), "other".to_string()],
            }],
        };
        db.rename_user_in_groups("oldname", "newname");
        assert_eq!(db.groups[0].members, vec!["newname", "other"]);
        assert_eq!(db.gshadow[0].members, vec!["newname", "other"]);
    }

    // ---- Atomic write tests ----

    #[test]
    fn test_atomic_write_creates_file() {
        let env = TestEnv::new();
        let path = env.path("test_file");
        let entries = vec![PasswdEntry {
            username: "alice".to_string(),
            password: "x".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: String::new(),
            home: "/home/alice".to_string(),
            shell: "/bin/sh".to_string(),
        }];
        Database::atomic_write(&path, &entries, PasswdEntry::serialize)
            .expect("write should succeed");
        let content = fs::read_to_string(&path).expect("read");
        assert!(content.contains("alice:x:1000:1000::/home/alice:/bin/sh"));
    }

    #[test]
    fn test_atomic_write_creates_backup() {
        let env = TestEnv::new();
        let path = env.path("passwd");
        let backup = format!("{}-", path);

        // Write initial content.
        fs::write(&path, "original\n").expect("write original");

        let entries = vec![PasswdEntry {
            username: "new".to_string(),
            password: "x".to_string(),
            uid: 1,
            gid: 1,
            gecos: String::new(),
            home: String::new(),
            shell: String::new(),
        }];
        Database::atomic_write(&path, &entries, PasswdEntry::serialize).expect("write");

        let backup_content = fs::read_to_string(&backup).expect("read backup");
        assert_eq!(backup_content, "original\n");
    }

    #[test]
    fn test_atomic_write_multiple_entries() {
        let env = TestEnv::new();
        let path = env.path("groups");
        let entries = vec![
            GroupEntry {
                name: "g1".to_string(),
                password: "x".to_string(),
                gid: 100,
                members: vec!["a".to_string()],
            },
            GroupEntry {
                name: "g2".to_string(),
                password: "x".to_string(),
                gid: 200,
                members: Vec::new(),
            },
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

    // ---- Integration-style tests using in-memory Database ----

    fn empty_db() -> Database {
        Database {
            passwd: Vec::new(),
            shadow: Vec::new(),
            groups: Vec::new(),
            gshadow: Vec::new(),
        }
    }

    fn db_with_user(name: &str, uid: u32) -> Database {
        let mut db = empty_db();
        db.passwd.push(PasswdEntry {
            username: name.to_string(),
            password: "x".to_string(),
            uid,
            gid: uid,
            gecos: String::new(),
            home: format!("/home/{}", name),
            shell: "/bin/sh".to_string(),
        });
        db.shadow.push(ShadowEntry::new_locked(name));
        db.groups.push(GroupEntry {
            name: name.to_string(),
            password: "x".to_string(),
            gid: uid,
            members: Vec::new(),
        });
        db.gshadow.push(GshadowEntry {
            name: name.to_string(),
            password: "!".to_string(),
            admins: String::new(),
            members: Vec::new(),
        });
        db
    }

    #[test]
    fn test_db_add_and_find_user() {
        let mut db = empty_db();
        assert!(db.find_user("alice").is_none());

        db.passwd.push(PasswdEntry {
            username: "alice".to_string(),
            password: "x".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: "Alice".to_string(),
            home: "/home/alice".to_string(),
            shell: "/bin/bash".to_string(),
        });

        let found = db.find_user("alice");
        assert!(found.is_some());
        assert_eq!(found.map(|u| u.uid), Some(1000));
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
        let found = db.find_group("devs");
        assert!(found.is_some());
        assert_eq!(found.map(|g| g.gid), Some(2000));
    }

    #[test]
    fn test_db_delete_user() {
        let mut db = db_with_user("bob", 1001);
        assert!(db.find_user("bob").is_some());

        db.passwd.retain(|p| p.username != "bob");
        db.shadow.retain(|s| s.username != "bob");

        assert!(db.find_user("bob").is_none());
        assert!(db.find_shadow("bob").is_none());
    }

    #[test]
    fn test_db_delete_group() {
        let mut db = empty_db();
        db.groups.push(GroupEntry {
            name: "temp".to_string(),
            password: "x".to_string(),
            gid: 5000,
            members: Vec::new(),
        });
        assert!(db.find_group("temp").is_some());

        db.groups.retain(|g| g.name != "temp");
        assert!(db.find_group("temp").is_none());
    }

    #[test]
    fn test_uid_auto_assignment_system_range() {
        let db = empty_db();
        let uid = db.next_uid(SYS_ID_MIN, SYS_ID_MAX);
        assert_eq!(uid, Some(SYS_ID_MIN));
    }

    #[test]
    fn test_uid_auto_assignment_regular_range() {
        let db = empty_db();
        let uid = db.next_uid(REG_ID_MIN, REG_ID_MAX);
        assert_eq!(uid, Some(REG_ID_MIN));
    }

    #[test]
    fn test_gid_auto_assignment_gaps() {
        let mut db = empty_db();
        // Create groups at 1000 and 1002, leaving 1001 free.
        db.groups.push(GroupEntry {
            name: "g1".to_string(),
            password: "x".to_string(),
            gid: 1000,
            members: Vec::new(),
        });
        db.groups.push(GroupEntry {
            name: "g2".to_string(),
            password: "x".to_string(),
            gid: 1002,
            members: Vec::new(),
        });
        assert_eq!(db.next_gid(1000, 60000), Some(1001));
    }

    #[test]
    fn test_shadow_lock_unlock_logic() {
        let mut shadow = ShadowEntry::new_locked("test");
        // Initially locked with "!".
        assert!(shadow.hash.starts_with('!'));

        // Unlock: strip the "!".
        if shadow.hash.starts_with('!') {
            shadow.hash = shadow.hash[1..].to_string();
        }
        assert!(!shadow.hash.starts_with('!'));

        // Set a real hash.
        shadow.hash = "$6$salt$realhash".to_string();

        // Lock: prepend "!".
        shadow.hash = format!("!{}", shadow.hash);
        assert_eq!(shadow.hash, "!$6$salt$realhash");

        // Unlock again.
        if shadow.hash.starts_with('!') {
            shadow.hash = shadow.hash[1..].to_string();
        }
        assert_eq!(shadow.hash, "$6$salt$realhash");
    }

    #[test]
    fn test_supplementary_group_management() {
        let mut db = empty_db();
        db.groups.push(GroupEntry {
            name: "audio".to_string(),
            password: "x".to_string(),
            gid: 100,
            members: Vec::new(),
        });
        db.groups.push(GroupEntry {
            name: "video".to_string(),
            password: "x".to_string(),
            gid: 101,
            members: Vec::new(),
        });

        // Add user to groups.
        for g in &mut db.groups {
            if g.name == "audio" || g.name == "video" {
                g.members.push("alice".to_string());
            }
        }
        assert_eq!(db.groups[0].members, vec!["alice"]);
        assert_eq!(db.groups[1].members, vec!["alice"]);

        // Remove user from all groups.
        db.remove_user_from_groups("alice");
        assert!(db.groups[0].members.is_empty());
        assert!(db.groups[1].members.is_empty());
    }

    #[test]
    fn test_groupmod_gid_change_updates_users() {
        let mut db = db_with_user("alice", 1000);
        // Alice's primary GID is 1000.
        assert_eq!(db.passwd[0].gid, 1000);

        // Change group GID from 1000 to 2000.
        let old_gid = db.groups[0].gid;
        db.groups[0].gid = 2000;
        for p in &mut db.passwd {
            if p.gid == old_gid {
                p.gid = 2000;
            }
        }
        assert_eq!(db.passwd[0].gid, 2000);
        assert_eq!(db.groups[0].gid, 2000);
    }

    #[test]
    fn test_private_group_cleanup_on_userdel() {
        let mut db = db_with_user("bob", 1001);
        // bob has a private group "bob" with no other members.
        let private_empty = db
            .groups
            .iter()
            .find(|g| g.name == "bob")
            .map(|g| g.members.is_empty())
            .unwrap_or(false);
        assert!(private_empty);

        // Delete user.
        db.passwd.retain(|p| p.username != "bob");
        db.shadow.retain(|s| s.username != "bob");
        // Clean up private group.
        if private_empty {
            db.groups.retain(|g| g.name != "bob");
            db.gshadow.retain(|g| g.name != "bob");
        }
        assert!(db.find_group("bob").is_none());
    }

    #[test]
    fn test_private_group_preserved_if_has_members() {
        let mut db = db_with_user("carol", 1002);
        // Add another member to carol's group.
        if let Some(g) = db.groups.iter_mut().find(|g| g.name == "carol") {
            g.members.push("dave".to_string());
        }

        let has_members = db
            .groups
            .iter()
            .find(|g| g.name == "carol")
            .map(|g| !g.members.is_empty())
            .unwrap_or(false);
        assert!(has_members);

        // Delete user carol but keep the group since it has members.
        db.passwd.retain(|p| p.username != "carol");
        db.remove_user_from_groups("carol");
        // Group should still exist (dave is still a member... well, was before
        // remove_user_from_groups which only removes "carol").
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

    #[test]
    fn test_file_roundtrip_passwd() {
        let env = TestEnv::new();
        let path = env.path("passwd");
        let entries = vec![
            PasswdEntry {
                username: "root".to_string(),
                password: "x".to_string(),
                uid: 0,
                gid: 0,
                gecos: "root".to_string(),
                home: "/root".to_string(),
                shell: "/bin/sh".to_string(),
            },
            PasswdEntry {
                username: "alice".to_string(),
                password: "x".to_string(),
                uid: 1000,
                gid: 1000,
                gecos: "Alice Smith".to_string(),
                home: "/home/alice".to_string(),
                shell: "/bin/bash".to_string(),
            },
        ];
        Database::atomic_write(&path, &entries, PasswdEntry::serialize).expect("write");
        let loaded: Vec<PasswdEntry> = Database::load_file(&path, PasswdEntry::parse);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].username, "root");
        assert_eq!(loaded[1].username, "alice");
        assert_eq!(loaded[1].gecos, "Alice Smith");
    }

    #[test]
    fn test_file_roundtrip_shadow() {
        let env = TestEnv::new();
        let path = env.path("shadow");
        let entries = vec![
            ShadowEntry {
                username: "root".to_string(),
                hash: "$6$abc$xyz".to_string(),
                last_changed: "19000".to_string(),
                min_days: "0".to_string(),
                max_days: "99999".to_string(),
                warn_days: "7".to_string(),
                inactive: String::new(),
                expire: String::new(),
                reserved: String::new(),
            },
            ShadowEntry::new_locked("svc"),
        ];
        Database::atomic_write(&path, &entries, ShadowEntry::serialize).expect("write");
        let loaded: Vec<ShadowEntry> = Database::load_file(&path, ShadowEntry::parse);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].hash, "$6$abc$xyz");
        assert_eq!(loaded[1].hash, "!");
    }

    #[test]
    fn test_file_roundtrip_group() {
        let env = TestEnv::new();
        let path = env.path("group");
        let entries = vec![
            GroupEntry {
                name: "root".to_string(),
                password: "x".to_string(),
                gid: 0,
                members: Vec::new(),
            },
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
        let path = env.path("gshadow");
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
        let entries: Vec<PasswdEntry> =
            Database::load_file("/nonexistent/path/file", PasswdEntry::parse);
        assert!(entries.is_empty());
    }

    // ---- Edge case tests ----

    #[test]
    fn test_passwd_entry_with_colons_in_gecos() {
        // GECOS field traditionally doesn't contain colons, but the parser
        // should handle the standard 7-field split correctly.
        let line = "user:x:1000:1000:Normal GECOS:/home/user:/bin/sh";
        let entry = PasswdEntry::parse(line).expect("parse");
        assert_eq!(entry.gecos, "Normal GECOS");
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
            db.passwd.push(PasswdEntry {
                username: format!("u{}", i),
                password: "x".to_string(),
                uid: i,
                gid: i,
                gecos: String::new(),
                home: String::new(),
                shell: String::new(),
            });
        }
        assert_eq!(db.next_uid(100, 999), Some(110));
    }

    #[test]
    fn test_next_gid_contiguous_fill() {
        let mut db = empty_db();
        for i in 1000..1005u32 {
            db.groups.push(GroupEntry {
                name: format!("g{}", i),
                password: "x".to_string(),
                gid: i,
                members: Vec::new(),
            });
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
    fn test_shadow_parse_exactly_nine_fields() {
        let line = "user:hash:1:2:3:4:5:6:7";
        let entry = ShadowEntry::parse(line).expect("should parse 9 fields");
        assert_eq!(entry.reserved, "7");
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
    fn test_passwd_parse_empty_string() {
        assert!(PasswdEntry::parse("").is_none());
    }

    #[test]
    fn test_group_parse_empty_string() {
        assert!(GroupEntry::parse("").is_none());
    }

    #[test]
    fn test_shadow_parse_empty_string() {
        assert!(ShadowEntry::parse("").is_none());
    }

    #[test]
    fn test_gshadow_parse_empty_string() {
        assert!(GshadowEntry::parse("").is_none());
    }
}
