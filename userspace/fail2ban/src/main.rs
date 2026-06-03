#![deny(clippy::all)]
//! fail2ban — personality CLI for Fail2Ban, the Python intrusion-prevention
//! daemon that scans log files for repeated authentication failures and
//! installs firewall blocks against the offending source IPs.
//!
//! Cyril Jaquier started Fail2Ban in 2004 as a simple Python script that
//! grew into the de-facto answer to "stop SSH brute-force in my logs".
//! It is the SSH-hardening tool that "just install fail2ban" has come to
//! mean across virtually every Linux distro: it ships in Debian/Ubuntu
//! main, in RHEL via EPEL, in Arch's community repo, and is the default
//! recommendation in every "how to harden a VPS" guide written this
//! century.
//!
//! The design is brutally pragmatic: a *jail* is the pairing of a *filter*
//! (a regex that says "this log line is a failed authentication for IP
//! X") plus an *action* (what to do about it — typically install an
//! iptables/nftables rule). A *backend* watches the log file (inotify,
//! polling, systemd-journal, gamin, …). When N hits accumulate within
//! a window, the action fires; after a bantime the action's `actionunban`
//! reverses it. Jails are wired up in /etc/fail2ban/jail.local.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Fail2Ban intrusion-prevention personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Fail2Ban at a glance: Python daemon, log-driven IP bans");
    println!("    history       2004 first release -> systemd-journal -> IPv6 -> Py3");
    println!("    architecture  fail2ban-server, fail2ban-client, jails, filters, actions");
    println!("    jails         Built-in jails: sshd, apache-auth, postfix, dovecot, ...");
    println!("    filters       Regex filters in /etc/fail2ban/filter.d/");
    println!("    actions       iptables-multiport, nftables, route, mail-whois, ...");
    println!("    backends      auto, polling, gamin, pyinotify, systemd-journal");
    println!("    cli           fail2ban-client subcommands");
    println!("    config        /etc/fail2ban/{{jail,fail2ban,filter,action}}.{{conf,d/,local}}");
    println!("    tuning        bantime, findtime, maxretry, ignoreip, ignorecommand");
    println!("    limitations   What fail2ban can't fix (rate-distributed brute force)");
    println!("    alternatives  CrowdSec, sshguard, denyhosts, abuseipdb-iptables");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() {
    println!("fail2ban 0.1.0 (OurOS personality CLI)");
}

fn run_about() {
    println!("Fail2Ban - log-driven, regex-matched, action-installing IP banning.");
    println!("  Origin:      Cyril Jaquier, 2004. Originally a single Python script");
    println!("               called BlockHosts; rewritten and renamed Fail2Ban for");
    println!("               the first public release that September.");
    println!("  Language:    Python (Py2 originally; Py3 since 0.10, 2017).");
    println!("  License:     GPL-2.0-or-later.");
    println!("  Repository:  github.com/fail2ban/fail2ban.");
    println!("  Model:       a daemon that tails log files, applies regex filters");
    println!("               to extract failed-auth events, accumulates per-IP");
    println!("               counters, and triggers a configurable action (typically");
    println!("               an iptables/nftables ban) when a threshold is crossed.");
    println!("  Stability:   the project is in long-term maintenance. New releases");
    println!("               cadence has slowed, but it remains the universally-");
    println!("               installed answer to log-derived brute-force defence.");
}

fn run_history() {
    println!("Project history.");
    println!("  Sep 2004:    first public release (0.1) by Cyril Jaquier.");
    println!("  2005-2008:   feature growth - per-jail bantime, multi-action chains,");
    println!("               iptables vs hosts.deny vs route-blackhole options.");
    println!("  2008:        0.8 series, the long-running Python 2 line.");
    println!("  2012:        IPv6 support landed in 0.9.");
    println!("  Aug 2014:    0.9.0 release - systemd-journal backend, IPv6 GA.");
    println!("  Aug 2017:    0.10.0 - Python 3 support, modular IP-set helpers,");
    println!("               improved performance for large log volumes.");
    println!("  Dec 2018:    0.11.0 - whoisIP integration, improved Postfix filters.");
    println!("  2019-2023:   0.11.x maintenance + 1.0 prep. 1.0.0 released Mar 2022.");
    println!("  1.0:         Drops Python 2, requires Py3.5+. IPv6 first-class.");
    println!("               nftables action a first-party citizen alongside iptables.");
    println!("  Current:     1.x maintenance; CrowdSec increasingly cited as the");
    println!("               'next-gen' option for new deployments.");
}

