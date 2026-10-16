//! `top_nls.c`: the text `top` shows, in the four tables upstream keeps.
//!
//! - [`HEAD`] (`N_col`): each field's column heading, by `EU_*` number.
//! - [`DESC`] (`N_fld`): each field's description on the Fields screen.
//! - The `Norm` table (`N_txt`, `N_fmt`): plain text and `printf` formats,
//!   one constant each, named as upstream's enum names them.
//! - The `Uniq` table (`N_unq`): the screens `show_special` draws, with its
//!   `~1`..`~8` attribute markers.
//!
//! Upstream looks each string up through `gettext`. SlateOS carries no
//! procps translations, so every string is the one compiled in -- as
//! upstream's are in the C locale.

/// `Head_nlstab`: column headings, by field.
pub const HEAD: [&str; crate::defs::EU_MAXPFLGS] = [
    "PID", "PPID", "UID", "USER", "RUID", "RUSER", "SUID", "SUSER", "GID", "GROUP", "PGRP", "TTY",
    "TPGID", "SID", "PR", "NI", "nTH", "P", "%CPU", "TIME", "TIME+", "%MEM", "VIRT", "SWAP", "RES",
    "CODE", "DATA", "SHR", "nMaj", "nMin", "nDRT", "S", "COMMAND", "WCHAN", "Flags", "CGROUPS",
    "SUPGIDS", "SUPGRPS", "TGID", "OOMa", "OOMs", "ENVIRON", "vMj", "vMn", "USED", "nsIPC",
    "nsMNT", "nsNET", "nsPID", "nsUSER", "nsUTS", "LXC", "RSan", "RSfd", "RSlk", "RSsh", "CGNAME",
    "NU", "LOGID", "EXE", "RSS", "PSS", "PSan", "PSfd", "PSsh", "USS", "ioR", "ioRop", "ioW",
    "ioWop", "AGID", "AGNI", "STARTED", "ELAPSED", "%CUU", "%CUC", "nsCGROUP", "nsTIME",
];

/// `Desc_nlstab`: field descriptions, by field.
pub const DESC: [&str; crate::defs::EU_MAXPFLGS] = [
    "Process Id",
    "Parent Process pid",
    "Effective User Id",
    "Effective User Name",
    "Real User Id",
    "Real User Name",
    "Saved User Id",
    "Saved User Name",
    "Group Id",
    "Group Name",
    "Process Group Id",
    "Controlling Tty",
    "Tty Process Grp Id",
    "Session Id",
    "Priority",
    "Nice Value",
    "Number of Threads",
    "Last Used Cpu (SMP)",
    "CPU Usage",
    "CPU Time",
    "CPU Time, hundredths",
    "Memory Usage (RES)",
    "Virtual Image (KiB)",
    "Swapped Size (KiB)",
    "Resident Size (KiB)",
    "Code Size (KiB)",
    "Data+Stack (KiB)",
    "Shared Memory (KiB)",
    "Major Page Faults",
    "Minor Page Faults",
    "Dirty Pages Count",
    "Process Status",
    "Command Name/Line",
    "Sleeping in Function",
    "Task Flags <sched.h>",
    "Control Groups",
    "Supp Groups IDs",
    "Supp Groups Names",
    "Thread Group Id",
    "OOMEM Adjustment",
    "OOMEM Score current",
    "Environment vars",
    "Major Faults delta",
    "Minor Faults delta",
    "Res+Swap Size (KiB)",
    "IPC namespace Inode",
    "MNT namespace Inode",
    "NET namespace Inode",
    "PID namespace Inode",
    "USER namespace Inode",
    "UTS namespace Inode",
    "LXC container name",
    "RES Anonymous (KiB)",
    "RES File-based (KiB)",
    "RES Locked (KiB)",
    "RES Shared (KiB)",
    "Control Group name",
    "Last Used NUMA node",
    "Login User Id",
    "Executable Path",
    "Res Mem (smaps), KiB",
    "Proportion RSS,  KiB",
    "Proportion Anon, KiB",
    "Proportion File, KiB",
    "Proportion Shrd, KiB",
    "Unique RSS, KiB",
    "I/O Bytes Read",
    "I/O Read Operations",
    "I/O Bytes Written",
    "I/O Write Operations",
    "Autogroup Identifier",
    "Autogroup Nice Value",
    "Start Time from boot",
    "Elapsed Running Time",
    "CPU Utilization",
    "Utilization + child",
    "CGRP namespace Inode",
    "TIME namespace Inode",
];

