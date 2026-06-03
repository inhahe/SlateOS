#![deny(clippy::all)]
//! logrotate — personality CLI for logrotate, the canonical Unix tool
//! for rotating, compressing, mailing, and removing system log files.
//!
//! Erik Troan started logrotate at Red Hat in 1996; it became part of
//! Red Hat Linux that year and has shipped in virtually every Linux
//! distribution since. The premise is simple and old enough that it
//! predates structured logging entirely: a daemon writes a log file
//! and never thinks about it. A separate, periodically-run housekeeper
//! (cron, then systemd timer) opens a config file, decides which logs
//! need rotating, mv's the current file aside (or copytruncate's it),
//! optionally compresses old generations, deletes the oldest, and
//! signals the daemon to reopen.
//!
//! The config language is famously terse and stateful — a directive
//! in /etc/logrotate.conf is a global default, every snippet under
//! /etc/logrotate.d/ overrides for one logfile group, and the daemon
//! keeps a state file at /var/lib/logrotate/logrotate.status
//! recording the last rotation timestamp per pattern. The combination
//! makes "rotate weekly, keep 4, compress with gzip" a four-line
//! snippet and "rotate when X megabytes, compress with xz, run a hook
//! to push to S3" only a few more.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — logrotate log-rotation personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         logrotate at a glance: cron-driven log housekeeper");
    println!("    history       Red Hat 1996 -> universal Unix tool -> 3.x today");
    println!("    config        /etc/logrotate.conf + /etc/logrotate.d/ snippets");
    println!("    directives    weekly/daily, rotate N, compress, missingok, ...");
    println!("    methods       Rename-and-reopen vs copytruncate");
    println!("    hooks         prerotate/postrotate/firstaction/lastaction blocks");
    println!("    cli           Command-line flags: -d, -f, -s, -v, --usage");
    println!("    state         /var/lib/logrotate/logrotate.status semantics");
    println!("    examples      Common snippet recipes");
    println!("    pitfalls      Race conditions, state-file confusion, large logs");
    println!("    alternatives  systemd-journald, savelog, rotatelogs, cronolog");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() {
    println!("logrotate 0.1.0 (OurOS personality CLI)");
}

fn run_about() {
    println!("logrotate - rotate, compress, mail, and remove log files.");
    println!("  Origin:    Erik Troan at Red Hat, 1996. Shipped in Red Hat Linux 4.0.");
    println!("  Language:  C. Single statically-linkable binary plus a config parser.");
    println!("  License:   GPL-2.0-or-later.");
    println!("  Repository: github.com/logrotate/logrotate.");
    println!("  Trigger:   not a daemon. Runs on demand, typically from cron.daily");
    println!("             or a systemd timer (logrotate.timer + logrotate.service).");
    println!("  Job:       walk the config, decide which logfile groups have crossed");
    println!("             the rotate threshold (age, size, force), rotate them,");
    println!("             compress/expire old generations, run pre/post hooks.");
    println!("  Footprint: one config tree + one state file + one cron entry. No");
    println!("             port, no socket, no persistent state at runtime.");
}

fn run_history() {
    println!("Project history.");
    println!("  1996:        first release inside Red Hat Linux 4.0.");
    println!("  1999-2003:   feature growth - copytruncate, dateext, sharedscripts.");
    println!("  2008:        development moves from in-tree-at-Red-Hat to a public");
    println!("               Fedora-hosted repo.");
    println!("  2014:        github.com/logrotate/logrotate becomes upstream;");
    println!("               actively maintained by community + Red Hat contributors.");
    println!("  2018:        3.15 - sharedstate, su <user> <group>, more hooks.");
    println!("  2020:        3.18 - ACL and SELinux-context preservation by default.");
    println!("  2022:        3.21 - 'addextension' and improved dateext format support.");
    println!("  2024-2025:   3.22+ - state-file race fixes, partial fixes for");
    println!("               world-writable rotated-file issues.");
}

fn run_config() {
    println!("Configuration.");
    println!("  /etc/logrotate.conf");
    println!("      Top-level config. Defaults for all logfile groups + an include");
    println!("      of /etc/logrotate.d/.");
    println!("  /etc/logrotate.d/<service>");
    println!("      Per-package drop-in snippets. Each snippet is a brace-enclosed");
    println!("      block listing one or more logfile patterns and the directives");
    println!("      that override the global defaults.");
    println!("  Grammar:");
    println!("      pattern1 pattern2 ... patternN {{");
    println!("          directive1");
    println!("          directive2 value");
    println!("          subblock {{");
    println!("              ...");
    println!("          }}");
    println!("      }}");
    println!("  Patterns:   filename, glob (*.log), or quoted path.");
    println!("  Comments:   # to end of line.");
    println!("  Include:    include /path/to/dir");
    println!("              Recursively read every regular file in dir as more config.");
    println!("  Tabs ok, semicolons no - this is a custom mini-language.");
}

