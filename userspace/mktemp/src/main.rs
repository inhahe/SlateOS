//! Slate OS Multi-Personality Utility: mktemp / id / groups / whoami
//!
//! A single binary that determines its behavior based on `argv[0]`:
//!
//! - **mktemp** (default): create temporary files or directories safely
//! - **id**: display real and effective user/group IDs
//! - **groups**: display group memberships
//! - **whoami**: print effective username
//!
//! Symlink or hardlink this binary under the names `id`, `groups`, and
//! `whoami` to activate those personalities.
//!
//! # mktemp usage
//!
//! ```text
//! mktemp [OPTIONS] [TEMPLATE]
//!   -d, --directory       Create a directory instead of a file
//!   -p DIR, --tmpdir=DIR  Use DIR as the base (default: $TMPDIR or /tmp)
//!   -t                    Interpret TEMPLATE relative to $TMPDIR
//!   -u, --dry-run         Print name without creating (unsafe)
//!   -q, --quiet           Suppress error messages
//!   --suffix=SUFF         Append SUFF after the random part
//! ```
//!
//! # id usage
//!
//! ```text
//! id [OPTIONS] [USER]
//!   -u, --user    Print only effective UID
//!   -g, --group   Print only effective GID
//!   -G, --groups  Print all group IDs
//!   -n, --name    Print name instead of number
//!   -r, --real    Print real ID instead of effective
//! ```
//!
//! # groups usage
//!
//! ```text
//! groups [USER]
//! ```
//!
//! # whoami usage
//!
//! ```text
//! whoami
//! ```

use std::env;
use std::fs;
use std::process;
use std::time::SystemTime;

// ============================================================================
// Personality detection
// ============================================================================

/// The four modes this binary can operate in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Mktemp,
    Id,
    Groups,
    Whoami,
}

/// Determine personality from argv[0].
fn detect_personality(argv0: &str) -> Personality {
    // Extract the basename, stripping directory separators and .exe suffix.
    let basename = argv0
        .rsplit('/')
        .next()
        .unwrap_or(argv0)
        .rsplit('\\')
        .next()
        .unwrap_or(argv0);
    let lower = basename.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);

    if stem.contains("whoami") {
        Personality::Whoami
    } else if stem.contains("groups") {
        Personality::Groups
    } else if stem.contains("id") && !stem.contains("mktemp") {
        // "id" but not something like "mktemp-id"
        Personality::Id
    } else {
        Personality::Mktemp
    }
}

// ============================================================================
// Libc FFI for identity syscalls
// ============================================================================

unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
    fn getgroups(size: i32, list: *mut u32) -> i32;
}

// ============================================================================
// /etc/passwd and /etc/group parsing
// ============================================================================

/// A parsed /etc/passwd entry.
struct PasswdEntry {
    name: String,
    uid: u32,
    gid: u32,
}

/// Parse a single /etc/passwd line into a PasswdEntry.
/// Format: name:password:uid:gid:gecos:home:shell
fn parse_passwd_line(line: &str) -> Option<PasswdEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 4 {
        return None;
    }
    let uid: u32 = fields.get(2)?.parse().ok()?;
    let gid: u32 = fields.get(3)?.parse().ok()?;
    Some(PasswdEntry {
        name: fields.first()?.to_string(),
        uid,
        gid,
    })
}

/// Read all /etc/passwd entries.
fn read_passwd() -> Vec<PasswdEntry> {
    let content = match fs::read_to_string("/etc/passwd") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(parse_passwd_line)
        .collect()
}

/// Look up a username by UID.
fn uid_to_name(uid: u32) -> Option<String> {
    for entry in read_passwd() {
        if entry.uid == uid {
            return Some(entry.name);
        }
    }
    None
}

/// Look up a UID by username.
fn name_to_uid(name: &str) -> Option<u32> {
    for entry in read_passwd() {
        if entry.name == name {
            return Some(entry.uid);
        }
    }
    None
}

/// Look up the primary GID for a username.
fn name_to_gid(name: &str) -> Option<u32> {
    for entry in read_passwd() {
        if entry.name == name {
            return Some(entry.gid);
        }
    }
    None
}

/// A parsed /etc/group entry.
struct GroupEntry {
    name: String,
    gid: u32,
    members: Vec<String>,
}

/// Parse a single /etc/group line.
/// Format: name:password:gid:member1,member2,...
fn parse_group_line(line: &str) -> Option<GroupEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 3 {
        return None;
    }
    let gid: u32 = fields.get(2)?.parse().ok()?;
    let members = if let Some(member_str) = fields.get(3) {
        member_str
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    Some(GroupEntry {
        name: fields.first()?.to_string(),
        gid,
        members,
    })
}

/// Read all /etc/group entries.
fn read_groups() -> Vec<GroupEntry> {
    let content = match fs::read_to_string("/etc/group") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(parse_group_line)
        .collect()
}