// --- Norm_nlstab --------------------------------------------------------------

pub const EXIT_SIGNALS_FMT: &str = concat!(
    "\tsignal %d (%s) was caught by %s, please\n",
    "\tsend bug reports to <procps@freelists.org>\n"
);
pub const HELP_CMDLINE_FMT: &str = concat!(
    "\n",
    "Usage:\n",
    " %s [options]\n",
    "\n",
    "Options:\n",
    " -b, --batch-mode                run in non-interactive batch mode\n",
    " -c, --cmdline-toggle            reverse last remembered 'c' state\n",
    " -d, --delay =SECS [.TENTHS]     iterative delay as SECS [.TENTHS]\n",
    " -E, --scale-summary-mem =SCALE  set mem as: k,m,g,t,p,e for SCALE\n",
    " -e, --scale-task-mem =SCALE     set mem with: k,m,g,t,p for SCALE\n",
    " -H, --threads-show              show tasks plus all their threads\n",
    " -i, --idle-toggle               reverse last remembered 'i' state\n",
    " -n, --iterations =NUMBER        exit on maximum iterations NUMBER\n",
    " -O, --list-fields               output all field names, then exit\n",
    " -o, --sort-override =FIELD      force sorting on this named FIELD\n",
    " -p, --pid =PIDLIST              monitor only the tasks in PIDLIST\n",
    " -S, --accum-time-toggle         reverse last remembered 'S' state\n",
    " -s, --secure-mode               run with secure mode restrictions\n",
    " -U, --filter-any-user =USER     show only processes owned by USER\n",
    " -u, --filter-only-euser =USER   show only processes owned by USER\n",
    " -w, --width [=COLUMNS]          change print width [,use COLUMNS]\n",
    " -1, --single-cpu-toggle         reverse last remembered '1' state\n",
    "\n",
    " -h, --help                      display this help text, then exit\n",
    " -V, --version                   output version information & exit\n",
    "\n",
    "For more details see top(1)."
);
pub const BAD_DELAYINT_FMT: &str = "bad delay interval '%s'";
pub const BAD_NITERATE_FMT: &str = "bad iterations argument '%s'";
pub const LIMIT_EXCEED_FMT: &str = "pid limit (%d) exceeded";
pub const BAD_MON_PIDS_FMT: &str = "bad pid '%s'";
pub const MISSING_ARGS_FMT: &str = "-%c argument missing";
pub const BAD_WIDTHARG_FMT: &str = "bad width arg '%s'";
pub const UNKNOWN_OPTS_FMT: &str = "unknown option '%s'";
pub const DELAY_SECURE_TXT: &str = "-d disallowed in \"secure\" mode";
pub const DELAY_BADARG_TXT: &str = "-d requires positive argument";
pub const ON_WORD_ONLY_TXT: &str = "On";
pub const OFF_ONE_WORD_TXT: &str = "Off";
pub const VERSION_OPTS_FMT: &str = "%s from %s";
pub const FOREST_MODES_FMT: &str = "Forest mode %s";
pub const FAIL_TTY_GET_TXT: &str = "failed tty get";
pub const FAIL_TTY_SET_FMT: &str = "failed tty set: %s";
pub const CHOOSE_GROUP_TXT: &str = "Choose field group (1 - 4)";
pub const DISABLED_CMD_TXT: &str = "Command disabled, 'A' mode required";
pub const DISABLED_WIN_FMT: &str = "Command disabled, activate %s with '-' or '_'";
pub const COLORS_NOMAP_TXT: &str = "No colors to map!";
pub const FAIL_RC_OPEN_FMT: &str = "Failed '%s' open: %s";
pub const WRITE_RCFILE_FMT: &str = "Wrote configuration to '%s'";
pub const DELAY_CHANGE_FMT: &str = "Change delay from %.1f to";
pub const THREADS_SHOW_FMT: &str = "Show threads %s";
pub const IRIX_CURMODE_FMT: &str = "Irix mode %s";
pub const GET_PID2KILL_FMT: &str = "PID to signal/kill [default pid = %d]";
pub const GET_SIGS_NUM_FMT: &str = "Send pid %d signal [%d/sigterm]";
pub const FAIL_SIGNALS_FMT: &str = "Failed signal pid '%d' with '%d': %s";
pub const BAD_SIGNALID_TXT: &str = "Invalid signal";
pub const GET_PID2NICE_FMT: &str = "PID to renice [default pid = %d]";
pub const GET_NICE_NUM_FMT: &str = "Renice PID %d to value";
pub const FAIL_RE_NICE_FMT: &str = "Failed renice of PID %d to %d: %s";
pub const NAME_WINDOWS_FMT: &str = "Rename window '%s' to (1-3 chars)";
pub const TIME_ACCUMED_FMT: &str = "Cumulative time %s";
pub const GET_MAX_TASK_FMT: &str = "Maximum tasks = %d, change to (0 is unlimited)";
pub const BAD_MAX_TASK_TXT: &str = "Invalid maximum";
pub const GET_USER_IDS_TXT: &str = "Which user (blank for all)";
pub const UNKNOWN_CMDS_TXT: &str = "Unknown command - try 'h' for help";
pub const SCROLL_COORD_FMT: &str = "scroll coordinates: y = %d/%d (tasks), x = %d/%d (fields)";
pub const BAD_NUMFLOAT_TXT: &str = "Unacceptable floating point";
pub const BAD_USERNAME_TXT: &str = "Invalid user";
pub const FOREST_VIEWS_TXT: &str = "forest view";
pub const FAIL_WIDEPID_TXT: &str = "failed pid maximum size test";
pub const FAIL_WIDECPU_TXT: &str = "failed number of cpus test";
pub const RC_BAD_FILES_FMT: &str = "incompatible rcfile, you should delete '%s'";
pub const RC_BAD_ENTRY_FMT: &str = "window entry #%d corrupt, please delete '%s'";
pub const NOT_ONSECURE_TXT: &str = "Unavailable in secure mode";
pub const NOT_SMP_CPUS_TXT: &str = "Only 1 cpu detected";
pub const BAD_INTEGERS_TXT: &str = "Unacceptable integer";
pub const SELECT_CLASH_TXT: &str = "conflicting process selections (U/p/u)";
pub const AMT_KILOBYTE_TXT: &str = "KiB";
pub const AMT_MEGABYTE_TXT: &str = "MiB";
pub const AMT_GIGABYTE_TXT: &str = "GiB";
pub const AMT_TERABYTE_TXT: &str = "TiB";
pub const AMT_PETABYTE_TXT: &str = "PiB";
pub const AMT_EXXABYTE_TXT: &str = "EiB";
pub const WORD_THREADS_TXT: &str = "Threads";
pub const WORD_PROCESS_TXT: &str = "Tasks";
pub const WORD_ALLCPUS_TXT: &str = "%Cpu(s):";
pub const WORD_EACHCPU_FMT: &str = "%%Cp%c%-3d:";
pub const WORD_ANOTHER_TXT: &str = "another ";
pub const FIND_NO_NEXT_TXT: &str = "Locate next inactive, use \"L\"";
pub const GET_FIND_STR_TXT: &str = "Locate string";
pub const FIND_NO_FIND_FMT: &str = "%s\"%s\" not found";
pub const XTRA_FIXWIDE_FMT: &str = "width incr is %d, change to (0 default, -1 auto)";
pub const XTRA_WARNCFG_TXT: &str = "rcfile has inspect/other-filter error(s), save anyway?";
pub const XTRA_BADFLDS_FMT: &str = "unrecognized field name '%s'";
pub const XTRA_WINSIZE_TXT: &str = "even using field names only, window is now too small";
pub const YINSP_DEMO01_TXT: &str = "Open Files";
pub const YINSP_DEMO02_TXT: &str = "NUMA Info";
pub const YINSP_DEMO03_TXT: &str = "Log";
pub const YINSP_DEQFMT_TXT: &str =
    "the '=' key will eventually show the actual file read or command(s) executed ...";
