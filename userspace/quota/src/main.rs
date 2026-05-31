// OurOS quota - disk quota management
//
// Multi-personality binary:
//   quota     - display disk usage and limits
//   edquota   - edit user/group quotas
//   repquota  - summarize quotas for a filesystem
//   quotaon   - enable disk quotas
//   quotaoff  - disable disk quotas

#![cfg_attr(not(test), no_main)]

// ── Constants ──────────────────────────────────────────────────────────

const QUOTA_BLOCK_SIZE: u64 = 1024; // 1 KiB blocks
const QUOTA_USR_FILE: &[u8] = b"aquota.user";
const QUOTA_GRP_FILE: &[u8] = b"aquota.group";
const QUOTA_VERSION: u32 = 2;

// Grace periods in seconds
const DEFAULT_BLOCK_GRACE: u64 = 604800; // 7 days
const DEFAULT_INODE_GRACE: u64 = 604800; // 7 days

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Personality {
    Quota,
    Edquota,
    Repquota,
    Quotaon,
    Quotaoff,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe")
    {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"edquota") {
        Personality::Edquota
    } else if name.eq_ignore_ascii_case(b"repquota") {
        Personality::Repquota
    } else if name.eq_ignore_ascii_case(b"quotaon") {
        Personality::Quotaon
    } else if name.eq_ignore_ascii_case(b"quotaoff") {
        Personality::Quotaoff
    } else {
        Personality::Quota
    }
}

// ── Data Structures ────────────────────────────────────────────────────

#[derive(Clone)]
struct QuotaInfo {
    block_usage: u64,        // current block usage in KB
    block_soft: u64,         // soft limit (warning threshold)
    block_hard: u64,         // hard limit (absolute maximum)
    block_grace: u64,        // grace period for soft limit (seconds)
    block_grace_expire: u64, // expiry timestamp for soft limit grace
    inode_usage: u64,        // current inode (file) count
    inode_soft: u64,         // soft limit
    inode_hard: u64,         // hard limit
    inode_grace: u64,        // grace period for soft limit
    inode_grace_expire: u64,
}

impl QuotaInfo {
    fn new() -> Self {
        QuotaInfo {
            block_usage: 0,
            block_soft: 0,
            block_hard: 0,
            block_grace: DEFAULT_BLOCK_GRACE,
            block_grace_expire: 0,
            inode_usage: 0,
            inode_soft: 0,
            inode_hard: 0,
            inode_grace: DEFAULT_INODE_GRACE,
            inode_grace_expire: 0,
        }
    }

    fn blocks_over_soft(&self) -> bool {
        self.block_soft > 0 && self.block_usage > self.block_soft
    }

    fn blocks_over_hard(&self) -> bool {
        self.block_hard > 0 && self.block_usage >= self.block_hard
    }

    fn inodes_over_soft(&self) -> bool {
        self.inode_soft > 0 && self.inode_usage > self.inode_soft
    }

    fn inodes_over_hard(&self) -> bool {
        self.inode_hard > 0 && self.inode_usage >= self.inode_hard
    }

    fn status_char(&self) -> u8 {
        if self.blocks_over_hard() || self.inodes_over_hard() {
            b'!' // over hard limit
        } else if self.blocks_over_soft() || self.inodes_over_soft() {
            b'+' // over soft limit (in grace)
        } else {
            b'-' // within limits
        }
    }
}