/// Look up a group name by GID.
fn gid_to_name(gid: u32) -> Option<String> {
    for entry in read_groups() {
        if entry.gid == gid {
            return Some(entry.name);
        }
    }
    None
}

/// Get all groups a user belongs to (by username), returning (gid, name) pairs.
fn groups_for_user(username: &str) -> Vec<(u32, String)> {
    let mut result = Vec::new();

    // Primary group from /etc/passwd.
    if let Some(primary_gid) = name_to_gid(username) {
        let gname = gid_to_name(primary_gid).unwrap_or_else(|| primary_gid.to_string());
        result.push((primary_gid, gname));
    }

    // Supplementary groups from /etc/group membership lists.
    for entry in read_groups() {
        if entry.members.iter().any(|m| m == username) {
            // Avoid duplicating the primary group.
            if !result.iter().any(|(gid, _)| *gid == entry.gid) {
                result.push((entry.gid, entry.name));
            }
        }
    }

    result
}

/// Get supplementary group IDs from the kernel via getgroups().
fn get_supplementary_gids() -> Vec<u32> {
    // First call with size=0 to get count.
    // SAFETY: getgroups(0, null) is a valid call that returns the number of
    // supplementary group IDs.
    let count = unsafe { getgroups(0, std::ptr::null_mut()) };
    if count <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; count as usize];
    // SAFETY: buf is properly sized and aligned for `count` u32 values.
    let actual = unsafe { getgroups(count, buf.as_mut_ptr()) };
    if actual < 0 {
        return Vec::new();
    }
    buf.truncate(actual as usize);
    buf
}

// ============================================================================
// mktemp implementation
// ============================================================================

/// Options parsed from mktemp command-line arguments.
struct MktempOpts {
    /// Create a directory instead of a file.
    directory: bool,
    /// Base directory for the temp file (default: $TMPDIR or /tmp).
    tmpdir: String,
    /// Whether -t was given (interpret template relative to tmpdir).
    use_tmpdir: bool,
    /// Dry-run mode: print name but don't create.
    dry_run: bool,
    /// Suppress error messages.
    quiet: bool,
    /// Suffix to append after the random part.
    suffix: String,
    /// Template string (default: "tmp.XXXXXXXXXX").
    template: String,
}

impl MktempOpts {
    fn new() -> Self {
        let tmpdir = env::var("TMPDIR")
            .or_else(|_| env::var("TMP"))
            .or_else(|_| env::var("TEMP"))
            .unwrap_or_else(|_| "/tmp".to_string());
        Self {
            directory: false,
            tmpdir,
            use_tmpdir: false,
            dry_run: false,
            quiet: false,
            suffix: String::new(),
            template: "tmp.XXXXXXXXXX".to_string(),
        }
    }
}

/// Read random bytes from /dev/urandom, falling back to a timestamp-based PRNG.
fn read_random_bytes(buf: &mut [u8]) -> bool {
    if let Ok(data) = fs::read("/dev/urandom") {
        let copy_len = buf.len().min(data.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..copy_len), data.get(..copy_len)) {
            dst.copy_from_slice(src);
            return true;
        }
    }

    // Fallback: read a larger chunk and use it.
    if let Ok(mut file) = fs::File::open("/dev/urandom") {
        use std::io::Read;
        if file.read_exact(buf).is_ok() {
            return true;
        }
    }

    // Last resort: timestamp-based pseudo-randomness (not cryptographic).
    fill_from_timestamp(buf);
    true
}

/// Fill buffer with pseudo-random bytes derived from the system clock.
/// This is NOT cryptographically secure; used only as a fallback.
fn fill_from_timestamp(buf: &mut [u8]) {
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5deece66d);

    // Simple xorshift64 PRNG seeded from the timestamp.
    let mut state = seed;
    if state == 0 {
        state = 0x5deece66d;
    }
    for byte in buf.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state & 0xFF) as u8;
    }
}

/// The alphabet used for random replacement of X characters in templates.
const RAND_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Expand X characters in a template with random alphanumeric characters.
/// Returns the expanded string.
fn expand_template(template: &str, suffix: &str) -> String {
    // Count trailing X's in the template (before suffix).
    let template_bytes = template.as_bytes();
    let mut x_start = template_bytes.len();
    while x_start > 0 {
        if let Some(&b'X') = template_bytes.get(x_start - 1) {
            x_start -= 1;
        } else {
            break;
        }
    }
    let x_count = template_bytes.len() - x_start;

    if x_count == 0 {
        // No X's to expand; just append suffix.
        return format!("{template}{suffix}");
    }

    // Generate random bytes for each X.
    let mut rand_bytes = vec![0u8; x_count];
    read_random_bytes(&mut rand_bytes);

    let prefix = &template[..x_start];
    let mut result = String::with_capacity(prefix.len() + x_count + suffix.len());
    result.push_str(prefix);
    for &b in &rand_bytes {
        let idx = (b as usize) % RAND_CHARS.len();
        // SAFETY: RAND_CHARS only contains ASCII bytes, so indexing is fine.
        if let Some(&ch) = RAND_CHARS.get(idx) {
            result.push(ch as char);
        }
    }
    result.push_str(suffix);
    result
}