pub const YINSP_DEQTYP_TXT: &str = "demo";
pub const YINSP_DSTORY_TXT: &str = concat!(
    "This is simulated output representing the contents of some file or the output\n",
    "from some command.  Exactly which commands and/or files are solely up to you.\n",
    "\n",
    "Although this text is for information purposes only, it can still be scrolled\n",
    "and searched like real output will be.  You are encouraged to experiment with\n",
    "those features as explained in the prologue above.\n",
    "\n",
    "To enable real Inspect functionality, entries must be added to the end of the\n",
    "top personal personal configuration file.  You could use your favorite editor\n",
    "to accomplish this, taking care not to disturb existing entries.\n",
    "\n",
    "Another way to add entries is illustrated below, but it risks overwriting the\n",
    "rcfile.  Redirected echoes must not replace (>) but append (>>) to that file.\n",
    "\n",
    "  /bin/echo -e \"pipe\\tOpen Files\\tlsof -P -p %d 2>&1\" >> ~/.toprc\n",
    "  /bin/echo -e \"file\\tNUMA Info\\t/proc/%d/numa_maps\" >> ~/.toprc\n",
    "  /bin/echo -e \"pipe\\tLog\\ttail -n200 /var/log/syslog | sort -Mr\" >> ~/.toprc\n",
    "\n",
    "If you don't know the location or name of the top rcfile, use the 'W' command\n",
    "and note those details.  After backing up the current rcfile, try issuing the\n",
    "above echoes exactly as shown, replacing '.toprc' as appropriate.  The safest\n",
    "approach would be to use copy then paste to avoid any typing mistakes.\n",
    "\n",
    "Finally, restart top to reveal what actual Inspect entries combined with this\n",
    "new command can offer.  The possibilities are endless, especially considering\n",
    "that 'pipe' type entries can include shell scripts too!\n",
    "\n",
    "For additional important information, please consult the top(1) man document.\n",
    "Then, enhance top with your very own customized 'file' and/or 'pipe' entries.\n",
    "\n",
    "Enjoy!\n"
);
pub const YINSP_NOENT1_TXT: &str = "to enable 'Y' press <Enter> then type 'W' and restart top";
pub const YINSP_FAILED_FMT: &str = "Selection failed with: %s\n";
pub const YINSP_PIDBAD_FMT: &str = "unable to inspect, pid %d not found";
pub const YINSP_PIDSEE_FMT: &str = "inspect at PID [default pid = %d]";
pub const YINSP_STATUS_FMT: &str = "%s: %*d-%-*d lines, %*d-%*d columns, %lu bytes read";
pub const YINSP_WAITIN_TXT: &str = "patience please, working ...";
pub const YINSP_WORKIN_TXT: &str = "working, use Ctrl-C to end ...";
pub const OSEL_PROMPTS_FMT: &str = "add filter #%d (%s) as: [!]FLD?VAL";
pub const OSEL_CASENOT_TXT: &str = "ignoring case";
pub const OSEL_CASEYES_TXT: &str = "case sensitive";
pub const OSEL_ERRDUPS_TXT: &str = "duplicate filter was ignored";
pub const OSEL_ERRDELM_FMT: &str = "'%s' filter delimiter is missing";
pub const OSEL_ERRVALU_FMT: &str = "'%s' filter value is missing";
pub const WORD_INCLUDE_TXT: &str = "include";
pub const WORD_EXCLUDE_TXT: &str = "exclude";
pub const OSEL_STATLIN_FMT: &str = "<Enter> to resume, filters: %s";
pub const WORD_NONEONE_TXT: &str = "none";
pub const NUMA_NODENAM_FMT: &str = "%%Node%-2d:";
pub const NUMA_NODEGET_FMT: &str = "expand which numa node (0-%d)";
pub const NUMA_NODEBAD_TXT: &str = "invalid numa node";
pub const NUMA_NODENOT_TXT: &str = "sorry, NUMA extensions unavailable";
pub const WORD_ABV_MEM_TXT: &str = "Mem ";
pub const WORD_ABV_SWP_TXT: &str = "Swap";
pub const LIB_ERRORMEM_FMT: &str = "library failed memory statistics, at %d: %s";
pub const LIB_ERRORCPU_FMT: &str = "library failed cpu statistics, at %d: %s";
pub const LIB_ERRORPID_FMT: &str = "library failed pids statistics, at %d: %s";
pub const BAD_MEMSCALE_FMT: &str = "bad memory scaling arg '%s'";
pub const XTRA_MODEBAD_TXT: &str = "wrong mode, command inactive";
pub const XTRA_WARNOLD_TXT: &str = "saving prevents older top from reading, save anyway?";
pub const X_RESTRICTED_TXT: &str = "sorry, restricted namespace with reduced functionality";
pub const AGNI_VALUEOF_FMT: &str = "set pid %d AGNI value to";
pub const AGNI_INVALID_TXT: &str = "valid AGNI range is -20 to +19";
pub const AGNI_NOTOPEN_FMT: &str = "autogroup open failed, %s";
pub const AGNI_NOWRITE_FMT: &str = "autogroup write failed, %s";
pub const X_BOT_CMDLIN_FMT: &str = "command line for pid %d, %s";
pub const X_BOT_CTLGRP_FMT: &str = "control groups for pid %d, %s";
pub const X_BOT_ENVIRN_FMT: &str = "environment for pid %d, %s";
pub const X_BOT_NAMESP_FMT: &str = "namespaces for pid %d, %s";
pub const X_BOT_NODATA_TXT: &str = "n/a";
pub const X_BOT_SUPGRP_FMT: &str = "supplementary groups for pid %d, %s";
pub const X_BOT_MSGLOG_TXT: &str = "message log, last 10 messages:";

