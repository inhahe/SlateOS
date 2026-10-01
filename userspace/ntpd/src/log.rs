//! Where the daemon's own messages go, as ntpd sends them: to the file a
//! `logfile` directive names, or else to syslog(3) as `ntpd` (`LOG_PID`,
//! `LOG_DAEMON`) -- and under `-d` to stderr as well.
//!
//! The messages are the ones an administrator needs without asking for them:
//! that the daemon started and with which servers, that it lost or regained
//! its servers, that it stepped the clock, and that it could not set the
//! clock or save its drift. A failure that repeats every poll is reported
//! when it starts and when it clears ([`Trouble`]), not once a poll.

use libcsyslog::{LOG_DAEMON, LOG_ERR, LOG_NOTICE, LOG_PID, LOG_WARNING};
use localtime::Zone;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The daemon's log.
pub struct Log {
    /// The `logfile` directive's file, if there is one.
    file: Option<PathBuf>,
    /// `-d`: every message to stderr too.
    debug: bool,
    /// Local time, for the file's timestamps; resolved once, as glibc's
    /// `localtime` reads `TZ` once.
    zone: Zone,
}

impl Log {
    /// Open the log. syslog(3) is opened even when there is a file: a message
    /// the file cannot take goes there instead, with the reason.
    #[must_use]
    pub fn open(file: Option<PathBuf>, debug: bool) -> Self {
        libcsyslog::openlog(b"ntpd", LOG_PID, LOG_DAEMON);
        Log {
            file,
            debug,
            zone: Zone::from_env(),
        }
    }

    /// Something an administrator would want to know: start, sync, a step.
    pub fn notice(&self, msg: &str) {
        self.write(LOG_NOTICE, msg);
    }

    /// Something wrong that the daemon rides out: no server reachable.
    pub fn warning(&self, msg: &str) {
        self.write(LOG_WARNING, msg);
    }

    /// Something the daemon failed to do: set the clock, save the drift.
    pub fn err(&self, msg: &str) {
        self.write(LOG_ERR, msg);
    }

    fn write(&self, severity: i32, msg: &str) {
        if self.debug {
            eprintln!("ntpd: {msg}");
        }
        let Some(path) = &self.file else {
            libcsyslog::syslog(severity, msg.as_bytes());
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let line = logfile_line(&self.zone, now, std::process::id(), msg);
        if let Err(e) = append(path, &line) {
            let why = format!("cannot write {}: {e}", quoting::quotef_os(path));
            libcsyslog::syslog(LOG_ERR, why.as_bytes());
            libcsyslog::syslog(severity, msg.as_bytes());
        }
    }
}

/// One line of a `logfile`, as ntpd writes it (`humanlogtime` and
/// `addto_syslog`): `26 Sep 11:00:00 ntpd[1234]: MSG`, in local time, the day
/// space-padded.
#[must_use]
pub fn logfile_line(zone: &Zone, t: i64, pid: u32, msg: &str) -> Vec<u8> {
    let tm = zone.localtime(t, 0);
    let mut line = localtime::strftime(b"%e %b %H:%M:%S", &tm);
    line.extend_from_slice(format!(" ntpd[{pid}]: {msg}\n").as_bytes());
    line
}

/// Append `line` to `path` in one write, creating the file if need be, so
/// that a rotated-away file is simply started again.
///
/// # Errors
///
/// The file cannot be opened or written.
pub fn append(path: &Path, line: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line)
}

/// A failure that may repeat every poll -- setting the clock, saving the
/// drift -- reported when it starts and when it clears, not every time.
#[derive(Debug, Default)]
pub struct Trouble {
    failing: bool,
}

impl Trouble {
    /// What to log after an attempt to `what` ("set the clock"): `Some` on
    /// the first failure of a run of them and on the first success after
    /// one, `None` otherwise. The bool is whether it is an error.
    pub fn after(&mut self, what: &str, result: Result<(), String>) -> Option<(bool, String)> {
        match result {
            Err(e) if !self.failing => {
                self.failing = true;
                Some((true, format!("cannot {what}: {e}")))
            }
            Ok(()) if self.failing => {
                self.failing = false;
                Some((false, format!("can {what} again")))
            }
            _ => None,
        }
    }

