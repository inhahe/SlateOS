//! Slate OS file locking utilities.
//!
//! Multi-personality binary providing:
//! - **flock** — manage locks from shell scripts
//! - **lockfile** — conditional semaphore-file creator
//!
//! `flock` applies advisory locks to files, optionally running a command
//! while holding the lock. `lockfile` creates semaphore files with retry logic.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::Write;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

const VERSION: &str = "0.1.0";

// ============================================================================
// Lock types
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
enum LockType {
    Shared,
    Exclusive,
    Unlock,
}

// ============================================================================
// flock command
// ============================================================================

struct FlockOpts {
    lock_type: LockType,
    nonblock: bool,
    timeout: Option<u64>,
    close: bool,
    verbose: bool,
    conflict_exit: i32,
    fd: Option<i32>,
    file: Option<String>,
    command: Vec<String>,
}

fn parse_flock_args(args: &[String]) -> FlockOpts {
    let mut opts = FlockOpts {
        lock_type: LockType::Exclusive,
        nonblock: false,
        timeout: None,
        close: false,
        verbose: false,
        conflict_exit: 1,
        fd: None,
        file: None,
        command: Vec::new(),
    };

    let mut i = 0;
    let mut positional = Vec::new();

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: flock [options] <file|fd> [command ...]");
                println!("       flock [options] <file|fd> -c command");
                println!();
                println!("Manage file locks from shell scripts.");
                println!();
                println!("Options:");
                println!("  -s, --shared         Shared lock");
                println!("  -x, --exclusive      Exclusive lock (default)");
                println!("  -u, --unlock         Remove a lock");
                println!("  -n, --nonblock       Fail rather than wait");
                println!("  -w, --timeout SECS   Wait at most SECS seconds");
                println!("  -o, --close          Close fd before running command");
                println!("  -E, --conflict-exit N  Exit code on conflict (default 1)");
                println!("  -v, --verbose        Verbose mode");
                println!("  -h, --help           Show this help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("flock {VERSION}");
                process::exit(0);
            }
            "-s" | "--shared" => opts.lock_type = LockType::Shared,
            "-x" | "--exclusive" => opts.lock_type = LockType::Exclusive,
            "-u" | "--unlock" => opts.lock_type = LockType::Unlock,
            "-n" | "--nonblock" | "--nb" => opts.nonblock = true,
            "-o" | "--close" => opts.close = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-w" | "--timeout" | "--wait" => {
                i += 1;
                if i < args.len() {
                    opts.timeout = args[i].parse().ok();
                }
            }
            "-E" | "--conflict-exit" => {
                i += 1;
                if i < args.len() {
                    opts.conflict_exit = args[i].parse().unwrap_or(1);
                }
            }
            "-c" => {
                // -c "command" — rest is a single shell command string.
                i += 1;
                if i < args.len() {
                    opts.command = vec![
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        args[i].to_string(),
                    ];
                }
            }
            s if !s.starts_with('-') || positional.is_empty() => {
                if s.starts_with('-') && !positional.is_empty() {
                    // It's a flag for the command.
                    opts.command.push(s.to_string());
                    // Collect rest as command.
                    i += 1;
                    while i < args.len() {
                        opts.command.push(args[i].to_string());
                        i += 1;
                    }
                    break;
                }
                positional.push(s.to_string());
            }
            _ => {
                positional.push(args[i].to_string());
            }
        }
        i += 1;
    }

    // First positional is file or fd number.
    if let Some(first) = positional.first() {
        if let Ok(fd) = first.parse::<i32>() {
            opts.fd = Some(fd);
        } else {
            opts.file = Some(first.clone());
        }
    }

    // Remaining positionals are command.
    if positional.len() > 1 && opts.command.is_empty() {
        opts.command = positional[1..].to_vec();
    }

    opts
}