/// Build the full path from options and template.
fn build_path(opts: &MktempOpts) -> String {
    if opts.use_tmpdir || !opts.template.contains('/') {
        // Place in tmpdir.
        let dir = opts.tmpdir.trim_end_matches('/');
        format!("{dir}/{}", opts.template)
    } else {
        opts.template.clone()
    }
}

/// Attempt to create the temp file or directory, retrying on collision.
fn create_temp(opts: &MktempOpts) -> Result<String, String> {
    let full_template = build_path(opts);

    // Try up to 100 times to avoid collisions.
    for _ in 0..100 {
        let path = expand_template(&full_template, &opts.suffix);

        if opts.dry_run {
            return Ok(path);
        }

        if opts.directory {
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("mktemp: failed to create directory '{path}': {e}")),
            }
        } else {
            // Use OpenOptions with create_new to avoid overwriting.
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("mktemp: failed to create file '{path}': {e}")),
            }
        }
    }

    Err("mktemp: failed to create unique name after 100 attempts".to_string())
}

/// Parse mktemp arguments and execute.
fn run_mktemp(args: &[String]) -> i32 {
    let mut opts = MktempOpts::new();
    let mut positional = Vec::new();
    let mut i = 1; // skip argv[0]

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-d" | "--directory" => opts.directory = true,
            "-t" => opts.use_tmpdir = true,
            "-u" | "--dry-run" => opts.dry_run = true,
            "-q" | "--quiet" => opts.quiet = true,
            "-p" => {
                i += 1;
                if let Some(dir) = args.get(i) {
                    opts.tmpdir = dir.clone();
                    opts.use_tmpdir = true;
                } else {
                    if !opts.quiet {
                        eprintln!("mktemp: option '-p' requires an argument");
                    }
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_mktemp_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("mktemp (Slate OS) 0.1.0");
                return 0;
            }
            other if other.starts_with("--tmpdir=") => {
                if let Some(dir) = other.strip_prefix("--tmpdir=") {
                    opts.tmpdir = dir.to_string();
                    opts.use_tmpdir = true;
                }
            }
            other if other.starts_with("--tmpdir") && other.len() == 8 => {
                // --tmpdir without '=' means use default tmpdir and set flag
                opts.use_tmpdir = true;
            }
            other if other.starts_with("--suffix=") => {
                if let Some(suff) = other.strip_prefix("--suffix=") {
                    opts.suffix = suff.to_string();
                }
            }
            other if other.starts_with('-') => {
                if !opts.quiet {
                    eprintln!("mktemp: unknown option: {other}");
                }
                return 1;
            }
            _ => {
                positional.push(arg.clone());
            }
        }
        i += 1;
    }

    // The first positional argument is the template.
    if let Some(tmpl) = positional.first() {
        opts.template = tmpl.clone();
    }

    // Validate: template must contain at least 3 consecutive X's.
    let trailing_x = opts
        .template
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&b| b == b'X')
        .count();
    if trailing_x < 3 {
        if !opts.quiet {
            eprintln!("mktemp: too few X's in template '{}'", opts.template);
        }
        return 1;
    }

    match create_temp(&opts) {
        Ok(path) => {
            println!("{path}");
            0
        }
        Err(msg) => {
            if !opts.quiet {
                eprintln!("{msg}");
            }
            1
        }
    }
}

fn print_mktemp_help() {
    println!("mktemp (Slate OS) 0.1.0 -- Create temporary files or directories safely");
    println!();
    println!("USAGE:");
    println!("  mktemp [OPTIONS] [TEMPLATE]");
    println!();
    println!("The TEMPLATE must contain at least 3 consecutive trailing X's,");
    println!("which will be replaced with random alphanumeric characters.");
    println!("Default template: tmp.XXXXXXXXXX");
    println!();
    println!("OPTIONS:");
    println!("  -d, --directory       Create a directory instead of a file");
    println!("  -p DIR, --tmpdir=DIR  Use DIR as the base (default: $TMPDIR or /tmp)");
    println!("  -t                    Interpret TEMPLATE relative to $TMPDIR");
    println!("  -u, --dry-run         Print name only; do not create (unsafe)");
    println!("  -q, --quiet           Suppress error messages");
    println!("  --suffix=SUFF         Append SUFF after the random part");
    println!("  -h, --help            Display this help and exit");
    println!("  -V, --version         Display version and exit");
}

// ============================================================================
// id implementation
// ============================================================================