fn run_directives() {
    println!("Common directives.");
    println!("  Schedule:");
    println!("    daily/weekly/monthly/yearly  Rotate on this calendar boundary.");
    println!("    hourly                       Rotate at every cron run within an hour.");
    println!("    size <N>[k|M|G]              Rotate when the file exceeds N bytes.");
    println!("    minsize <N>                  Rotate only if file is at least N bytes.");
    println!("    maxsize <N>                  Force rotate if file exceeds N (even if");
    println!("                                 schedule says no).");
    println!("    maxage <days>                Delete generations older than this.");
    println!("  Retention:");
    println!("    rotate <N>                   Keep N rotated copies.");
    println!("    start <N>                    Start numbering at N (default 1).");
    println!("  Naming:");
    println!("    dateext                      Use YYYYMMDD extension instead of .1.");
    println!("    dateformat <fmt>             strftime-like, default %Y%m%d.");
    println!("    extension <ext>              Append this before the date suffix.");
    println!("    addextension <ext>           Append after rotation, keeping it on");
    println!("                                 every rotated file.");
    println!("  Compression:");
    println!("    compress / nocompress        gzip the rotated file.");
    println!("    delaycompress                Skip compression of the .1 generation");
    println!("                                 (so daemons that hold the FD don't lose");
    println!("                                 their last log).");
    println!("    compresscmd / uncompresscmd  Override gzip with xz, zstd, bzip2.");
    println!("    compressoptions              Args for the compresscmd.");
    println!("    compressext                  File extension for the compressor.");
    println!("  Hygiene:");
    println!("    missingok / nomissingok      Don't error if the log doesn't exist.");
    println!("    ifempty / notifempty         Rotate a zero-byte file? Default ifempty.");
    println!("    create [<mode> <owner> <group>]  After rotation, create an empty");
    println!("                                     file with these attrs (chmod+chown).");
    println!("    copy                         Copy then leave original alone.");
    println!("    copytruncate                 Copy, then truncate the original to 0.");
    println!("    sharedscripts / nosharedscripts  Run pre/postrotate once per pattern");
    println!("                                     group rather than per matched file.");
    println!("    su <user> <group>            Read/write rotated files as this UID/GID.");
    println!("    mail <addr>                  Mail the oldest expiring log to addr.");
    println!("    olddir <dir>                 Move rotated files into a sibling dir.");
    println!("    nomail                       Don't bother mailing on expire.");
}

fn run_methods() {
    println!("Rotation methods - the two big philosophies.");
    println!();
    println!("  Rename-and-reopen (default):");
    println!("    1. mv access.log access.log.1");
    println!("    2. Send signal (usually SIGHUP) so the daemon reopens its log file.");
    println!("    3. Compress access.log.1 -> .gz if `compress` is set.");
    println!("    Pros: atomic, no data loss between rename and reopen, cheap.");
    println!("    Cons: requires the daemon to handle a reopen signal. Daemons that");
    println!("          keep the FD open without reopening will continue writing");
    println!("          into access.log.1 - silent data loss.");
    println!();
    println!("  copytruncate:");
    println!("    1. cp access.log access.log.1");
    println!("    2. > access.log (truncate to zero).");
    println!("    Pros: works for daemons that can't or won't reopen on signal");
    println!("          (Java apps, anything using mmap, anything in containers");
    println!("          where signalling the right PID is awkward).");
    println!("    Cons: there is a window between copy and truncate where new writes");
    println!("          land in access.log and are then truncated away - small data loss.");
    println!("          Large logs become I/O-expensive to copy.");
    println!();
    println!("Other:");
    println!("    copy            Just copy, never truncate. Daemon keeps writing");
    println!("                    forever; useful only as a backup.");
    println!("    olddir          Move rotated copies to a sibling directory; keeps");
    println!("                    the live log directory tidy.");
}

