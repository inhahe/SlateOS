//! Slate OS password-aging utility.
//!
//! `chage` shows and changes the six numbers that make up an account's
//! password-aging policy: when the password was last changed, how soon and how
//! late it may be changed again, how long before expiry the user is warned,
//! how long after expiry the account still accepts it, and the day the account
//! itself expires.
//!
//! # It writes now
//!
//! Two things were wrong with the version this replaces, and the second is the
//! reason for the first.
//!
//! It **never wrote anything**. Every option was parsed, the entry was
//! modified in memory, `chage: updated aging for <user>` was printed to
//! stderr, and the resulting `/etc/shadow` line was printed to *stdout* — and
//! that was all. Nothing on disk changed. A policy an administrator set with
//! this command was gone the moment the process exited.
//!
//! And it read the wrong file. `design-decisions.md` §353 makes `/etc/shadow`
//! *generated* from `/etc/users.yaml`, so even a version that had written the
//! line correctly would have had its work undone by the next account change
//! from any other tool. The six numbers live in the database, as
//! [`userdb::Aging`], and that is what this edits.
//!
//! A third followed from reading a file that may not exist: when
//! `/etc/shadow` could not be read, it **invented three accounts** — `root`,
//! `user` and `nobody`, with made-up password hashes and made-up policies —
//! and reported their fabricated aging as fact. `chage -l root` on a machine
//! with no shadow file printed a policy nobody had ever set.
//!
//! # Absent is not zero
//!
//! Each of the six is optional, and an absent one is *no policy*, not a policy
//! of zero. The distinction is not pedantic: `0` in the maximum-days column
//! means "expired the day it was set", while an empty column means "does not
//! expire". So `-1` on the command line clears a field, and a field nobody has
//! set prints as `-1` rather than as an invented default.
//!
//! # Usage
//!
//! ```text
//! chage -l USER            Show the aging policy
//! chage USER               Change it, prompting for each value
//! chage -m DAYS USER       Minimum days between password changes
//! chage -M DAYS USER       Maximum days a password stays valid
//! chage -W DAYS USER       Days of warning before expiry
//! chage -I DAYS USER       Days after expiry before the account stops working
//! chage -E DATE USER       Date the account itself expires
//! chage -d DATE USER       Date the password was last changed
//! ```
//!
//! `DAYS` is a number, or `-1` to clear the field. `DATE` is `YYYY-MM-DD`, a
//! number of days since 1970-01-01, or `-1` to clear it.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;
use userdb::{Aging, UserDb};

const VERSION: &str = "0.1.0";

// ============================================================================
// The account database
// ============================================================================

/// Read the database, or explain why it could not be read.
///
/// A database that cannot be read is **not** an empty one, and it is certainly
/// not three invented accounts. Reporting the failure is the whole of the fix
/// for the fabricated `root`/`user`/`nobody` entries this used to produce:
/// there is no answer to "what is root's password policy?" that can be given
/// without the file, so none is given.
fn load(path: &Path) -> Result<UserDb, String> {
    UserDb::load(path).map_err(|e| format!("cannot read `{}': {e}", path.display()))
}

/// Write the database back, regenerating `/etc/passwd` and `/etc/shadow`.
fn store(db: &UserDb, path: &Path) -> Result<(), String> {
    db.save(path)
        .map_err(|e| format!("cannot write `{}': {e}", path.display()))
}

// ============================================================================
// Values on the command line
// ============================================================================

/// A `DAYS` argument: a count, or `-1` for "no policy".
///
/// Negative means *clear the field*, not "store minus one day". `chage(1)` and
/// `passwd(1)` both spell "no policy" as `-1` on the command line, but a
/// literal `-1` left in `/etc/shadow` is read by glibc as a date one day
/// before the epoch — so a `-1` that reached the file would turn "never
/// expires" into "expired since 1969", which is the opposite of what was asked.
///
/// # Errors
///
/// A string that is not a number, since storing something else would put a
/// value in a column that a later reader will take for a day count.
fn parse_days(text: &str) -> Result<Option<i64>, String> {
    let text = text.trim();
    match text.parse::<i64>() {
        Ok(days) if days < 0 => Ok(None),
        Ok(days) => Ok(Some(days)),
        Err(_) => Err(format!("invalid number of days: `{text}'")),
    }
}

