//! Where cron jobs live on disk — spelled once, for every program that agrees
//! or must.
//!
//! # Why this crate exists
//!
//! `crontab -e` wrote `/var/spool/cron/alice`. Both daemons read
//! `/var/spool/cron/crontabs/alice`. **The job never ran**, and nothing
//! reported it: `crontab -l` reads back the same file it wrote, so the crontab
//! listed correctly and simply did not fire. There is no error path for a file
//! written where nobody looks.
//!
//! Five paths were spelled in four crates, under a different name in nearly
//! every one:
//!
//! | Path | `cron` | `crond` | `crontab` | `at` |
//! |---|---|---|---|---|
//! | user crontabs | `CRONTAB_SPOOL_DIR` | `USER_CRONTAB_DIR` | `SPOOL_DIR` ✗ | — |
//! | system crontab | `SYSTEM_CRONTAB` | `SYSTEM_CRONTAB` | — | — |
//! | anacrontab | `ANACRONTAB` | `ANACRONTAB_PATH` | — | — |
//! | anacron spool | `ANACRON_SPOOL_DIR` | `ANACRON_SPOOL` | — | — |
//! | at spool | `AT_SPOOL_DIR` | — | — | `SPOOL_DIR` |
//!
//! The ✗ is the one that disagreed. Four spellings of one path across four
//! crates is an enumeration that needs one entry per instance, and the next
//! instance is wrong by construction — which is exactly what happened.
//!
//! # Why a shared constant rather than a gate
//!
//! A checker could compare the literals and refuse when they drift. A shared
//! constant makes the drift unrepresentable, which is better than detectable:
//! there is no state in which the daemons and the editor can disagree, so
//! there is nothing for a gate to catch and no window in which a job silently
//! does not run. The same reasoning put the utmp record layout in `utmpfile`
//! and the chmod grammar in `modechange`.
//!
//! # These are absolute paths on the target
//!
//! They are `&str` rather than `Path` because they are joined with a username
//! or a job id at the point of use, and every caller already does that.

/// Per-user crontab files, one per account, named for the account.
///
/// **The daemons decide this path**, because they are what makes a job run; an
/// editor writing anywhere else produces a file nobody reads. `userspace/cron`
/// and `userspace/crond` both already agreed on this value, and
/// `userspace/crontab` was the one that did not.
pub const USER_CRONTABS: &str = "/var/spool/cron/crontabs";

/// The system-wide crontab, whose lines carry an extra user field.
pub const SYSTEM_CRONTAB: &str = "/etc/crontab";

/// Drop-in directory for packages that ship cron jobs.
pub const SYSTEM_CRON_D: &str = "/etc/cron.d";

/// anacron's job table, for jobs measured in days rather than clock times.
pub const ANACRONTAB: &str = "/etc/anacrontab";

/// Where anacron records when each job last ran.
pub const ANACRON_SPOOL: &str = "/var/spool/anacron";

/// Queued `at` jobs, one file per job id.
pub const AT_SPOOL: &str = "/var/spool/at";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_spool_is_under_the_cron_spool_but_is_not_it() {
        // The bug was writing to the parent. Both are plausible spellings --
        // Debian uses `crontabs`, Red Hat does not -- so the test pins which
        // one this system chose rather than asserting a truth about cron.
        assert_eq!(USER_CRONTABS, "/var/spool/cron/crontabs");
        assert!(USER_CRONTABS.starts_with("/var/spool/cron"));
        assert_ne!(USER_CRONTABS, "/var/spool/cron");
    }

    #[test]
    fn every_path_is_absolute() {
        // A relative path here would resolve against whatever directory the
        // daemon happened to be started in, which differs between the boot
        // path and a shell.
        for p in [
            USER_CRONTABS,
            SYSTEM_CRONTAB,
            SYSTEM_CRON_D,
            ANACRONTAB,
            ANACRON_SPOOL,
            AT_SPOOL,
        ] {
            assert!(p.starts_with('/'), "{p} is not absolute");
            assert!(!p.ends_with('/'), "{p} has a trailing slash to join onto");
        }
    }

    #[test]
    fn the_spools_are_distinct() {
        let all = [
            USER_CRONTABS,
            SYSTEM_CRONTAB,
            SYSTEM_CRON_D,
            ANACRONTAB,
            ANACRON_SPOOL,
            AT_SPOOL,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "two names for one path defeats the point");
            }
        }
    }
}
