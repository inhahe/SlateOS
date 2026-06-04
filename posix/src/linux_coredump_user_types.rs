//! `fs/coredump.c` — core dump driver constants (core_pattern format
//! specifiers, helper hand-off, max recursion).
//!
//! When a process crashes, the kernel parses /proc/sys/kernel/core_pattern
//! to decide where the dump goes: a literal filename, a pipe to a helper
//! program (`|/path/to/helper %P %e ...`), or a special directory.

// ---------------------------------------------------------------------------
// core_pattern leading character
// ---------------------------------------------------------------------------

/// Pattern starting with '|' pipes the dump to a helper program.
pub const COREDUMP_PIPE_PREFIX: char = '|';

// ---------------------------------------------------------------------------
// core_pattern format specifiers (man 5 core)
// ---------------------------------------------------------------------------

/// %% — literal percent sign.
pub const COREDUMP_FMT_PERCENT: char = '%';
/// %c — RLIMIT_CORE soft limit.
pub const COREDUMP_FMT_RLIMIT: char = 'c';
/// %d — dumpable bit.
pub const COREDUMP_FMT_DUMPABLE: char = 'd';
/// %e — executable filename without path (16 chars).
pub const COREDUMP_FMT_EXECNAME: char = 'e';
/// %E — executable pathname with slashes replaced by '!'.
pub const COREDUMP_FMT_PATHNAME: char = 'E';
/// %g — numeric real GID.
pub const COREDUMP_FMT_GID: char = 'g';
/// %h — hostname (utsname.nodename).
pub const COREDUMP_FMT_HOSTNAME: char = 'h';
/// %i — TID in PID-namespace of the dumped thread.
pub const COREDUMP_FMT_TID_NS: char = 'i';
/// %I — TID in initial PID-namespace.
pub const COREDUMP_FMT_TID_INIT: char = 'I';
/// %p — PID in PID-namespace of the dumped thread.
pub const COREDUMP_FMT_PID_NS: char = 'p';
/// %P — PID in initial PID-namespace.
pub const COREDUMP_FMT_PID_INIT: char = 'P';
/// %s — signal number that caused the dump.
pub const COREDUMP_FMT_SIGNAL: char = 's';
/// %t — seconds since Unix epoch.
pub const COREDUMP_FMT_TIME: char = 't';
/// %u — numeric real UID.
pub const COREDUMP_FMT_UID: char = 'u';

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum length of core_pattern.
pub const CORENAME_MAX_SIZE: usize = 128;
/// Maximum executable name printed by %e (TASK_COMM_LEN-1).
pub const TASK_COMM_LEN_VISIBLE: usize = 15;

// ---------------------------------------------------------------------------
// Default core_pattern values
// ---------------------------------------------------------------------------

pub const COREDUMP_PATTERN_DEFAULT: &str = "core";
/// systemd-coredump installs this pipe pattern.
pub const COREDUMP_PATTERN_SYSTEMD: &str =
    "|/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h";

// ---------------------------------------------------------------------------
// /proc tunables
// ---------------------------------------------------------------------------

pub const PROC_SYS_CORE_PATTERN: &str = "/proc/sys/kernel/core_pattern";
pub const PROC_SYS_CORE_PIPE_LIMIT: &str = "/proc/sys/kernel/core_pipe_limit";
pub const PROC_SYS_CORE_USES_PID: &str = "/proc/sys/kernel/core_uses_pid";

/// Default core_pipe_limit — 0 means unlimited concurrent helpers.
pub const CORE_PIPE_LIMIT_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_prefix_is_pipe_char() {
        assert_eq!(COREDUMP_PIPE_PREFIX, '|');
    }

    #[test]
    fn test_fmt_specifiers_distinct_letters() {
        let f = [
            COREDUMP_FMT_PERCENT,
            COREDUMP_FMT_RLIMIT,
            COREDUMP_FMT_DUMPABLE,
            COREDUMP_FMT_EXECNAME,
            COREDUMP_FMT_PATHNAME,
            COREDUMP_FMT_GID,
            COREDUMP_FMT_HOSTNAME,
            COREDUMP_FMT_TID_NS,
            COREDUMP_FMT_TID_INIT,
            COREDUMP_FMT_PID_NS,
            COREDUMP_FMT_PID_INIT,
            COREDUMP_FMT_SIGNAL,
            COREDUMP_FMT_TIME,
            COREDUMP_FMT_UID,
        ];
        for (i, &c) in f.iter().enumerate() {
            for &d in &f[i + 1..] {
                assert_ne!(c, d);
            }
        }
    }

    #[test]
    fn test_pid_specifiers_case_pair() {
        // Lowercase = namespace-relative; uppercase = init namespace.
        assert_eq!(COREDUMP_FMT_PID_NS.to_ascii_uppercase(), COREDUMP_FMT_PID_INIT);
        assert_eq!(COREDUMP_FMT_TID_NS.to_ascii_uppercase(), COREDUMP_FMT_TID_INIT);
    }

    #[test]
    fn test_corename_max_size_128() {
        assert_eq!(CORENAME_MAX_SIZE, 128);
        assert!(CORENAME_MAX_SIZE.is_power_of_two());
    }

    #[test]
    fn test_task_comm_len_visible_is_15() {
        assert_eq!(TASK_COMM_LEN_VISIBLE, 15);
    }

    #[test]
    fn test_default_pattern_is_literal_core() {
        assert_eq!(COREDUMP_PATTERN_DEFAULT, "core");
        // systemd pattern starts with the pipe character.
        assert!(COREDUMP_PATTERN_SYSTEMD.starts_with('|'));
    }

    #[test]
    fn test_proc_paths_under_kernel() {
        for p in [
            PROC_SYS_CORE_PATTERN,
            PROC_SYS_CORE_PIPE_LIMIT,
            PROC_SYS_CORE_USES_PID,
        ] {
            assert!(p.starts_with("/proc/sys/kernel/"));
        }
    }

    #[test]
    fn test_pipe_limit_default_is_zero() {
        assert_eq!(CORE_PIPE_LIMIT_DEFAULT, 0);
    }
}