fn run_hooks() {
    println!("Hook blocks - shell snippets executed at rotation milestones.");
    println!("  firstaction ... endscript");
    println!("    Runs once before *any* file in the pattern group is rotated.");
    println!("    Typically used to stop a service: systemctl stop foo.");
    println!("  prerotate ... endscript");
    println!("    Runs *per matched file* before its rotation (unless sharedscripts");
    println!("    is set, in which case once per pattern group).");
    println!("  postrotate ... endscript");
    println!("    Runs after rotation, before compression. The most common hook -");
    println!("    'kill -HUP $(cat /run/foo.pid)' lives here.");
    println!("  lastaction ... endscript");
    println!("    Runs once after the entire pattern group is done.");
    println!("    Typically a 'systemctl start foo' / 'restart foo' / S3 push.");
    println!("  preremove ... endscript");
    println!("    Runs just before the oldest generation is deleted. Use to archive");
    println!("    the file off-box (rsync, restic, s3 cp).");
    println!();
    println!("Variables: $1 is the file being rotated; $0 is logrotate itself.");
    println!("Hooks run with /bin/sh; nonzero exit aborts that file's rotation.");
}

fn run_cli() {
    println!("Command-line interface.");
    println!("    logrotate [options] <config-file>");
    println!();
    println!("  Common flags:");
    println!("    -d, --debug             Don't do anything; verbose dry-run. Implies -v.");
    println!("                            Recommended every time you edit a snippet.");
    println!("    -f, --force             Rotate even if the schedule says not yet.");
    println!("                            Useful after editing rotate count + wanting it");
    println!("                            to apply immediately.");
    println!("    -s, --state <file>      Override the state file path. Default");
    println!("                            /var/lib/logrotate/logrotate.status.");
    println!("    -v, --verbose           Print decisions as they're made.");
    println!("    -m, --mail <cmd>        Mail command for `mail` directive expiry.");
    println!("                            Default 'sendmail -t'.");
    println!("    -l, --log <file>        Write rotation log here (3.19+).");
    println!("    --skip-state-lock       Skip the lock on the state file (dangerous;");
    println!("                            races with parallel runs).");
    println!("    --usage / -h / --help   Show usage.");
    println!("    --version               Show version.");
    println!();
    println!("Examples:");
    println!("    logrotate -d /etc/logrotate.conf     # dry-run everything");
    println!("    logrotate -f /etc/logrotate.d/nginx  # force one snippet now");
    println!("    logrotate -s /tmp/logrotate.state /etc/logrotate.conf");
}

fn run_state() {
    println!("State file - /var/lib/logrotate/logrotate.status.");
    println!("  Purpose:   records the last rotation timestamp per pattern so that");
    println!("             daily/weekly/monthly schedules are calendar-based, not");
    println!("             'whenever cron happened to run.' Without this file every");
    println!("             cron tick would rotate everything.");
    println!("  Format:    plain text. First line is 'logrotate state -- version 2'.");
    println!("             Each subsequent line is a quoted path + ISO-ish timestamp.");
    println!("  Lock:      logrotate locks the file with flock(2) before reading;");
    println!("             a parallel invocation blocks (or with --skip-state-lock,");
    println!("             races and corrupts the file).");
    println!("  Common bug: the state file is owned by root and not writable by the");
    println!("             user logrotate ran as (su directive). The rotation appears");
    println!("             to succeed but the next run repeats it. Always run as root,");
    println!("             or override -s to a path the runtime user owns.");
}

fn run_examples() {
    println!("Example snippets.");
    println!();
    println!("  /etc/logrotate.d/nginx:");
    println!("      /var/log/nginx/*.log {{");
    println!("          daily");
    println!("          missingok");
    println!("          rotate 14");
    println!("          compress");
    println!("          delaycompress");
    println!("          notifempty");
    println!("          create 0640 www-data adm");
    println!("          sharedscripts");
    println!("          postrotate");
    println!("              /usr/bin/nginx -s reopen >/dev/null 2>&1 || true");
    println!("          endscript");
    println!("      }}");
    println!();
    println!("  /etc/logrotate.d/mysql (copytruncate variant):");
    println!("      /var/log/mysql/*.log {{");
    println!("          weekly");
    println!("          rotate 8");
    println!("          missingok");
    println!("          notifempty");
    println!("          copytruncate");
    println!("          compress");
    println!("          delaycompress");
    println!("      }}");
    println!();
    println!("  Size-triggered snippet for a chatty app:");
    println!("      /var/log/myapp/*.log {{");
    println!("          size 100M");
    println!("          rotate 5");
    println!("          maxsize 200M");
    println!("          compress");
    println!("          compresscmd /usr/bin/zstd");
    println!("          compressext .zst");
    println!("          compressoptions -19 --rm");
    println!("          dateext");
    println!("          dateformat -%Y%m%d-%s");
    println!("      }}");
}