fn run_architecture() {
    println!("Architecture.");
    println!("  fail2ban-server  - the daemon. Loads /etc/fail2ban/fail2ban.conf for");
    println!("                     its own settings, then enumerates enabled jails.");
    println!("                     Listens on a UNIX socket (/var/run/fail2ban/");
    println!("                     fail2ban.sock) for commands.");
    println!("  fail2ban-client  - thin CLI that talks to the server's socket.");
    println!("                     'reload', 'status', 'set <jail> banip <ip>', etc.");
    println!("  fail2ban-regex   - offline tool that applies a filter to a sample log");
    println!("                     line to debug regex matches. Required reading when");
    println!("                     authoring a new filter.");
    println!("  Jail = Filter + Action + Backend + tunables:");
    println!("    Filter    Python file in /etc/fail2ban/filter.d/<name>.conf with a");
    println!("              `failregex` pattern (or list). Capture group `<HOST>` is");
    println!("              the IP / hostname extracted from each match.");
    println!("    Action    File in /etc/fail2ban/action.d/<name>.conf with shell");
    println!("              commands for actionstart / actionstop / actioncheck /");
    println!("              actionban / actionunban.");
    println!("    Backend   How the log is tailed - polling (default), pyinotify");
    println!("              (inotify), gamin, systemd (journald), auto.");
    println!("    Tunables  bantime, findtime, maxretry, ignoreip, ignorecommand,");
    println!("              ignoreself, banaction, logpath, logencoding, mode.");
    println!("  Recidive jail - a meta-jail that watches fail2ban.log itself and");
    println!("                  installs much longer bans on IPs that keep coming back.");
}

fn run_jails() {
    println!("Notable built-in jails (a small sample of /etc/fail2ban/jail.d/).");
    println!("  sshd               OpenSSH auth failures - the canonical reason to install fail2ban.");
    println!("  sshd-ddos          High-rate SSH connect attempts (pre-auth abuse).");
    println!("  apache-auth        Apache 401 Basic/Digest auth failures.");
    println!("  apache-badbots     Apache requests matching known scraper/bot UAs.");
    println!("  apache-noscript    Apache hits trying to execute non-script files as PHP/CGI.");
    println!("  apache-overflows   Apache requests with absurd URL/header sizes.");
    println!("  nginx-http-auth    nginx auth_basic 401 events.");
    println!("  nginx-botsearch    nginx hits matching vulnerability-scanner URL patterns.");
    println!("  postfix            SMTP relay/auth abuse - Postfix smtpd reject lines.");
    println!("  postfix-sasl       SASL auth failures via Postfix.");
    println!("  dovecot            IMAP/POP3 auth failures.");
    println!("  pure-ftpd          Pure-FTPd login failures.");
    println!("  vsftpd             vsftpd login failures.");
    println!("  proftpd            ProFTPD login failures.");
    println!("  wordpress          WordPress wp-login.php brute force (third-party).");
    println!("  named-refused      ISC BIND queries from unauthorized clients.");
    println!("  recidive           Meta-jail: bans IPs that fail2ban already banned recently.");
    println!("  webmin-auth        Webmin 401 login failures.");
    println!("  asterisk           Asterisk SIP auth failures.");
    println!("  mysqld-auth        MySQL/MariaDB auth failures.");
    println!("  pam-generic        Generic libpam pam_unix auth-failure lines.");
}

fn run_filters() {
    println!("Filters - regex extraction of failed-auth events.");
    println!("  Location:  /etc/fail2ban/filter.d/<name>.conf");
    println!("  Schema:    INI file with [INCLUDES] (optional common.local),");
    println!("             [Definition] section containing:");
    println!("               failregex = <regex with <HOST> capture>");
    println!("               ignoreregex = <regex of lines to ignore>");
    println!("               datepattern = <strftime/regex hint, optional>");
    println!("  <HOST>:    a magic placeholder that expands to a regex matching");
    println!("             IPv4, IPv6, or hostname. failregex MUST contain it.");
    println!("  Example:   sshd filter snippet:");
    println!("    failregex = ^%(__prefix_line)s(?:error: PAM: )?[aA]uthentication failure");
    println!("                  for .* from <HOST>");
    println!("                ^%(__prefix_line)s(?:error: PAM: )?Failed publickey");
    println!("                  for .* from <HOST>");
    println!("                ^%(__prefix_line)sInvalid user .* from <HOST>");
    println!("  Authoring: drop a sample log line into a file, then:");
    println!("    fail2ban-regex sample.log /etc/fail2ban/filter.d/mythng.conf");
    println!("  Output shows which failregex matched and which HOST was extracted.");
}

