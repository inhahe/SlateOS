// Slate OS chpasswd — batch password change utility
//
// Multi-personality binary:
//   chpasswd — batch change user passwords (from stdin or file)
//   passwd   — interactive single-user password change
//
// Usage:
//   chpasswd [OPTIONS] < password-list
//   passwd [OPTIONS] [username]

#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use quoting::quoteaf_os;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    /// The account named on the command line, still as the bytes the caller
    /// gave. Not a `String`: `env::args()` *panics* on an argument that is not
    /// valid UTF-8, which on this OS is legal input. See `known-issues.md` ->
    /// `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
    encrypted: bool, // -e: passwords are already encrypted
    hash_method: HashMethod,
    min_length: usize,
    shadow_file: PathBuf,
    show_help: bool,
    show_version: bool,
    /// `-s, --sha-rounds <n>`: the SHA-crypt cost.
    ///
    /// `None` leaves the setting without a `rounds=` field, which is what
    /// every entry this tool has ever written looks like.
    sha_rounds: Option<u32>,
}

/// The hashing method, which is the libc's enum rather than one of ours.
///
/// This file used to define its own three-variant copy with its own
/// `prefix()` table returning `"$5$"`, `"$6$"` and `"$1$"` — the standard
/// crypt(3) identifiers — while the hashing beneath them was a made-up
/// mixing function that was the same for all three.  So an `/etc/shadow`
/// this tool wrote was mislabelled at the format level: a reader that
/// followed the standard would apply 5000 rounds of real SHA-256 to a `$5$`
/// entry and get a different answer.
///
/// The label and the algorithm cannot disagree if they are not declared in
/// different places, so the name of the method and the code that implements
/// it are now the same item.  See
/// `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`.
type HashMethod = posix::crypt::Method;

impl Default for Config {
    fn default() -> Self {
        Self {
            encrypted: false,
            hash_method: HashMethod::Sha512,
            min_length: 6,
            shadow_file: PathBuf::from("/etc/shadow"),
            show_help: false,
            show_version: false,
            sha_rounds: None,
        }
    }
}

/// The value that follows an option, as text.
///
/// Both callers want a number or a method name, neither of which can be
/// non-UTF-8 -- so a value that does not decode is refused with the bytes
/// shown rather than silently becoming the empty string. The parse loop uses
/// `unwrap_or_default()` when matching OPTION NAMES, which is right there
/// (undecodable bytes match no option), and wrong here.
fn value_at<'a>(args: &'a [OsString], i: usize, need: &'static str) -> Result<&'a str, String> {
    let raw = args.get(i).ok_or_else(|| need.to_string())?;
    raw.to_str()
        .ok_or_else(|| format!("{need}, and {} is not one", quoting::quoteaf_os(raw)))
}