/// A `DATE` argument: `YYYY-MM-DD`, a day number, or `-1`/empty for "never".
///
/// # Errors
///
/// A string that is neither, for the reason [`parse_days`] refuses one.
fn parse_date(text: &str) -> Result<Option<i64>, String> {
    let text = text.trim();
    if text.is_empty() || text == "never" {
        return Ok(None);
    }
    if let Ok(days) = text.parse::<i64>() {
        return Ok(if days < 0 { None } else { Some(days) });
    }
    userdb::days_from_date(text)
        .map(Some)
        .ok_or_else(|| format!("invalid date `{text}': expected YYYY-MM-DD, a day count, or -1"))
}

// ============================================================================
// Display
// ============================================================================

/// A day count as `-l` prints it: the number, or `-1` for no policy.
///
/// `-1` and not a blank or a word, so that what is printed is what would be
/// typed to set it back to this.
fn show_days(value: Option<i64>) -> String {
    match value {
        Some(days) => days.to_string(),
        None => "-1".to_string(),
    }
}

/// A date as `-l` prints it, with `never` for a field nobody set.
fn show_date(value: Option<i64>) -> String {
    match value {
        Some(days) => userdb::date_from_days(days),
        None => "never".to_string(),
    }
}

/// The day a password expires: the day it was set, plus its maximum age.
///
/// `None` unless *both* are known, because neither alone is an expiry date —
/// and unlike the old code, which added an absent maximum as `99999` and an
/// absent change date as `0`, this does not invent the missing half and then
/// print the sum as a fact.
fn expiry_day(aging: &Aging) -> Option<i64> {
    aging.changed?.checked_add(aging.max_days?)
}

/// The day the account stops accepting the password: its expiry, plus the
/// inactive period.
fn inactive_day(aging: &Aging) -> Option<i64> {
    expiry_day(aging)?.checked_add(aging.inactive_days?)
}

fn display_aging(out: &mut impl Write, aging: &Aging) {
    // A password dated to the epoch is `/etc/shadow`'s way of saying "must be
    // changed at the next login", which is what `passwd -e` writes. Printing
    // it as the date 1970-01-01 would be true and useless.
    let last_change = match aging.changed {
        Some(0) => "password must be changed".to_string(),
        other => show_date(other),
    };
    let _ = writeln!(out, "Last password change\t\t\t\t\t: {last_change}");
    let _ = writeln!(
        out,
        "Password expires\t\t\t\t\t: {}",
        show_date(expiry_day(aging))
    );
    let _ = writeln!(
        out,
        "Password inactive\t\t\t\t\t: {}",
        show_date(inactive_day(aging))
    );
    let _ = writeln!(
        out,
        "Account expires\t\t\t\t\t\t: {}",
        show_date(aging.expires)
    );
    let _ = writeln!(
        out,
        "Minimum number of days between password change\t\t: {}",
        show_days(aging.min_days)
    );
    let _ = writeln!(
        out,
        "Maximum number of days between password change\t\t: {}",
        show_days(aging.max_days)
    );
    let _ = writeln!(
        out,
        "Number of days of warning before password expires\t: {}",
        show_days(aging.warn_days)
    );
}

// ============================================================================
// The interactive form
// ============================================================================

/// Ask for one value, offering the current one as the default.
///
/// End-of-input keeps the current value rather than clearing it: a `chage`
/// whose stdin closes must not silently remove an expiry policy.
fn ask(prompt: &str, current: &str) -> Option<String> {
    eprint!("\t{prompt} [{current}]: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            let answer = line.trim().to_string();
            if answer.is_empty() {
                None
            } else {
                Some(answer)
            }
        }
    }
}