struct QuotaEntry {
    id: u32,
    name: Vec<u8>,
    info: QuotaInfo,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum QuotaType {
    User,
    Group,
}

// ── Argument Parsing ───────────────────────────────────────────────────

struct QuotaArgs {
    quota_type: QuotaType,
    filesystem: Option<Vec<u8>>,
    ids: Vec<Vec<u8>>,
    verbose: bool,
    quiet: bool,
    human_readable: bool,
    no_wrap: bool,
    show_help: bool,
    show_version: bool,
    all: bool,
    print_state: bool, // for quotaon -p
}

fn parse_quota_args(args: &[Vec<u8>]) -> QuotaArgs {
    let mut result = QuotaArgs {
        quota_type: QuotaType::User,
        filesystem: None,
        ids: Vec::new(),
        verbose: false,
        quiet: false,
        human_readable: false,
        no_wrap: false,
        show_help: false,
        show_version: false,
        all: false,
        print_state: false,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-u" || arg == b"--user" {
            result.quota_type = QuotaType::User;
        } else if arg == b"-g" || arg == b"--group" {
            result.quota_type = QuotaType::Group;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"-q" || arg == b"--quiet" {
            result.quiet = true;
        } else if arg == b"--human-readable" || arg == b"-s" {
            result.human_readable = true;
        } else if arg == b"-w" || arg == b"--no-wrap" {
            result.no_wrap = true;
        } else if arg == b"-a" || arg == b"--all" {
            result.all = true;
        } else if arg == b"-p" || arg == b"--print-state" {
            result.print_state = true;
        } else if arg == b"-f" || arg == b"--filesystem" {
            i += 1;
            if i < args.len() {
                result.filesystem = Some(args[i].clone());
            }
        } else if !arg.starts_with(b"-") {
            result.ids.push(arg.clone());
        }
        i += 1;
    }

    result
}

// ── quota command ──────────────────────────────────────────────────────

fn cmd_quota(args: &QuotaArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: quota [options] [user|group...]\n\n");
        print_out(b"Display disk usage and limits.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -u, --user          display user quotas (default)\n");
        print_out(b"  -g, --group         display group quotas\n");
        print_out(b"  -v, --verbose       display quotas even if no limits set\n");
        print_out(b"  -q, --quiet         only display over-limit info\n");
        print_out(b"  -s, --human-readable  show sizes in human-readable format\n");
        print_out(b"  -w, --no-wrap       do not wrap long lines\n");
        print_out(b"  -f, --filesystem FS only report on filesystem FS\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"quota (OurOS) 1.0.0\n");
        return 0;
    }

    // In real implementation: query quota subsystem
    let type_name = match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    };

    let targets = if args.ids.is_empty() {
        // Show current user's quota
        vec![b"root".to_vec()]
    } else {
        args.ids.clone()
    };

    print_out(b"Disk quotas for ");
    print_out(type_name);
    print_out(b" ");
    if !targets.is_empty() {
        print_out(&targets[0]);
    }
    print_out(b" (uid 0):\n");

    print_out(b"     Filesystem  blocks   quota   limit   grace   files   quota   limit   grace\n");

    // Simulated output
    let info = QuotaInfo::new();
    print_out(b"      /dev/sda1       0       0       0               0       0       0        \n");

    0
}

// ── edquota command ────────────────────────────────────────────────────

fn cmd_edquota(args: &QuotaArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: edquota [options] user|group...\n\n");
        print_out(b"Edit user or group quotas.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -u, --user          edit user quotas (default)\n");
        print_out(b"  -g, --group         edit group quotas\n");
        print_out(b"  -f, --filesystem FS only edit quotas on filesystem FS\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"edquota (OurOS) 1.0.0\n");
        return 0;
    }

    if args.ids.is_empty() {
        print_err(b"edquota: no user or group specified\n");
        return 1;
    }

    let type_name = match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    };

    for id in &args.ids {
        print_out(b"Editing quotas for ");
        print_out(type_name);
        print_out(b" ");
        print_out(id);
        print_out(b":\n");

        // In real implementation: open temp file with current quotas,
        // launch editor, parse result, update quotas
        print_out(b"Quotas for ");
        print_out(type_name);
        print_out(b" ");
        print_out(id);
        print_out(b":\n");
        print_out(b"  Filesystem   blocks (soft)  (hard)  inodes (soft)  (hard)\n");
        print_out(b"  /dev/sda1         0      0      0       0      0      0\n");
    }

    0
}

// ── repquota command ───────────────────────────────────────────────────

fn cmd_repquota(args: &QuotaArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: repquota [options] [filesystem...]\n\n");
        print_out(b"Summarize quotas for a filesystem.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -u, --user          report user quotas (default)\n");
        print_out(b"  -g, --group         report group quotas\n");
        print_out(b"  -a, --all           report on all filesystems\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -s, --human-readable  human-readable sizes\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"repquota (OurOS) 1.0.0\n");
        return 0;
    }

    if !args.all && args.ids.is_empty() && args.filesystem.is_none() {
        print_err(b"repquota: no filesystem specified (use -a for all)\n");
        return 1;
    }

    let type_name = match args.quota_type {
        QuotaType::User => b"User".as_slice(),
        QuotaType::Group => b"Group",
    };

    // Header
    print_out(b"*** Report for ");
    print_out(match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    });
    print_out(b" quotas on device ");
    if let Some(ref fs) = args.filesystem {
        print_out(fs);
    } else if !args.ids.is_empty() {
        print_out(&args.ids[0]);
    } else {
        print_out(b"/dev/sda1");
    }
    print_out(b"\n");

    print_out(b"Block grace time: 7days; Inode grace time: 7days\n");

    // Column headers
    if args.human_readable {
        print_out(b"                        Block limits                File limits\n");
        print_out(b"User            used    soft    hard  grace    used  soft  hard  grace\n");
    } else {
        print_out(b"                        Block limits                File limits\n");
        print_out(b"User            used    soft    hard  grace    used  soft  hard  grace\n");
    }

    print_out(b"----------------------------------------------------------------------\n");

    // Simulated entries
    print_out(b"root      --       0       0       0                0     0     0        \n");

    0
}