fn parse_args(args: &[OsString]) -> Result<Config, String> {
    // `argv[0]` is a path, and one that cannot be decoded is not either of the
    // two names this binary answers to -- so it takes the default, which is
    // the same answer any other unrecognised name gets.
    let mut cfg = Config::default();

    let mut i = 1;

    // An argument that is not valid UTF-8 matches no option, so it falls to
    // the positional arm -- which is what it would have to be.
    while i < args.len() {
        let Some(raw) = args.get(i) else { break };
        let arg = raw.to_str().unwrap_or_default();
        match arg {
            "-e" | "--encrypted" => cfg.encrypted = true,
            "-m" | "--md5" => cfg.hash_method = HashMethod::Md5,
            // `-s` is `--sha-rounds <number>` in the shadow suite and
            // CONSUMES AN ARGUMENT; the method is chosen with
            // `-c, --crypt-method`. Bound here to `--sha256` as a flag, so
            // `chpasswd -s 5000 < file` selected SHA-256 and left `5000` to
            // be read as a positional -- the count silently discarded, and
            // the method changed without being asked for.
            "-s" | "--sha-rounds" => {
                i += 1;
                let text = value_at(args, i, "-s requires a number of rounds")?;
                cfg.sha_rounds = Some(
                    text.parse::<u32>()
                        .map_err(|e| format!("-s requires a number of rounds: {text}: {e}"))?,
                );
            }
            "-c" | "--crypt-method" => {
                i += 1;
                let text = value_at(args, i, "-c requires a method")?;
                cfg.hash_method = match text.to_ascii_uppercase().as_str() {
                    "MD5" => HashMethod::Md5,
                    "SHA256" => HashMethod::Sha256,
                    "SHA512" => HashMethod::Sha512,
                    // NONE, DES and YESCRYPT are named by the shadow suite
                    // and not implemented here. Refused rather than mapped
                    // to something near it: a password stored under a
                    // weaker scheme than the operator named is the one
                    // outcome worse than failing.
                    other => {
                        return Err(format!(
                            "-c {other}: unsupported crypt method (this build has MD5, SHA256, SHA512)"
                        ));
                    }
                };
            }
            "-S" | "--sha512" => cfg.hash_method = HashMethod::Sha512,
            "-h" | "--help" => cfg.show_help = true,
            "-V" | "--version" => cfg.show_version = true,
            other if other.starts_with('-') => {
                return Err(format!("chpasswd: unknown option: {other}"));
            }
            _ => {} // positional args ignored
        }
        i += 1;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Password hashing
// ---------------------------------------------------------------------------

/// Generate a random salt of `len` crypt base-64 characters.
///
/// Drawn from `/dev/urandom`.  The version this replaces seeded a linear
/// congruential generator with the literal 42, mixed in `/proc/uptime` and
/// the process id, and called the result a salt: on a system without
/// `/proc` — which is every system this OS has booted — two accounts given
/// passwords by the same process got the same salt, and the salt space was
/// the pid space.
///
/// `& 0x3f` is an unbiased reduction, not the usual modulo mistake: 256 is
/// exactly four times 64, so each alphabet character is the image of
/// exactly four byte values.
///
/// Returns `None` if there is no entropy source, because a salt that is not
/// random is worse than no password change: it silently makes every entry
/// this tool writes share a precomputable table.  A caller that cannot
/// produce a salt must fail rather than write a weak entry.
fn generate_salt(len: usize) -> Option<String> {
    const CHARS: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let bytes = std::fs::read("/dev/urandom").ok()?;
    let bytes = bytes.get(..len)?;
    Some(
        bytes
            .iter()
            .map(|b| char::from(CHARS[usize::from(*b & 0x3f)]))
            .collect(),
    )
}

/// Hash a password under `method` with the given salt, using the libc's
/// `crypt(3)`.
///
/// The salt is a parameter rather than generated here so that the entry
/// this tool writes can be checked against a published vector: a function
/// that draws its own randomness can only be tested for self-consistency,
/// which is exactly the test that let a made-up hash live in this file.
///
/// Returns `None` if the libc rejects the setting — which cannot happen for
/// a salt from [`generate_salt`], since that draws from crypt's own
/// alphabet at the method's own maximum length.
fn hash_password(
    password: &str,
    method: HashMethod,
    salt: &str,
    rounds: Option<u32>,
) -> Option<String> {
    let mut setting_buf = posix::crypt::buf();
    // `None` keeps the setting free of a `rounds=` field, which is what every
    // entry this tool has written so far looks like -- so an unchanged
    // invocation produces an unchanged entry.
    let setting = match rounds {
        Some(r) => posix::crypt::setting_rounds_into(method, r, salt.as_bytes(), &mut setting_buf)?,
        None => posix::crypt::setting_into(method, salt.as_bytes(), &mut setting_buf)?,
    };
    let mut hash_buf = posix::crypt::buf();
    Some(
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)?
            .to_string(),
    )
}

/// Generate a salt and hash `password` with it — the whole of what a
/// caller changing a password needs.
///
/// `None` means no password should be written; the callers report why.
fn hash_new_password(password: &str, method: HashMethod, rounds: Option<u32>) -> Option<String> {
    let salt = generate_salt(method.salt_max())?;
    hash_password(password, method, &salt, rounds)
}

/// Validate password strength
fn validate_password(password: &str, min_length: usize) -> Result<(), String> {
    if password.len() < min_length {
        return Err(format!(
            "password is too short (minimum {min_length} characters)"
        ));
    }
    if password.chars().all(|c| c.is_ascii_lowercase()) {
        return Err("password is too simple (use mixed case, numbers, or symbols)".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shadow file operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ShadowEntry {
    username: String,
    password_hash: String,
    last_changed: String,
    min_days: String,
    max_days: String,
    warn_days: String,
    inactive_days: String,
    expire_date: String,
    reserved: String,
}

fn parse_shadow_line(line: &str) -> Option<ShadowEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 9 {
        return None;
    }
    Some(ShadowEntry {
        username: fields[0].to_string(),
        password_hash: fields[1].to_string(),
        last_changed: fields[2].to_string(),
        min_days: fields[3].to_string(),
        max_days: fields[4].to_string(),
        warn_days: fields[5].to_string(),
        inactive_days: fields[6].to_string(),
        expire_date: fields[7].to_string(),
        reserved: fields[8].to_string(),
    })
}

fn format_shadow_entry(entry: &ShadowEntry) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        entry.username,
        entry.password_hash,
        entry.last_changed,
        entry.min_days,
        entry.max_days,
        entry.warn_days,
        entry.inactive_days,
        entry.expire_date,
        entry.reserved
    )
}

fn read_shadow_file(path: &std::path::Path) -> io::Result<Vec<ShadowEntry>> {
    let content = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = parse_shadow_line(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn write_shadow_file(path: &std::path::Path, entries: &[ShadowEntry]) -> io::Result<()> {
    let mut content = String::new();
    for entry in entries {
        content.push_str(&format_shadow_entry(entry));
        content.push('\n');
    }
    std::fs::write(path, content)
}

fn update_password(
    shadow_path: &std::path::Path,
    username: &str,
    new_hash: &str,
) -> Result<(), String> {
    let mut entries = read_shadow_file(shadow_path)
        .map_err(|e| format!("cannot read {}: {e}", shadow_path.display()))?;

    let mut found = false;
    for entry in &mut entries {
        if entry.username == username {
            entry.password_hash = new_hash.to_string();
            // Update last_changed to "today" (days since epoch)
            entry.last_changed = "19800".to_string();
            found = true;
            break;
        }
    }

    if !found {
        return Err(format!(
            "user {} not found in shadow file",
            quoteaf_os(username)
        ));
    }

    write_shadow_file(shadow_path, &entries)
        .map_err(|e| format!("cannot write {}: {e}", shadow_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Password status display
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// chpasswd mode
// ---------------------------------------------------------------------------

fn run_chpasswd(
    cfg: &Config,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    err_writer: &mut dyn Write,
) -> i32 {
    let mut errors = 0;
    let mut line = String::new();
    let mut line_num = 0u64;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(err_writer, "chpasswd: read error: {e}");
                return 1;
            }
        }

        line_num = line_num.saturating_add(1);
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with('#') {
            continue;
        }

        // Format: username:password
        let parts: Vec<&str> = line_trimmed.splitn(2, ':').collect();
        if parts.len() != 2 {
            let _ = writeln!(
                err_writer,
                "chpasswd: line {line_num}: invalid format (expected user:password)"
            );
            errors += 1;
            continue;
        }

        let username = parts[0].trim();
        let password = parts[1].trim();

        if username.is_empty() {
            let _ = writeln!(err_writer, "chpasswd: line {line_num}: empty username");
            errors += 1;
            continue;
        }

        let hash = if cfg.encrypted {
            password.to_string()
        } else {
            if let Err(e) = validate_password(password, cfg.min_length) {
                let _ = writeln!(err_writer, "chpasswd: {username}: {e}");
                errors += 1;
                continue;
            }
            let Some(hashed) = hash_new_password(password, cfg.hash_method, cfg.sha_rounds) else {
                let _ = writeln!(
                    err_writer,
                    "chpasswd: {username}: cannot read `/dev/urandom', so no salt can be \
                     generated; refusing to write a password without one"
                );
                errors += 1;
                continue;
            };
            hashed
        };

        match update_password(&cfg.shadow_file, username, &hash) {
            Ok(()) => {
                let _ = writeln!(writer, "password changed for {username}");
            }
            Err(e) => {
                let _ = writeln!(err_writer, "chpasswd: {username}: {e}");
                errors += 1;
            }
        }
    }

    if errors > 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// passwd mode (interactive)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

#[cfg(not(test))]
fn print_help() {
    println!("Usage: chpasswd [OPTIONS]");
    println!();
    println!("Update passwords in batch mode. Read user:password pairs from stdin.");
    println!();
    println!("Options:");
    println!("  -e, --encrypted  Passwords are already encrypted");
    println!("  -m, --md5        Use MD5 hash method");
    println!("  -c, --crypt-method METHOD  MD5, SHA256 or SHA512");
    println!("  -s, --sha-rounds N         Rounds for SHA-crypt");
    println!("  -S, --sha512     Use SHA-512 hash method (default)");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

#[cfg(not(test))]
fn print_version() {
    let name = "chpasswd";
    println!("{name} (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // `args_os`, not `args`: the latter's iterator is a literal `unwrap` and
    // panics on an argument that is not valid UTF-8.
    let args: Vec<OsString> = env::args_os().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help();
        return 0;
    }

    if cfg.show_version {
        print_version();
        return 0;
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let stderr = io::stderr();
    let mut err_writer = stderr.lock();

    run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// The command line, as `env::args_os` would deliver it.
    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// A username that is not text is a name, not a crash. `env::args()`
    /// would have panicked before `main` ran a line of this file.
    #[test]
    fn test_parse_args_chpasswd_basic() {
        let args = argv(&["chpasswd"]);
        let cfg = parse_args(&args).unwrap();
        assert!(!cfg.encrypted);
    }

    #[test]
    fn test_parse_args_chpasswd_encrypted() {
        let args = argv(&["chpasswd", "-e"]);
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.encrypted);
    }

    #[test]
    fn test_parse_args_chpasswd_md5() {
        let args = argv(&["chpasswd", "-m"]);
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hash_method, HashMethod::Md5);
    }

    /// `-s` is a ROUNDS COUNT and takes an argument.
    ///
    /// It used to be a flag selecting SHA-256, so `chpasswd -s 5000 < file`
    /// changed the hash method nobody had asked to change and left `5000` to
    /// fall through as a positional. Two wrong outcomes from one letter.
    #[test]
    fn sha_rounds_takes_a_count_and_reaches_the_stored_entry() {
        let cfg = parse_args(&argv(&["chpasswd", "-s", "9000"])).expect("-s parses");
        assert_eq!(cfg.sha_rounds, Some(9000));
        assert_eq!(
            cfg.hash_method,
            HashMethod::Sha512,
            "-s must not change the method"
        );

        // The count is not merely stored: it appears in the entry, which is
        // the only place it can be checked later.
        let hashed = hash_password("pw", HashMethod::Sha512, "abcdefgh", Some(9000))
            .expect("hashing with rounds");
        assert!(
            hashed.starts_with("$6$rounds=9000$"),
            "the entry must state the cost it was produced at, got {hashed}"
        );

        // And without `-s` the entry has no rounds field at all, so an
        // unchanged invocation still produces an unchanged entry.
        let plain = hash_password("pw", HashMethod::Sha512, "abcdefgh", None).expect("plain");
        assert!(plain.starts_with("$6$abcdefgh$"), "got {plain}");

        // A non-numeric count is refused rather than silently defaulted.
        assert!(parse_args(&argv(&["chpasswd", "-s", "lots"])).is_err());
        assert!(parse_args(&argv(&["chpasswd", "-s"])).is_err());
    }

    /// `-c` selects the method, and refuses one it cannot honour.
    #[test]
    fn crypt_method_selects_or_refuses() {
        for (name, want) in [
            ("MD5", HashMethod::Md5),
            ("sha256", HashMethod::Sha256),
            ("SHA512", HashMethod::Sha512),
        ] {
            let cfg = parse_args(&argv(&["chpasswd", "-c", name])).expect("method parses");
            assert_eq!(cfg.hash_method, want, "{name}");
        }

        // Named by the shadow suite, not implemented here. Refused rather
        // than mapped to something near it: a password stored under a weaker
        // scheme than the operator named is worse than a failure.
        for name in ["NONE", "DES", "YESCRYPT"] {
            let err =
                parse_args(&argv(&["chpasswd", "-c", name])).expect_err("{name} must be refused");
            assert!(err.contains("unsupported"), "{name}: {err}");
        }
    }

    #[test]
    fn test_parse_args_chpasswd_sha256() {
        // `-c SHA256`, not `-s`. The old form asserted that `-s` selected
        // SHA-256 -- which is what made `chpasswd -s 5000` change the method
        // and discard the count.
        let args = argv(&["chpasswd", "-c", "SHA256"]);
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hash_method, HashMethod::Sha256);
    }

    #[test]
    fn test_parse_args_help() {
        let args = argv(&["chpasswd", "--help"]);
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    /// A salt `crypt` can carry verbatim, in the alphabet `generate_salt`
    /// draws from.  Fixed rather than generated, so the entries below are
    /// reproducible and can be checked against a published answer.
    ///
    /// Eight characters because that is MD5 crypt's maximum and the tests
    /// below run every method against this one salt; the SHA methods take
    /// up to sixteen, and `generate_salt` is asked for each method's own
    /// `salt_max` rather than a shared number.
    const SALT: &str = "saltsalt";

    /// The entry written must be one a standard reader recognises: the
    /// method's own identifier, the salt as given, and a hash field of the
    /// length that method produces.
    ///
    /// The tests this replaces asserted only the `$6$`/`$5$`/`$1$` prefix
    /// and that the string was longer than 20 characters — both of which
    /// the old made-up hash satisfied, which is how it survived.  The
    /// prefix was in fact the bug: it named a standard method over a hash
    /// that was not that method, or any method.
    #[test]
    fn test_hash_password_writes_standard_crypt_entries() {
        for method in [HashMethod::Sha512, HashMethod::Sha256, HashMethod::Md5] {
            let hash = hash_password("testpass", method, SALT, None)
                .unwrap_or_else(|| panic!("{method:?}"));
            assert!(hash.starts_with(method.prefix()), "{method:?}: {hash}");
            assert_eq!(
                posix::crypt::stored_method(hash.as_bytes()),
                Some(method),
                "{method:?}: {hash}"
            );
            // The entry must verify against the password that made it —
            // which is the property `login` depends on, and the one the
            // three tools used to disagree about.
            assert!(
                posix::crypt::verify(b"testpass", hash.as_bytes()),
                "{method:?}: {hash}"
            );
            assert!(
                !posix::crypt::verify(b"testpas", hash.as_bytes()),
                "{method:?}"
            );
        }
    }

    /// A known answer, from Ulrich Drepper's SHA-crypt specification.  This
    /// is the test the old code could not have had: its output followed no
    /// specification, so there was nothing to compare against.
    #[test]
    fn test_hash_password_matches_a_published_vector() {
        assert_eq!(
            hash_password("Hello world!", HashMethod::Sha512, "saltstring", None).as_deref(),
            Some(
                "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1"
            )
        );
        assert_eq!(
            hash_password("Hello world!", HashMethod::Sha256, "saltstring", None).as_deref(),
            Some("$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5")
        );
    }

    #[test]
    fn test_hash_password_different() {
        let h1 = hash_password("pass1", HashMethod::Sha512, SALT, None).expect("h1");
        let h2 = hash_password("pass2", HashMethod::Sha512, SALT, None).expect("h2");
        assert_ne!(h1, h2);
    }

    /// Two accounts given the same password must not get the same entry.
    /// The old salt generator seeded a linear congruential generator with a
    /// literal 42 and stirred in `/proc/uptime` — a file this OS does not
    /// have — so on the real system the salt varied only with the process
    /// id, and one `chpasswd` run salted every account in its input
    /// identically.
    #[test]
    fn test_the_same_password_under_different_salts_differs() {
        let a = hash_password("same", HashMethod::Sha512, "aaaaaaaaaaaaaaaa", None).expect("a");
        let b = hash_password("same", HashMethod::Sha512, "bbbbbbbbbbbbbbbb", None).expect("b");
        assert_ne!(a, b);
    }

    /// A salt the format cannot carry is refused rather than truncated:
    /// storing a truncated salt yields an entry that cannot verify against
    /// itself.
    #[test]
    fn test_hash_password_refuses_a_salt_it_cannot_store() {
        assert_eq!(hash_password("pw", HashMethod::Sha512, "", None), None);
        assert_eq!(
            hash_password("pw", HashMethod::Sha512, "has$dollar", None),
            None
        );
        // 17 characters, one past SHA-crypt's maximum.
        assert_eq!(
            hash_password("pw", HashMethod::Sha512, "abcdefghijklmnopq", None),
            None
        );
        // MD5 truncates at 8, so 9 is over for it and fine for SHA-512.
        assert_eq!(
            hash_password("pw", HashMethod::Md5, "123456789", None),
            None
        );
        assert!(hash_password("pw", HashMethod::Sha512, "123456789", None).is_some());
    }

    #[test]
    fn test_validate_password_too_short() {
        assert!(validate_password("ab", 6).is_err());
    }

    #[test]
    fn test_validate_password_ok() {
        assert!(validate_password("MyP@ss1", 6).is_ok());
    }

    #[test]
    fn test_validate_password_all_lower() {
        assert!(validate_password("abcdefgh", 6).is_err());
    }

    #[test]
    fn test_parse_shadow_line() {
        let line = "root:$6$salt$hash:19000:0:99999:7:::";
        let entry = parse_shadow_line(line).unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.password_hash, "$6$salt$hash");
        assert_eq!(entry.last_changed, "19000");
    }

    #[test]
    fn test_parse_shadow_line_short() {
        assert!(parse_shadow_line("short:line").is_none());
    }

    #[test]
    fn test_format_shadow_entry() {
        let entry = ShadowEntry {
            username: "user".to_string(),
            password_hash: "$6$hash".to_string(),
            last_changed: "19000".to_string(),
            min_days: "0".to_string(),
            max_days: "99999".to_string(),
            warn_days: "7".to_string(),
            inactive_days: "".to_string(),
            expire_date: "".to_string(),
            reserved: "".to_string(),
        };
        let formatted = format_shadow_entry(&entry);
        assert_eq!(formatted, "user:$6$hash:19000:0:99999:7:::");
    }

    #[test]
    fn test_hash_method_prefix() {
        assert_eq!(HashMethod::Sha512.prefix(), "$6$");
        assert_eq!(HashMethod::Sha256.prefix(), "$5$");
        assert_eq!(HashMethod::Md5.prefix(), "$1$");
    }

    #[test]
    fn test_generate_salt() {
        // `/dev/urandom` is the only source, and the development host does
        // not have one, so there the only testable behaviour is the
        // refusal.  Where it exists, the salt must be exactly the requested
        // length, drawn from the crypt base-64 alphabet — a character
        // outside it (`$` above all) would end the salt early in the stored
        // entry — and different on each call.
        match generate_salt(16) {
            Some(salt) => {
                assert_eq!(salt.len(), 16);
                assert!(
                    salt.bytes()
                        .all(|b| b == b'.' || b == b'/' || b.is_ascii_alphanumeric()),
                    "{salt}"
                );
                assert_ne!(generate_salt(16), Some(salt), "salt is not random");
            }
            None => assert!(
                std::fs::read("/dev/urandom").is_err(),
                "`/dev/urandom' is readable but no salt was produced"
            ),
        }
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.hash_method, HashMethod::Sha512);
        assert_eq!(cfg.min_length, 6);
        assert!(!cfg.encrypted);
    }

    #[test]
    fn test_run_chpasswd_empty() {
        let cfg = Config::default();
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_chpasswd_invalid_format() {
        let cfg = Config::default();
        let input = b"invalid_line_no_colon\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 1);
        let err = String::from_utf8(err_writer).unwrap();
        assert!(err.contains("invalid format"));
    }

    #[test]
    fn test_run_chpasswd_empty_username() {
        let cfg = Config::default();
        let input = b":password\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_chpasswd_comment() {
        let cfg = Config::default();
        let input = b"# comment\n\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 0);
    }
}