// --- Uniq_nlstab --------------------------------------------------------------

pub const KEYS_HELPBAS_FMT: &str = concat!(
    "Help for Interactive Commands~2 - %s\n",
    "Window ~1%s~6: ~1Cumulative mode ~3%s~2.  ~1System~6: ~1Delay ~3%.1f secs~2; ~1Secure mode ~3%s~2.\n",
    "\n",
    "  Z~5,~1B~5,E,e   Global: '~1Z~2' colors; '~1B~2' bold; '~1E~2'/'~1e~2' summary/task memory scale\n",
    "  l,t,m,I,0 Toggle: '~1l~2' load avg; '~1t~2' task/cpu; '~1m~2' memory; '~1I~2' Irix; '~10~2' zeros\n",
    "  1,2,3,4,5 Toggle: '~11~2/~12~2/~13~2' cpu/numa views; '~14~2' cpus abreast; '~15~2' P/E-cores\n",
    "  f,X       Fields: '~1f~2' add/remove/order/sort; '~1X~2' increase fixed-width fields\n",
    "\n",
    "  L,&,<,> . Locate: '~1L~2'/'~1&~2' find/again; Move sort column: '~1<~2'/'~1>~2' left/right\n",
    "  R,H,J,C . Toggle: '~1R~2' Sort; '~1H~2' Threads; '~1J~2' Num justify; '~1C~2' Coordinates\n",
    "  c,i,S,j . Toggle: '~1c~2' Cmd name/line; '~1i~2' Idle; '~1S~2' Time; '~1j~2' Str justify\n",
    "  x~5,~1y~5     . Toggle highlights: '~1x~2' sort field; '~1y~2' running tasks\n",
    "  z~5,~1b~5     . Toggle: '~1z~2' color/mono; '~1b~2' bold/reverse (only if 'x' or 'y')\n",
    "  u,U,o,O . Filter by: '~1u~2'/'~1U~2' effective/any user; '~1o~2'/'~1O~2' other criteria\n",
    "  n,#,^O  . Set: '~1n~2'/'~1#~2' max tasks displayed; Show: ~1Ctrl~2+'~1O~2' other filter(s)\n",
    "  V,v,F   . Toggle: '~1V~2' forest view; '~1v~2' hide/show children; '~1F~2' keep focused\n",
    "\n",
    "%s",
    "  ^G,K,N,U  View: ctl groups ~1^G~2; cmdline ~1^K~2; environment ~1^N~2; supp groups ~1^U~2\n",
    "  Y,!,^E,P  Inspect '~1Y~2'; Combine Cpus '~1!~2'; Scale time ~1^E~2; View namespaces ~1^P~2\n",
    "  W,q       Write config file '~1W~2'; Quit '~1q~2'\n",
    "          ( commands shown with '.' require a ~1visible~2 task display ~1window~2 ) \n",
    "Press '~1h~2' or '~1?~2' for help with ~1Windows~2,\n",
    "Type 'q' or <Esc> to continue "
);

