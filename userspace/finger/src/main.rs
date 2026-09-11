// Slate OS finger — user information lookup
//
// Multi-personality binary:
//   finger — user information lookup (RFC 1288)
//   pinky  — lightweight finger
//
// IT USED TO ANSWER TO `w` TOO, and that name now belongs to `userspace/who`,
// which performs the operation: `who -w` computes JCPU and PCPU, which this
// program has no columns for at all, and derives WHAT from the session
// leader's cmdline where this printed a fixed "-". design-decisions 1019 --
// the name belongs to whichever program does the work.
//
// Usage:
//   finger [OPTIONS] [user@host | user...]
//   pinky [OPTIONS] [user...]

#![cfg_attr(not(test), no_main)]
// Several fields are read from utmp/finger structures whose full layout we
// preserve even when the current minimal output doesn't render every
// column (pid, home_phone, idle_sort, OldMail status). The MailStatus
// variants share the `Mail` postfix because that matches the finger
// protocol's terminology.
#![allow(dead_code, clippy::enum_variant_names)]

use std::io::{self, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Finger,
    Pinky,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "pinky" => Personality::Pinky,
        // `finger` is the default now that `w` is gone -- and it is also the
        // crate's name, so argv[0] and the package agree.
        _ => Personality::Finger,
    }
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UtmpEntry {
    /// Username as it appears in utmp. NOT necessarily UTF-8: our names allow
    /// every byte except `/` and NUL, and decoding lossily would map two
    /// different users onto one string.
    user: Vec<u8>,
    tty: Vec<u8>,
    host: Vec<u8>,
    login_time: u64,
    pid: u32,
    /// Seconds since the terminal was last written to, or `None` when that
    /// could not be found out. THIS WAS `u64` AND ALWAYS ZERO -- set where the
    /// entry is built and assigned nowhere else -- which `format_idle` renders
    /// as "  .  ", meaning active right now. Every user, always, including one
    /// away for three hours.
    idle_secs: Option<u64>,
    what: Vec<u8>,
}

#[derive(Debug, Clone)]
struct UserInfo {
    username: Vec<u8>,
    real_name: Vec<u8>,
    /// `None` when the recorded home directory is not UTF-8; see
    /// [`read_passwd_gecos`].
    home_dir: Option<String>,
    shell: Vec<u8>,
    office: Vec<u8>,
    office_phone: Vec<u8>,
    home_phone: Vec<u8>,
    plan: Option<String>,
    project: Option<String>,
    mail_status: MailStatus,
    login_sessions: Vec<UtmpEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MailStatus {
    NoMail,
    OldMail,
    NewMail,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    users: Vec<String>,
    no_header: bool,
    short_format: bool,
    long_format: bool,
    idle_sort: bool,
    show_help: bool,
    show_version: bool,
    // finger-specific
    no_plan: bool,
    no_project: bool,
    match_real_name: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Finger,
            users: Vec::new(),
            no_header: false,
            short_format: false,
            long_format: false,
            idle_sort: false,
            show_help: false,
            show_version: false,
            no_plan: false,
            no_project: false,
            match_real_name: true,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Finger);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    // Iterating rather than indexing a counter. `&args[i]` can panic and
    // `i += 1` can overflow -- neither can actually happen here, but saying so
    // in a comment is weaker than not writing the constructs, and this loop
    // never needed the index for anything else.
    for arg in args.iter().skip(1) {
        match personality {
            Personality::Finger => match arg.as_str() {
                "-l" => cfg.long_format = true,
                "-s" => cfg.short_format = true,
                "-p" => cfg.no_plan = true,
                "-m" => cfg.match_real_name = false,
                "--help" => cfg.show_help = true,
                "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("finger: unknown option: {other}"));
                }
                _ => cfg.users.push(arg.clone()),
            },
            Personality::Pinky => match arg.as_str() {
                "-l" => cfg.long_format = true,
                "-s" => cfg.short_format = true,
                // pinky's own help says "-f  Omit header in short
                // format", and this set `from_field`, which only `run_w` ever
                // read -- so `pinky -f` did nothing at all. `no_header` is the
                // field run_pinky actually consults.
                "-f" => cfg.no_header = true,
                "-w" => {} // omit name field
                "-i" => {} // show IPs
                "-b" => cfg.no_plan = true,
                "-h" => cfg.no_header = true,
                "-p" => cfg.no_project = true,
                "--help" => cfg.show_help = true,
                "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("pinky: unknown option: {other}"));
                }
                _ => cfg.users.push(arg.clone()),
            },
        }
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// System information gathering
// ---------------------------------------------------------------------------

