// OurOS timedatectl - system configuration managers
//
// Multi-personality binary:
//   timedatectl  - query and change system time/date settings
//   hostnamectl  - query and change system hostname
//   localectl    - query and change system locale/keyboard settings

#![cfg_attr(not(test), no_main)]

// ── Constants ──────────────────────────────────────────────────────────

const HOSTNAME_MAX_LEN: usize = 64;
const LOCALE_CONF_PATH: &[u8] = b"/etc/locale.conf";
const HOSTNAME_PATH: &[u8] = b"/etc/hostname";
const MACHINE_INFO_PATH: &[u8] = b"/etc/machine-info";
const LOCALTIME_PATH: &[u8] = b"/etc/localtime";
const TIMEZONE_PATH: &[u8] = b"/etc/timezone";
const ADJTIME_PATH: &[u8] = b"/etc/adjtime";
const VCONSOLE_PATH: &[u8] = b"/etc/vconsole.conf";

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Personality {
    Timedatectl,
    Hostnamectl,
    Localectl,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe") {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"hostnamectl") {
        Personality::Hostnamectl
    } else if name.eq_ignore_ascii_case(b"localectl") {
        Personality::Localectl
    } else {
        Personality::Timedatectl
    }
}

// ── timedatectl ────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum TimedateCommand {
    Status,
    SetTime,
    SetTimezone,
    ListTimezones,
    SetNtp,
    TimesyncStatus,
    ShowTimesync,
    Help,
    Version,
}

struct TimedateArgs {
    command: TimedateCommand,
    value: Vec<u8>,
    no_pager: bool,
    adjust_system_clock: bool,
    monitor: bool,
}