fn run_pitfalls() {
    println!("Common gotchas.");
    println!("  1. Daemon doesn't reopen FD.");
    println!("     Default rename-and-reopen requires a postrotate hook (or a");
    println!("     daemon that handles SIGHUP). Without it, the daemon keeps");
    println!("     writing into the *renamed* file, so logs appear to vanish.");
    println!("     Solution: postrotate kill -HUP / nginx -s reopen / systemctl");
    println!("     kill -s USR1 / copytruncate as a fallback.");
    println!("  2. copytruncate race window.");
    println!("     A few KiB written between the cp and the truncate are lost.");
    println!("     Acceptable for application logs; not acceptable for audit logs.");
    println!("     Audit log volumes should use rename-and-reopen with signalled");
    println!("     reopen, even if it complicates the daemon.");
    println!("  3. delaycompress + sharedscripts gotcha.");
    println!("     With delaycompress, .1 is uncompressed so the daemon can still");
    println!("     finish flushing buffered writes to it. Combined with");
    println!("     sharedscripts and parallel daemons, you can race compression");
    println!("     with the daemon's flush. Use a sleep in postrotate or accept");
    println!("     that .2 is the first compressed file.");
    println!("  4. notifempty hides issues.");
    println!("     A service that has stopped writing logs entirely will never");
    println!("     trigger rotation -> no new file -> further writes (if the");
    println!("     service comes back) skip the rotation accounting.");
    println!("  5. State-file path mismatch under systemd vs cron.");
    println!("     If the cron job and the systemd timer both run logrotate but");
    println!("     point at different state files, you get double rotations.");
    println!("     Stick to one runner per box.");
    println!("  6. Large file copy on copytruncate.");
    println!("     A 5 GiB log will block I/O for seconds. Either rotate by size");
    println!("     before it gets that big, or move to rename-and-reopen.");
    println!("  7. World-writable created file under `create 0666 ...`.");
    println!("     Tempting to keep 'create 0666 root root' to avoid permission");
    println!("     headaches; the file is then writable by every local user.");
    println!("     Use 0640 + the right group instead.");
}

fn run_alternatives() {
    println!("Alternatives and adjacent tools.");
    println!("  systemd-journald   Modern systemd boxes route most logs through");
    println!("                     journald, which does its own rotation (SystemMaxUse,");
    println!("                     SystemMaxFileSize, etc.). When journald is the only");
    println!("                     consumer of a daemon's stderr, you don't need");
    println!("                     logrotate for that daemon at all.");
    println!("  savelog            Debian's classic rotation script in debianutils.");
    println!("                     Single shell command; no schedule, no daemon -");
    println!("                     callers handle the scheduling.");
    println!("  rotatelogs         Apache httpd ships rotatelogs as a piped-log");
    println!("                     helper: 'CustomLog \"|/usr/bin/rotatelogs ...\"'.");
    println!("                     Rotation happens inline in the log pipeline.");
    println!("  cronolog           Same idea as rotatelogs but with strftime-named");
    println!("                     output files. Quasi-deprecated in favour of rotatelogs.");
    println!("  logadm             Solaris' analogue; semantics very close to logrotate.");
    println!("  Promtail / Vector / Fluent Bit");
    println!("                     Ship-then-discard model. The log file's rotation");
    println!("                     is irrelevant because the shipper keeps an offset");
    println!("                     in its own state, and the upstream is the source of");
    println!("                     truth. Many modern stacks throw away logrotate entirely.");
    println!("  s6-log / runit-svlogd");
    println!("                     Daemon-supervisor-integrated log rotators. Each");
    println!("                     service has its own log pipeline supervisor that");
    println!("                     rotates per size + number-to-keep. Replaces both");
    println!("                     syslogd and logrotate in the supervised-service style.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "logrotate".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "config" => run_config(),
        "directives" => run_directives(),
        "methods" => run_methods(),
        "hooks" => run_hooks(),
        "cli" => run_cli(),
        "state" => run_state(),
        "examples" => run_examples(),
        "pitfalls" => run_pitfalls(),
        "alternatives" => run_alternatives(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs_all_subcommands() {
        run_about();
        run_history();
        run_config();
        run_directives();
        run_methods();
        run_hooks();
        run_cli();
        run_state();
        run_examples();
        run_pitfalls();
        run_alternatives();
    }

    #[test]
    fn help_and_version() {
        print_help("logrotate");
        print_version();
    }
}