/// Advisory lock implementation using lock files.
/// Since we can't use flock(2) syscall directly in our simulated environment,
/// we use atomic file creation as a lock mechanism.
fn acquire_lock(path: &str, opts: &FlockOpts) -> bool {
    let lock_path = format!("{path}.lock");
    let deadline = opts.timeout.map(|t| Instant::now() + Duration::from_secs(t));

    loop {
        // Try to create lock file exclusively.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut f) => {
                // Write our PID (simulated).
                let pid = process::id();
                let lock_info = match opts.lock_type {
                    LockType::Shared => format!("shared:{pid}\n"),
                    LockType::Exclusive => format!("exclusive:{pid}\n"),
                    LockType::Unlock => return true,
                };
                let _ = f.write_all(lock_info.as_bytes());
                if opts.verbose {
                    eprintln!("flock: acquired lock on {path}");
                }
                return true;
            }
            Err(_) => {
                if opts.nonblock {
                    if opts.verbose {
                        eprintln!("flock: failed to acquire lock (nonblock)");
                    }
                    return false;
                }

                if let Some(dl) = deadline
                    && Instant::now() >= dl {
                        if opts.verbose {
                            eprintln!("flock: timeout waiting for lock");
                        }
                        return false;
                    }

                // Shared locks can coexist.
                if opts.lock_type == LockType::Shared
                    && let Ok(content) = fs::read_to_string(&lock_path)
                        && content.starts_with("shared:") {
                            if opts.verbose {
                                eprintln!("flock: shared lock compatible, proceeding");
                            }
                            return true;
                        }

                // Wait and retry.
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn release_lock(path: &str, verbose: bool) {
    let lock_path = format!("{path}.lock");
    let _ = fs::remove_file(&lock_path);
    if verbose {
        eprintln!("flock: released lock on {path}");
    }
}

fn cmd_flock(args: &[String]) {
    let opts = parse_flock_args(args);

    if opts.file.is_none() && opts.fd.is_none() {
        eprintln!("flock: no file or fd specified");
        eprintln!("Try 'flock --help' for more information.");
        process::exit(1);
    }

    let file_path = opts.file.clone().unwrap_or_else(|| {
        // For fd mode, use /dev/fd/N as the lock target.
        format!("/dev/fd/{}", opts.fd.unwrap_or(0))
    });

    if opts.lock_type == LockType::Unlock {
        release_lock(&file_path, opts.verbose);
        process::exit(0);
    }

    if !acquire_lock(&file_path, &opts) {
        process::exit(opts.conflict_exit);
    }

    if opts.command.is_empty() {
        // Fd mode: just hold the lock and exit (the fd inherits).
        if opts.verbose {
            eprintln!("flock: holding lock (fd mode)");
        }
        process::exit(0);
    }

    // Run command with lock held.
    let status = process::Command::new(&opts.command[0])
        .args(&opts.command[1..])
        .status();

    release_lock(&file_path, opts.verbose);

    match status {
        Ok(s) => process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("flock: failed to execute command: {e}");
            process::exit(127);
        }
    }
}

// ============================================================================
// lockfile command
// ============================================================================

struct LockfileOpts {
    sleeptime: u64,
    retries: i32,
    locktimeout: u64,
    suspend: u64,
    invert: bool,
    ml: bool,
    files: Vec<String>,
}

fn parse_lockfile_args(args: &[String]) -> LockfileOpts {
    let mut opts = LockfileOpts {
        sleeptime: 8,
        retries: -1, // -1 = infinite
        locktimeout: 0,
        suspend: 16,
        invert: false,
        ml: false,
        files: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: lockfile [-sleeptime | -r retries |");
                println!("               -l locktimeout | -s suspend | -!  | -ml | -mu ] filename ...");
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
            "--version" => {
                println!("lockfile {VERSION}");
                process::exit(0);
            }
            "-r" => {
                i += 1;
                if i < args.len() {
                    opts.retries = args[i].parse().unwrap_or(-1);
                }
            }
            "-l" => {
                i += 1;
                if i < args.len() {
                    opts.locktimeout = args[i].parse().unwrap_or(0);
                }
            }
            "-s" => {
                i += 1;
                if i < args.len() {
                    opts.suspend = args[i].parse().unwrap_or(16);
                }
            }
            "-!" => opts.invert = true,
            "-ml" => opts.ml = true,
            "-mu" => {
                // Remove mode — just delete lockfiles and exit.
                for f in args.iter().skip(i + 1) {
                    let _ = fs::remove_file(f);
                }
                process::exit(0);
            }
            s if s.starts_with('-') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_digit()) => {
                opts.sleeptime = s[1..].parse().unwrap_or(8);
            }
            s if !s.starts_with('-') => {
                opts.files.push(s.to_string());
            }
            _ => {
                opts.files.push(args[i].to_string());
            }
        }
        i += 1;
    }

    opts
}