// ── quotaon command ────────────────────────────────────────────────────

fn cmd_quotaon(args: &QuotaArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: quotaon [options] [-a | filesystem...]\n\n");
        print_out(b"Enable disk quotas.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -u, --user          enable user quotas (default)\n");
        print_out(b"  -g, --group         enable group quotas\n");
        print_out(b"  -a, --all           enable quotas on all filesystems\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -p, --print-state   print current quota state\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"quotaon (OurOS) 1.0.0\n");
        return 0;
    }

    if args.print_state {
        return quotaon_print_state(args);
    }

    if !args.all && args.ids.is_empty() {
        print_err(b"quotaon: no filesystem specified (use -a for all)\n");
        return 1;
    }

    let type_name = match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    };

    if args.all {
        if args.verbose {
            print_out(b"Enabling ");
            print_out(type_name);
            print_out(b" quotas on all filesystems...\n");
        }
        // In real implementation: scan /etc/fstab for usrquota/grpquota options
        // and enable quotas on each
    } else {
        for fs in &args.ids {
            if args.verbose {
                print_out(b"Enabling ");
                print_out(type_name);
                print_out(b" quotas on ");
                print_out(fs);
                print_out(b"\n");
            }
            // In real implementation: quotactl(Q_QUOTAON, ...)
        }
    }

    0
}

fn quotaon_print_state(args: &QuotaArgs) -> i32 {
    let type_name = match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    };

    // In real implementation: query kernel for quota state
    print_out(type_name);
    print_out(b" quota on / (/dev/sda1) is off\n");
    0
}

// ── quotaoff command ───────────────────────────────────────────────────

fn cmd_quotaoff(args: &QuotaArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: quotaoff [options] [-a | filesystem...]\n\n");
        print_out(b"Disable disk quotas.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -u, --user          disable user quotas (default)\n");
        print_out(b"  -g, --group         disable group quotas\n");
        print_out(b"  -a, --all           disable quotas on all filesystems\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"quotaoff (OurOS) 1.0.0\n");
        return 0;
    }

    if !args.all && args.ids.is_empty() {
        print_err(b"quotaoff: no filesystem specified (use -a for all)\n");
        return 1;
    }

    let type_name = match args.quota_type {
        QuotaType::User => b"user".as_slice(),
        QuotaType::Group => b"group",
    };

    if args.all {
        if args.verbose {
            print_out(b"Disabling ");
            print_out(type_name);
            print_out(b" quotas on all filesystems...\n");
        }
    } else {
        for fs in &args.ids {
            if args.verbose {
                print_out(b"Disabling ");
                print_out(type_name);
                print_out(b" quotas on ");
                print_out(fs);
                print_out(b"\n");
            }
        }
    }

    0
}

// ── Size Formatting ────────────────────────────────────────────────────

fn format_blocks_human(blocks: u64) -> Vec<u8> {
    let bytes = blocks.saturating_mul(QUOTA_BLOCK_SIZE);
    if bytes < 1024 {
        let mut buf = format_u64(bytes);
        buf.push(b'B');
        return buf;
    }
    let kb = bytes / 1024;
    if kb < 1024 {
        let mut buf = format_u64(kb);
        buf.push(b'K');
        return buf;
    }
    let mb = kb / 1024;
    if mb < 1024 {
        let mut buf = format_u64(mb);
        buf.push(b'M');
        return buf;
    }
    let gb = mb / 1024;
    let mut buf = format_u64(gb);
    buf.push(b'G');
    buf
}