/// The sessions recorded in `/var/run/utmp`, or none if it cannot be read.
///
/// WHAT THIS REPLACED, because the shape is worth keeping in view: it opened
/// the file with `read_to_string` and split each line on `:`, expecting
/// `user:tty:host:time:pid:what`. utmp is binary -- 384-byte records, which
/// `posix` declares and `who` and `uptime` both read correctly -- so the parse
/// matched nothing, EVERY TIME, and the function fell through to
///
///     if entries.is_empty() && let Ok(user) = env::var("USER")
///
/// and pushed a session for whoever `$USER` named, on a tty called "console",
/// with a login time of 0. So `w` always showed exactly one user, always the
/// caller, always invented -- and `USER=root w` said root was logged in.
///
/// There is no fallback now. If utmp cannot be read or holds no sessions, the
/// answer is that we do not know of any, which prints as no rows. A list of
/// logged-in users is not somewhere to guess.
fn read_utmp_entries() -> Vec<UtmpEntry> {
    let Ok(data) = std::fs::read("/var/run/utmp") else {
        return Vec::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    utmpfile::parse(&data)
        .into_iter()
        .filter(utmpfile::Record::is_user_session)
        .map(|r| UtmpEntry {
            // Measured before `tty` is moved into the struct.
            idle_secs: ttyidle::idle_secs(&r.tty, now),
            user: r.user,
            tty: r.tty,
            host: r.host,
            login_time: r.login_time,
            pid: u32::try_from(r.pid).unwrap_or(0),
            // utmp has no WHAT field; real `w` derives it from the session
            // leader's /proc/PID/cmdline. This has always printed "-" -- the
            // text parse that would have filled it never matched a line -- so
            // "-" is what it prints, and now says only what is true.
            what: b"-".to_vec(),
        })
        .collect()
}

/// `(real_name, home_dir, shell, office, office_phone)` for `username`.
///
/// Reads `/etc/passwd` as BYTES and compares the name byte-for-byte. It used
/// `read_to_string`, so a passwd file containing one non-UTF-8 byte anywhere
/// -- in any user's GECOS comment, not necessarily this one's -- failed the
/// whole read and every lookup silently returned the defaults.
///
/// `home_dir` is `None` when it is not valid UTF-8, because the only use for
/// it here is building a `PathBuf` for `.plan` and `.project`, and this crate
/// is built for a host target where a path cannot be made from arbitrary
/// bytes. Refusing to open a file we cannot name is honest; guessing at the
/// name is not.
/// What `/etc/passwd` says about one user.
///
/// A named struct rather than a five-element tuple because four of the five
/// were `String` and the call sites destructured them positionally -- swapping
/// `office` and `office_phone` would have compiled and printed a phone number
/// under Office.
struct PasswdFields {
    real_name: Vec<u8>,
    /// `None` when the recorded home directory is not UTF-8; see the note on
    /// [`read_passwd_gecos`].
    home_dir: Option<String>,
    shell: Vec<u8>,
    office: Vec<u8>,
    office_phone: Vec<u8>,
}

impl PasswdFields {
    /// What an unknown user looks like: the name we were asked about, and
    /// nothing else claimed.
    fn defaults_for(username: &[u8]) -> Self {
        Self {
            real_name: username.to_vec(),
            home_dir: None,
            shell: Vec::new(),
            office: Vec::new(),
            office_phone: Vec::new(),
        }
    }
}

fn read_passwd_gecos(username: &[u8]) -> PasswdFields {
    let Ok(content) = std::fs::read("/etc/passwd") else {
        return PasswdFields::defaults_for(username);
    };

    for line in content.split(|&b| b == b'\n') {
        let fields: Vec<&[u8]> = line.split(|&b| b == b':').collect();
        if fields.len() >= 7 && fields.first() == Some(&username) {
            let gecos: Vec<&[u8]> = fields
                .get(4)
                .map_or_else(Vec::new, |g| g.split(|&b| b == b',').collect());
            let part = |n: usize| gecos.get(n).map_or_else(Vec::new, |b| b.to_vec());
            let real_name = if gecos.first().is_some_and(|f| !f.is_empty()) {
                part(0)
            } else {
                username.to_vec()
            };
            return PasswdFields {
                real_name,
                home_dir: fields
                    .get(5)
                    .and_then(|b| str::from_utf8(b).ok())
                    .map(str::to_owned),
                shell: fields.get(6).map_or_else(Vec::new, |b| b.to_vec()),
                office: part(1),
                office_phone: part(2),
            };
        }
    }

    PasswdFields::defaults_for(username)
}

fn get_user_info(username: &str) -> UserInfo {
    // The name arrives from argv, which this crate already holds as `String`.
    // Everything downstream of the passwd lookup is bytes.
    let pw = read_passwd_gecos(username.as_bytes());

    // Read .plan file. `home_dir` is None when it is not UTF-8, so there is no
    // path to build and no file to read -- see read_passwd_gecos.
    let home_dir = pw.home_dir.clone().unwrap_or_default();
    let plan = if !home_dir.is_empty() {
        let plan_path = PathBuf::from(&home_dir).join(".plan");
        std::fs::read_to_string(plan_path).ok()
    } else {
        None
    };

    // Read .project file
    let project = if !home_dir.is_empty() {
        let proj_path = PathBuf::from(&home_dir).join(".project");
        std::fs::read_to_string(proj_path).ok()
    } else {
        None
    };

    // Check mail
    let mail_path = format!("/var/mail/{username}");
    let mail_status = match std::fs::metadata(&mail_path) {
        Ok(meta) => {
            if meta.len() > 0 {
                MailStatus::NewMail // simplified
            } else {
                MailStatus::NoMail
            }
        }
        Err(_) => MailStatus::NoMail,
    };

    let sessions = read_utmp_entries()
        .into_iter()
        .filter(|e| e.user == username.as_bytes())
        .collect();

    UserInfo {
        username: username.as_bytes().to_vec(),
        real_name: pw.real_name,
        home_dir: pw.home_dir,
        shell: pw.shell,
        office: pw.office,
        office_phone: pw.office_phone,
        // /etc/passwd's GECOS has a fourth comma-separated field for the home
        // phone. It was never read and is not read now; leaving it empty says
        // "not known" rather than "not present".
        home_phone: Vec::new(),
        plan,
        project,
        mail_status,
        login_sessions: sessions,
    }
}

/// The `up …` field of the header line.
///
/// Returns `(unknown)` rather than a duration when `/proc/uptime` cannot be
/// read or does not begin with a number. It used to return **`"up  0:00"`** --
/// a machine that booted within the last minute, which is a perfectly ordinary
/// thing for a real system to say and gave a caller no way to tell the two
/// apart.
fn get_uptime_str() -> String {
    format_uptime_field(std::fs::read_to_string("/proc/uptime").ok().as_deref())
}

/// The `load average:` field of the header line.
///
/// Returns `(unknown)` rather than three zeroes when `/proc/loadavg` cannot be
/// read or is short. It used to return **`"load average: 0.00, 0.00, 0.00"`**,
/// which is what an idle machine reports -- and an idle machine is exactly
/// what someone running `w` might be trying to confirm.
fn get_load_avg() -> String {
    format_load_field(std::fs::read_to_string("/proc/loadavg").ok().as_deref())
}

/// What `w` prints for a field whose source it could not read.
///
/// Parenthesised so that it cannot be read as a value: an uptime may be
/// `0:00` and a load average may be `0.00`, but neither is ever `(unknown)`.
const UNKNOWN_FIELD: &str = "(unknown)";

/// Split out of [`get_uptime_str`] so the no-file and malformed-file paths can
/// be tested, which they could not be while the read was inside the formatter.
fn format_uptime_field(content: Option<&str>) -> String {
    let Some(secs) = content
        .and_then(|c| c.split_whitespace().next().map(str::to_string))
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
    else {
        return format!("up {UNKNOWN_FIELD}");
    };
    let total_secs = secs as u64;
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    if days > 0 {
        format!("up {days} day(s), {hours:2}:{mins:02}")
    } else {
        format!("up {hours:2}:{mins:02}")
    }
}

/// Split out of [`get_load_avg`] for the same reason.
fn format_load_field(content: Option<&str>) -> String {
    let parts: Vec<&str> = content.unwrap_or_default().split_whitespace().collect();
    match parts.get(..3) {
        Some([one, five, fifteen]) => format!("load average: {one}, {five}, {fifteen}"),
        _ => format!("load average: {UNKNOWN_FIELD}"),
    }
}

fn get_current_time() -> String {
    // Simplified — would read from system clock
    "00:00:00".to_string()
}

fn format_idle(secs: Option<u64>) -> String {
    // "?" for not-known, which used to be indistinguishable from "  .  " --
    // this column's word for active right now.
    let Some(secs) = secs else {
        return "  ?  ".to_string();
    };
    if secs == 0 {
        return "  .  ".to_string();
    }
    if secs < 60 {
        return format!(" {secs:2}s ");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins:2}:{:02} ", secs % 60);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours:2}:{:02}m", mins % 60);
    }
    let days = hours / 24;
    format!("{days:2}days")
}