    /// [`Trouble::after`], logged.
    pub fn report(&mut self, log: &Log, what: &str, result: Result<(), String>) {
        match self.after(what, result) {
            Some((true, msg)) => log.err(&msg),
            Some((false, msg)) => log.notice(&msg),
            None => {}
        }
    }
}

/// Whether the servers answer, reported on each change.
#[derive(Debug, Default)]
pub struct Reach {
    /// `None` before the first poll.
    reachable: Option<bool>,
}

impl Reach {
    /// What to log after a poll that did (`Some(server, stratum)`) or did not
    /// (`None`) produce a usable sample: `Some` only when that changes what
    /// the last poll said -- including the very first poll.
    pub fn after(
        &mut self,
        synced: Option<(&str, u8)>,
        servers: &[String],
    ) -> Option<(bool, String)> {
        let now = synced.is_some();
        if self.reachable == Some(now) {
            return None;
        }
        self.reachable = Some(now);
        Some(match synced {
            Some((server, stratum)) => (
                false,
                format!("synchronized to {server}, stratum {stratum}"),
            ),
            None => (
                true,
                format!("no server reachable ({})", servers.join(", ")),
            ),
        })
    }

    /// [`Reach::after`], logged: a sync as a notice, a loss as a warning.
    pub fn report(&mut self, log: &Log, synced: Option<(&str, u8)>, servers: &[String]) {
        match self.after(synced, servers) {
            Some((true, msg)) => log.warning(&msg),
            Some((false, msg)) => log.notice(&msg),
            None => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn utc() -> Zone {
        Zone::utc()
    }

    #[test]
    fn a_logfile_line_is_ntpds() {
        // 2026-09-26T06:37:16Z.
        assert_eq!(
            logfile_line(&utc(), 1_790_404_636, 42, "ntpd starting"),
            b"26 Sep 06:37:16 ntpd[42]: ntpd starting\n"
        );
        // The day is space-padded: 2026-09-06T06:37:16Z.
        assert_eq!(
            logfile_line(&utc(), 1_788_676_636, 7, "x"),
            b" 6 Sep 06:37:16 ntpd[7]: x\n"
        );
    }

    #[test]
    fn a_failure_is_reported_when_it_starts_and_when_it_clears() {
        let mut t = Trouble::default();
        assert_eq!(t.after("set the clock", Ok(())), None);
        assert_eq!(
            t.after("set the clock", Err("Operation not permitted".into())),
            Some((true, "cannot set the clock: Operation not permitted".into()))
        );
        assert_eq!(t.after("set the clock", Err("again".into())), None);
        assert_eq!(
            t.after("set the clock", Ok(())),
            Some((false, "can set the clock again".into()))
        );
        assert_eq!(t.after("set the clock", Ok(())), None);
    }

    #[test]
    fn reachability_is_reported_on_each_change_and_the_first_poll() {
        let servers = vec!["a".to_string(), "b".to_string()];
        let mut r = Reach::default();
        assert_eq!(
            r.after(None, &servers),
            Some((true, "no server reachable (a, b)".into()))
        );
        assert_eq!(r.after(None, &servers), None);
        assert_eq!(
            r.after(Some(("b", 2)), &servers),
            Some((false, "synchronized to b, stratum 2".into()))
        );
        assert_eq!(r.after(Some(("a", 1)), &servers), None);
        let mut first_ok = Reach::default();
        assert_eq!(
            first_ok.after(Some(("a", 1)), &servers),
            Some((false, "synchronized to a, stratum 1".into()))
        );
    }

    #[test]
    fn a_logfile_is_appended_to_and_created() {
        let dir = scratchdir::ScratchDir::new("ntpd_log_append");
        let path = dir.path("ntp.log");
        append(&path, b"one\n").unwrap();
        append(&path, b"two\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\n");
        assert!(append(&dir.path("no/such/dir/ntp.log"), b"x\n").is_err());
    }
}
