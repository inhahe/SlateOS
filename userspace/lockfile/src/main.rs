//! lockfile -- create semaphore files, as procmail's `lockfile(1)` does.
//!
//! This was a personality of `userspace/flock`, chosen by argv[0], which
//! nothing could reach: no `lockfile` executable was ever produced
//! (`scripts/multicall-aliases-baseline.txt` listed it). It is its own crate
//! now, so it has a producer and an identity of its own; the code moved
//! unchanged when `flock` became a port of util-linux's.

use quoting::{os_bytes, quotef, quotef_os};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

/// What `-v` reports: this crate's version, as it did as a personality of
/// `flock`.
const VERSION: &str = "0.1.0";

/// Why `lockfile` refused its command line.
///
/// Returned rather than printed so a test can prove the refusal happens at
/// all: an unknown option used to fall through to the operand list, and
/// `lockfile`'s whole job is to create the files it is named.
#[derive(Debug, PartialEq, Eq)]
enum UsageError {
    /// An option this build does not know, as it appeared in argv.
    ///
    /// The whole word is kept rather than a parsed letter, because whether
    /// it renders as `unrecognized option '--list'` or as `invalid option
    /// -- 'Z'` is getopt's rule, not this parser's, and `usageerror` owns
    /// it.
    UnknownOption(Vec<u8>),
}

impl UsageError {
    /// The diagnostic body, without the `flock: ` prefix. The wording is
    /// getopt's, shared with every other program here through `usageerror`
    /// and measured rather than invented: `unrecognized option` for a long
    /// one, `invalid option -- 'c'` for a short one, and they are not
    /// interchangeable.
    fn message(&self) -> String {
        match self {
            Self::UnknownOption(arg) => usageerror::unknown_option(arg),
        }
    }
}

#[derive(Debug)]
struct LockfileOpts {
    sleeptime: u64,
    retries: i32,
    locktimeout: u64,
    suspend: u64,
    invert: bool,
    ml: bool,
    files: Vec<OsString>,
}

/// A number from argv, or `default` for anything that is not one -- this
/// program's behaviour, not procmail's (known-issues
/// TD-B-LOCKFILE-IS-NOT-PROCMAILS).
fn number_or<T: std::str::FromStr>(word: Option<&OsString>, default: T) -> T {
    word.and_then(|w| w.to_str())
        .and_then(|w| w.parse().ok())
        .unwrap_or(default)
}

fn parse_lockfile_args(args: &[OsString]) -> Result<LockfileOpts, UsageError> {
    let mut opts = LockfileOpts {
        sleeptime: 8,
        retries: -1, // -1 = infinite
        locktimeout: 0,
        suspend: 16,
        invert: false,
        ml: false,
        files: Vec::new(),
    };

    let mut i = 0usize;
    while let Some(arg) = args.get(i) {
        // Options are ASCII; a file name is whatever bytes it is.
        let word = os_bytes(arg);
        match &*word {
            b"-h" | b"--help" => {
                println!("Usage: lockfile [-sleeptime | -r retries |");
                println!(
                    "               -l locktimeout | -s suspend | -!  | -ml | -mu ] filename ..."
                );
                println!();
                println!("Create semaphore files.");
                println!();
                println!("Options:");
                println!("  -<N>              Sleep N seconds between retries (default 8)");
                println!("  -r N              Retry N times (-1 = forever, default -1)");
                println!("  -l N              Lock timeout in seconds (0 = no timeout)");
                println!("  -s N              Suspend N seconds after removing stale lock");
                println!("  -!                Invert return value");
                println!("  -ml               Create lock using strstrstrstr of lock (strstr)");
                println!("  -mu               Remove lock");
                println!("  -h, --help        Show this help");
                println!("  --version         Show version");
                process::exit(0);
            }
            b"--version" => {
                println!("lockfile {VERSION}");
                process::exit(0);
            }
            b"-r" => {
                i = i.saturating_add(1);
                if args.get(i).is_some() {
                    opts.retries = number_or(args.get(i), -1);
                }
            }
            b"-l" => {
                i = i.saturating_add(1);
                if args.get(i).is_some() {
                    opts.locktimeout = number_or(args.get(i), 0);
                }
            }
            b"-s" => {
                i = i.saturating_add(1);
                if args.get(i).is_some() {
                    opts.suspend = number_or(args.get(i), 16);
                }
            }
            b"-!" => opts.invert = true,
            b"-ml" => opts.ml = true,
            b"-mu" => {
                // Unlock mode: the remaining words are the lock files to
                // remove. A failure here used to be discarded and the exit
                // status was 0 either way, so a script writing
                // `lockfile -mu "$lock" || recover` never learned that the
                // lock it thought it had dropped was still held.
                let mut failed = 0usize;
                for f in args.iter().skip(i.saturating_add(1)) {
                    match fs::remove_file(f) {
                        Ok(()) => {}
                        // Already absent is the state unlocking wanted.
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            eprintln!("lockfile: cannot remove {}: {e}", quotef(&os_bytes(f)));
                            failed = failed.saturating_add(1);
                        }
                    }
                }
                process::exit(i32::from(failed > 0));
            }
            [b'-', digits @ ..] if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) => {
                opts.sleeptime = std::str::from_utf8(digits)
                    .ok()
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(8);
            }
            // Everything else beginning with a dash is an option this build
            // does not have. It used to fall through to the arm below and
            // become a *file to create*, so `lockfile --typo f` created a
            // file named `--typo` and exited 0 -- reporting success for a
            // command line it had not understood. A lone `-` is a file named
            // `-`, not an option, so the length test lets it through.
            w if w.starts_with(b"-") && w.len() > 1 => {
                return Err(UsageError::UnknownOption(w.to_vec()));
            }
            _ => {
                opts.files.push(arg.clone());
            }
        }
        i = i.saturating_add(1);
    }

    Ok(opts)
}

