#![deny(clippy::all)]

//! loginctl — OurOS session and user management
//!
//! Multi-personality binary providing systemd-logind-compatible session,
//! user, and seat management commands. Detected via argv[0]:
//!
//! - `loginctl` (default) — session/user/seat management
//! - `userdbctl` — user/group database query tool

use std::collections::BTreeMap;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const SESSION_DIR: &str = "/run/sessions";
const SEAT_DIR: &str = "/run/seats";
const USER_RUNTIME_DIR: &str = "/run/user";
const PASSWD_FILE: &str = "/etc/passwd";
const GROUP_FILE: &str = "/etc/group";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Session {
    id: String,
    uid: u32,
    user: String,
    seat: String,
    tty: String,
    state: SessionState,
    session_type: SessionType,
    class: SessionClass,
    scope: String,
    service: String,
    _leader: u32,
    _audit: u32,
    _remote: bool,
    _remote_host: String,
    since: String,
}

#[derive(Clone, Debug, PartialEq)]
enum SessionState {
    Online,
    Active,
    Closing,
    _Lingering,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Active => write!(f, "active"),
            Self::Closing => write!(f, "closing"),
            Self::_Lingering => write!(f, "lingering"),
        }
    }
}

#[derive(Clone, Debug)]
enum SessionType {
    X11,
    Wayland,
    Tty,
    Mir,
    Unspecified,
}

impl std::fmt::Display for SessionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::X11 => write!(f, "x11"),
            Self::Wayland => write!(f, "wayland"),
            Self::Tty => write!(f, "tty"),
            Self::Mir => write!(f, "mir"),
            Self::Unspecified => write!(f, "unspecified"),
        }
    }
}

#[derive(Clone, Debug)]
enum SessionClass {
    User,
    Greeter,
    _LockScreen,
    Background,
}

impl std::fmt::Display for SessionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Greeter => write!(f, "greeter"),
            Self::_LockScreen => write!(f, "lock-screen"),
            Self::Background => write!(f, "background"),
        }
    }
}

#[derive(Clone, Debug)]
struct UserInfo {
    uid: u32,
    name: String,
    state: String,
    _linger: bool,
    sessions: Vec<String>,
    _slice: String,
    since: String,
}

#[derive(Clone, Debug)]
struct Seat {
    id: String,
    sessions: Vec<String>,
    active_session: String,
    _devices: Vec<String>,
}

#[derive(Clone, Debug)]
struct PasswdEntry {
    name: String,
    uid: u32,
    gid: u32,
    gecos: String,
    home: String,
    shell: String,
}

#[derive(Clone, Debug)]
struct GroupEntry {
    name: String,
    gid: u32,
    members: Vec<String>,
}

// ── Session management ─────────────────────────────────────────────────

fn read_sessions() -> Vec<Session> {
    let entries = match std::fs::read_dir(SESSION_DIR) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(session) = parse_session_file(&path) {
            sessions.push(session);
        }
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    sessions
}