/// Walk the six fields, in the order `chage(1)` asks for them.
///
/// A value that does not parse ends the form rather than being skipped: the
/// administrator is editing a policy field by field, and carrying on past a
/// rejected answer would save the other five as though the sixth had been
/// accepted.
fn interactive(aging: &Aging) -> Result<Aging, String> {
    let mut next = *aging;
    eprintln!("Enter the new value, or press ENTER for the default");
    eprintln!();

    if let Some(answer) = ask("Minimum Password Age", &show_days(aging.min_days)) {
        next.min_days = parse_days(&answer)?;
    }
    if let Some(answer) = ask("Maximum Password Age", &show_days(aging.max_days)) {
        next.max_days = parse_days(&answer)?;
    }
    if let Some(answer) = ask(
        "Last Password Change (YYYY-MM-DD)",
        &show_date(aging.changed),
    ) {
        next.changed = parse_date(&answer)?;
    }
    if let Some(answer) = ask("Password Expiration Warning", &show_days(aging.warn_days)) {
        next.warn_days = parse_days(&answer)?;
    }
    if let Some(answer) = ask("Password Inactive", &show_days(aging.inactive_days)) {
        next.inactive_days = parse_days(&answer)?;
    }
    if let Some(answer) = ask(
        "Account Expiration Date (YYYY-MM-DD)",
        &show_date(aging.expires),
    ) {
        next.expires = parse_date(&answer)?;
    }
    Ok(next)
}

// ============================================================================
// Who is asking
// ============================================================================

/// The caller's uid, from the environment the login process sets.
fn current_uid() -> u32 {
    env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn current_username() -> Option<String> {
    env::var("USER").ok()
}

// ============================================================================
// Arguments
// ============================================================================

/// What a run was asked to do.
struct Args {
    list: bool,
    username: Option<String>,
    /// The requested changes, in the order the options were given. Empty means
    /// no `-m`/`-M`/`-W`/`-I`/`-E`/`-d` was passed at all, which is what tells
    /// a bare `chage USER` apart from one that set a field to its own value.
    changes: Vec<Change>,
}

/// One field to set.
enum Change {
    Min(Option<i64>),
    Max(Option<i64>),
    Warn(Option<i64>),
    Inactive(Option<i64>),
    Expires(Option<i64>),
    Changed(Option<i64>),
}

impl Change {
    fn apply(&self, aging: &mut Aging) {
        match self {
            Self::Min(v) => aging.min_days = *v,
            Self::Max(v) => aging.max_days = *v,
            Self::Warn(v) => aging.warn_days = *v,
            Self::Inactive(v) => aging.inactive_days = *v,
            Self::Expires(v) => aging.expires = *v,
            Self::Changed(v) => aging.changed = *v,
        }
    }
}

fn print_usage() {
    eprintln!("Usage: chage [options] <username>");
    eprintln!();
    eprintln!("Show or change a user's password aging policy.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -l, --list            Show the aging policy");
    eprintln!("  -m, --mindays DAYS    Minimum days between password changes");
    eprintln!("  -M, --maxdays DAYS    Maximum days a password stays valid");
    eprintln!("  -W, --warndays DAYS   Days of warning before expiry");
    eprintln!("  -I, --inactive DAYS   Days after expiry before the account stops");
    eprintln!("  -E, --expiredate DATE Date the account itself expires");
    eprintln!("  -d, --lastday DATE    Date the password was last changed");
    eprintln!("  -h, --help            Show this help");
    eprintln!("  -V, --version         Show the version");
    eprintln!();
    eprintln!("DAYS is a number, or -1 to clear the field.");
    eprintln!("DATE is YYYY-MM-DD, a count of days since 1970-01-01, or -1 for never.");
}

/// The value that follows an option, advancing past it.
///
/// # Errors
///
/// If the option was the last argument. A missing value is refused rather than
/// treated as absent, because every option here that takes one has no meaning
/// without it, and the alternative -- silently ignoring the option -- is how a
/// command comes to report success for work it did not do.
fn value_at(argv: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i = i.saturating_add(1);
    argv.get(*i)
        .cloned()
        .ok_or_else(|| format!("option {name} requires an argument"))
}

/// # Errors
///
/// Any malformed option or value. Parsing is completed before anything is
/// read or written, so a run with a bad argument changes nothing.
fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        list: false,
        username: None,
        changes: Vec::new(),
    };
    let mut i = 0;

    while let Some(arg) = argv.get(i).map(String::as_str) {
        // Bound in each arm rather than up front, because reading a value
        // advances `i` and only the arms that take one may do that.
        let mut value = |name: &str| value_at(argv, &mut i, name);
        match arg {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("chage {VERSION}");
                process::exit(0);
            }
            "-l" | "--list" => args.list = true,
            "-m" | "--mindays" => {
                let v = value("-m")?;
                args.changes.push(Change::Min(parse_days(&v)?));
            }
            "-M" | "--maxdays" => {
                let v = value("-M")?;
                args.changes.push(Change::Max(parse_days(&v)?));
            }
            "-W" | "--warndays" => {
                let v = value("-W")?;
                args.changes.push(Change::Warn(parse_days(&v)?));
            }
            "-I" | "--inactive" => {
                let v = value("-I")?;
                args.changes.push(Change::Inactive(parse_days(&v)?));
            }
            "-E" | "--expiredate" => {
                let v = value("-E")?;
                args.changes.push(Change::Expires(parse_date(&v)?));
            }
            "-d" | "--lastday" => {
                let v = value("-d")?;
                args.changes.push(Change::Changed(parse_date(&v)?));
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                if args.username.is_some() {
                    return Err(format!("unexpected argument: {other}"));
                }
                args.username = Some(other.to_string());
            }
        }
        i = i.saturating_add(1);
    }
    Ok(args)
}