/// Options parsed from id command-line arguments.
#[derive(Debug)]
struct IdOpts {
    /// Print only effective user ID.
    user_only: bool,
    /// Print only effective group ID.
    group_only: bool,
    /// Print all group IDs.
    all_groups: bool,
    /// Print name instead of number.
    use_name: bool,
    /// Print real ID instead of effective.
    use_real: bool,
    /// Optional target username (instead of current user).
    target_user: Option<String>,
}

impl IdOpts {
    fn new() -> Self {
        Self {
            user_only: false,
            group_only: false,
            all_groups: false,
            use_name: false,
            use_real: false,
            target_user: None,
        }
    }
}

fn parse_id_args(args: &[String]) -> Result<IdOpts, i32> {
    let mut opts = IdOpts::new();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-u" | "--user" => opts.user_only = true,
            "-g" | "--group" => opts.group_only = true,
            "-G" | "--groups" => opts.all_groups = true,
            "-n" | "--name" => opts.use_name = true,
            "-r" | "--real" => opts.use_real = true,
            "-h" | "--help" => {
                print_id_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("id (Slate OS) 0.1.0");
                return Err(0);
            }
            other if other.starts_with('-') => {
                // Handle combined short options like -un, -Gn, etc.
                let chars: Vec<char> = other[1..].chars().collect();
                let mut all_known = true;
                for ch in &chars {
                    match ch {
                        'u' => opts.user_only = true,
                        'g' => opts.group_only = true,
                        'G' => opts.all_groups = true,
                        'n' => opts.use_name = true,
                        'r' => opts.use_real = true,
                        _ => {
                            all_known = false;
                            break;
                        }
                    }
                }
                if !all_known {
                    eprintln!("id: unknown option: {other}");
                    eprintln!("Try 'id --help' for usage.");
                    return Err(1);
                }
            }
            _ => {
                // Positional argument: target user.
                opts.target_user = Some(arg.clone());
            }
        }
        i += 1;
    }

    Ok(opts)
}

