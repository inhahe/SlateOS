//! SlateOS User Account Management
//!
//! Unified tool for creating, modifying, and deleting user accounts.
//! Manages /etc/users.yaml (our OS's user database format) and
//! user home directories.
//!
//! # Usage
//!
//! ```text
//! useradm add <username>           Create a new user
//! useradm del <username>           Delete a user
//! useradm mod <username> [opts]    Modify a user
//! useradm passwd <username>        Change a user's password
//! useradm list                     List all users
//! useradm info <username>          Show user details
//! useradm lock <username>          Lock account (disable login)
//! useradm unlock <username>        Unlock account
//! useradm groups <username>        Show group memberships
//! ```

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// User data model
// ============================================================================

#[derive(Clone)]
struct User {
    uid: u32,
    username: String,
    display_name: String,
    password_hash: String,
    salt: String,
    shell: String,
    home: String,
    groups: Vec<String>,
    admin: bool,
    locked: bool,
    avatar: String,
}

impl User {
    fn new(uid: u32, username: &str) -> Self {
        User {
            uid,
            username: username.to_string(),
            display_name: username.to_string(),
            password_hash: String::new(),
            salt: String::new(),
            shell: "/bin/sh".to_string(),
            home: format!("/home/{username}"),
            groups: vec!["users".to_string()],
            admin: false,
            locked: false,
            avatar: String::new(),
        }
    }
}

// ============================================================================
// YAML user database
// ============================================================================

const USER_DB_PATH: &str = "/etc/users.yaml";