fn run_actions() {
    println!("Actions - what to do once the threshold is crossed.");
    println!("  Location:  /etc/fail2ban/action.d/<name>.conf");
    println!("  Schema:    INI file with [Definition] containing shell snippets:");
    println!("               actionstart   Run once on jail start (create chain).");
    println!("               actionstop    Run once on jail stop (drop chain).");
    println!("               actioncheck   Verify the rule chain still exists.");
    println!("               actionban     Run per banned IP (insert rule).");
    println!("               actionunban   Run per unbanned IP (remove rule).");
    println!("  Built-ins (a small sample):");
    println!("    iptables-multiport    Insert -j REJECT in a per-jail chain.");
    println!("    iptables-allports     Block ALL ports for the IP (heavy hammer).");
    println!("    iptables-ipset-proto6 Use ipset for O(log n) ban-list scaling.");
    println!("    nftables-multiport    nft add rule inet f2b-<jail> ip saddr ... drop");
    println!("    nftables-allports     nft equivalent of -j REJECT for all ports.");
    println!("    route                 Black-hole route via `ip route add ... reject`.");
    println!("    hostsdeny             Append to /etc/hosts.deny (legacy tcpwrap).");
    println!("    mail-whois            Send a notification email with whois output.");
    println!("    mail-whois-lines      Same plus the matching log lines.");
    println!("    abuseipdb             Report banned IP to AbuseIPDB.");
    println!("    cloudflare            Set Cloudflare access rule via API.");
    println!("    badips.com            Report via badips.com community lists.");
    println!("    sendmail-buffered     Batch ban notifications hourly.");
    println!("  Chain multiple: banaction = nftables-multiport, mail-whois-lines.");
}

fn run_backends() {
    println!("Log-tailing backends.");
    println!("  auto          Try pyinotify, then gamin, then polling. Default.");
    println!("  polling       Plain stat()+lseek+read every 1 s. Always works, lowest");
    println!("                code path, slightly higher CPU on huge log volumes.");
    println!("  pyinotify     Linux inotify subscriptions; near-zero latency, near-zero");
    println!("                CPU at idle. Requires the python3-pyinotify package.");
    println!("  gamin         FAM/Gamin abstraction. Historical; rarely used today.");
    println!("  systemd       Read structured records from journald rather than the");
    println!("                plain-text log file. Required when the service does not");
    println!("                write to /var/log/auth.log but only to journald (most");
    println!("                modern systemd-only distros).");
    println!("                Match journald fields via journalmatch = _SYSTEMD_UNIT=sshd.service.");
    println!("  Per-jail:    backend = systemd  (overrides the global default).");
}

fn run_cli() {
    println!("fail2ban-client subcommands.");
    println!("  start                 Start the server (also done by the unit file).");
    println!("  stop                  Stop the server gracefully.");
    println!("  reload                Re-read config, reconcile jails. Idempotent.");
    println!("  reload <jail>         Re-read just that jail.");
    println!("  status                Print the list of active jails.");
    println!("  status <jail>         Show jail's filter / action / current banlist.");
    println!("  ping                  Are you there? Returns 'pong'.");
    println!("  set <jail> banip <ip>     Manually ban an IP.");
    println!("  set <jail> unbanip <ip>   Manually unban an IP.");
    println!("  set loglevel DEBUG    Bump verbosity at runtime.");
    println!("  set dbpurgeage <s>    Override fail2ban.db retention window.");
    println!("  unban --all           Drop every active ban across every jail.");
    println!("  get <jail> bantime    Read effective bantime value.");
    println!("  get <jail> ignoreip   Read effective ignore list.");
    println!();
    println!("Examples:");
    println!("    fail2ban-client status sshd");
    println!("    fail2ban-client set sshd banip 198.51.100.42");
    println!("    fail2ban-client unban --all");
}

fn run_config() {
    println!("Configuration layout.");
    println!("  /etc/fail2ban/fail2ban.conf   Global daemon settings (do not edit).");
    println!("  /etc/fail2ban/fail2ban.local  User overrides for daemon settings.");
    println!("  /etc/fail2ban/jail.conf       Distro-supplied jail definitions (do");
    println!("                                not edit - upgrades will clobber it).");
    println!("  /etc/fail2ban/jail.local      THE file you actually edit. Override");
    println!("                                or enable jails here.");
    println!("  /etc/fail2ban/jail.d/         Drop-in directory; one .conf per jail.");
    println!("                                Preferred by Debian for distro-shipped");
    println!("                                third-party jails (sshd.conf, nginx.conf).");
    println!("  /etc/fail2ban/filter.d/       Filter regex files (.conf + .local).");
    println!("  /etc/fail2ban/action.d/       Action shell-command files.");
    println!("  /var/log/fail2ban.log         Where the daemon logs.");
    println!("  /var/lib/fail2ban/fail2ban.db SQLite DB recording past bans, used to");
    println!("                                re-instate bans across daemon restart.");
    println!();
    println!("Override convention: never edit .conf - copy to .local with just the");
    println!("keys you want to change. dpkg/rpm upgrades overwrite .conf, leave .local.");
}