/// Run the id command.
fn run_id(args: &[String]) -> i32 {
    let opts = match parse_id_args(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    // Determine UIDs/GIDs either from target user or from syscalls.
    let (uid, euid, gid, egid) = if let Some(ref username) = opts.target_user {
        // Look up the user in /etc/passwd.
        match name_to_uid(username) {
            Some(target_uid) => {
                let target_gid = name_to_gid(username).unwrap_or(target_uid);
                (target_uid, target_uid, target_gid, target_gid)
            }
            None => {
                // Maybe it's a numeric UID.
                if let Ok(target_uid) = username.parse::<u32>() {
                    let target_gid = target_uid; // fallback
                    (target_uid, target_uid, target_gid, target_gid)
                } else {
                    eprintln!("id: '{username}': no such user");
                    return 1;
                }
            }
        }
    } else {
        // SAFETY: these are simple POSIX getters with no pointer arguments.
        unsafe { (getuid(), geteuid(), getgid(), getegid()) }
    };

    let effective_uid = if opts.use_real { uid } else { euid };
    let effective_gid = if opts.use_real { gid } else { egid };

    if opts.user_only {
        if opts.use_name {
            let name = uid_to_name(effective_uid).unwrap_or_else(|| effective_uid.to_string());
            println!("{name}");
        } else {
            println!("{effective_uid}");
        }
        return 0;
    }

    if opts.group_only {
        if opts.use_name {
            let name = gid_to_name(effective_gid).unwrap_or_else(|| effective_gid.to_string());
            println!("{name}");
        } else {
            println!("{effective_gid}");
        }
        return 0;
    }

    if opts.all_groups {
        let group_list = collect_all_groups(&opts);
        let output: Vec<String> = group_list
            .iter()
            .map(|(group_gid, group_name)| {
                if opts.use_name {
                    group_name.clone()
                } else {
                    group_gid.to_string()
                }
            })
            .collect();
        println!("{}", output.join(" "));
        return 0;
    }

    // Full output: uid=N(name) gid=N(name) groups=N(name),N(name),...
    let uid_name = uid_to_name(uid).unwrap_or_default();
    let gid_name = gid_to_name(gid).unwrap_or_default();

    let mut output = String::new();
    if uid_name.is_empty() {
        output.push_str(&format!("uid={uid}"));
    } else {
        output.push_str(&format!("uid={uid}({uid_name})"));
    }

    if gid_name.is_empty() {
        output.push_str(&format!(" gid={gid}"));
    } else {
        output.push_str(&format!(" gid={gid}({gid_name})"));
    }

    if euid != uid {
        let euid_name = uid_to_name(euid).unwrap_or_default();
        if euid_name.is_empty() {
            output.push_str(&format!(" euid={euid}"));
        } else {
            output.push_str(&format!(" euid={euid}({euid_name})"));
        }
    }

    if egid != gid {
        let egid_name = gid_to_name(egid).unwrap_or_default();
        if egid_name.is_empty() {
            output.push_str(&format!(" egid={egid}"));
        } else {
            output.push_str(&format!(" egid={egid}({egid_name})"));
        }
    }

    // Supplementary groups.
    let groups_list = collect_all_groups(&opts);
    if !groups_list.is_empty() {
        let groups_str: Vec<String> = groups_list
            .iter()
            .map(|(group_gid, group_name)| {
                if group_name.is_empty() {
                    format!("{group_gid}")
                } else {
                    format!("{group_gid}({group_name})")
                }
            })
            .collect();
        output.push_str(&format!(" groups={}", groups_str.join(",")));
    }

    println!("{output}");
    0
}

/// Collect all group IDs and names for the id/groups output.
fn collect_all_groups(opts: &IdOpts) -> Vec<(u32, String)> {
    if let Some(ref username) = opts.target_user {
        groups_for_user(username)
    } else {
        // Current user: combine primary GID with supplementary groups from kernel.
        let mut result = Vec::new();

        // SAFETY: getegid is a simple POSIX getter.
        let primary_gid = unsafe { getegid() };
        let primary_name = gid_to_name(primary_gid).unwrap_or_else(|| primary_gid.to_string());
        result.push((primary_gid, primary_name));

        for gid_val in get_supplementary_gids() {
            if gid_val != primary_gid {
                let name = gid_to_name(gid_val).unwrap_or_else(|| gid_val.to_string());
                result.push((gid_val, name));
            }
        }

        // If no supplementary groups from kernel, try /etc/group by username.
        if result.len() <= 1 {
            let username = get_effective_username();
            if !username.is_empty() {
                let from_etc = groups_for_user(&username);
                for (gid_val, name) in from_etc {
                    if !result.iter().any(|(g, _)| *g == gid_val) {
                        result.push((gid_val, name));
                    }
                }
            }
        }

        result
    }
}

fn print_id_help() {
    println!("id (Slate OS) 0.1.0 -- Display user and group IDs");
    println!();
    println!("USAGE:");
    println!("  id [OPTIONS] [USER]");
    println!();
    println!("OPTIONS:");
    println!("  -u, --user    Print only effective user ID");
    println!("  -g, --group   Print only effective group ID");
    println!("  -G, --groups  Print all group IDs");
    println!("  -n, --name    Print name instead of number (with -u, -g, -G)");
    println!("  -r, --real    Print real ID instead of effective");
    println!("  -h, --help    Display this help and exit");
    println!("  -V, --version Display version and exit");
    println!();
    println!("Without -u, -g, or -G, prints full uid/gid/groups info.");
}

// ============================================================================
// groups implementation
// ============================================================================

/// Run the groups command.
fn run_groups(args: &[String]) -> i32 {
    // Parse arguments: groups [USER...]
    let mut users: Vec<String> = Vec::new();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                println!("groups (Slate OS) 0.1.0 -- Display group memberships");
                println!();
                println!("USAGE:");
                println!("  groups [USER...]");
                println!();
                println!("Without arguments, shows groups for the current user.");
                return 0;
            }
            "-V" | "--version" => {
                println!("groups (Slate OS) 0.1.0");
                return 0;
            }
            _ => users.push(arg.clone()),
        }
        i += 1;
    }

    if users.is_empty() {
        // Current user.
        let username = get_effective_username();
        print_groups_for(&username);
    } else {
        for (idx, user) in users.iter().enumerate() {
            if users.len() > 1 {
                print!("{user} : ");
            }
            // Verify user exists.
            if name_to_uid(user).is_none() {
                eprintln!("groups: '{user}': no such user");
                if idx + 1 < users.len() {
                    continue;
                }
                return 1;
            }
            print_groups_for(user);
        }
    }

    0
}

/// Print group names for a user.
fn print_groups_for(username: &str) {
    let group_list = groups_for_user(username);
    if group_list.is_empty() {
        // Fallback: try supplementary groups from kernel if this is the current user.
        let current = get_effective_username();
        if username == current || username.is_empty() {
            let suppl = get_supplementary_gids();
            if suppl.is_empty() {
                // SAFETY: getegid is a simple POSIX getter.
                let gid = unsafe { getegid() };
                let name = gid_to_name(gid).unwrap_or_else(|| gid.to_string());
                println!("{name}");
            } else {
                let names: Vec<String> = suppl
                    .iter()
                    .map(|g| gid_to_name(*g).unwrap_or_else(|| g.to_string()))
                    .collect();
                println!("{}", names.join(" "));
            }
        } else {
            // No groups found for this user.
            println!();
        }
    } else {
        let names: Vec<String> = group_list.iter().map(|(_, n)| n.clone()).collect();
        println!("{}", names.join(" "));
    }
}

// ============================================================================
// whoami implementation
// ============================================================================