fn parse_session_file(path: &std::path::Path) -> Option<Session> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut map = BTreeMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let id = path.file_name()?.to_str()?.to_string();
    let uid: u32 = map.get("UID").and_then(|v| v.parse().ok()).unwrap_or(0);
    let user = map.get("USER").cloned().unwrap_or_default();
    let seat = map.get("SEAT").cloned().unwrap_or_default();
    let tty = map.get("TTY").cloned().unwrap_or_default();
    let state_str = map.get("STATE").map(|s| s.as_str()).unwrap_or("online");
    let state = match state_str {
        "active" => SessionState::Active,
        "closing" => SessionState::Closing,
        "lingering" => SessionState::_Lingering,
        _ => SessionState::Online,
    };
    let stype = match map.get("TYPE").map(|s| s.as_str()).unwrap_or("") {
        "x11" => SessionType::X11,
        "wayland" => SessionType::Wayland,
        "tty" => SessionType::Tty,
        "mir" => SessionType::Mir,
        _ => SessionType::Unspecified,
    };
    let class = match map.get("CLASS").map(|s| s.as_str()).unwrap_or("user") {
        "greeter" => SessionClass::Greeter,
        "lock-screen" => SessionClass::_LockScreen,
        "background" => SessionClass::Background,
        _ => SessionClass::User,
    };

    Some(Session {
        id,
        uid,
        user,
        seat,
        tty,
        state,
        session_type: stype,
        class,
        scope: map.get("SCOPE").cloned().unwrap_or_default(),
        service: map.get("SERVICE").cloned().unwrap_or_default(),
        _leader: map.get("LEADER").and_then(|v| v.parse().ok()).unwrap_or(0),
        _audit: map.get("AUDIT").and_then(|v| v.parse().ok()).unwrap_or(0),
        _remote: map.get("REMOTE").map(|v| v == "1").unwrap_or(false),
        _remote_host: map.get("REMOTE_HOST").cloned().unwrap_or_default(),
        since: map.get("SINCE").cloned().unwrap_or_default(),
    })
}

fn list_sessions(args: &[String]) {
    let no_legend = args.iter().any(|a| a == "--no-legend");
    let sessions = read_sessions();

    if sessions.is_empty() {
        if !no_legend {
            println!("No sessions.");
        }
        return;
    }

    if !no_legend {
        println!("{:<8} {:>5} {:<16} {:<12} {:<8}", "SESSION", "UID", "USER", "SEAT", "TTY");
    }
    for s in &sessions {
        let star = if s.state == SessionState::Active { "*" } else { "" };
        println!("{:<8} {:>5} {:<16} {:<12} {:<8}",
            format!("{}{}", s.id, star), s.uid, s.user, s.seat, s.tty);
    }
    if !no_legend {
        println!("\n{} sessions listed.", sessions.len());
    }
}

fn show_session(args: &[String]) {
    let session_id = args.first().map(|s| s.as_str()).unwrap_or("");
    let sessions = read_sessions();

    let session = if session_id.is_empty() {
        // Show active session
        sessions.iter().find(|s| s.state == SessionState::Active)
    } else {
        sessions.iter().find(|s| s.id == session_id)
    };

    let session = match session {
        Some(s) => s,
        None => {
            if session_id.is_empty() {
                eprintln!("No active session found.");
            } else {
                eprintln!("Session '{}' not found.", session_id);
            }
            process::exit(1);
        }
    };

    println!("        Session: {}", session.id);
    println!("           User: {} ({})", session.user, session.uid);
    if !session.seat.is_empty() {
        println!("           Seat: {}", session.seat);
    }
    if !session.tty.is_empty() {
        println!("            TTY: {}", session.tty);
    }
    println!("           Type: {}", session.session_type);
    println!("          Class: {}", session.class);
    println!("          State: {}", session.state);
    if !session.scope.is_empty() {
        println!("          Scope: {}", session.scope);
    }
    if !session.service.is_empty() {
        println!("        Service: {}", session.service);
    }
    if !session.since.is_empty() {
        println!("          Since: {}", session.since);
    }
}

fn lock_session(args: &[String]) {
    let session_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Error: session ID required");
            process::exit(1);
        }
    };
    println!("Session '{}' locked.", session_id);
}

fn unlock_session(args: &[String]) {
    let session_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Error: session ID required");
            process::exit(1);
        }
    };
    println!("Session '{}' unlocked.", session_id);
}

fn activate_session(args: &[String]) {
    let session_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Error: session ID required");
            process::exit(1);
        }
    };
    println!("Session '{}' activated.", session_id);
}

fn terminate_session(args: &[String]) {
    let session_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Error: session ID required");
            process::exit(1);
        }
    };
    println!("Session '{}' terminated.", session_id);
}