fn format_grace(seconds: u64) -> Vec<u8> {
    if seconds == 0 {
        return b"none".to_vec();
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;

    if days > 0 {
        let mut buf = format_u64(days);
        buf.extend_from_slice(b"days");
        return buf;
    }
    if hours > 0 {
        let mut buf = format_u64(hours);
        buf.push(b':');
        if mins < 10 {
            buf.push(b'0');
        }
        buf.extend_from_slice(&format_u64(mins));
        return buf;
    }
    let mut buf = format_u64(mins);
    buf.extend_from_slice(b"min");
    buf
}

// ── Utility Functions ──────────────────────────────────────────────────

fn format_u64(mut n: u64) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    buf.reverse();
    buf
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .map(|p| p + 1)
        .unwrap_or(start);
    if start >= end { &[] } else { &s[start..end] }
}

fn print_out(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stdout().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn print_err(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn get_args() -> Vec<Vec<u8>> {
    #[cfg(not(test))]
    {
        std::env::args().map(|a| a.into_bytes()).collect()
    }
    #[cfg(test)]
    {
        Vec::new()
    }
}

// ── Entry Point ────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args = get_args();
    if args.is_empty() {
        print_err(b"quota: unable to determine program name\n");
        return 1;
    }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();
    let parsed = parse_quota_args(&rest);

    match personality {
        Personality::Quota => cmd_quota(&parsed),
        Personality::Edquota => cmd_edquota(&parsed),
        Personality::Repquota => cmd_repquota(&parsed),
        Personality::Quotaon => cmd_quotaon(&parsed),
        Personality::Quotaoff => cmd_quotaoff(&parsed),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality Detection ──────────────────────────────────

    #[test]
    fn test_detect_quota() {
        assert_eq!(detect_personality(b"quota"), Personality::Quota);
        assert_eq!(detect_personality(b"/usr/bin/quota"), Personality::Quota);
    }

    #[test]
    fn test_detect_edquota() {
        assert_eq!(detect_personality(b"edquota"), Personality::Edquota);
    }

    #[test]
    fn test_detect_repquota() {
        assert_eq!(detect_personality(b"repquota"), Personality::Repquota);
    }

    #[test]
    fn test_detect_quotaon() {
        assert_eq!(detect_personality(b"quotaon"), Personality::Quotaon);
    }

    #[test]
    fn test_detect_quotaoff() {
        assert_eq!(detect_personality(b"quotaoff"), Personality::Quotaoff);
    }

    #[test]
    fn test_detect_with_exe() {
        assert_eq!(detect_personality(b"quota.exe"), Personality::Quota);
    }

    // ── QuotaInfo ──────────────────────────────────────────────

    #[test]
    fn test_quota_info_new() {
        let info = QuotaInfo::new();
        assert_eq!(info.block_usage, 0);
        assert_eq!(info.block_soft, 0);
        assert_eq!(info.block_hard, 0);
        assert_eq!(info.inode_usage, 0);
        assert_eq!(info.block_grace, DEFAULT_BLOCK_GRACE);
    }

    #[test]
    fn test_quota_within_limits() {
        let info = QuotaInfo {
            block_usage: 100,
            block_soft: 500,
            block_hard: 1000,
            inode_usage: 10,
            inode_soft: 50,
            inode_hard: 100,
            ..QuotaInfo::new()
        };
        assert!(!info.blocks_over_soft());
        assert!(!info.blocks_over_hard());
        assert!(!info.inodes_over_soft());
        assert!(!info.inodes_over_hard());
        assert_eq!(info.status_char(), b'-');
    }

    #[test]
    fn test_quota_over_soft() {
        let info = QuotaInfo {
            block_usage: 600,
            block_soft: 500,
            block_hard: 1000,
            ..QuotaInfo::new()
        };
        assert!(info.blocks_over_soft());
        assert!(!info.blocks_over_hard());
        assert_eq!(info.status_char(), b'+');
    }

    #[test]
    fn test_quota_over_hard() {
        let info = QuotaInfo {
            block_usage: 1000,
            block_soft: 500,
            block_hard: 1000,
            ..QuotaInfo::new()
        };
        assert!(info.blocks_over_soft());
        assert!(info.blocks_over_hard());
        assert_eq!(info.status_char(), b'!');
    }

    #[test]
    fn test_quota_inode_over_soft() {
        let info = QuotaInfo {
            inode_usage: 60,
            inode_soft: 50,
            inode_hard: 100,
            ..QuotaInfo::new()
        };
        assert!(info.inodes_over_soft());
        assert!(!info.inodes_over_hard());
        assert_eq!(info.status_char(), b'+');
    }

    #[test]
    fn test_quota_no_limits() {
        let info = QuotaInfo::new();
        assert!(!info.blocks_over_soft());
        assert!(!info.blocks_over_hard());
        assert!(!info.inodes_over_soft());
        assert!(!info.inodes_over_hard());
        assert_eq!(info.status_char(), b'-');
    }

    // ── Size Formatting ────────────────────────────────────────

    #[test]
    fn test_format_blocks_human() {
        assert_eq!(format_blocks_human(0), b"0B");
        assert_eq!(format_blocks_human(1), b"1K"); // 1 block = 1K
        assert_eq!(format_blocks_human(1024), b"1M"); // 1024 blocks = 1M
        assert_eq!(format_blocks_human(1048576), b"1G");
    }

    #[test]
    fn test_format_grace() {
        assert_eq!(format_grace(0), b"none");
        assert_eq!(format_grace(60), b"1min");
        assert_eq!(format_grace(3600), b"1:00");
        assert_eq!(format_grace(86400), b"1days");
        assert_eq!(format_grace(604800), b"7days");
    }

    #[test]
    fn test_format_grace_mixed() {
        assert_eq!(format_grace(3660), b"1:01");
        assert_eq!(format_grace(7200), b"2:00");
    }

    // ── Argument Parsing ───────────────────────────────────────

    #[test]
    fn test_parse_quota_defaults() {
        let args = parse_quota_args(&[]);
        assert_eq!(args.quota_type, QuotaType::User);
        assert!(!args.verbose);
        assert!(!args.all);
    }

    #[test]
    fn test_parse_quota_group() {
        let args = parse_quota_args(&[b"-g".to_vec()]);
        assert_eq!(args.quota_type, QuotaType::Group);
    }

    #[test]
    fn test_parse_quota_verbose() {
        let args = parse_quota_args(&[b"-v".to_vec()]);
        assert!(args.verbose);
    }

    #[test]
    fn test_parse_quota_human() {
        let args = parse_quota_args(&[b"-s".to_vec()]);
        assert!(args.human_readable);
    }

    #[test]
    fn test_parse_quota_filesystem() {
        let args = parse_quota_args(&[b"-f".to_vec(), b"/dev/sda1".to_vec()]);
        assert_eq!(args.filesystem.as_deref(), Some(b"/dev/sda1".as_slice()));
    }

    #[test]
    fn test_parse_quota_ids() {
        let args = parse_quota_args(&[b"user1".to_vec(), b"user2".to_vec()]);
        assert_eq!(args.ids.len(), 2);
        assert_eq!(&args.ids[0], b"user1");
        assert_eq!(&args.ids[1], b"user2");
    }

    // ── Command Functions ──────────────────────────────────────

    #[test]
    fn test_quota_help() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: true,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_quota(&args), 0);
    }

    #[test]
    fn test_edquota_no_user() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_edquota(&args), 1);
    }

    #[test]
    fn test_edquota_with_user() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: vec![b"testuser".to_vec()],
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_edquota(&args), 0);
    }

    #[test]
    fn test_repquota_no_fs() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_repquota(&args), 1);
    }

    #[test]
    fn test_repquota_all() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: true,
            print_state: false,
        };
        assert_eq!(cmd_repquota(&args), 0);
    }

    #[test]
    fn test_quotaon_no_fs() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_quotaon(&args), 1);
    }

    #[test]
    fn test_quotaon_all() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: true,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: true,
            print_state: false,
        };
        assert_eq!(cmd_quotaon(&args), 0);
    }

    #[test]
    fn test_quotaoff_no_fs() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: false,
        };
        assert_eq!(cmd_quotaoff(&args), 1);
    }

    #[test]
    fn test_quotaoff_all() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: true,
            print_state: false,
        };
        assert_eq!(cmd_quotaoff(&args), 0);
    }

    #[test]
    fn test_quotaon_print_state() {
        let args = QuotaArgs {
            quota_type: QuotaType::User,
            filesystem: None,
            ids: Vec::new(),
            verbose: false,
            quiet: false,
            human_readable: false,
            no_wrap: false,
            show_help: false,
            show_version: false,
            all: false,
            print_state: true,
        };
        assert_eq!(cmd_quotaon(&args), 0);
    }

    // ── Utility Functions ──────────────────────────────────────

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
        assert_eq!(format_u64(1000), b"1000");
    }

    #[test]
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hello  "), b"hello");
        assert_eq!(trim_bytes(b""), b"" as &[u8]);
    }
}