pub const WINDOWS_HELP_FMT: &str = concat!(
    "Help for Windows / Field Groups~2 - \"Current Window\" = ~1 %s ~6\n",
    "\n",
    ". Use multiple ~1windows~2, each with separate config opts (color,fields,sort,etc)\n",
    ". The 'current' window controls the ~1Summary Area~2 and responds to your ~1Commands~2\n",
    "  . that window's ~1task display~2 can be turned ~1Off~2 & ~1On~2, growing/shrinking others\n",
    "  . with ~1NO~2 task display, some commands will be ~1disabled~2 ('i','R','n','c', etc)\n",
    "    until a ~1different window~2 has been activated, making it the 'current' window\n",
    ". You ~1change~2 the 'current' window by: ~1 1~2) cycling forward/backward;~1 2~2) choosing\n",
    "  a specific field group; or~1 3~2) exiting the color mapping or fields screens\n",
    ". Commands ~1available anytime   -------------~2\n",
    "    A       . Alternate display mode toggle, show ~1Single~2 / ~1Multiple~2 windows\n",
    "    g       . Choose another field group and make it 'current', or change now\n",
    "              by selecting a number from: ~1 1~2 =%s;~1 2~2 =%s;~1 3~2 =%s; or~1 4~2 =%s\n",
    ". Commands ~1requiring~2 '~1A~2' mode~1  -------------~2\n",
    "    G       . Change the ~1Name~5 of the 'current' window/field group\n",
    " ~1*~4  a , w   . Cycle through all four windows:  '~1a~5' Forward; '~1w~5' Backward\n",
    " ~1*~4  - , _   . Show/Hide:  '~1-~5' ~1Current~2 window; '~1_~5' all ~1Visible~2/~1Invisible~2\n",
    "  The screen will be divided evenly between task displays.  But you can make\n",
    "  some ~1larger~2 or ~1smaller~2, using '~1n~2' and '~1i~2' commands.  Then later you could:\n",
    " ~1*~4  = , +   . Rebalance tasks:  '~1=~5' ~1Current~2 window; '~1+~5' ~1Every~2 window\n",
    "              (this also forces the ~1current~2 or ~1every~2 window to become visible)\n",
    "\n",
    "In '~1A~2' mode, '~1*~4' keys are your ~1essential~2 commands.  Please try the '~1a~2' and '~1w~2'\n",
    "commands plus the 'g' sub-commands NOW.  Press <Enter> to make 'Current' "
);