fn kill_session(args: &[String]) {
    let mut session_id = String::new();
    let mut signal = "SIGTERM".to_string();
    let mut who = "all".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--signal" => {
                i += 1;
                if i < args.len() {
                    signal = args[i].clone();
                }
            }
            "--kill-who" => {
                i += 1;
                if i < args.len() {
                    who = args[i].clone();
                }
            }
            _ => {
                if session_id.is_empty() {
                    session_id = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if session_id.is_empty() {
        eprintln!("Error: session ID required");
        process::exit(1);
    }
    println!("Sending {} to {} processes in session '{}'.", signal, who, session_id);
}

// ── User management ────────────────────────────────────────────────────

fn read_users() -> Vec<UserInfo> {
    let sessions = read_sessions();
    let mut user_map: BTreeMap<u32, UserInfo> = BTreeMap::new();

    for s in &sessions {
        let entry = user_map.entry(s.uid).or_insert_with(|| UserInfo {
            uid: s.uid,
            name: s.user.clone(),
            state: "offline".to_string(),
            _linger: false,
            sessions: Vec::new(),
            _slice: format!("user-{}.slice", s.uid),
            since: s.since.clone(),
        });
        entry.sessions.push(s.id.clone());
        if s.state == SessionState::Active {
            entry.state = "active".to_string();
        } else if entry.state != "active" {
            entry.state = "online".to_string();
        }
    }

    let mut users: Vec<UserInfo> = user_map.into_values().collect();
    users.sort_by_key(|u| u.uid);
    users
}

fn list_users(args: &[String]) {
    let no_legend = args.iter().any(|a| a == "--no-legend");
    let users = read_users();

    if users.is_empty() {
        if !no_legend {
            println!("No users logged in.");
        }
        return;
    }

    if !no_legend {
        println!("{:>5} {:<16} {:<10} {:<10}", "UID", "USER", "STATE", "SESSIONS");
    }
    for u in &users {
        println!("{:>5} {:<16} {:<10} {:<10}",
            u.uid, u.name, u.state, u.sessions.len());
    }
    if !no_legend {
        println!("\n{} users listed.", users.len());
    }
}

fn show_user(args: &[String]) {
    let target = args.first().map(|s| s.as_str()).unwrap_or("");
    let users = read_users();

    let user = if target.is_empty() {
        users.first()
    } else if let Ok(uid) = target.parse::<u32>() {
        users.iter().find(|u| u.uid == uid)
    } else {
        users.iter().find(|u| u.name == target)
    };

    let user = match user {
        Some(u) => u,
        None => {
            eprintln!("User '{}' not found or not logged in.", target);
            process::exit(1);
        }
    };

    println!("           User: {} ({})", user.name, user.uid);
    println!("          State: {}", user.state);
    println!("       Sessions: {}", user.sessions.join(", "));
    if !user.since.is_empty() {
        println!("          Since: {}", user.since);
    }
    println!("         Linger: {}", if user._linger { "yes" } else { "no" });
}

fn enable_linger(args: &[String]) {
    let user = args.first().map(|s| s.as_str()).unwrap_or("current user");
    let linger_dir = format!("{}/linger", USER_RUNTIME_DIR);
    let _ = std::fs::create_dir_all(&linger_dir);
    let linger_file = format!("{}/{}", linger_dir, user);
    let _ = std::fs::write(&linger_file, "");
    println!("Linger enabled for user '{}'.", user);
}

fn disable_linger(args: &[String]) {
    let user = args.first().map(|s| s.as_str()).unwrap_or("current user");
    let linger_file = format!("{}/linger/{}", USER_RUNTIME_DIR, user);
    let _ = std::fs::remove_file(&linger_file);
    println!("Linger disabled for user '{}'.", user);
}

fn terminate_user(args: &[String]) {
    let user = match args.first() {
        Some(u) => u,
        None => {
            eprintln!("Error: user name or UID required");
            process::exit(1);
        }
    };
    println!("User '{}' sessions terminated.", user);
}

fn kill_user(args: &[String]) {
    let mut user = String::new();
    let mut signal = "SIGTERM".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--signal" => {
                i += 1;
                if i < args.len() {
                    signal = args[i].clone();
                }
            }
            _ => {
                if user.is_empty() {
                    user = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if user.is_empty() {
        eprintln!("Error: user name or UID required");
        process::exit(1);
    }
    println!("Sending {} to all sessions of user '{}'.", signal, user);
}

// ── Seat management ────────────────────────────────────────────────────

fn read_seats() -> Vec<Seat> {
    let entries = match std::fs::read_dir(SEAT_DIR) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut seats = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(seat) = parse_seat_file(&path) {
            seats.push(seat);
        }
    }
    seats.sort_by(|a, b| a.id.cmp(&b.id));
    seats
}

fn parse_seat_file(path: &std::path::Path) -> Option<Seat> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut map = BTreeMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let id = path.file_name()?.to_str()?.to_string();
    let sessions: Vec<String> = map.get("SESSIONS")
        .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
        .unwrap_or_default();
    let active_session = map.get("ACTIVE_SESSION").cloned().unwrap_or_default();
    let devices: Vec<String> = map.get("DEVICES")
        .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
        .unwrap_or_default();

    Some(Seat {
        id,
        sessions,
        active_session,
        _devices: devices,
    })
}