fn run_tuning() {
    println!("Per-jail tunables.");
    println!("  enabled       true to activate the jail. Default false.");
    println!("  filter        Name of the filter (without .conf) to use.");
    println!("  logpath       Path to the log file (or @journal for systemd backend).");
    println!("  backend       auto, polling, pyinotify, systemd.");
    println!("  maxretry      Failures within findtime before triggering a ban.");
    println!("                Default 5; tune lower for high-value services.");
    println!("  findtime      Sliding window (seconds) for counting failures. Default");
    println!("                600 (10 min).");
    println!("  bantime       Duration of the ban (seconds). Default 600. Use -1 or");
    println!("                'inf' for permanent. Recidive jail typically sets 1 week.");
    println!("  bantime.increment  Boolean. If true, the bantime grows exponentially");
    println!("                     on repeat offenders.");
    println!("  bantime.factor     Multiplier for the exponent (default 1).");
    println!("  bantime.maxtime    Cap on the grown bantime.");
    println!("  bantime.formula    Custom Python expression for the bantime curve.");
    println!("  ignoreip      Whitespace-separated CIDR list never to ban.");
    println!("                CRITICAL: include your management IP, or you will");
    println!("                eventually lock yourself out.");
    println!("  ignorecommand External executable; nonzero exit = do NOT ban.");
    println!("                Used for tying into allowlist APIs.");
    println!("  ignoreself    true (default) to skip the box's own addresses.");
    println!("  banaction     Which action file to use. Default banaction depends");
    println!("                on the iptables/nftables availability detection.");
    println!("  banaction_allports  Variant used when the abuse spans many ports.");
}

fn run_limitations() {
    println!("What fail2ban can't fix.");
    println!("  Distributed brute force: 1000 botnet IPs each trying 3 passwords/day");
    println!("    each won't ever cross maxretry - by the time the kth IP has tried");
    println!("    enough, findtime has rolled and the counter is gone. The right");
    println!("    answer is key-only SSH and rate-limit-by-network in nftables.");
    println!("  Log latency: fail2ban scans logs the daemon wrote. If sshd batches");
    println!("    or buffers, the ban can lag the abuse window. Synchronous logging");
    println!("    (rsyslog without journal-buffering on SSH) helps.");
    println!("  Encrypted attacks: anything that doesn't produce a log line that");
    println!("    matches a filter is invisible. TLS BREACH-style attacks, slowloris,");
    println!("    application-layer abuse without a 4xx response - all unaffected.");
    println!("  Auth providers external to the box: SAML SSO failures at an IdP");
    println!("    are not visible to the host's fail2ban.");
    println!("  Big SQLite db: fail2ban.db grows unboundedly if dbpurgeage is wrong;");
    println!("    huge DBs make daemon restart slow.");
    println!("  Filter false positives: every additional regex adds latency to every");
    println!("    log line tailed. ignoreregex helps but is fragile.");
    println!("  IPv6 hosts behind NAT: banning the source v6 may ban a whole subnet.");
    println!("    Tune at /64 granularity, not /128, when banning v6 brute force.");
}

fn run_alternatives() {
    println!("Other intrusion-prevention tools.");
    println!("  CrowdSec     Open-source successor in spirit - Go agent, signed");
    println!("               community block-list (the CrowdSec Console), pluggable");
    println!("               'scenarios' replace regex filters. Centralised");
    println!("               aggregation across many hosts. Increasingly recommended");
    println!("               for new deployments.");
    println!("  sshguard     C daemon scoped narrowly to brute-force SSH/FTP/IMAP.");
    println!("               Smaller resource footprint than fail2ban; ships in");
    println!("               *BSD bases. pf, ipfw, iptables, nftables targets.");
    println!("  denyhosts    Older Python tool - SSH-only, populated /etc/hosts.deny.");
    println!("               Inactive upstream; do not deploy on new boxes.");
    println!("  ipset + nft rules + abuseipdb-iptables");
    println!("               Hand-rolled approach: fetch a reputation list, load it");
    println!("               into an ipset, install one nft rule. Massively faster");
    println!("               at scale but no autoreversal.");
    println!("  ban-hammer   Custom systemd-journald reactive bans via journalctl -f.");
    println!("               Often written ad-hoc on hardened boxes.");
    println!("  AppArmor / SELinux audit -> auditd ipset feeder");
    println!("               LSM-derived bans for processes that misbehave.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fail2ban".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "architecture" => run_architecture(),
        "jails" => run_jails(),
        "filters" => run_filters(),
        "actions" => run_actions(),
        "backends" => run_backends(),
        "cli" => run_cli(),
        "config" => run_config(),
        "tuning" => run_tuning(),
        "limitations" => run_limitations(),
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
        run_architecture();
        run_jails();
        run_filters();
        run_actions();
        run_backends();
        run_cli();
        run_config();
        run_tuning();
        run_limitations();
        run_alternatives();
    }

    #[test]
    fn help_and_version() {
        print_help("fail2ban");
        print_version();
    }
}