/// Read all users from /etc/users.yaml.
///
/// Our format (simplified YAML):
/// ```yaml
/// users:
///   - uid: 0
///     username: root
///     display_name: "System Administrator"
///     password_hash: "..."
///     salt: "..."
///     shell: /bin/sh
///     home: /root
///     groups: [root, admin]
///     admin: true
///     locked: false
/// ```
fn read_users() -> Vec<User> {
    let content = match fs::read_to_string(USER_DB_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut users = Vec::new();
    let mut current: Option<User> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
            // New user entry.
            if let Some(user) = current.take() {
                users.push(user);
            }
            let uid: u32 = trimmed.split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            current = Some(User::new(uid, ""));
        } else if let Some(ref mut user) = current {
            if let Some(val) = trimmed.strip_prefix("username:") {
                user.username = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("display_name:") {
                user.display_name = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("password_hash:") {
                user.password_hash = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("salt:") {
                user.salt = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("shell:") {
                user.shell = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("home:") {
                user.home = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("groups:") {
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                user.groups = val.split(',')
                    .map(|g| g.trim().trim_matches('"').to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            } else if let Some(val) = trimmed.strip_prefix("admin:") {
                user.admin = val.trim() == "true";
            } else if let Some(val) = trimmed.strip_prefix("locked:") {
                user.locked = val.trim() == "true";
            } else if let Some(val) = trimmed.strip_prefix("avatar:") {
                user.avatar = val.trim().trim_matches('"').to_string();
            }
        }
    }

    if let Some(user) = current {
        users.push(user);
    }

    users
}

/// Write all users back to /etc/users.yaml.
fn write_users(users: &[User]) -> Result<(), String> {
    let mut yaml = String::from("# SlateOS user database\n# Managed by useradm — do not edit manually\nusers:\n");

    for user in users {
        yaml.push_str(&format!("  - uid: {}\n", user.uid));
        yaml.push_str(&format!("    username: \"{}\"\n", user.username));
        yaml.push_str(&format!("    display_name: \"{}\"\n", user.display_name));
        yaml.push_str(&format!("    password_hash: \"{}\"\n", user.password_hash));
        yaml.push_str(&format!("    salt: \"{}\"\n", user.salt));
        yaml.push_str(&format!("    shell: \"{}\"\n", user.shell));
        yaml.push_str(&format!("    home: \"{}\"\n", user.home));

        let groups_str: Vec<String> = user.groups.iter()
            .map(|g| format!("\"{g}\""))
            .collect();
        yaml.push_str(&format!("    groups: [{}]\n", groups_str.join(", ")));
        yaml.push_str(&format!("    admin: {}\n", user.admin));
        yaml.push_str(&format!("    locked: {}\n", user.locked));

        if !user.avatar.is_empty() {
            yaml.push_str(&format!("    avatar: \"{}\"\n", user.avatar));
        }
    }

    fs::write(USER_DB_PATH, yaml).map_err(|e| format!("write error: {e}"))
}

// ============================================================================
// Password hashing
// ============================================================================

/// Simple password hash using SHA-256 with salt.
/// Returns (hash_hex, salt_hex).
fn hash_password(password: &str, salt: &str) -> String {
    // SHA-256 of salt + password (simplified — real implementation would
    // use PBKDF2 or Argon2, but we only have std available).
    let input = format!("{salt}{password}");
    sha256_hex(input.as_bytes())
}

/// Generate a random salt by reading /dev/urandom.
fn generate_salt() -> String {
    let mut bytes = [0u8; 16];

    if let Ok(data) = fs::read("/dev/urandom") {
        for (i, b) in data.iter().take(16).enumerate() {
            bytes[i] = *b;
        }
    } else {
        // Fallback: use uptime as entropy source (not cryptographically secure).
        if let Ok(uptime) = fs::read_to_string("/proc/uptime") {
            let hash_input = format!("salt-{uptime}");
            let hex = sha256_hex(hash_input.as_bytes());
            return hex[..32].to_string();
        }
    }

    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 hash (minimal implementation for password hashing).
fn sha256_hex(data: &[u8]) -> String {
    // SHA-256 constants.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Padding.
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process 64-byte blocks.
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{v:08x}")).collect()
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_add(username: &str, args: &[String]) {
    let mut users = read_users();

    if users.iter().any(|u| u.username == username) {
        eprintln!("error: user '{}' already exists", username);
        process::exit(1);
    }

    // Validate username.
    if username.is_empty() || username.len() > 32 {
        eprintln!("error: username must be 1-32 characters");
        process::exit(1);
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        eprintln!("error: username must be alphanumeric (plus _ - .)");
        process::exit(1);
    }

    // Next UID.
    let uid = users.iter().map(|u| u.uid).max().unwrap_or(999) + 1;
    let uid = if uid < 1000 { 1000 } else { uid };

    let mut user = User::new(uid, username);

    // Parse optional arguments.
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--shell" | "-s" => {
                if i + 1 < args.len() {
                    user.shell = args[i + 1].clone();
                    i += 2;
                } else { i += 1; }
            }
            "--home" | "-d" => {
                if i + 1 < args.len() {
                    user.home = args[i + 1].clone();
                    i += 2;
                } else { i += 1; }
            }
            "--name" | "-c" => {
                if i + 1 < args.len() {
                    user.display_name = args[i + 1].clone();
                    i += 2;
                } else { i += 1; }
            }
            "--groups" | "-G" => {
                if i + 1 < args.len() {
                    user.groups = args[i + 1].split(',')
                        .map(|g| g.trim().to_string())
                        .collect();
                    i += 2;
                } else { i += 1; }
            }
            "--admin" => {
                user.admin = true;
                if !user.groups.contains(&"admin".to_string()) {
                    user.groups.push("admin".to_string());
                }
                i += 1;
            }
            "--uid" | "-u" => {
                if i + 1 < args.len() {
                    user.uid = args[i + 1].parse().unwrap_or(uid);
                    i += 2;
                } else { i += 1; }
            }
            _ => i += 1,
        }
    }

    // Prompt for password.
    print!("Password for {username}: ");
    let _ = io::stdout().flush();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        eprintln!("error reading password");
        process::exit(1);
    }
    let password = password.trim();

    if password.is_empty() {
        eprintln!("error: password cannot be empty");
        process::exit(1);
    }

    let salt = generate_salt();
    user.password_hash = hash_password(password, &salt);
    user.salt = salt;

    users.push(user.clone());

    if let Err(e) = write_users(&users) {
        eprintln!("error writing user database: {e}");
        process::exit(1);
    }

    // Create home directory.
    if let Err(e) = fs::create_dir_all(&user.home) {
        eprintln!("warning: could not create home directory {}: {e}", user.home);
    }

    println!("Created user '{}' (uid={}, home={})", user.username, user.uid, user.home);
}

fn cmd_del(username: &str) {
    let mut users = read_users();
    let before = users.len();

    // Prevent deleting root.
    if username == "root" {
        eprintln!("error: cannot delete root user");
        process::exit(1);
    }

    users.retain(|u| u.username != username);

    if users.len() == before {
        eprintln!("error: user '{}' not found", username);
        process::exit(1);
    }

    if let Err(e) = write_users(&users) {
        eprintln!("error writing user database: {e}");
        process::exit(1);
    }

    // Optionally remove home directory.
    let home = format!("/home/{username}");
    if std::path::Path::new(&home).exists() {
        print!("Remove home directory {}? [y/N] ", home);
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_ok()
            && answer.trim().to_lowercase() == "y" {
                if let Err(e) = fs::remove_dir_all(&home) {
                    eprintln!("warning: could not remove {home}: {e}");
                } else {
                    println!("Removed {home}");
                }
            }
    }

    println!("Deleted user '{}'", username);
}

fn cmd_passwd(username: &str) {
    let mut users = read_users();

    let user = match users.iter_mut().find(|u| u.username == username) {
        Some(u) => u,
        None => {
            eprintln!("error: user '{}' not found", username);
            process::exit(1);
        }
    };

    print!("New password for {username}: ");
    let _ = io::stdout().flush();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        eprintln!("error reading password");
        process::exit(1);
    }
    let password = password.trim();

    if password.is_empty() {
        eprintln!("error: password cannot be empty");
        process::exit(1);
    }

    print!("Confirm password: ");
    let _ = io::stdout().flush();
    let mut confirm = String::new();
    if io::stdin().read_line(&mut confirm).is_err() {
        eprintln!("error reading confirmation");
        process::exit(1);
    }

    if password != confirm.trim() {
        eprintln!("error: passwords do not match");
        process::exit(1);
    }

    let salt = generate_salt();
    user.password_hash = hash_password(password, &salt);
    user.salt = salt;

    if let Err(e) = write_users(&users) {
        eprintln!("error writing user database: {e}");
        process::exit(1);
    }

    println!("Password updated for '{}'", username);
}

fn cmd_list() {
    let users = read_users();

    if users.is_empty() {
        println!("No users found (is {} readable?)", USER_DB_PATH);
        return;
    }

    println!("{:<6} {:<16} {:<24} {:<16} {:<6} Groups",
        "UID", "Username", "Display Name", "Shell", "Admin");
    println!("{:<6} {:<16} {:<24} {:<16} {:<6} ------",
        "---", "--------", "------------", "-----", "-----");

    for user in &users {
        let admin = if user.admin { "yes" } else { "no" };
        let locked = if user.locked { " (locked)" } else { "" };
        println!("{:<6} {:<16} {:<24} {:<16} {:<6} {}{}",
            user.uid,
            user.username,
            if user.display_name.len() > 22 {
                &user.display_name[..22]
            } else {
                &user.display_name
            },
            user.shell,
            admin,
            user.groups.join(", "),
            locked,
        );
    }
}

fn cmd_info(username: &str) {
    let users = read_users();

    let user = match users.iter().find(|u| u.username == username) {
        Some(u) => u,
        None => {
            eprintln!("error: user '{}' not found", username);
            process::exit(1);
        }
    };

    println!("Username:     {}", user.username);
    println!("UID:          {}", user.uid);
    println!("Display name: {}", user.display_name);
    println!("Home:         {}", user.home);
    println!("Shell:        {}", user.shell);
    println!("Groups:       {}", user.groups.join(", "));
    println!("Admin:        {}", if user.admin { "yes" } else { "no" });
    println!("Locked:       {}", if user.locked { "yes" } else { "no" });
    if !user.avatar.is_empty() {
        println!("Avatar:       {}", user.avatar);
    }
}

fn cmd_lock(username: &str) {
    let mut users = read_users();
    let user = match users.iter_mut().find(|u| u.username == username) {
        Some(u) => u,
        None => {
            eprintln!("error: user '{}' not found", username);
            process::exit(1);
        }
    };

    user.locked = true;
    if let Err(e) = write_users(&users) {
        eprintln!("error: {e}");
        process::exit(1);
    }
    println!("Locked account '{}'", username);
}

fn cmd_unlock(username: &str) {
    let mut users = read_users();
    let user = match users.iter_mut().find(|u| u.username == username) {
        Some(u) => u,
        None => {
            eprintln!("error: user '{}' not found", username);
            process::exit(1);
        }
    };

    user.locked = false;
    if let Err(e) = write_users(&users) {
        eprintln!("error: {e}");
        process::exit(1);
    }
    println!("Unlocked account '{}'", username);
}

fn cmd_groups(username: &str) {
    let users = read_users();
    let user = match users.iter().find(|u| u.username == username) {
        Some(u) => u,
        None => {
            eprintln!("error: user '{}' not found", username);
            process::exit(1);
        }
    };

    println!("{}: {}", user.username, user.groups.join(" "));
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("SlateOS User Account Manager v0.1.0");
    println!();
    println!("Manage user accounts in the system.");
    println!();
    println!("USAGE:");
    println!("  useradm <command> <username> [options]");
    println!();
    println!("COMMANDS:");
    println!("  add <user> [opts]   Create a new user");
    println!("  del <user>          Delete a user");
    println!("  mod <user> [opts]   Modify user properties");
    println!("  passwd <user>       Change password");
    println!("  list                List all users");
    println!("  info <user>         Show user details");
    println!("  lock <user>         Lock account (prevent login)");
    println!("  unlock <user>       Unlock account");
    println!("  groups <user>       Show group memberships");
    println!();
    println!("ADD/MOD OPTIONS:");
    println!("  --shell, -s <path>  Login shell (default: /bin/sh)");
    println!("  --home, -d <path>   Home directory (default: /home/<user>)");
    println!("  --name, -c <name>   Display name");
    println!("  --groups, -G <g,g>  Group memberships (comma-separated)");
    println!("  --admin             Grant admin privileges");
    println!("  --uid, -u <uid>     Set specific UID");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "add" | "useradd" | "create" => {
            if args.len() < 3 {
                eprintln!("error: 'add' requires a username");
                process::exit(1);
            }
            cmd_add(&args[2], &args[3..]);
        }
        "del" | "userdel" | "delete" | "rm" => {
            if args.len() < 3 {
                eprintln!("error: 'del' requires a username");
                process::exit(1);
            }
            cmd_del(&args[2]);
        }
        "mod" | "usermod" | "modify" => {
            if args.len() < 3 {
                eprintln!("error: 'mod' requires a username");
                process::exit(1);
            }
            // Modify reuses add logic on existing user.
            let mut users = read_users();
            let idx = match users.iter().position(|u| u.username == args[2]) {
                Some(i) => i,
                None => {
                    eprintln!("error: user '{}' not found", args[2]);
                    process::exit(1);
                }
            };

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--shell" | "-s" if i + 1 < args.len() => {
                        users[idx].shell = args[i + 1].clone();
                        i += 2;
                    }
                    "--home" | "-d" if i + 1 < args.len() => {
                        users[idx].home = args[i + 1].clone();
                        i += 2;
                    }
                    "--name" | "-c" if i + 1 < args.len() => {
                        users[idx].display_name = args[i + 1].clone();
                        i += 2;
                    }
                    "--groups" | "-G" if i + 1 < args.len() => {
                        users[idx].groups = args[i + 1].split(',')
                            .map(|g| g.trim().to_string())
                            .collect();
                        i += 2;
                    }
                    "--admin" => {
                        users[idx].admin = true;
                        if !users[idx].groups.contains(&"admin".to_string()) {
                            users[idx].groups.push("admin".to_string());
                        }
                        i += 1;
                    }
                    _ => i += 1,
                }
            }

            if let Err(e) = write_users(&users) {
                eprintln!("error: {e}");
                process::exit(1);
            }
            println!("Modified user '{}'", args[2]);
        }
        "passwd" | "password" => {
            if args.len() < 3 {
                eprintln!("error: 'passwd' requires a username");
                process::exit(1);
            }
            cmd_passwd(&args[2]);
        }
        "list" | "ls" => cmd_list(),
        "info" | "show" => {
            if args.len() < 3 {
                eprintln!("error: 'info' requires a username");
                process::exit(1);
            }
            cmd_info(&args[2]);
        }
        "lock" => {
            if args.len() < 3 {
                eprintln!("error: 'lock' requires a username");
                process::exit(1);
            }
            cmd_lock(&args[2]);
        }
        "unlock" => {
            if args.len() < 3 {
                eprintln!("error: 'unlock' requires a username");
                process::exit(1);
            }
            cmd_unlock(&args[2]);
        }
        "groups" => {
            if args.len() < 3 {
                eprintln!("error: 'groups' requires a username");
                process::exit(1);
            }
            cmd_groups(&args[2]);
        }
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'useradm help' for usage.");
            process::exit(1);
        }
    }
}