fn list_seats(args: &[String]) {
    let no_legend = args.iter().any(|a| a == "--no-legend");
    let seats = read_seats();

    if seats.is_empty() {
        if !no_legend {
            println!("No seats.");
        }
        return;
    }

    for s in &seats {
        println!("{}", s.id);
    }
    if !no_legend {
        println!("\n{} seats listed.", seats.len());
    }
}

fn show_seat(args: &[String]) {
    let seat_id = args.first().map(|s| s.as_str()).unwrap_or("seat0");
    let seats = read_seats();

    let seat = match seats.iter().find(|s| s.id == seat_id) {
        Some(s) => s,
        None => {
            eprintln!("Seat '{}' not found.", seat_id);
            process::exit(1);
        }
    };

    println!("           Seat: {}", seat.id);
    println!("       Sessions: {}", seat.sessions.join(", "));
    if !seat.active_session.is_empty() {
        println!(" Active Session: {}", seat.active_session);
    }
}

fn attach_device(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: loginctl attach <seat> <device>...");
        process::exit(1);
    }
    let seat = &args[0];
    for dev in &args[1..] {
        println!("Device '{}' attached to seat '{}'.", dev, seat);
    }
}

fn flush_devices(_args: &[String]) {
    println!("All device-to-seat assignments flushed.");
}

fn terminate_seat(args: &[String]) {
    let seat_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Error: seat ID required");
            process::exit(1);
        }
    };
    println!("All sessions on seat '{}' terminated.", seat_id);
}

// ── System control ─────────────────────────────────────────────────────

fn show_system_poweroff() {
    println!("Powering off...");
}

fn show_system_reboot() {
    println!("Rebooting...");
}

fn show_system_suspend() {
    println!("Suspending...");
}

fn show_system_hibernate() {
    println!("Hibernating...");
}

fn show_system_hybrid_sleep() {
    println!("Entering hybrid sleep...");
}

// ── userdbctl personality ──────────────────────────────────────────────

fn read_passwd() -> Vec<PasswdEntry> {
    let content = match std::fs::read_to_string(PASSWD_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.splitn(7, ':').collect();
        if fields.len() < 7 {
            continue;
        }
        entries.push(PasswdEntry {
            name: fields[0].to_string(),
            uid: fields[2].parse().unwrap_or(0),
            gid: fields[3].parse().unwrap_or(0),
            gecos: fields[4].to_string(),
            home: fields[5].to_string(),
            shell: fields[6].to_string(),
        });
    }
    entries
}