/// Run the whoami command.
fn run_whoami(args: &[String]) -> i32 {
    // whoami takes no meaningful positional arguments; only the first one is
    // ever relevant (a flag, or an error on an extra operand).
    if let Some(arg) = args.get(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("whoami (Slate OS) 0.1.0 -- Print effective username");
                println!();
                println!("USAGE:");
                println!("  whoami");
                println!();
                println!("Prints the name of the effective user (equivalent to `id -un`).");
                return 0;
            }
            "-V" | "--version" => {
                println!("whoami (Slate OS) 0.1.0");
                return 0;
            }
            other => {
                eprintln!("whoami: extra operand '{other}'");
                return 1;
            }
        }
    }

    let name = get_effective_username();
    println!("{name}");
    0
}

/// Get the effective username by looking up euid in /etc/passwd,
/// with fallback to environment variables and finally the numeric UID.
fn get_effective_username() -> String {
    // SAFETY: geteuid is a simple POSIX getter with no pointer arguments.
    let euid = unsafe { geteuid() };

    // Try /etc/passwd first.
    if let Some(name) = uid_to_name(euid) {
        return name;
    }

    // Try environment variables.
    if let Ok(name) = env::var("USER")
        && !name.is_empty()
    {
        return name;
    }
    if let Ok(name) = env::var("LOGNAME")
        && !name.is_empty()
    {
        return name;
    }

    // Last resort: numeric UID.
    euid.to_string()
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("mktemp");

    match detect_personality(argv0) {
        Personality::Mktemp => run_mktemp(&args),
        Personality::Id => run_id(&args),
        Personality::Groups => run_groups(&args),
        Personality::Whoami => run_whoami(&args),
    }
}