fn cmd_lockfile(args: &[OsString]) {
    let opts = match parse_lockfile_args(args) {
        Ok(opts) => opts,
        Err(why) => {
            eprintln!("lockfile: {}", why.message());
            eprintln!("Try 'lockfile --help' for more information.");
            process::exit(64);
        }
    };

    if opts.files.is_empty() {
        eprintln!("lockfile: no files specified");
        process::exit(1);
    }

    let mut success = true;

    for file in &opts.files {
        let mut attempts: i32 = 0;
        // A deadline too far off to represent is no deadline at all.
        let deadline = if opts.locktimeout > 0 {
            Instant::now().checked_add(Duration::from_secs(opts.locktimeout))
        } else {
            None
        };

        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)
            {
                Ok(mut f) => {
                    // The PID inside is informational; the file's existence
                    // is the lock, and it exists.
                    let _ = f.write_all(format!("{}\n", process::id()).as_bytes());
                    break;
                }
                Err(_) => {
                    attempts = attempts.saturating_add(1);
                    if opts.retries >= 0 && attempts > opts.retries {
                        eprintln!("lockfile: giving up on lock file {}", quotef_os(file));
                        success = false;
                        break;
                    }

                    if let Some(dl) = deadline
                        && Instant::now() >= dl
                    {
                        // Check for stale lock. A removal that fails leaves
                        // the lock in place, and the next try reports it.
                        let _ = fs::remove_file(file);
                        thread::sleep(Duration::from_secs(opts.suspend));
                        continue;
                    }

                    thread::sleep(Duration::from_secs(opts.sleeptime));
                }
            }
        }
    }

    let exit_code = if success { 0 } else { 1 };
    let exit_code = if opts.invert {
        if exit_code == 0 { 1 } else { 0 }
    } else {
        exit_code
    };
    process::exit(exit_code);
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    cmd_lockfile(&args);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Build an argv the way a shell would.
    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// `lockfile`'s whole job is to create the files it is named, so an
    /// unknown option falling through to the operand list is not a parsing
    /// nicety -- it creates a file called `--typo` and exits 0.
    #[test]
    fn lockfile_refuses_an_unknown_long_option() {
        let err = parse_lockfile_args(&argv(&["--typo", "lock"])).unwrap_err();
        assert_eq!(err, UsageError::UnknownOption(b"--typo".to_vec()));
    }

    #[test]
    fn lockfile_refuses_an_unknown_short_option() {
        let err = parse_lockfile_args(&argv(&["-q", "lock"])).unwrap_err();
        assert_eq!(err, UsageError::UnknownOption(b"-q".to_vec()));
    }

    /// The refusal must not swallow `-<N>`, which is how lockfile spells its
    /// sleep interval -- the digit arm has to be tried before the dash arm.
    #[test]
    fn lockfile_still_reads_a_numeric_sleeptime() {
        let opts = parse_lockfile_args(&argv(&["-5", "lock"])).expect("valid");
        assert_eq!(opts.sleeptime, 5);
        assert_eq!(opts.files, argv(&["lock"]));
    }

    /// A lone dash names a file, so it must survive the refusal arms.
    #[test]
    fn lockfile_treats_a_lone_dash_as_a_file() {
        let opts = parse_lockfile_args(&argv(&["-"])).expect("valid");
        assert_eq!(opts.files, argv(&["-"]));
    }

    #[test]
    fn lockfile_keeps_its_real_options_working() {
        let opts = parse_lockfile_args(&argv(&["-r", "3", "-!", "-ml", "a", "b"])).expect("valid");
        assert_eq!(opts.retries, 3);
        assert!(opts.invert);
        assert!(opts.ml);
        assert_eq!(opts.files, argv(&["a", "b"]));
    }

    #[test]
    fn test_parse_lockfile_defaults() {
        let args = argv(&["test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.sleeptime, 8);
        assert_eq!(opts.retries, -1);
        assert_eq!(opts.locktimeout, 0);
        assert_eq!(opts.suspend, 16);
        assert!(!opts.invert);
        assert_eq!(opts.files, argv(&["test.lock"]));
    }

    #[test]
    fn test_parse_lockfile_retries() {
        let args = argv(&["-r", "5", "test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.retries, 5);
    }

    #[test]
    fn test_parse_lockfile_sleeptime() {
        let args = argv(&["-3", "test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.sleeptime, 3);
    }

    #[test]
    fn test_parse_lockfile_invert() {
        let args = argv(&["-!", "test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert!(opts.invert);
    }

    #[test]
    fn test_parse_lockfile_locktimeout() {
        let args = argv(&["-l", "60", "test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.locktimeout, 60);
    }

    #[test]
    fn test_parse_lockfile_suspend() {
        let args = argv(&["-s", "30", "test.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.suspend, 30);
    }

    #[test]
    fn test_parse_lockfile_multiple_files() {
        let args = argv(&["a.lock", "b.lock"]);
        let opts = parse_lockfile_args(&args).expect("valid command line");
        assert_eq!(opts.files.len(), 2);
    }
}