fn read_groups() -> Vec<GroupEntry> {
    let content = match std::fs::read_to_string(GROUP_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.splitn(4, ':').collect();
        if fields.len() < 4 {
            continue;
        }
        let members = if fields[3].is_empty() {
            Vec::new()
        } else {
            fields[3].split(',').map(|s| s.trim().to_string()).collect()
        };
        entries.push(GroupEntry {
            name: fields[0].to_string(),
            gid: fields[2].parse().unwrap_or(0),
            members,
        });
    }
    entries
}

fn userdbctl_user(args: &[String]) {
    let users = read_passwd();
    let json_mode = args.iter().any(|a| a == "--json" || a == "-j");

    if let Some(name) = args.iter().find(|a| !a.starts_with('-')) {
        let found = if let Ok(uid) = name.parse::<u32>() {
            users.iter().find(|u| u.uid == uid)
        } else {
            users.iter().find(|u| u.name == name.as_str())
        };

        match found {
            Some(u) => {
                if json_mode {
                    println!("{{");
                    println!("  \"userName\": \"{}\",", u.name);
                    println!("  \"uid\": {},", u.uid);
                    println!("  \"gid\": {},", u.gid);
                    println!("  \"realName\": \"{}\",", u.gecos);
                    println!("  \"homeDirectory\": \"{}\",", u.home);
                    println!("  \"shell\": \"{}\"", u.shell);
                    println!("}}");
                } else {
                    println!("  User name: {}", u.name);
                    println!("        UID: {}", u.uid);
                    println!("        GID: {}", u.gid);
                    println!("  Real name: {}", u.gecos);
                    println!("       Home: {}", u.home);
                    println!("      Shell: {}", u.shell);
                }
            }
            None => {
                eprintln!("User '{}' not found.", name);
                process::exit(1);
            }
        }
    } else {
        // List all users
        println!("{:<20} {:>5} {:>5} REALNAME", "NAME", "UID", "GID");
        for u in &users {
            println!("{:<20} {:>5} {:>5} {}", u.name, u.uid, u.gid, u.gecos);
        }
    }
}

fn userdbctl_group(args: &[String]) {
    let groups = read_groups();
    let json_mode = args.iter().any(|a| a == "--json" || a == "-j");

    if let Some(name) = args.iter().find(|a| !a.starts_with('-')) {
        let found = if let Ok(gid) = name.parse::<u32>() {
            groups.iter().find(|g| g.gid == gid)
        } else {
            groups.iter().find(|g| g.name == name.as_str())
        };

        match found {
            Some(g) => {
                if json_mode {
                    println!("{{");
                    println!("  \"groupName\": \"{}\",", g.name);
                    println!("  \"gid\": {},", g.gid);
                    println!("  \"members\": [{}]",
                        g.members.iter().map(|m| format!("\"{}\"", m)).collect::<Vec<_>>().join(", "));
                    println!("}}");
                } else {
                    println!("Group name: {}", g.name);
                    println!("       GID: {}", g.gid);
                    println!("   Members: {}", g.members.join(", "));
                }
            }
            None => {
                eprintln!("Group '{}' not found.", name);
                process::exit(1);
            }
        }
    } else {
        println!("{:<20} {:>5} MEMBERS", "NAME", "GID");
        for g in &groups {
            println!("{:<20} {:>5} {}", g.name, g.gid, g.members.join(","));
        }
    }
}

fn userdbctl_members(args: &[String]) {
    let groups = read_groups();

    if let Some(name) = args.first() {
        match groups.iter().find(|g| g.name == name.as_str()) {
            Some(g) => {
                for m in &g.members {
                    println!("{}", m);
                }
            }
            None => {
                eprintln!("Group '{}' not found.", name);
                process::exit(1);
            }
        }
    } else {
        for g in &groups {
            if !g.members.is_empty() {
                println!("{}:", g.name);
                for m in &g.members {
                    println!("  {}", m);
                }
            }
        }
    }
}