pub const COLOR_CUSTOM_FMT: &str = concat!(
    "Help for color mapping~2 - \"Current Window\" = ~1 %s ~6\n",
    "\n",
    "   color - 04:25:44 up 8 days, 50 min,  7 users,  load average:\n",
    "   Tasks:~3  64 ~2total,~3   2 ~3running,~3  62 ~2sleeping,~3   0 ~2stopped,~3\n",
    "   %%Cpu(s):~3  76.5 ~2user,~3  11.2 ~2system,~3   0.0 ~2nice,~3  12.3 ~2idle~3\n",
    "   ~1 Nasty Message! ~4  -or-  ~1Input Prompt~5\n",
    "   ~1  PID TTY     PR  NI %%CPU    TIME+   VIRT SWAP S COMMAND    ~6\n",
    "   17284 ~8pts/2  ~7  8   0  0.0   0:00.75  1380    0 S /bin/bash   ~8\n",
    "   ~1 8601 pts/1    7 -10  0.4   0:00.03   916    0 R color -b -z~7\n",
    "   11005 ~8?      ~7  9   0  0.0   0:02.50  2852 1008 S amor -sessi~8\n",
    "   available toggles: ~1B~2 =disable bold globally (~1%s~2),\n",
    "       ~1z~2 =color/mono (~1%s~2), ~1b~2 =tasks \"bold\"/reverse (~1%s~2)\n",
    "\n",
    "1) Select a ~1target~2 as an upper case letter, ~1current target~2 is ~1 %c ~4:\n",
    "   S~2 = Summary Data,~1  M~2 = Messages/Prompts,\n",
    "   H~2 = Column Heads,~1  T~2 = Task Information\n",
    "2) Select a ~1color~2 as a number or use the up/down arrow keys\n",
    "   to raise/lower the %d colors value, ~1current color~2 is ~1 %d ~4:\n",
    "   0~2 = black,~1  1~2 = red,    ~1  2~2 = green,~1  3~2 = yellow,\n",
    "   4~2 = blue, ~1  5~2 = magenta,~1  6~2 = cyan, ~1  7~2 = white\n",
    "\n",
    "3) Then use these keys when finished:\n",
    "   'q' or <Esc> to abort changes to window '~1%s~2'\n",
    "   'a' or 'w' to commit & change another, <Enter> to commit and end "
);