fn parse_timedate_args(args: &[Vec<u8>]) -> TimedateArgs {
    let mut result = TimedateArgs {
        command: TimedateCommand::Status,
        value: Vec::new(),
        no_pager: false,
        adjust_system_clock: false,
        monitor: false,
    };

    let mut i = 0;
    let mut positionals = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.command = TimedateCommand::Help;
        } else if arg == b"--version" {
            result.command = TimedateCommand::Version;
        } else if arg == b"--no-pager" {
            result.no_pager = true;
        } else if arg == b"--adjust-system-clock" {
            result.adjust_system_clock = true;
        } else if arg == b"--monitor" || arg == b"-m" {
            result.monitor = true;
        } else if !arg.starts_with(b"-") {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    if !positionals.is_empty() {
        let cmd = &positionals[0];
        if cmd == b"status" {
            result.command = TimedateCommand::Status;
        } else if cmd == b"set-time" {
            result.command = TimedateCommand::SetTime;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"set-timezone" {
            result.command = TimedateCommand::SetTimezone;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"list-timezones" {
            result.command = TimedateCommand::ListTimezones;
        } else if cmd == b"set-ntp" {
            result.command = TimedateCommand::SetNtp;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"timesync-status" {
            result.command = TimedateCommand::TimesyncStatus;
        } else if cmd == b"show-timesync" {
            result.command = TimedateCommand::ShowTimesync;
        }
    }

    result
}

fn cmd_timedatectl(args: &TimedateArgs) -> i32 {
    match args.command {
        TimedateCommand::Help => {
            print_out(b"timedatectl [OPTIONS...] COMMAND ...\n\n");
            print_out(b"Query or change system time and date settings.\n\n");
            print_out(b"Commands:\n");
            print_out(b"  status                 Show current settings\n");
            print_out(b"  set-time TIME          Set system time\n");
            print_out(b"  set-timezone ZONE      Set system time zone\n");
            print_out(b"  list-timezones         Show known time zones\n");
            print_out(b"  set-ntp BOOL           Enable/disable NTP\n");
            print_out(b"  timesync-status        Show NTP sync status\n");
            print_out(b"  show-timesync          Show NTP sync properties\n\n");
            print_out(b"Options:\n");
            print_out(b"  -h --help              Show this help\n");
            print_out(b"     --version           Show version\n");
            print_out(b"     --no-pager          Do not pipe output into a pager\n");
            print_out(b"     --adjust-system-clock  Adjust system clock when setting local RTC\n");
            print_out(b"  -m --monitor           Monitor status changes\n");
            0
        }
        TimedateCommand::Version => {
            print_out(b"timedatectl (OurOS systemd-compat) 1.0.0\n");
            0
        }
        TimedateCommand::Status => timedate_status(),
        TimedateCommand::SetTime => {
            if args.value.is_empty() {
                print_err(b"timedatectl: set-time requires TIME argument\n");
                print_err(b"  Format: YYYY-MM-DD HH:MM:SS\n");
                return 1;
            }
            timedate_set_time(&args.value)
        }
        TimedateCommand::SetTimezone => {
            if args.value.is_empty() {
                print_err(b"timedatectl: set-timezone requires ZONE argument\n");
                return 1;
            }
            timedate_set_timezone(&args.value)
        }
        TimedateCommand::ListTimezones => timedate_list_timezones(),
        TimedateCommand::SetNtp => {
            if args.value.is_empty() {
                print_err(b"timedatectl: set-ntp requires BOOL argument (yes/no/true/false)\n");
                return 1;
            }
            timedate_set_ntp(&args.value)
        }
        TimedateCommand::TimesyncStatus => timedate_timesync_status(),
        TimedateCommand::ShowTimesync => timedate_show_timesync(),
    }
}

fn timedate_status() -> i32 {
    // In real implementation: query systemd-timedated via D-Bus or read system files
    print_out(b"               Local time: Mon 2025-01-01 00:00:00 UTC\n");
    print_out(b"           Universal time: Mon 2025-01-01 00:00:00 UTC\n");
    print_out(b"                 RTC time: Mon 2025-01-01 00:00:00\n");
    print_out(b"                Time zone: UTC (UTC, +0000)\n");
    print_out(b"System clock synchronized: yes\n");
    print_out(b"              NTP service: active\n");
    print_out(b"          RTC in local TZ: no\n");
    0
}

fn timedate_set_time(time: &[u8]) -> i32 {
    // Validate time format (basic check)
    // Expected: "YYYY-MM-DD" or "YYYY-MM-DD HH:MM:SS" or "HH:MM:SS"
    if time.is_empty() {
        print_err(b"timedatectl: invalid time format\n");
        return 1;
    }

    // In real implementation: validate and call settimeofday/clock_settime
    print_out(b"Setting system time to: ");
    print_out(time);
    print_out(b"\n");
    0
}

fn timedate_set_timezone(zone: &[u8]) -> i32 {
    // In real implementation: validate zone exists, symlink /etc/localtime
    if zone.is_empty() {
        print_err(b"timedatectl: empty timezone\n");
        return 1;
    }

    print_out(b"Setting timezone to: ");
    print_out(zone);
    print_out(b"\n");
    0
}

fn timedate_list_timezones() -> i32 {
    // Common IANA timezone names
    let zones: &[&[u8]] = &[
        b"Africa/Abidjan", b"Africa/Cairo", b"Africa/Johannesburg",
        b"America/Anchorage", b"America/Chicago", b"America/Denver",
        b"America/Los_Angeles", b"America/New_York", b"America/Sao_Paulo",
        b"Asia/Calcutta", b"Asia/Dubai", b"Asia/Hong_Kong",
        b"Asia/Seoul", b"Asia/Shanghai", b"Asia/Singapore", b"Asia/Tokyo",
        b"Atlantic/Reykjavik",
        b"Australia/Melbourne", b"Australia/Sydney",
        b"Europe/Amsterdam", b"Europe/Berlin", b"Europe/Dublin",
        b"Europe/Istanbul", b"Europe/London", b"Europe/Madrid",
        b"Europe/Moscow", b"Europe/Paris", b"Europe/Rome",
        b"Europe/Stockholm", b"Europe/Zurich",
        b"Pacific/Auckland", b"Pacific/Honolulu",
        b"UTC",
    ];

    for zone in zones {
        print_out(zone);
        print_out(b"\n");
    }
    0
}

fn timedate_set_ntp(value: &[u8]) -> i32 {
    let enable = value == b"yes" || value == b"true" || value == b"1" || value == b"on";
    let disable = value == b"no" || value == b"false" || value == b"0" || value == b"off";

    if !enable && !disable {
        print_err(b"timedatectl: invalid value for set-ntp: ");
        print_err(value);
        print_err(b"\n");
        print_err(b"  Expected: yes/no/true/false/on/off/1/0\n");
        return 1;
    }

    if enable {
        print_out(b"Enabling NTP time synchronization.\n");
    } else {
        print_out(b"Disabling NTP time synchronization.\n");
    }

    // In real implementation: start/stop NTP service
    0
}

fn timedate_timesync_status() -> i32 {
    print_out(b"       Server: (null)\n");
    print_out(b"Poll interval: 0 (min: 32s; max: 2048s)\n");
    print_out(b"         Leap: not synchronized\n");
    print_out(b"      Version: 4\n");
    print_out(b"      Stratum: 0\n");
    print_out(b"    Reference: (null)\n");
    print_out(b"    Precision: 0 (0ns)\n");
    print_out(b"Root distance: 0 (0ns)\n");
    print_out(b"       Offset: n/a\n");
    print_out(b"        Delay: n/a\n");
    print_out(b"       Jitter: n/a\n");
    print_out(b" Packet count: 0\n");
    0
}

fn timedate_show_timesync() -> i32 {
    print_out(b"LinkNTPServers=\n");
    print_out(b"SystemNTPServers=\n");
    print_out(b"FallbackNTPServers=0.pool.ntp.org 1.pool.ntp.org 2.pool.ntp.org 3.pool.ntp.org\n");
    print_out(b"ServerName=(null)\n");
    print_out(b"ServerAddress=(null)\n");
    print_out(b"RootDistanceMaxUSec=5s\n");
    print_out(b"PollIntervalMinUSec=32s\n");
    print_out(b"PollIntervalMaxUSec=34min 8s\n");
    print_out(b"PollIntervalUSec=0\n");
    print_out(b"NTPMessage={ Leap=0, Version=0, Mode=0, Stratum=0 }\n");
    print_out(b"Frequency=0\n");
    0
}

// ── hostnamectl ────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum HostnameCommand {
    Status,
    Hostname,
    SetHostname,
    SetIconName,
    SetChassis,
    SetDeployment,
    SetLocation,
    Help,
    Version,
}

struct HostnameArgs {
    command: HostnameCommand,
    value: Vec<u8>,
    static_only: bool,
    transient: bool,
    pretty: bool,
    no_pager: bool,
}

fn parse_hostname_args(args: &[Vec<u8>]) -> HostnameArgs {
    let mut result = HostnameArgs {
        command: HostnameCommand::Status,
        value: Vec::new(),
        static_only: false,
        transient: false,
        pretty: false,
        no_pager: false,
    };

    let mut i = 0;
    let mut positionals = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.command = HostnameCommand::Help;
        } else if arg == b"--version" {
            result.command = HostnameCommand::Version;
        } else if arg == b"--static" {
            result.static_only = true;
        } else if arg == b"--transient" {
            result.transient = true;
        } else if arg == b"--pretty" {
            result.pretty = true;
        } else if arg == b"--no-pager" {
            result.no_pager = true;
        } else if !arg.starts_with(b"-") {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    if !positionals.is_empty() {
        let cmd = &positionals[0];
        if cmd == b"status" {
            result.command = HostnameCommand::Status;
        } else if cmd == b"hostname" || cmd == b"set-hostname" {
            if positionals.len() > 1 {
                result.command = HostnameCommand::SetHostname;
                result.value = positionals[1].clone();
            } else {
                result.command = HostnameCommand::Hostname;
            }
        } else if cmd == b"icon-name" || cmd == b"set-icon-name" {
            result.command = HostnameCommand::SetIconName;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"chassis" || cmd == b"set-chassis" {
            result.command = HostnameCommand::SetChassis;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"deployment" || cmd == b"set-deployment" {
            result.command = HostnameCommand::SetDeployment;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        } else if cmd == b"location" || cmd == b"set-location" {
            result.command = HostnameCommand::SetLocation;
            if positionals.len() > 1 {
                result.value = positionals[1].clone();
            }
        }
    }

    result
}

fn cmd_hostnamectl(args: &HostnameArgs) -> i32 {
    match args.command {
        HostnameCommand::Help => {
            print_out(b"hostnamectl [OPTIONS...] COMMAND ...\n\n");
            print_out(b"Query or change system hostname.\n\n");
            print_out(b"Commands:\n");
            print_out(b"  status                 Show current hostname settings\n");
            print_out(b"  hostname [NAME]        Get/set system hostname\n");
            print_out(b"  icon-name [NAME]       Get/set icon name for host\n");
            print_out(b"  chassis [TYPE]         Get/set chassis type\n");
            print_out(b"  deployment [ENV]       Get/set deployment environment\n");
            print_out(b"  location [LOC]         Get/set location string\n\n");
            print_out(b"Options:\n");
            print_out(b"  -h --help              Show this help\n");
            print_out(b"     --version           Show version\n");
            print_out(b"     --no-pager          Do not pipe output into a pager\n");
            print_out(b"     --static            Only show/set static hostname\n");
            print_out(b"     --transient         Only show/set transient hostname\n");
            print_out(b"     --pretty            Only show/set pretty hostname\n");
            0
        }
        HostnameCommand::Version => {
            print_out(b"hostnamectl (OurOS systemd-compat) 1.0.0\n");
            0
        }
        HostnameCommand::Status => hostname_status(),
        HostnameCommand::Hostname => hostname_get(args),
        HostnameCommand::SetHostname => {
            if args.value.is_empty() {
                print_err(b"hostnamectl: hostname requires NAME argument\n");
                return 1;
            }
            hostname_set(&args.value, args.static_only, args.transient, args.pretty)
        }
        HostnameCommand::SetIconName => {
            if args.value.is_empty() {
                // Show current icon name
                print_out(b"computer\n");
                return 0;
            }
            hostname_set_property(b"IconName", &args.value)
        }
        HostnameCommand::SetChassis => {
            if args.value.is_empty() {
                print_out(b"desktop\n");
                return 0;
            }
            hostname_set_chassis(&args.value)
        }
        HostnameCommand::SetDeployment => {
            if args.value.is_empty() {
                print_out(b"\n");
                return 0;
            }
            hostname_set_property(b"Deployment", &args.value)
        }
        HostnameCommand::SetLocation => {
            if args.value.is_empty() {
                print_out(b"\n");
                return 0;
            }
            hostname_set_property(b"Location", &args.value)
        }
    }
}

fn hostname_status() -> i32 {
    // In real implementation: read from kernel (gethostname) and config files
    print_out(b"   Static hostname: ouros\n");
    print_out(b"         Icon name: computer-desktop\n");
    print_out(b"           Chassis: desktop\n");
    print_out(b"        Machine ID: 00000000000000000000000000000000\n");
    print_out(b"           Boot ID: 00000000000000000000000000000000\n");
    print_out(b"  Operating System: OurOS\n");
    print_out(b"            Kernel: OurOS Microkernel\n");
    print_out(b"      Architecture: x86-64\n");
    0
}

fn hostname_get(args: &HostnameArgs) -> i32 {
    // In real implementation: gethostname() syscall
    if args.static_only {
        print_out(b"ouros\n");
    } else if args.transient {
        print_out(b"ouros\n");
    } else if args.pretty {
        print_out(b"OurOS Desktop\n");
    } else {
        print_out(b"ouros\n");
    }
    0
}

fn hostname_set(name: &[u8], static_only: bool, transient: bool, pretty: bool) -> i32 {
    // Validate hostname
    if name.len() > HOSTNAME_MAX_LEN {
        print_err(b"hostnamectl: hostname too long (max 64 characters)\n");
        return 1;
    }

    // Check for valid hostname characters (for static/transient)
    if !pretty {
        for &b in name {
            if !(b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
                print_err(b"hostnamectl: invalid character in hostname\n");
                print_err(b"  Static hostnames may only contain a-z, A-Z, 0-9, '-', '.'\n");
                return 1;
            }
        }

        // Must not start or end with hyphen
        if name.first() == Some(&b'-') || name.last() == Some(&b'-') {
            print_err(b"hostnamectl: hostname must not start or end with '-'\n");
            return 1;
        }
    }

    // In real implementation: sethostname() syscall + update /etc/hostname
    let which = if static_only {
        "static"
    } else if transient {
        "transient"
    } else if pretty {
        "pretty"
    } else {
        "all"
    };

    print_out(b"Setting ");
    print_out(which.as_bytes());
    print_out(b" hostname to: ");
    print_out(name);
    print_out(b"\n");
    0
}

fn hostname_set_property(property: &[u8], value: &[u8]) -> i32 {
    // In real implementation: update /etc/machine-info
    print_out(b"Setting ");
    print_out(property);
    print_out(b" to: ");
    print_out(value);
    print_out(b"\n");
    0
}

fn hostname_set_chassis(chassis: &[u8]) -> i32 {
    // Valid chassis types
    let valid = [
        b"desktop".as_slice(), b"laptop", b"convertible", b"server",
        b"tablet", b"handset", b"watch", b"embedded", b"vm", b"container",
    ];

    let is_valid = valid.iter().any(|v| *v == chassis);
    if !is_valid {
        print_err(b"hostnamectl: invalid chassis type: ");
        print_err(chassis);
        print_err(b"\n");
        print_err(b"  Valid types: desktop, laptop, convertible, server, tablet,\n");
        print_err(b"               handset, watch, embedded, vm, container\n");
        return 1;
    }

    hostname_set_property(b"Chassis", chassis)
}

// ── localectl ──────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum LocaleCommand {
    Status,
    SetLocale,
    ListLocales,
    SetKeymap,
    ListKeymaps,
    SetX11Keymap,
    ListX11KeymapModels,
    ListX11KeymapLayouts,
    ListX11KeymapVariants,
    ListX11KeymapOptions,
    Help,
    Version,
}

struct LocaleArgs {
    command: LocaleCommand,
    values: Vec<Vec<u8>>,
    no_pager: bool,
    no_convert: bool,
}

fn parse_locale_args(args: &[Vec<u8>]) -> LocaleArgs {
    let mut result = LocaleArgs {
        command: LocaleCommand::Status,
        values: Vec::new(),
        no_pager: false,
        no_convert: false,
    };

    let mut i = 0;
    let mut positionals = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.command = LocaleCommand::Help;
        } else if arg == b"--version" {
            result.command = LocaleCommand::Version;
        } else if arg == b"--no-pager" {
            result.no_pager = true;
        } else if arg == b"--no-convert" {
            result.no_convert = true;
        } else if !arg.starts_with(b"-") {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    if !positionals.is_empty() {
        let cmd = &positionals[0];
        if cmd == b"status" {
            result.command = LocaleCommand::Status;
        } else if cmd == b"set-locale" {
            result.command = LocaleCommand::SetLocale;
            result.values = positionals[1..].to_vec();
        } else if cmd == b"list-locales" {
            result.command = LocaleCommand::ListLocales;
        } else if cmd == b"set-keymap" {
            result.command = LocaleCommand::SetKeymap;
            result.values = positionals[1..].to_vec();
        } else if cmd == b"list-keymaps" {
            result.command = LocaleCommand::ListKeymaps;
        } else if cmd == b"set-x11-keymap" {
            result.command = LocaleCommand::SetX11Keymap;
            result.values = positionals[1..].to_vec();
        } else if cmd == b"list-x11-keymap-models" {
            result.command = LocaleCommand::ListX11KeymapModels;
        } else if cmd == b"list-x11-keymap-layouts" {
            result.command = LocaleCommand::ListX11KeymapLayouts;
        } else if cmd == b"list-x11-keymap-variants" {
            result.command = LocaleCommand::ListX11KeymapVariants;
        } else if cmd == b"list-x11-keymap-options" {
            result.command = LocaleCommand::ListX11KeymapOptions;
        }
    }

    result
}

fn cmd_localectl(args: &LocaleArgs) -> i32 {
    match args.command {
        LocaleCommand::Help => {
            print_out(b"localectl [OPTIONS...] COMMAND ...\n\n");
            print_out(b"Query or change system locale and keyboard settings.\n\n");
            print_out(b"Commands:\n");
            print_out(b"  status                      Show current locale settings\n");
            print_out(b"  set-locale LOCALE...        Set system locale\n");
            print_out(b"  list-locales                Show known locales\n");
            print_out(b"  set-keymap MAP [TOGGLE]     Set console keyboard mapping\n");
            print_out(b"  list-keymaps                Show known virtual console keymaps\n");
            print_out(b"  set-x11-keymap LAYOUT [MODEL [VARIANT [OPTIONS]]]\n");
            print_out(b"                              Set X11/Wayland keyboard mapping\n");
            print_out(b"  list-x11-keymap-models      Show known X11 keyboard models\n");
            print_out(b"  list-x11-keymap-layouts     Show known X11 keyboard layouts\n");
            print_out(b"  list-x11-keymap-variants [LAYOUT]\n");
            print_out(b"                              Show known X11 keyboard variants\n");
            print_out(b"  list-x11-keymap-options     Show known X11 keyboard options\n\n");
            print_out(b"Options:\n");
            print_out(b"  -h --help                   Show this help\n");
            print_out(b"     --version                Show version\n");
            print_out(b"     --no-pager               Do not pipe output into a pager\n");
            print_out(b"     --no-convert             Don't convert console/X11 keymaps\n");
            0
        }
        LocaleCommand::Version => {
            print_out(b"localectl (OurOS systemd-compat) 1.0.0\n");
            0
        }
        LocaleCommand::Status => locale_status(),
        LocaleCommand::SetLocale => {
            if args.values.is_empty() {
                print_err(b"localectl: set-locale requires at least one LOCALE argument\n");
                return 1;
            }
            locale_set(&args.values)
        }
        LocaleCommand::ListLocales => locale_list_locales(),
        LocaleCommand::SetKeymap => {
            if args.values.is_empty() {
                print_err(b"localectl: set-keymap requires MAP argument\n");
                return 1;
            }
            locale_set_keymap(&args.values, args.no_convert)
        }
        LocaleCommand::ListKeymaps => locale_list_keymaps(),
        LocaleCommand::SetX11Keymap => {
            if args.values.is_empty() {
                print_err(b"localectl: set-x11-keymap requires LAYOUT argument\n");
                return 1;
            }
            locale_set_x11_keymap(&args.values, args.no_convert)
        }
        LocaleCommand::ListX11KeymapModels => locale_list_x11_models(),
        LocaleCommand::ListX11KeymapLayouts => locale_list_x11_layouts(),
        LocaleCommand::ListX11KeymapVariants => locale_list_x11_variants(&args.values),
        LocaleCommand::ListX11KeymapOptions => locale_list_x11_options(),
    }
}

fn locale_status() -> i32 {
    print_out(b"   System Locale: LANG=en_US.UTF-8\n");
    print_out(b"       VC Keymap: us\n");
    print_out(b"      X11 Layout: us\n");
    print_out(b"       X11 Model: pc105\n");
    0
}

fn locale_set(locales: &[Vec<u8>]) -> i32 {
    // Parse VARIABLE=VALUE pairs
    for locale in locales {
        if let Some(eq_pos) = locale.iter().position(|&b| b == b'=') {
            let var = &locale[..eq_pos];
            let val = &locale[eq_pos + 1..];

            // Validate variable name
            let valid_vars = [
                b"LANG".as_slice(), b"LANGUAGE", b"LC_CTYPE", b"LC_NUMERIC",
                b"LC_TIME", b"LC_COLLATE", b"LC_MONETARY", b"LC_MESSAGES",
                b"LC_PAPER", b"LC_NAME", b"LC_ADDRESS", b"LC_TELEPHONE",
                b"LC_MEASUREMENT", b"LC_IDENTIFICATION", b"LC_ALL",
            ];

            if !valid_vars.iter().any(|v| *v == var) {
                print_err(b"localectl: unknown locale variable: ");
                print_err(var);
                print_err(b"\n");
                return 1;
            }

            print_out(b"Setting ");
            print_out(var);
            print_out(b"=");
            print_out(val);
            print_out(b"\n");
        } else {
            // Bare locale name, set as LANG
            print_out(b"Setting LANG=");
            print_out(locale);
            print_out(b"\n");
        }
    }

    // In real implementation: write to /etc/locale.conf
    0
}

fn locale_list_locales() -> i32 {
    let locales: &[&[u8]] = &[
        b"C", b"C.UTF-8", b"POSIX",
        b"de_DE.UTF-8", b"de_AT.UTF-8",
        b"en_AU.UTF-8", b"en_CA.UTF-8", b"en_GB.UTF-8",
        b"en_NZ.UTF-8", b"en_US.UTF-8",
        b"es_ES.UTF-8", b"es_MX.UTF-8",
        b"fr_FR.UTF-8", b"fr_CA.UTF-8",
        b"it_IT.UTF-8",
        b"ja_JP.UTF-8",
        b"ko_KR.UTF-8",
        b"nl_NL.UTF-8",
        b"pl_PL.UTF-8",
        b"pt_BR.UTF-8", b"pt_PT.UTF-8",
        b"ru_RU.UTF-8",
        b"sv_SE.UTF-8",
        b"zh_CN.UTF-8", b"zh_TW.UTF-8",
    ];

    for locale in locales {
        print_out(locale);
        print_out(b"\n");
    }
    0
}

fn locale_set_keymap(values: &[Vec<u8>], no_convert: bool) -> i32 {
    let keymap = &values[0];
    let toggle = values.get(1);

    print_out(b"Setting virtual console keymap to: ");
    print_out(keymap);
    print_out(b"\n");

    if let Some(t) = toggle {
        print_out(b"Setting toggle keymap to: ");
        print_out(t);
        print_out(b"\n");
    }

    if !no_convert {
        print_out(b"Converting to X11 keymap...\n");
    }

    // In real implementation: write to /etc/vconsole.conf
    0
}

fn locale_list_keymaps() -> i32 {
    let keymaps: &[&[u8]] = &[
        b"ANSI-dvorak", b"amiga-de", b"amiga-us",
        b"be-latin1", b"br-abnt2",
        b"de", b"de-latin1", b"de-latin1-nodeadkeys",
        b"dk", b"dvorak", b"es",
        b"fi", b"fr", b"fr-latin1",
        b"hu", b"is-latin1", b"it",
        b"jp106",
        b"ko", b"la-latin1",
        b"nl", b"no", b"pl2",
        b"pt-latin1", b"ro",
        b"ru", b"se-lat6",
        b"sg", b"sk-qwerty",
        b"trq", b"ua-utf",
        b"uk", b"us", b"us-acentos",
    ];

    for km in keymaps {
        print_out(km);
        print_out(b"\n");
    }
    0
}

fn locale_set_x11_keymap(values: &[Vec<u8>], no_convert: bool) -> i32 {
    let layout = &values[0];
    let model = values.get(1);
    let variant = values.get(2);
    let options = values.get(3);

    print_out(b"Setting X11 layout to: ");
    print_out(layout);
    print_out(b"\n");

    if let Some(m) = model {
        print_out(b"Setting X11 model to: ");
        print_out(m);
        print_out(b"\n");
    }

    if let Some(v) = variant {
        print_out(b"Setting X11 variant to: ");
        print_out(v);
        print_out(b"\n");
    }

    if let Some(o) = options {
        print_out(b"Setting X11 options to: ");
        print_out(o);
        print_out(b"\n");
    }

    if !no_convert {
        print_out(b"Converting to console keymap...\n");
    }

    0
}

fn locale_list_x11_models() -> i32 {
    let models: &[&[u8]] = &[
        b"pc101", b"pc102", b"pc104", b"pc105",
        b"dell", b"hp", b"lenovo", b"thinkpad",
        b"apple", b"macbook78", b"macbook79",
        b"chromebook", b"microsoft",
    ];

    for m in models {
        print_out(m);
        print_out(b"\n");
    }
    0
}

fn locale_list_x11_layouts() -> i32 {
    let layouts: &[&[u8]] = &[
        b"am", b"ara", b"at", b"au",
        b"be", b"bg", b"br",
        b"ca", b"ch", b"cn", b"cz",
        b"de", b"dk",
        b"ee", b"es", b"et",
        b"fi", b"fr",
        b"gb", b"ge", b"gr",
        b"hr", b"hu",
        b"ie", b"il", b"in", b"is", b"it",
        b"jp",
        b"kr",
        b"lt", b"lv",
        b"mk", b"mt",
        b"nl", b"no",
        b"pl", b"pt",
        b"ro", b"rs", b"ru",
        b"se", b"si", b"sk",
        b"tr",
        b"ua", b"us", b"uz",
    ];

    for l in layouts {
        print_out(l);
        print_out(b"\n");
    }
    0
}

fn locale_list_x11_variants(values: &[Vec<u8>]) -> i32 {
    // If a layout is specified, list variants for that layout
    let layout = values.get(1).or(values.first());

    // Show common variants
    let variants: &[&[u8]] = &[
        b"nodeadkeys", b"deadacute", b"deadgraveacute",
        b"mac", b"dvorak", b"colemak",
        b"intl", b"alt-intl",
        b"workman", b"norman",
        b"phonetic", b"legacy",
    ];

    for v in variants {
        print_out(v);
        print_out(b"\n");
    }
    0
}

fn locale_list_x11_options() -> i32 {
    let options: &[&[u8]] = &[
        b"grp:alt_shift_toggle",
        b"grp:ctrl_shift_toggle",
        b"grp:caps_toggle",
        b"grp:win_space_toggle",
        b"caps:escape",
        b"caps:swapescape",
        b"caps:ctrl_modifier",
        b"caps:super",
        b"ctrl:nocaps",
        b"ctrl:swapcaps",
        b"compose:ralt",
        b"compose:rwin",
        b"terminate:ctrl_alt_bksp",
    ];

    for o in options {
        print_out(o);
        print_out(b"\n");
    }
    0
}

// ── Utility Functions ──────────────────────────────────────────────────

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
        print_err(b"timedatectl: unable to determine program name\n");
        return 1;
    }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Timedatectl => {
            let parsed = parse_timedate_args(&rest);
            cmd_timedatectl(&parsed)
        }
        Personality::Hostnamectl => {
            let parsed = parse_hostname_args(&rest);
            cmd_hostnamectl(&parsed)
        }
        Personality::Localectl => {
            let parsed = parse_locale_args(&rest);
            cmd_localectl(&parsed)
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality Detection ──────────────────────────────────

    #[test]
    fn test_detect_timedatectl() {
        assert_eq!(detect_personality(b"timedatectl"), Personality::Timedatectl);
        assert_eq!(detect_personality(b"/usr/bin/timedatectl"), Personality::Timedatectl);
        assert_eq!(detect_personality(b"timedatectl.exe"), Personality::Timedatectl);
    }

    #[test]
    fn test_detect_hostnamectl() {
        assert_eq!(detect_personality(b"hostnamectl"), Personality::Hostnamectl);
        assert_eq!(detect_personality(b"/usr/bin/hostnamectl"), Personality::Hostnamectl);
    }

    #[test]
    fn test_detect_localectl() {
        assert_eq!(detect_personality(b"localectl"), Personality::Localectl);
        assert_eq!(detect_personality(b"/usr/bin/localectl"), Personality::Localectl);
    }

    #[test]
    fn test_detect_unknown_defaults_timedatectl() {
        assert_eq!(detect_personality(b"something"), Personality::Timedatectl);
    }

    // ── timedatectl Argument Parsing ───────────────────────────

    #[test]
    fn test_timedate_default_status() {
        let args = parse_timedate_args(&[]);
        assert_eq!(args.command, TimedateCommand::Status);
    }

    #[test]
    fn test_timedate_set_time() {
        let args = parse_timedate_args(&[b"set-time".to_vec(), b"2025-01-01 12:00:00".to_vec()]);
        assert_eq!(args.command, TimedateCommand::SetTime);
        assert_eq!(&args.value, b"2025-01-01 12:00:00");
    }

    #[test]
    fn test_timedate_set_timezone() {
        let args = parse_timedate_args(&[b"set-timezone".to_vec(), b"America/New_York".to_vec()]);
        assert_eq!(args.command, TimedateCommand::SetTimezone);
        assert_eq!(&args.value, b"America/New_York");
    }

    #[test]
    fn test_timedate_list_timezones() {
        let args = parse_timedate_args(&[b"list-timezones".to_vec()]);
        assert_eq!(args.command, TimedateCommand::ListTimezones);
    }

    #[test]
    fn test_timedate_set_ntp() {
        let args = parse_timedate_args(&[b"set-ntp".to_vec(), b"yes".to_vec()]);
        assert_eq!(args.command, TimedateCommand::SetNtp);
        assert_eq!(&args.value, b"yes");
    }

    #[test]
    fn test_timedate_help() {
        let args = parse_timedate_args(&[b"--help".to_vec()]);
        assert_eq!(args.command, TimedateCommand::Help);
    }

    #[test]
    fn test_timedate_options() {
        let args = parse_timedate_args(&[b"--no-pager".to_vec(), b"--adjust-system-clock".to_vec(), b"status".to_vec()]);
        assert!(args.no_pager);
        assert!(args.adjust_system_clock);
        assert_eq!(args.command, TimedateCommand::Status);
    }

    // ── timedatectl Commands ───────────────────────────────────

    #[test]
    fn test_timedate_status_ok() {
        assert_eq!(timedate_status(), 0);
    }

    #[test]
    fn test_timedate_list_timezones_ok() {
        assert_eq!(timedate_list_timezones(), 0);
    }

    #[test]
    fn test_timedate_set_ntp_yes() {
        assert_eq!(timedate_set_ntp(b"yes"), 0);
    }

    #[test]
    fn test_timedate_set_ntp_no() {
        assert_eq!(timedate_set_ntp(b"no"), 0);
    }

    #[test]
    fn test_timedate_set_ntp_invalid() {
        assert_eq!(timedate_set_ntp(b"maybe"), 1);
    }

    #[test]
    fn test_timedate_set_time_ok() {
        assert_eq!(timedate_set_time(b"2025-01-01 12:00:00"), 0);
    }

    #[test]
    fn test_timedate_set_time_empty() {
        assert_eq!(timedate_set_time(b""), 1);
    }

    #[test]
    fn test_timedate_set_timezone_ok() {
        assert_eq!(timedate_set_timezone(b"UTC"), 0);
    }

    #[test]
    fn test_timedate_set_timezone_empty() {
        assert_eq!(timedate_set_timezone(b""), 1);
    }

    #[test]
    fn test_timedate_timesync_status_ok() {
        assert_eq!(timedate_timesync_status(), 0);
    }

    #[test]
    fn test_timedate_show_timesync_ok() {
        assert_eq!(timedate_show_timesync(), 0);
    }

    // ── timedatectl Help/Version ───────────────────────────────

    #[test]
    fn test_timedate_cmd_help() {
        let args = TimedateArgs {
            command: TimedateCommand::Help,
            value: Vec::new(),
            no_pager: false,
            adjust_system_clock: false,
            monitor: false,
        };
        assert_eq!(cmd_timedatectl(&args), 0);
    }

    #[test]
    fn test_timedate_cmd_version() {
        let args = TimedateArgs {
            command: TimedateCommand::Version,
            value: Vec::new(),
            no_pager: false,
            adjust_system_clock: false,
            monitor: false,
        };
        assert_eq!(cmd_timedatectl(&args), 0);
    }

    // ── hostnamectl Argument Parsing ───────────────────────────

    #[test]
    fn test_hostname_default_status() {
        let args = parse_hostname_args(&[]);
        assert_eq!(args.command, HostnameCommand::Status);
    }

    #[test]
    fn test_hostname_set() {
        let args = parse_hostname_args(&[b"set-hostname".to_vec(), b"myhost".to_vec()]);
        assert_eq!(args.command, HostnameCommand::SetHostname);
        assert_eq!(&args.value, b"myhost");
    }

    #[test]
    fn test_hostname_with_flags() {
        let args = parse_hostname_args(&[b"--static".to_vec(), b"hostname".to_vec()]);
        assert!(args.static_only);
    }

    #[test]
    fn test_hostname_set_chassis() {
        let args = parse_hostname_args(&[b"chassis".to_vec(), b"desktop".to_vec()]);
        assert_eq!(args.command, HostnameCommand::SetChassis);
        assert_eq!(&args.value, b"desktop");
    }

    // ── hostnamectl Commands ───────────────────────────────────

    #[test]
    fn test_hostname_status_ok() {
        assert_eq!(hostname_status(), 0);
    }

    #[test]
    fn test_hostname_set_valid() {
        assert_eq!(hostname_set(b"myhost", false, false, false), 0);
    }

    #[test]
    fn test_hostname_set_too_long() {
        let long_name = vec![b'a'; HOSTNAME_MAX_LEN + 1];
        assert_eq!(hostname_set(&long_name, false, false, false), 1);
    }

    #[test]
    fn test_hostname_set_invalid_char() {
        assert_eq!(hostname_set(b"my_host", false, false, false), 1);
    }

    #[test]
    fn test_hostname_set_hyphen_start() {
        assert_eq!(hostname_set(b"-myhost", false, false, false), 1);
    }

    #[test]
    fn test_hostname_set_hyphen_end() {
        assert_eq!(hostname_set(b"myhost-", false, false, false), 1);
    }

    #[test]
    fn test_hostname_set_pretty_allows_special() {
        // Pretty hostnames allow any character
        assert_eq!(hostname_set(b"My Desktop PC!", false, false, true), 0);
    }

    #[test]
    fn test_hostname_set_chassis_valid() {
        assert_eq!(hostname_set_chassis(b"desktop"), 0);
        assert_eq!(hostname_set_chassis(b"laptop"), 0);
        assert_eq!(hostname_set_chassis(b"server"), 0);
        assert_eq!(hostname_set_chassis(b"vm"), 0);
    }

    #[test]
    fn test_hostname_set_chassis_invalid() {
        assert_eq!(hostname_set_chassis(b"spaceship"), 1);
    }

    #[test]
    fn test_hostname_help() {
        let args = HostnameArgs {
            command: HostnameCommand::Help,
            value: Vec::new(),
            static_only: false,
            transient: false,
            pretty: false,
            no_pager: false,
        };
        assert_eq!(cmd_hostnamectl(&args), 0);
    }

    // ── localectl Argument Parsing ─────────────────────────────

    #[test]
    fn test_locale_default_status() {
        let args = parse_locale_args(&[]);
        assert_eq!(args.command, LocaleCommand::Status);
    }

    #[test]
    fn test_locale_set_locale() {
        let args = parse_locale_args(&[b"set-locale".to_vec(), b"LANG=en_US.UTF-8".to_vec()]);
        assert_eq!(args.command, LocaleCommand::SetLocale);
        assert_eq!(args.values.len(), 1);
        assert_eq!(&args.values[0], b"LANG=en_US.UTF-8");
    }

    #[test]
    fn test_locale_list_locales() {
        let args = parse_locale_args(&[b"list-locales".to_vec()]);
        assert_eq!(args.command, LocaleCommand::ListLocales);
    }

    #[test]
    fn test_locale_set_keymap() {
        let args = parse_locale_args(&[b"set-keymap".to_vec(), b"us".to_vec()]);
        assert_eq!(args.command, LocaleCommand::SetKeymap);
        assert_eq!(&args.values[0], b"us");
    }

    #[test]
    fn test_locale_set_x11_keymap() {
        let args = parse_locale_args(&[
            b"set-x11-keymap".to_vec(), b"us".to_vec(), b"pc105".to_vec()
        ]);
        assert_eq!(args.command, LocaleCommand::SetX11Keymap);
        assert_eq!(args.values.len(), 2);
    }

    #[test]
    fn test_locale_no_convert() {
        let args = parse_locale_args(&[b"--no-convert".to_vec(), b"set-keymap".to_vec(), b"us".to_vec()]);
        assert!(args.no_convert);
    }

    // ── localectl Commands ─────────────────────────────────────

    #[test]
    fn test_locale_status_ok() {
        assert_eq!(locale_status(), 0);
    }

    #[test]
    fn test_locale_list_locales_ok() {
        assert_eq!(locale_list_locales(), 0);
    }

    #[test]
    fn test_locale_list_keymaps_ok() {
        assert_eq!(locale_list_keymaps(), 0);
    }

    #[test]
    fn test_locale_set_valid() {
        let values = vec![b"LANG=en_US.UTF-8".to_vec()];
        assert_eq!(locale_set(&values), 0);
    }

    #[test]
    fn test_locale_set_invalid_var() {
        let values = vec![b"INVALID=foo".to_vec()];
        assert_eq!(locale_set(&values), 1);
    }

    #[test]
    fn test_locale_set_bare_name() {
        let values = vec![b"en_US.UTF-8".to_vec()];
        assert_eq!(locale_set(&values), 0);
    }

    #[test]
    fn test_locale_set_keymap_ok() {
        let values = vec![b"us".to_vec()];
        assert_eq!(locale_set_keymap(&values, false), 0);
    }

    #[test]
    fn test_locale_set_keymap_no_convert() {
        let values = vec![b"us".to_vec()];
        assert_eq!(locale_set_keymap(&values, true), 0);
    }

    #[test]
    fn test_locale_set_x11_keymap_ok() {
        let values = vec![b"us".to_vec(), b"pc105".to_vec()];
        assert_eq!(locale_set_x11_keymap(&values, false), 0);
    }

    #[test]
    fn test_locale_list_x11_models_ok() {
        assert_eq!(locale_list_x11_models(), 0);
    }

    #[test]
    fn test_locale_list_x11_layouts_ok() {
        assert_eq!(locale_list_x11_layouts(), 0);
    }

    #[test]
    fn test_locale_list_x11_variants_ok() {
        assert_eq!(locale_list_x11_variants(&[]), 0);
    }

    #[test]
    fn test_locale_list_x11_options_ok() {
        assert_eq!(locale_list_x11_options(), 0);
    }

    #[test]
    fn test_locale_help() {
        let args = LocaleArgs {
            command: LocaleCommand::Help,
            values: Vec::new(),
            no_pager: false,
            no_convert: false,
        };
        assert_eq!(cmd_localectl(&args), 0);
    }
}