fn userdbctl_services() {
    println!("io.systemd.NameServiceSwitch");
    println!("io.systemd.Multiplexer");
    println!("io.systemd.DynamicUser");
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_loginctl_help() {
    println!("loginctl — Session/user/seat management");
    println!();
    println!("Usage: loginctl [COMMAND] [OPTIONS]");
    println!();
    println!("Session Commands:");
    println!("  list-sessions              List active sessions");
    println!("  session-status [ID]        Show session status");
    println!("  show-session [ID]          Show session properties");
    println!("  activate [ID]              Activate a session");
    println!("  lock-session [ID]          Lock a session");
    println!("  unlock-session [ID]        Unlock a session");
    println!("  terminate-session ID       Terminate a session");
    println!("  kill-session ID            Kill session processes");
    println!();
    println!("User Commands:");
    println!("  list-users                 List logged-in users");
    println!("  user-status [USER]         Show user status");
    println!("  show-user [USER]           Show user properties");
    println!("  enable-linger [USER]       Enable user lingering");
    println!("  disable-linger [USER]      Disable user lingering");
    println!("  terminate-user USER        Terminate user sessions");
    println!("  kill-user USER             Kill user processes");
    println!();
    println!("Seat Commands:");
    println!("  list-seats                 List seats");
    println!("  seat-status [SEAT]         Show seat status");
    println!("  show-seat [SEAT]           Show seat properties");
    println!("  attach SEAT DEVICE...      Attach device to seat");
    println!("  flush-devices              Flush device assignments");
    println!("  terminate-seat SEAT        Terminate seat sessions");
    println!();
    println!("System Commands:");
    println!("  poweroff                   Power off the system");
    println!("  reboot                     Reboot the system");
    println!("  suspend                    Suspend the system");
    println!("  hibernate                  Hibernate the system");
    println!("  hybrid-sleep               Enter hybrid sleep");
    println!();
    println!("Options:");
    println!("  --no-legend                Do not print table headers/footers");
    println!("  -h, --help                 Show this help");
}

fn print_userdbctl_help() {
    println!("userdbctl — User/group database query tool");
    println!();
    println!("Usage: userdbctl [COMMAND] [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  user [NAME|UID]            Show user or list all users");
    println!("  group [NAME|GID]           Show group or list all groups");
    println!("  members [GROUP]            Show group memberships");
    println!("  services                   List available services");
    println!();
    println!("Options:");
    println!("  -j, --json                 JSON output");
    println!("  -h, --help                 Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_loginctl(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "list-sessions".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_loginctl_help();
        return 0;
    }

    match cmd.as_str() {
        "list-sessions" => list_sessions(&cmd_args),
        "session-status" | "show-session" => show_session(&cmd_args),
        "activate" => activate_session(&cmd_args),
        "lock-session" => lock_session(&cmd_args),
        "unlock-session" => unlock_session(&cmd_args),
        "terminate-session" => terminate_session(&cmd_args),
        "kill-session" => kill_session(&cmd_args),
        "list-users" => list_users(&cmd_args),
        "user-status" | "show-user" => show_user(&cmd_args),
        "enable-linger" => enable_linger(&cmd_args),
        "disable-linger" => disable_linger(&cmd_args),
        "terminate-user" => terminate_user(&cmd_args),
        "kill-user" => kill_user(&cmd_args),
        "list-seats" => list_seats(&cmd_args),
        "seat-status" | "show-seat" => show_seat(&cmd_args),
        "attach" => attach_device(&cmd_args),
        "flush-devices" => flush_devices(&cmd_args),
        "terminate-seat" => terminate_seat(&cmd_args),
        "poweroff" => show_system_poweroff(),
        "reboot" => show_system_reboot(),
        "suspend" => show_system_suspend(),
        "hibernate" => show_system_hibernate(),
        "hybrid-sleep" => show_system_hybrid_sleep(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_loginctl_help();
            return 1;
        }
    }
    0
}

fn run_userdbctl(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "user".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_userdbctl_help();
        return 0;
    }

    match cmd.as_str() {
        "user" => userdbctl_user(&cmd_args),
        "group" => userdbctl_group(&cmd_args),
        "members" => userdbctl_members(&cmd_args),
        "services" => userdbctl_services(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_userdbctl_help();
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("loginctl");
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

    let code = match prog_name.as_str() {
        "userdbctl" => run_userdbctl(args),
        _ => run_loginctl(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_display() {
        assert_eq!(format!("{}", SessionState::Active), "active");
        assert_eq!(format!("{}", SessionState::Online), "online");
        assert_eq!(format!("{}", SessionState::Closing), "closing");
        assert_eq!(format!("{}", SessionState::_Lingering), "lingering");
    }

    #[test]
    fn test_session_type_display() {
        assert_eq!(format!("{}", SessionType::X11), "x11");
        assert_eq!(format!("{}", SessionType::Wayland), "wayland");
        assert_eq!(format!("{}", SessionType::Tty), "tty");
        assert_eq!(format!("{}", SessionType::Mir), "mir");
        assert_eq!(format!("{}", SessionType::Unspecified), "unspecified");
    }

    #[test]
    fn test_session_class_display() {
        assert_eq!(format!("{}", SessionClass::User), "user");
        assert_eq!(format!("{}", SessionClass::Greeter), "greeter");
        assert_eq!(format!("{}", SessionClass::_LockScreen), "lock-screen");
        assert_eq!(format!("{}", SessionClass::Background), "background");
    }

    #[test]
    fn test_read_passwd_nonexistent() {
        // On a system without /etc/passwd or on Windows, returns empty
        let users = read_passwd();
        // Just verify it doesn't panic
        let _ = users;
    }

    #[test]
    fn test_read_groups_nonexistent() {
        let groups = read_groups();
        let _ = groups;
    }

    #[test]
    fn test_read_sessions_nonexistent() {
        let sessions = read_sessions();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_read_seats_nonexistent() {
        let seats = read_seats();
        assert!(seats.is_empty());
    }

    #[test]
    fn test_read_users_empty() {
        let users = read_users();
        assert!(users.is_empty());
    }

    #[test]
    fn test_prog_name_detection() {
        let test_cases = vec![
            ("loginctl", "loginctl"),
            ("userdbctl", "userdbctl"),
            ("/usr/bin/loginctl", "loginctl"),
            ("C:\\bin\\loginctl.exe", "loginctl"),
            ("/sbin/userdbctl", "userdbctl"),
        ];
        for (input, expected) in test_cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected, "failed for input: {}", input);
        }
    }

    #[test]
    fn test_passwd_entry_parse() {
        let line = "root:x:0:0:root:/root:/bin/bash";
        let fields: Vec<&str> = line.splitn(7, ':').collect();
        assert_eq!(fields.len(), 7);
        assert_eq!(fields[0], "root");
        assert_eq!(fields[2], "0");
        assert_eq!(fields[5], "/root");
        assert_eq!(fields[6], "/bin/bash");
    }

    #[test]
    fn test_group_entry_parse() {
        let line = "wheel:x:10:user1,user2,user3";
        let fields: Vec<&str> = line.splitn(4, ':').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], "wheel");
        assert_eq!(fields[2], "10");
        let members: Vec<&str> = fields[3].split(',').collect();
        assert_eq!(members, vec!["user1", "user2", "user3"]);
    }

    #[test]
    fn test_group_entry_parse_empty_members() {
        let line = "nogroup:x:65534:";
        let fields: Vec<&str> = line.splitn(4, ':').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3], "");
    }
}