pub const KEYS_HELPEXT_FMT: &str = "  d,k,r,^R '~1d~2' set delay; '~1k~2' kill; '~1r~2' renice; ~1Ctrl~2+'~1R~2' renice autogroup\n";

pub const FIELD_HEADER_FMT: &str = concat!(
    "Fields Management~2 for window ~1%s~6, whose current sort field is ~1%s~2\n",
    "   Navigate with Up/Dn, Right selects for move then <Enter> or Left commits,\n",
    "   'd' or <Space> toggles display, 's' sets sort.  Use 'q' or <Esc> to end!\n"
);

pub const STATE_LINE_1_FMT: &str = concat!(
    "%s:~3",
    " %3u ~2total,~3 %3u ~2running,~3 %3u ~2sleeping,~3 %3u ~2stopped,~3 %3u ~2zombie~3\n"
);

pub const STATE_LIN2X6_FMT: &str = concat!(
    "%s~3",
    " %#5.1f ~2us,~3 %#5.1f ~2sy,~3 %#5.1f ~2ni,~3 %#5.1f ~2id,~3 %#5.1f ~2wa,~3 %#5.1f ~2hi,~3 %#5.1f ~2si~3 ~1"
);

pub const STATE_LIN2X7_FMT: &str = concat!(
    "%s~3",
    "%#5.1f ~2us,~3%#5.1f ~2sy,~3%#5.1f ~2ni,~3%#5.1f ~2id,~3%#5.1f ~2wa,~3%#5.1f ~2hi,~3%#5.1f ~2si,~3%#5.1f ~2st~3 ~1"
);

pub const MEMORY_LINE1_FMT: &str =
    "%s %s:~3 %9.9s~2total,~3 %9.9s~2free,~3 %9.9s~2used,~3 %9.9s~2buff/cache~3 ~1    ";
pub const MEMORY_LINE2_FMT: &str =
    "%s %s:~3 %9.9s~2total,~3 %9.9s~2free,~3 %9.9s~2used.~3 %9.9s~2avail %s~3";

pub const YINSP_HDSELS_FMT: &str = concat!(
    "Inspection~2 Pause at: pid ~1%d~6, running ~1%s~6\n",
    "Use~2:  left/right then <Enter> to ~1select~5 an option; 'q' or <Esc> to ~1end~5 !\n",
    "Options~2: ~1%s\n"
);

pub const YINSP_HDVIEW_FMT: &str = concat!(
    "Inspection~2 View at pid: ~1%s~3, running ~1%s~3.  Locating: ~1%s~6\n",
    "Use~2:  left/right/up/down/etc to ~1navigate~5 the output; 'L'/'&' to ~1locate~5/~1next~5.\n",
    "Or~2:   <Enter> to ~1select another~5; 'q' or <Esc> to ~1end~5 !\n"
);