fn format_login_time(timestamp: u64) -> String {
    if timestamp == 0 {
        return " ?????".to_string();
    }
    // Simplified time formatting
    let hours = (timestamp / 3600) % 24;
    let mins = (timestamp / 60) % 60;
    format!("{hours:02}:{mins:02}")
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn run_finger(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    if cfg.users.is_empty() {
        // No user specified — show all logged in users (short format)
        let entries = read_utmp_entries();
        writeln!(
            writer,
            "Login     Name              Tty      Idle  Login Time   Office     Office Phone"
        )?;
        for entry in &entries {
            let pw = read_passwd_gecos(&entry.user);
            write_col(writer, &entry.user, 9)?;
            writer.write_all(b" ")?;
            write_col(writer, &pw.real_name, 17)?;
            writer.write_all(b" ")?;
            write_col(writer, &entry.tty, 8)?;
            write!(
                writer,
                " {} {:<12} ",
                format_idle(entry.idle_secs),
                format_login_time(entry.login_time)
            )?;
            write_col(writer, &pw.office, 10)?;
            writer.write_all(b" ")?;
            write_col(writer, &pw.office_phone, 12)?;
            writeln!(writer)?;
        }
        return Ok(());
    }

    // For each specified user, show long format
    for (idx, username) in cfg.users.iter().enumerate() {
        // Check for user@host (remote finger)
        if username.contains('@') {
            writeln!(writer, "[{username}]")?;
            writeln!(writer, "Remote finger not supported")?;
            continue;
        }

        let info = get_user_info(username);

        if cfg.short_format && !cfg.long_format {
            // Short format
            writer.write_all(b"Login: ")?;
            write_col(writer, &info.username, 20)?;
            writer.write_all(b" Name: ")?;
            writer.write_all(&info.real_name)?;
            writeln!(writer)?;
        } else {
            // Long format
            if idx > 0 {
                writeln!(writer)?;
            }
            writer.write_all(b"Login: ")?;
            write_col(writer, &info.username, 20)?;
            writer.write_all(b" Name: ")?;
            writer.write_all(&info.real_name)?;
            writeln!(writer)?;
            writer.write_all(b"Directory: ")?;
            write_col(
                writer,
                info.home_dir.as_deref().unwrap_or("").as_bytes(),
                22,
            )?;
            writer.write_all(b" Shell: ")?;
            writer.write_all(&info.shell)?;
            writeln!(writer)?;
            if !info.office.is_empty() || !info.office_phone.is_empty() {
                writer.write_all(b"Office: ")?;
                write_col(writer, &info.office, 24)?;
                writer.write_all(b" Office Phone: ")?;
                writer.write_all(&info.office_phone)?;
                writeln!(writer)?;
            }

            // Login sessions
            if info.login_sessions.is_empty() {
                writeln!(writer, "Never logged in.")?;
            } else {
                for session in &info.login_sessions {
                    write!(writer, "On since ")?;
                    write!(writer, "{}", format_login_time(session.login_time))?;
                    writer.write_all(b" on ")?;
                    writer.write_all(&session.tty)?;
                    writeln!(writer)?;
                }
            }

            // Mail
            match info.mail_status {
                MailStatus::NewMail => writeln!(writer, "New mail received.")?,
                MailStatus::OldMail => writeln!(writer, "Mail last read.")?,
                MailStatus::NoMail => writeln!(writer, "No mail.")?,
            }

            // Plan/project
            if !cfg.no_plan {
                if !cfg.no_project
                    && let Some(ref project) = info.project
                {
                    writeln!(writer, "Project: {}", project.trim())?;
                }
                if let Some(ref plan) = info.plan {
                    writeln!(writer, "Plan:")?;
                    write!(writer, "{plan}")?;
                } else {
                    writeln!(writer, "No Plan.")?;
                }
            }
        }
    }

    Ok(())
}

fn run_pinky(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    if cfg.users.is_empty() || cfg.short_format {
        // Short format — list all logged in users
        let entries = read_utmp_entries();
        let entries: Vec<&UtmpEntry> = if cfg.users.is_empty() {
            entries.iter().collect()
        } else {
            entries
                .iter()
                .filter(|e| cfg.users.iter().any(|u| u.as_bytes() == e.user.as_slice()))
                .collect()
        };

        if !cfg.no_header {
            writeln!(
                writer,
                "Login    Name                 TTY      Idle   When         Where"
            )?;
        }
        for entry in &entries {
            let pw = read_passwd_gecos(&entry.user);
            write_col(writer, &entry.user, 8)?;
            writer.write_all(b" ")?;
            write_col(writer, &pw.real_name, 20)?;
            writer.write_all(b" ")?;
            write_col(writer, &entry.tty, 8)?;
            write!(
                writer,
                " {} {:<12} ",
                format_idle(entry.idle_secs),
                format_login_time(entry.login_time)
            )?;
            writer.write_all(&entry.host)?;
            writeln!(writer)?;
        }
    } else {
        // Long format for specified users
        for username in &cfg.users {
            let info = get_user_info(username);
            writer.write_all(b"Login name: ")?;
            write_col(writer, &info.username, 28)?;
            writer.write_all(b" In real life: ")?;
            writer.write_all(&info.real_name)?;
            writeln!(writer)?;
            writer.write_all(b"Directory: ")?;
            // An un-namable home directory prints as empty rather than as a
            // guess at what the bytes might have said.
            write_col(
                writer,
                info.home_dir.as_deref().unwrap_or("").as_bytes(),
                29,
            )?;
            writer.write_all(b" Shell: ")?;
            writer.write_all(&info.shell)?;
            writeln!(writer)?;
            if info.login_sessions.is_empty() {
                writeln!(writer, "Never logged in.")?;
            } else {
                for session in &info.login_sessions {
                    write!(
                        writer,
                        "On since {} on ",
                        format_login_time(session.login_time)
                    )?;
                    writer.write_all(&session.tty)?;
                    if !session.host.is_empty() {
                        writer.write_all(b" from ")?;
                        writer.write_all(&session.host)?;
                    }
                    writeln!(writer)?;
                }
            }
            if !cfg.no_plan {
                if let Some(ref plan) = info.plan {
                    writeln!(writer, "Plan:")?;
                    write!(writer, "{plan}")?;
                } else {
                    writeln!(writer, "No Plan.")?;
                }
            }
        }
    }

    Ok(())
}

/// `bytes`, cut to at most `max` bytes.
///
/// This took a `&str` and did `s[..max]`, which PANICS when `max` falls inside
/// a multi-byte character -- a user called "café" crashed `w` at a width of 3.
/// Cutting bytes cannot panic. It can split a character, which shows as one
/// replacement glyph in the terminal and is what every C tool here does.
fn truncate_bytes(bytes: &[u8], max: usize) -> &[u8] {
    bytes.get(..max).unwrap_or(bytes)
}

/// Write `bytes` cut to `width` and padded with spaces to `width`.
///
/// The `{:<8}` formatting this replaces needs a `str`, which is the whole
/// reason the fields used to be decoded. Padding counts BYTES, as the C tools
/// do -- a column is a column of the file, not of grapheme clusters.
fn write_col(writer: &mut dyn Write, bytes: &[u8], width: usize) -> io::Result<()> {
    let shown = truncate_bytes(bytes, width);
    writer.write_all(shown)?;
    for _ in shown.len()..width {
        writer.write_all(b" ")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::Finger => {
            println!("Usage: finger [OPTIONS] [user[@host]...]");
            println!();
            println!("User information lookup program.");
            println!();
            println!("Options:");
            println!("  -l               Long output format");
            println!("  -s               Short output format");
            println!("  -p               Don't show .plan file");
            println!("  -m               Match user names only (not real names)");
            println!("  --help           Show this help");
            println!("  --version        Show version");
        }
        Personality::Pinky => {
            println!("Usage: pinky [OPTIONS] [user...]");
            println!();
            println!("Lightweight finger.");
            println!();
            println!("Options:");
            println!("  -l               Long output format");
            println!("  -s               Short output format (default)");
            println!("  -f               Omit header in short format");
            println!("  -b               Omit .plan file in long format");
            println!("  -h               Omit column headings");
            println!("  -p               Omit .project file");
            println!("  -w               Omit full name in short format");
            println!("  --help           Show this help");
            println!("  --version        Show version");
        }
    }
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Finger => "finger",
        Personality::Pinky => "pinky",
    };
    println!("{name} (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // Fully qualified rather than imported: `main` is the only user of it, and
    // this crate is `no_main` under cfg(test), so a `use std::env;` at the top
    // is genuinely unused in the test build and warns there. That warning is
    // how I learned nothing else in the file reads the environment any more --
    // the $USER fallback was the last caller.
    let args: Vec<String> = std::env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help(cfg.personality);
        return 0;
    }

    if cfg.show_version {
        print_version(cfg.personality);
        return 0;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let result = match cfg.personality {
        Personality::Finger => run_finger(&cfg, &mut writer),
        Personality::Pinky => run_pinky(&cfg, &mut writer),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            let name = match cfg.personality {
                Personality::Finger => "finger",
                Personality::Pinky => "pinky",
            };
            eprintln!("{name}: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
// A test that unwraps is asserting the call succeeded, and a panic names the
// line. CLAUDE.md allows these in test modules for that reason.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A machine whose uptime cannot be read does not report having just
    /// booted.
    ///
    /// `"up  0:00"` is what this returned, and it is a thing a real system
    /// says a minute after power-on -- so nothing downstream, and nobody
    /// reading the header, could tell the two apart.
    #[test]
    fn an_unreadable_uptime_is_not_a_freshly_booted_machine() {
        assert_eq!(format_uptime_field(None), "up (unknown)");
        assert_eq!(format_uptime_field(Some("")), "up (unknown)");
        assert_eq!(format_uptime_field(Some("garbage 0")), "up (unknown)");
        assert_eq!(format_uptime_field(Some("-1 0")), "up (unknown)");
    }

    /// ...and a readable one still formats as it did.
    #[test]
    fn a_readable_uptime_formats_as_before() {
        assert_eq!(format_uptime_field(Some("60.0 30.0")), "up  0:01");
        assert_eq!(format_uptime_field(Some("3600 0")), "up  1:00");
        assert_eq!(format_uptime_field(Some("90000 0")), "up 1 day(s),  1:00");
    }

    /// A machine whose load cannot be read does not report being idle.
    ///
    /// Three zeroes is what an idle machine reports, and confirming a machine
    /// is idle is one of the reasons to run `w` at all.
    #[test]
    fn an_unreadable_load_average_is_not_an_idle_machine() {
        assert_eq!(format_load_field(None), "load average: (unknown)");
        assert_eq!(format_load_field(Some("")), "load average: (unknown)");
        assert_eq!(
            format_load_field(Some("0.10 0.20")),
            "load average: (unknown)"
        );
    }

    /// ...and a readable one still formats as it did.
    #[test]
    fn a_readable_load_average_formats_as_before() {
        assert_eq!(
            format_load_field(Some("0.10 0.20 0.30 1/234 5678")),
            "load average: 0.10, 0.20, 0.30"
        );
    }

    /// Neither marker can be mistaken for a value.
    #[test]
    fn the_unknown_marker_is_not_a_possible_reading() {
        assert!(UNKNOWN_FIELD.contains('('));
        assert!(UNKNOWN_FIELD.parse::<f64>().is_err());
    }

    #[test]
    fn test_detect_personality_finger() {
        assert_eq!(detect_personality("finger"), Personality::Finger);
        assert_eq!(detect_personality("/usr/bin/finger"), Personality::Finger);
    }

    #[test]
    fn test_detect_personality_pinky() {
        assert_eq!(detect_personality("pinky"), Personality::Pinky);
        assert_eq!(detect_personality("/usr/bin/pinky"), Personality::Pinky);
    }

    #[test]
    fn test_parse_args_w_user() {
        let args = vec!["finger".to_string(), "root".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.users, vec!["root"]);
    }

    #[test]
    fn pinky_dash_f_omits_the_header_as_its_help_promises() {
        // It set `from_field`, which only `run_w` read, so `pinky -f` did
        // nothing while the help said "Omit header in short format".
        let args = vec!["pinky".to_string(), "-f".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_header);
    }

    #[test]
    fn an_unknown_option_is_refused_under_the_new_default() {
        // `-h` was `w`'s. With `w` gone the default personality is finger,
        // which does not take it -- and says so rather than ignoring it.
        let args = vec!["w".to_string(), "-h".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_w_short() {
        let args = vec!["finger".to_string(), "-s".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.short_format);
    }

    #[test]
    fn test_parse_args_finger_long() {
        let args = vec!["finger".to_string(), "-l".to_string(), "user1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.long_format);
        assert_eq!(cfg.personality, Personality::Finger);
    }

    #[test]
    fn test_parse_args_finger_no_plan() {
        let args = vec!["finger".to_string(), "-p".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_plan);
    }

    #[test]
    fn test_parse_args_finger_no_match() {
        let args = vec!["finger".to_string(), "-m".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(!cfg.match_real_name);
    }

    #[test]
    fn test_parse_args_pinky_basic() {
        let args = vec!["pinky".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Pinky);
    }

    #[test]
    fn test_parse_args_pinky_long() {
        let args = vec!["pinky".to_string(), "-l".to_string(), "user1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.long_format);
    }

    #[test]
    fn test_parse_args_pinky_no_plan() {
        let args = vec!["pinky".to_string(), "-b".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_plan);
    }

    #[test]
    fn test_parse_args_version() {
        for name in &["finger", "pinky"] {
            let args = vec![name.to_string(), "--version".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_version);
        }
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["finger", "pinky"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_format_idle_zero() {
        // The distinction this column never made: "?" is we could not find
        // out, "." is the terminal was touched this second. Every user got
        // "." before, because idle_secs was the literal 0 and nothing ever
        // measured it.
        assert_eq!(format_idle(None), "  ?  ");
        assert_eq!(format_idle(Some(0)), "  .  ");
    }

    #[test]
    fn test_format_idle_seconds() {
        assert_eq!(format_idle(Some(30)), " 30s ");
    }

    #[test]
    fn test_format_idle_minutes() {
        let result = format_idle(Some(300)); // 5 minutes
        assert!(result.contains("5:"));
    }

    #[test]
    fn test_format_idle_hours() {
        let result = format_idle(Some(7200)); // 2 hours
        assert!(result.contains("2:"));
    }

    #[test]
    fn test_format_idle_days() {
        let result = format_idle(Some(172800)); // 2 days
        assert!(result.contains("days"));
    }

    #[test]
    fn test_format_login_time_zero() {
        assert_eq!(format_login_time(0), " ?????");
    }

    #[test]
    fn test_format_login_time_normal() {
        let result = format_login_time(43200); // 12:00
        assert_eq!(result, "12:00");
    }

    #[test]
    fn truncation_leaves_a_short_value_alone() {
        assert_eq!(truncate_bytes(b"hello", 10), b"hello");
        assert_eq!(truncate_bytes(b"hello", 5), b"hello");
    }

    #[test]
    fn truncation_cuts_a_long_value() {
        assert_eq!(truncate_bytes(b"hello world", 5), b"hello");
    }

    #[test]
    fn truncation_in_the_middle_of_a_character_does_not_panic() {
        // THE REASON THIS TAKES BYTES. `truncate_str` did `s[..max]` on a
        // &str, which panics when max lands inside a multi-byte character --
        // so a user called "café" crashed `w` at any width that split the é.
        // Cutting bytes cannot panic; it can split a character, which is one
        // replacement glyph in the terminal and what the C tools do too.
        assert_eq!(truncate_bytes("café".as_bytes(), 4), b"caf\xc3");
        assert_eq!(truncate_bytes(b"caf\xff\xfe", 4), b"caf\xff");
    }

    #[test]
    fn a_column_pads_to_its_width_and_never_past_it() {
        let mut out = Vec::new();
        write_col(&mut out, b"ab", 5).unwrap();
        assert_eq!(out, b"ab   ");

        let mut out = Vec::new();
        write_col(&mut out, b"abcdefgh", 3).unwrap();
        assert_eq!(
            out, b"abc",
            "an over-long value is cut, not allowed to shove the row"
        );

        let mut out = Vec::new();
        write_col(&mut out, b"", 3).unwrap();
        assert_eq!(out, b"   ");
    }

    #[test]
    fn a_non_utf8_username_reaches_the_column_intact() {
        // The whole point of the change: this used to be decoded with
        // from_utf8_lossy somewhere between utmp and the terminal.
        let mut out = Vec::new();
        write_col(&mut out, b"caf\xe9", 6).unwrap();
        assert_eq!(out, b"caf\xe9  ");
    }

    #[test]
    fn test_run_finger_no_users() {
        let cfg = Config {
            personality: Personality::Finger,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_finger(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Login"));
    }

    #[test]
    fn test_run_pinky_no_users() {
        let cfg = Config {
            personality: Personality::Pinky,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_pinky(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Login"));
    }

    #[test]
    fn test_run_finger_remote() {
        let cfg = Config {
            personality: Personality::Finger,
            users: vec!["user@remote.host".to_string()],
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_finger(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Remote finger not supported"));
    }

    #[test]
    fn test_mail_status() {
        assert_ne!(MailStatus::NewMail, MailStatus::NoMail);
        assert_ne!(MailStatus::OldMail, MailStatus::NoMail);
    }

    #[test]
    fn test_get_uptime_str() {
        let s = get_uptime_str();
        assert!(s.contains("up"));
    }

    #[test]
    fn test_get_load_avg() {
        let s = get_load_avg();
        assert!(s.contains("load average"));
    }

    #[test]
    fn test_parse_args_w_unknown() {
        let args = vec!["w".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_finger_unknown() {
        let args = vec!["finger".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_pinky_unknown() {
        let args = vec!["pinky".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }
}