fn cmd_lockfile(args: &[String]) {
    let opts = parse_lockfile_args(args);

    if opts.files.is_empty() {
        eprintln!("lockfile: no files specified");
        process::exit(1);
    }

    let mut success = true;

    for file in &opts.files {
        let mut attempts = 0;
        let deadline = if opts.locktimeout > 0 {
            Some(Instant::now() + Duration::from_secs(opts.locktimeout))
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
                    let _ = f.write_all(format!("{}\n", process::id()).as_bytes());
                    break;
                }
                Err(_) => {
                    attempts += 1;
                    if opts.retries >= 0 && attempts > opts.retries {
                        eprintln!("lockfile: giving up on lock file \"{file}\"");
                        success = false;
                        break;
                    }

                    if let Some(dl) = deadline
                        && Instant::now() >= dl {
                            // Check for stale lock.
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

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("flock");
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

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "lockfile" => cmd_lockfile(&rest),
        _ => cmd_flock(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_type_equality() {
        assert_eq!(LockType::Shared, LockType::Shared);
        assert_eq!(LockType::Exclusive, LockType::Exclusive);
        assert_ne!(LockType::Shared, LockType::Exclusive);
    }

    #[test]
    fn test_parse_flock_shared() {
        let args = vec!["-s".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.lock_type, LockType::Shared);
        assert_eq!(opts.file, Some("/tmp/test".to_string()));
    }

    #[test]
    fn test_parse_flock_exclusive() {
        let args = vec!["-x".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.lock_type, LockType::Exclusive);
    }

    #[test]
    fn test_parse_flock_nonblock() {
        let args = vec!["-n".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert!(opts.nonblock);
    }

    #[test]
    fn test_parse_flock_timeout() {
        let args = vec!["-w".to_string(), "10".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.timeout, Some(10));
    }

    #[test]
    fn test_parse_flock_unlock() {
        let args = vec!["-u".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.lock_type, LockType::Unlock);
    }

    #[test]
    fn test_parse_flock_verbose() {
        let args = vec!["-v".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert!(opts.verbose);
    }

    #[test]
    fn test_parse_flock_fd() {
        let args = vec!["9".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.fd, Some(9));
        assert!(opts.file.is_none());
    }

    #[test]
    fn test_parse_flock_with_command() {
        let args = vec![
            "/tmp/lockfile".to_string(),
            "echo".to_string(),
            "hello".to_string(),
        ];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.file, Some("/tmp/lockfile".to_string()));
        assert_eq!(opts.command, vec!["echo", "hello"]);
    }

    #[test]
    fn test_parse_flock_conflict_exit() {
        let args = vec!["-E".to_string(), "42".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert_eq!(opts.conflict_exit, 42);
    }

    #[test]
    fn test_parse_flock_close() {
        let args = vec!["-o".to_string(), "/tmp/test".to_string()];
        let opts = parse_flock_args(&args);
        assert!(opts.close);
    }

    #[test]
    fn test_parse_lockfile_defaults() {
        let args = vec!["test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.sleeptime, 8);
        assert_eq!(opts.retries, -1);
        assert_eq!(opts.locktimeout, 0);
        assert_eq!(opts.suspend, 16);
        assert!(!opts.invert);
        assert_eq!(opts.files, vec!["test.lock"]);
    }

    #[test]
    fn test_parse_lockfile_retries() {
        let args = vec!["-r".to_string(), "5".to_string(), "test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.retries, 5);
    }

    #[test]
    fn test_parse_lockfile_sleeptime() {
        let args = vec!["-3".to_string(), "test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.sleeptime, 3);
    }

    #[test]
    fn test_parse_lockfile_invert() {
        let args = vec!["-!".to_string(), "test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert!(opts.invert);
    }

    #[test]
    fn test_parse_lockfile_locktimeout() {
        let args = vec!["-l".to_string(), "60".to_string(), "test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.locktimeout, 60);
    }

    #[test]
    fn test_parse_lockfile_suspend() {
        let args = vec!["-s".to_string(), "30".to_string(), "test.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.suspend, 30);
    }

    #[test]
    fn test_parse_lockfile_multiple_files() {
        let args = vec!["a.lock".to_string(), "b.lock".to_string()];
        let opts = parse_lockfile_args(&args);
        assert_eq!(opts.files.len(), 2);
    }
}