// ============================================================================
// The command
// ============================================================================

fn cmd_chage(argv: &[String], path: &Path, caller_uid: u32) -> i32 {
    let args = match parse_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("chage: {e}");
            print_usage();
            return 1;
        }
    };

    let Some(username) = args.username else {
        eprintln!("chage: no username specified");
        print_usage();
        return 1;
    };

    if args.list && !args.changes.is_empty() {
        eprintln!("chage: -l cannot be combined with an option that changes something");
        return 1;
    }

    // Only root may change an aging policy, and only root may read another
    // account's. A user may read their own: the policy governs when they will
    // next be made to change their password, which they are entitled to know.
    let own_account = current_username().as_deref() == Some(username.as_str());
    if caller_uid != 0 && !(args.list && own_account) {
        eprintln!("chage: only root may change password aging information");
        return 1;
    }

    let mut db = match load(path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("chage: {e}");
            return 1;
        }
    };

    let Some(record) = db.find(&username) else {
        eprintln!("chage: user {} does not exist", quoteaf_os(&username));
        return 1;
    };
    let aging = record.aging();

    if args.list {
        let stdout = io::stdout();
        display_aging(&mut stdout.lock(), &aging);
        return 0;
    }

    let next = if args.changes.is_empty() {
        eprintln!(
            "Changing the aging information for {}",
            quoteaf_os(&username)
        );
        match interactive(&aging) {
            Ok(next) => next,
            Err(e) => {
                eprintln!("chage: {e}");
                return 1;
            }
        }
    } else {
        let mut next = aging;
        for change in &args.changes {
            change.apply(&mut next);
        }
        next
    };

    if next == aging {
        // Nothing to write, and saying "updated" would be the very claim this
        // command used to make falsely.
        eprintln!(
            "chage: aging information unchanged for {}",
            quoteaf_os(&username)
        );
        return 0;
    }

    let Some(record) = db.find_mut(&username) else {
        eprintln!(
            "chage: internal error: user {} was present a moment ago and is not now",
            quoteaf_os(&username)
        );
        return 1;
    };
    record.set_aging(&next);

    if let Err(e) = store(&db, path) {
        eprintln!("chage: {e}");
        return 1;
    }

    eprintln!(
        "chage: aging information changed for {}",
        quoteaf_os(&username)
    );
    0
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    let path = Path::new(userdb::DEFAULT_PATH);
    process::exit(cmd_chage(&argv, path, current_uid()));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    /// A database holding one account with the given policy, saved, and the
    /// path a command would be given for it.
    fn scratch_with(scratch: &ScratchDir, aging: &Aging) -> std::path::PathBuf {
        let path = scratch.path("users.yaml");
        let mut record = userdb::Record::new();
        record.set(userdb::field::USERNAME, "alice");
        record.set_uid(1000);
        record.set_aging(aging);
        let mut db = UserDb::new();
        db.push(record);
        db.save(&path).expect("save");
        path
    }

    fn aging_at(path: &Path) -> Aging {
        load(path)
            .expect("load")
            .find("alice")
            .expect("alice")
            .aging()
    }

    fn run(args: &[&str], path: &Path) -> i32 {
        let argv: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        cmd_chage(&argv, path, 0)
    }

    // ---- The bug this rewrite exists for ----

    /// A change reaches the file. The version this replaces printed the line
    /// it would have written to stdout, said "updated aging for alice", and
    /// wrote nothing: the policy was gone when the process exited.
    #[test]
    fn a_change_is_written_and_is_still_there_afterwards() {
        let scratch = ScratchDir::new("chage-writes");
        let path = scratch_with(&scratch, &Aging::default());

        assert_eq!(run(&["-M", "90", "alice"], &path), 0);

        assert_eq!(aging_at(&path).max_days, Some(90));
    }

    /// ...and it reaches the generated `/etc/shadow` too, in the column the
    /// format defines. Editing the database is only half of the job; the file
    /// the rest of the system reads has to agree.
    #[test]
    fn a_change_reaches_the_generated_shadow_file() {
        let scratch = ScratchDir::new("chage-shadow");
        let path = scratch_with(&scratch, &Aging::default());

        assert_eq!(run(&["-M", "90", "-W", "7", "alice"], &path), 0);

        let shadow = std::fs::read_to_string(scratch.path(userdb::SHADOW_NAME)).expect("shadow");
        let line = shadow
            .lines()
            .find(|l| l.starts_with("alice:"))
            .expect("a line for alice");
        let fields: Vec<&str> = line.split(':').collect();
        // login:password:lastchg:min:max:warn:inactive:expire:
        assert_eq!(fields.get(4), Some(&"90"), "{line}");
        assert_eq!(fields.get(5), Some(&"7"), "{line}");
        assert_eq!(fields.get(3), Some(&""), "an untouched field stays empty");
    }

    /// Setting one field leaves the other five as they were.
    #[test]
    fn setting_one_field_leaves_the_others_alone() {
        let scratch = ScratchDir::new("chage-one-field");
        let path = scratch_with(
            &scratch,
            &Aging {
                min_days: Some(1),
                max_days: Some(60),
                warn_days: Some(7),
                inactive_days: Some(14),
                expires: Some(20000),
                changed: Some(19000),
            },
        );

        assert_eq!(run(&["-M", "90", "alice"], &path), 0);

        let aging = aging_at(&path);
        assert_eq!(aging.max_days, Some(90));
        assert_eq!(aging.min_days, Some(1));
        assert_eq!(aging.warn_days, Some(7));
        assert_eq!(aging.inactive_days, Some(14));
        assert_eq!(aging.expires, Some(20000));
        assert_eq!(aging.changed, Some(19000));
    }

    /// Several options in one run all land.
    #[test]
    fn several_options_in_one_run_all_land() {
        let scratch = ScratchDir::new("chage-several");
        let path = scratch_with(&scratch, &Aging::default());

        assert_eq!(
            run(
                &["-m", "1", "-M", "90", "-W", "7", "-I", "14", "alice"],
                &path
            ),
            0
        );

        let aging = aging_at(&path);
        assert_eq!(aging.min_days, Some(1));
        assert_eq!(aging.max_days, Some(90));
        assert_eq!(aging.warn_days, Some(7));
        assert_eq!(aging.inactive_days, Some(14));
    }

    /// `-1` clears a field rather than storing minus one day. A literal `-1`
    /// in the file is a date one day before the epoch, so writing it through
    /// would turn "never expires" into "expired since 1969".
    #[test]
    fn minus_one_clears_a_field() {
        let scratch = ScratchDir::new("chage-clear");
        let path = scratch_with(
            &scratch,
            &Aging {
                max_days: Some(90),
                expires: Some(20000),
                ..Aging::default()
            },
        );

        assert_eq!(run(&["-M", "-1", "-E", "-1", "alice"], &path), 0);

        let aging = aging_at(&path);
        assert_eq!(aging.max_days, None);
        assert_eq!(aging.expires, None);

        let shadow = std::fs::read_to_string(scratch.path(userdb::SHADOW_NAME)).expect("shadow");
        let line = shadow
            .lines()
            .find(|l| l.starts_with("alice:"))
            .expect("a line for alice");
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields.get(4), Some(&""), "{line}");
        assert_eq!(fields.get(7), Some(&""), "{line}");
    }

    /// `-E` takes a date and stores a day number. The column holds days; a
    /// date copied into it verbatim is read as no expiry at all.
    #[test]
    fn an_expiry_date_is_stored_as_the_day_number_the_column_holds() {
        let scratch = ScratchDir::new("chage-expiry-date");
        let path = scratch_with(&scratch, &Aging::default());

        assert_eq!(run(&["-E", "2024-01-01", "alice"], &path), 0);

        assert_eq!(aging_at(&path).expires, Some(19723));
    }

    /// A date that is not a date changes nothing at all -- not even the fields
    /// named by the options that parsed.
    #[test]
    fn a_bad_value_leaves_the_whole_policy_untouched() {
        let scratch = ScratchDir::new("chage-bad-value");
        let path = scratch_with(
            &scratch,
            &Aging {
                max_days: Some(60),
                ..Aging::default()
            },
        );

        assert_eq!(run(&["-M", "90", "-E", "next tuesday", "alice"], &path), 1);

        assert_eq!(
            aging_at(&path).max_days,
            Some(60),
            "the -M that parsed must not have been applied either"
        );
    }

    #[test]
    fn an_unknown_user_is_refused() {
        let scratch = ScratchDir::new("chage-unknown");
        let path = scratch_with(&scratch, &Aging::default());
        assert_eq!(run(&["-M", "90", "bob"], &path), 1);
    }

    #[test]
    fn a_missing_username_is_refused() {
        let scratch = ScratchDir::new("chage-nouser");
        let path = scratch_with(&scratch, &Aging::default());
        assert_eq!(run(&["-M", "90"], &path), 1);
    }

    /// A database that cannot be read is reported, not replaced with invented
    /// accounts. The version this replaces answered `chage -l root` from three
    /// fabricated entries -- complete with made-up password hashes -- whenever
    /// `/etc/shadow` was missing.
    #[test]
    fn an_unreadable_database_is_not_three_invented_accounts() {
        let scratch = ScratchDir::new("chage-missing");
        // A directory where the file should be: `UserDb::load` treats *absent*
        // as empty, by design, so absence alone cannot be used to test this.
        let path = scratch.path("users.yaml");
        std::fs::create_dir_all(&path).expect("mkdir");

        assert_eq!(run(&["-l", "root"], &path), 1);
    }

    /// An account that is simply not there is "does not exist", even when the
    /// database is readable and empty -- never a default policy.
    #[test]
    fn an_empty_database_has_no_root_account_to_report_on() {
        let scratch = ScratchDir::new("chage-empty");
        let path = scratch.path("users.yaml");
        UserDb::new().save(&path).expect("save");

        assert_eq!(run(&["-l", "root"], &path), 1);
    }

    // ---- Values on the command line ----

    #[test]
    fn a_day_count_is_a_number_or_a_clear() {
        assert_eq!(parse_days("0"), Ok(Some(0)));
        assert_eq!(parse_days("90"), Ok(Some(90)));
        assert_eq!(parse_days(" 90 "), Ok(Some(90)));
        assert_eq!(parse_days("-1"), Ok(None));
        assert!(parse_days("").is_err());
        assert!(parse_days("ninety").is_err());
    }

    #[test]
    fn a_date_is_a_date_a_day_count_or_a_clear() {
        assert_eq!(parse_date("2024-01-01"), Ok(Some(19723)));
        assert_eq!(parse_date("19723"), Ok(Some(19723)));
        assert_eq!(parse_date("-1"), Ok(None));
        assert_eq!(parse_date(""), Ok(None));
        assert_eq!(parse_date("never"), Ok(None));
        assert!(parse_date("2024-13-01").is_err());
        assert!(parse_date("next tuesday").is_err());
    }

    // ---- What `-l` prints ----

    fn listing(aging: &Aging) -> String {
        let mut out = Vec::new();
        display_aging(&mut out, aging);
        String::from_utf8(out).expect("ascii")
    }

    /// A policy nobody set prints as `never` and `-1`, never as the
    /// `0 99999 7` the old code invented. A `0` in the maximum column is not
    /// "no expiry policy", it is "expired the day it was set".
    #[test]
    fn an_unset_policy_prints_as_unset() {
        let text = listing(&Aging::default());
        assert!(
            text.contains("Last password change\t\t\t\t\t: never"),
            "{text}"
        );
        assert!(text.contains("Password expires\t\t\t\t\t: never"), "{text}");
        assert!(
            text.contains("Password inactive\t\t\t\t\t: never"),
            "{text}"
        );
        assert!(
            text.contains("Account expires\t\t\t\t\t\t: never"),
            "{text}"
        );
        assert!(text.contains(": -1\n"), "{text}");
        assert!(!text.contains("99999"), "{text}");
    }

    /// The expiry date is the change date plus the maximum age -- and is
    /// `never` unless *both* are known. The old code supplied `99999` for an
    /// absent maximum and `0` for an absent change date, then printed the sum.
    #[test]
    fn the_expiry_date_needs_both_halves_to_be_a_date() {
        let text = listing(&Aging {
            changed: Some(19723),
            max_days: Some(90),
            ..Aging::default()
        });
        assert!(
            text.contains("Password expires\t\t\t\t\t: 2024-03-31"),
            "{text}"
        );

        let half = listing(&Aging {
            changed: Some(19723),
            ..Aging::default()
        });
        assert!(half.contains("Password expires\t\t\t\t\t: never"), "{half}");
    }

    /// The inactive date is the expiry plus the inactive period.
    #[test]
    fn the_inactive_date_is_the_expiry_plus_the_inactive_period() {
        let text = listing(&Aging {
            changed: Some(19723),
            max_days: Some(90),
            inactive_days: Some(10),
            ..Aging::default()
        });
        assert!(
            text.contains("Password inactive\t\t\t\t\t: 2024-04-10"),
            "{text}"
        );
    }

    /// A password dated to the epoch is `/etc/shadow`'s "must be changed at
    /// the next login", which is what `passwd -e` writes. Printing it as
    /// 1970-01-01 would be true and useless.
    #[test]
    fn an_expired_password_says_so_rather_than_printing_the_epoch() {
        let text = listing(&Aging {
            changed: Some(0),
            ..Aging::default()
        });
        assert!(
            text.contains("Last password change\t\t\t\t\t: password must be changed"),
            "{text}"
        );
    }

    // ---- Arguments ----

    #[test]
    fn an_option_missing_its_value_is_refused() {
        assert!(parse_args(&["-M".to_string()]).is_err());
        assert!(parse_args(&["-E".to_string()]).is_err());
    }

    #[test]
    fn an_unknown_option_is_refused() {
        assert!(parse_args(&["-Z".to_string(), "alice".to_string()]).is_err());
    }

    #[test]
    fn a_second_username_is_refused() {
        let argv = ["alice".to_string(), "bob".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    /// `-l` reads and the other options write; asking for both in one run is a
    /// contradiction rather than a sequence.
    #[test]
    fn listing_and_changing_in_one_run_is_refused() {
        let scratch = ScratchDir::new("chage-list-and-set");
        let path = scratch_with(&scratch, &Aging::default());
        assert_eq!(run(&["-l", "-M", "90", "alice"], &path), 1);
        assert_eq!(aging_at(&path).max_days, None);
    }

    // ---- Who may ask ----

    /// Only root may change a policy.
    #[test]
    fn an_ordinary_user_may_not_change_a_policy() {
        let scratch = ScratchDir::new("chage-perm");
        let path = scratch_with(&scratch, &Aging::default());
        let argv = ["-M".to_string(), "90".to_string(), "alice".to_string()];

        assert_eq!(cmd_chage(&argv, &path, 1000), 1);
        assert_eq!(aging_at(&path).max_days, None);
    }

    /// ...nor read someone else's. `USER` is not set in the test process, so
    /// no account is the caller's own here.
    #[test]
    fn an_ordinary_user_may_not_read_another_accounts_policy() {
        let scratch = ScratchDir::new("chage-perm-list");
        let path = scratch_with(&scratch, &Aging::default());
        let argv = ["-l".to_string(), "alice".to_string()];

        assert_eq!(cmd_chage(&argv, &path, 1000), 1);
    }
}