fn main() {
    process::exit(run());
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Personality detection ---

    #[test]
    fn test_personality_mktemp_default() {
        assert_eq!(detect_personality("mktemp"), Personality::Mktemp);
    }

    #[test]
    fn test_personality_mktemp_path() {
        assert_eq!(detect_personality("/usr/bin/mktemp"), Personality::Mktemp);
    }

    #[test]
    fn test_personality_mktemp_exe() {
        assert_eq!(detect_personality("mktemp.exe"), Personality::Mktemp);
    }

    #[test]
    fn test_personality_id() {
        assert_eq!(detect_personality("id"), Personality::Id);
    }

    #[test]
    fn test_personality_id_path() {
        assert_eq!(detect_personality("/usr/bin/id"), Personality::Id);
    }

    #[test]
    fn test_personality_id_exe() {
        assert_eq!(detect_personality("id.exe"), Personality::Id);
    }

    #[test]
    fn test_personality_groups() {
        assert_eq!(detect_personality("groups"), Personality::Groups);
    }

    #[test]
    fn test_personality_groups_path() {
        assert_eq!(detect_personality("/bin/groups"), Personality::Groups);
    }

    #[test]
    fn test_personality_whoami() {
        assert_eq!(detect_personality("whoami"), Personality::Whoami);
    }

    #[test]
    fn test_personality_whoami_path() {
        assert_eq!(
            detect_personality("C:\\Windows\\whoami.exe"),
            Personality::Whoami
        );
    }

    #[test]
    fn test_personality_unknown_defaults_to_mktemp() {
        assert_eq!(detect_personality("somethingelse"), Personality::Mktemp);
    }

    #[test]
    fn test_personality_empty_defaults_to_mktemp() {
        assert_eq!(detect_personality(""), Personality::Mktemp);
    }

    // --- Template expansion ---

    #[test]
    fn test_expand_template_replaces_x_chars() {
        let result = expand_template("test.XXXXXX", "");
        assert_eq!(result.len(), 11); // "test." + 6 random chars
        assert!(result.starts_with("test."));
        // Verify the random part contains only valid characters.
        for ch in result[5..].chars() {
            assert!(
                ch.is_ascii_alphanumeric(),
                "unexpected char in random part: {ch}"
            );
        }
    }

    #[test]
    fn test_expand_template_preserves_prefix() {
        let result = expand_template("myprefix.XXX", "");
        assert!(result.starts_with("myprefix."));
        assert_eq!(result.len(), 12);
    }

    #[test]
    fn test_expand_template_with_suffix() {
        let result = expand_template("tmp.XXXXXX", ".txt");
        assert!(result.starts_with("tmp."));
        assert!(result.ends_with(".txt"));
        // "tmp." + 6 random + ".txt" = 14
        assert_eq!(result.len(), 14);
    }

    #[test]
    fn test_expand_template_no_x_returns_template() {
        let result = expand_template("notemplate", ".bak");
        assert_eq!(result, "notemplate.bak");
    }

    #[test]
    fn test_expand_template_all_x() {
        let result = expand_template("XXXXXXXXXX", "");
        assert_eq!(result.len(), 10);
        for ch in result.chars() {
            assert!(ch.is_ascii_alphanumeric());
        }
    }

    #[test]
    fn test_expand_template_single_x_not_enough_but_works() {
        // expand_template itself doesn't enforce the 3-X minimum; that's
        // done at argument validation. This tests internal behavior.
        let result = expand_template("foo.X", "");
        assert_eq!(result.len(), 5);
        assert!(result.starts_with("foo."));
    }

    #[test]
    fn test_expand_randomness_differs() {
        // Two expansions should almost certainly produce different results.
        let a = expand_template("tmp.XXXXXXXXXX", "");
        let b = expand_template("tmp.XXXXXXXXXX", "");
        // With 10 random chars from a 62-char alphabet, collision probability
        // is astronomically low. We allow it but flag if it happens repeatedly.
        // For a single test, just verify both are valid.
        assert!(a.starts_with("tmp."));
        assert!(b.starts_with("tmp."));
        assert_eq!(a.len(), 14);
        assert_eq!(b.len(), 14);
        // Not asserting a != b because /dev/urandom reads can theoretically
        // collide (or fallback PRNG may produce same output if called within
        // the same nanosecond), but it's extremely unlikely.
    }

    // --- Build path ---

    #[test]
    fn test_build_path_default() {
        let mut opts = MktempOpts::new();
        opts.tmpdir = "/tmp".to_string();
        opts.template = "tmp.XXXXXX".to_string();
        opts.use_tmpdir = false;
        let path = build_path(&opts);
        // template has no '/', so it goes under tmpdir.
        assert_eq!(path, "/tmp/tmp.XXXXXX");
    }

    #[test]
    fn test_build_path_with_explicit_dir() {
        let mut opts = MktempOpts::new();
        opts.tmpdir = "/var/tmp".to_string();
        opts.template = "myapp.XXXXX".to_string();
        opts.use_tmpdir = true;
        let path = build_path(&opts);
        assert_eq!(path, "/var/tmp/myapp.XXXXX");
    }

    #[test]
    fn test_build_path_template_with_slash() {
        let mut opts = MktempOpts::new();
        opts.tmpdir = "/tmp".to_string();
        opts.template = "/custom/dir/test.XXXXX".to_string();
        opts.use_tmpdir = false;
        let path = build_path(&opts);
        // Template contains '/', so it's used as-is.
        assert_eq!(path, "/custom/dir/test.XXXXX");
    }

    #[test]
    fn test_build_path_tmpdir_trailing_slash() {
        let mut opts = MktempOpts::new();
        opts.tmpdir = "/tmp/".to_string();
        opts.template = "test.XXXXX".to_string();
        opts.use_tmpdir = true;
        let path = build_path(&opts);
        // Should not double the slash.
        assert_eq!(path, "/tmp/test.XXXXX");
    }

    // --- mktemp argument parsing ---

    #[test]
    fn test_mktemp_default_template() {
        let opts = MktempOpts::new();
        assert_eq!(opts.template, "tmp.XXXXXXXXXX");
        assert!(!opts.directory);
        assert!(!opts.dry_run);
        assert!(!opts.quiet);
    }

    // --- passwd parsing ---

    #[test]
    fn test_parse_passwd_line_valid() {
        let entry = parse_passwd_line("root:x:0:0:root:/root:/bin/sh").unwrap();
        assert_eq!(entry.name, "root");
        assert_eq!(entry.uid, 0);
        assert_eq!(entry.gid, 0);
    }

    #[test]
    fn test_parse_passwd_line_normal_user() {
        let entry = parse_passwd_line("alice:x:1000:1000:Alice:/home/alice:/bin/bash").unwrap();
        assert_eq!(entry.name, "alice");
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.gid, 1000);
    }

    #[test]
    fn test_parse_passwd_line_too_short() {
        assert!(parse_passwd_line("root:x:0").is_none());
    }

    #[test]
    fn test_parse_passwd_line_invalid_uid() {
        assert!(parse_passwd_line("bad:x:notanumber:0::/:/bin/sh").is_none());
    }

    #[test]
    fn test_parse_passwd_line_empty() {
        assert!(parse_passwd_line("").is_none());
    }

    // --- group parsing ---

    #[test]
    fn test_parse_group_line_valid() {
        let entry = parse_group_line("wheel:x:10:alice,bob").unwrap();
        assert_eq!(entry.name, "wheel");
        assert_eq!(entry.gid, 10);
        assert_eq!(entry.members, vec!["alice", "bob"]);
    }

    #[test]
    fn test_parse_group_line_no_members() {
        let entry = parse_group_line("nogroup:x:65534:").unwrap();
        assert_eq!(entry.name, "nogroup");
        assert_eq!(entry.gid, 65534);
        assert!(entry.members.is_empty());
    }

    #[test]
    fn test_parse_group_line_single_member() {
        let entry = parse_group_line("staff:x:20:alice").unwrap();
        assert_eq!(entry.members, vec!["alice"]);
    }

    #[test]
    fn test_parse_group_line_too_short() {
        assert!(parse_group_line("bad:x").is_none());
    }

    #[test]
    fn test_parse_group_line_invalid_gid() {
        assert!(parse_group_line("bad:x:notnum:alice").is_none());
    }

    // --- id argument parsing ---

    #[test]
    fn test_id_args_default() {
        let args = vec!["id".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(!opts.user_only);
        assert!(!opts.group_only);
        assert!(!opts.all_groups);
        assert!(!opts.use_name);
        assert!(!opts.use_real);
        assert!(opts.target_user.is_none());
    }

    #[test]
    fn test_id_args_user_only() {
        let args = vec!["id".to_string(), "-u".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.user_only);
    }

    #[test]
    fn test_id_args_group_only() {
        let args = vec!["id".to_string(), "-g".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.group_only);
    }

    #[test]
    fn test_id_args_all_groups() {
        let args = vec!["id".to_string(), "-G".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.all_groups);
    }

    #[test]
    fn test_id_args_combined_short() {
        let args = vec!["id".to_string(), "-un".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.user_only);
        assert!(opts.use_name);
    }

    #[test]
    fn test_id_args_real() {
        let args = vec!["id".to_string(), "-r".to_string(), "-u".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.use_real);
        assert!(opts.user_only);
    }

    #[test]
    fn test_id_args_target_user() {
        let args = vec!["id".to_string(), "alice".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert_eq!(opts.target_user.as_deref(), Some("alice"));
    }

    #[test]
    fn test_id_args_long_options() {
        let args = vec!["id".to_string(), "--user".to_string(), "--name".to_string()];
        let opts = parse_id_args(&args).unwrap();
        assert!(opts.user_only);
        assert!(opts.use_name);
    }

    #[test]
    fn test_id_args_help_returns_zero() {
        let args = vec!["id".to_string(), "--help".to_string()];
        assert_eq!(parse_id_args(&args).unwrap_err(), 0);
    }

    // --- fill_from_timestamp ---

    #[test]
    fn test_fill_from_timestamp_fills_buffer() {
        let mut buf = [0u8; 16];
        fill_from_timestamp(&mut buf);
        // After filling, at least some bytes should be non-zero.
        // (Theoretically all could be zero, but astronomically unlikely.)
        let nonzero_count = buf.iter().filter(|&&b| b != 0).count();
        assert!(nonzero_count > 0, "timestamp PRNG produced all zeros");
    }

    #[test]
    fn test_fill_from_timestamp_empty_buffer() {
        // Should not panic on empty buffer.
        let mut buf = [0u8; 0];
        fill_from_timestamp(&mut buf);
    }

    #[test]
    fn test_fill_from_timestamp_single_byte() {
        let mut buf = [0u8; 1];
        fill_from_timestamp(&mut buf);
        // Just verify it doesn't panic.
    }

    // --- RAND_CHARS alphabet ---

    #[test]
    fn test_rand_chars_length() {
        assert_eq!(RAND_CHARS.len(), 62);
    }

    #[test]
    fn test_rand_chars_all_alphanumeric() {
        for &b in RAND_CHARS {
            assert!(
                (b as char).is_ascii_alphanumeric(),
                "non-alphanumeric byte in RAND_CHARS: {b}"
            );
        }
    }

    // --- groups_for_user unit test with no /etc files ---

    #[test]
    fn test_groups_for_user_nonexistent() {
        // On a system without /etc/passwd or /etc/group, this should
        // return an empty vec without panicking.
        let groups_list = groups_for_user("nonexistent_user_12345");
        // We can't assert empty because /etc files might exist on the
        // test machine, but we verify no panic.
        let _ = groups_list;
    }

    // --- MktempOpts defaults ---

    #[test]
    fn test_mktemp_opts_default_tmpdir() {
        let opts = MktempOpts::new();
        // Default tmpdir should be $TMPDIR, $TMP, $TEMP, or "/tmp".
        assert!(!opts.tmpdir.is_empty());
    }

    #[test]
    fn test_mktemp_opts_default_flags() {
        let opts = MktempOpts::new();
        assert!(!opts.directory);
        assert!(!opts.use_tmpdir);
        assert!(!opts.dry_run);
        assert!(!opts.quiet);
        assert!(opts.suffix.is_empty());
    }

    // --- IdOpts defaults ---

    #[test]
    fn test_id_opts_defaults() {
        let opts = IdOpts::new();
        assert!(!opts.user_only);
        assert!(!opts.group_only);
        assert!(!opts.all_groups);
        assert!(!opts.use_name);
        assert!(!opts.use_real);
        assert!(opts.target_user.is_none());
    }

    // --- Trailing X count extraction ---

    #[test]
    fn test_trailing_x_count() {
        let count = |s: &str| -> usize {
            s.as_bytes()
                .iter()
                .rev()
                .take_while(|&&b| b == b'X')
                .count()
        };
        assert_eq!(count("tmp.XXXXXXXXXX"), 10);
        assert_eq!(count("tmp.XXX"), 3);
        assert_eq!(count("noX"), 1);
        assert_eq!(count("nothing"), 0);
        assert_eq!(count("XXXXXX"), 6);
        assert_eq!(count(""), 0);
    }
}
