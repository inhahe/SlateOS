// OurOS modprobe - kernel module management tools
//
// Multi-personality binary:
//   modprobe  - load/unload kernel modules with dependency resolution
//   insmod    - insert a kernel module (no dependency handling)
//   rmmod     - remove a kernel module
//   lsmod     - list loaded kernel modules
//   modinfo   - show information about a kernel module
//   depmod    - generate module dependency database
//
// Many constants, struct fields, and associated `new` constructors below
// are declared for the kmod / modules.dep / modules.alias / modprobe.conf
// ABI vocabulary but are not exercised yet — they're kept for the
// upcoming real depmod and config parsers. Allow dead_code file-wide so
// the protocol surface stays visible without warning spam.
#![allow(dead_code)]
#![cfg_attr(not(test), no_main)]

// ── Constants ──────────────────────────────────────────────────────────

const MAX_MODULES: usize = 512;
const MAX_DEPS: usize = 32;
const MAX_NAME_LEN: usize = 64;
const MAX_PATH_LEN: usize = 256;
const MAX_PARAM_LEN: usize = 128;
const MAX_LINE_LEN: usize = 512;
const MAX_PARAMS: usize = 32;
const MAX_ALIASES: usize = 64;

// Module states
const MOD_LIVE: u8 = 0;
const MOD_LOADING: u8 = 1;
const MOD_UNLOADING: u8 = 2;

// Module info section names
const INFO_LICENSE: &[u8] = b"license";
const INFO_AUTHOR: &[u8] = b"author";
const INFO_DESCRIPTION: &[u8] = b"description";
const INFO_VERSION: &[u8] = b"version";
const INFO_ALIAS: &[u8] = b"alias";
const INFO_DEPENDS: &[u8] = b"depends";
const INFO_FIRMWARE: &[u8] = b"firmware";
const INFO_PARM: &[u8] = b"parm";

// Config paths
const MODULES_DIR: &[u8] = b"/lib/modules";
const MODULES_DEP: &[u8] = b"modules.dep";
const MODULES_ALIAS: &[u8] = b"modules.alias";
const MODULES_SYMBOLS: &[u8] = b"modules.symbols";
const MODPROBE_CONF: &[u8] = b"/etc/modprobe.d";

// ── Output Helpers ─────────────────────────────────────────────────────

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

// ── Data Types ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Modprobe,
    Insmod,
    Rmmod,
    Lsmod,
    Modinfo,
    Depmod,
}

#[derive(Clone, Copy)]
struct ModuleName {
    data: [u8; MAX_NAME_LEN],
    len: usize,
}

impl ModuleName {
    fn new() -> Self {
        Self {
            data: [0u8; MAX_NAME_LEN],
            len: 0,
        }
    }

    fn from_bytes(s: &[u8]) -> Self {
        let mut m = Self::new();
        let copy_len = s.len().min(MAX_NAME_LEN);
        m.data[..copy_len].copy_from_slice(&s[..copy_len]);
        m.len = copy_len;
        m
    }

    fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }
}

#[derive(Clone, Copy)]
struct ModulePath {
    data: [u8; MAX_PATH_LEN],
    len: usize,
}

impl ModulePath {
    fn new() -> Self {
        Self {
            data: [0u8; MAX_PATH_LEN],
            len: 0,
        }
    }

    fn from_bytes(s: &[u8]) -> Self {
        let mut m = Self::new();
        let copy_len = s.len().min(MAX_PATH_LEN);
        m.data[..copy_len].copy_from_slice(&s[..copy_len]);
        m.len = copy_len;
        m
    }

    fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

#[derive(Clone, Copy)]
struct ModuleParam {
    name: [u8; MAX_PARAM_LEN],
    name_len: usize,
    value: [u8; MAX_PARAM_LEN],
    value_len: usize,
}

impl ModuleParam {
    fn new() -> Self {
        Self {
            name: [0u8; MAX_PARAM_LEN],
            name_len: 0,
            value: [0u8; MAX_PARAM_LEN],
            value_len: 0,
        }
    }
}

/// Represents a loaded kernel module
#[derive(Clone, Copy)]
struct LoadedModule {
    name: ModuleName,
    size: u64,     // size in bytes
    refcount: u32, // reference count
    state: u8,     // MOD_LIVE, MOD_LOADING, MOD_UNLOADING
    used_by: [ModuleName; 8],
    used_by_count: usize,
}

impl LoadedModule {
    fn new() -> Self {
        Self {
            name: ModuleName::new(),
            size: 0,
            refcount: 0,
            state: MOD_LIVE,
            used_by: [ModuleName::new(); 8],
            used_by_count: 0,
        }
    }
}

/// Module information from .ko file or modinfo section
#[derive(Clone)]
struct ModuleInfo {
    name: ModuleName,
    path: ModulePath,
    filename: ModulePath,
    license: [u8; 64],
    license_len: usize,
    author: [u8; 128],
    author_len: usize,
    description: [u8; 256],
    desc_len: usize,
    version: [u8; 32],
    version_len: usize,
    depends: [ModuleName; MAX_DEPS],
    dep_count: usize,
    aliases: [[u8; 64]; MAX_ALIASES],
    alias_count: usize,
    params: [ModuleParam; MAX_PARAMS],
    param_count: usize,
    firmware: [[u8; 64]; 8],
    firmware_count: usize,
    srcversion: [u8; 32],
    srcver_len: usize,
}

impl ModuleInfo {
    fn new() -> Self {
        Self {
            name: ModuleName::new(),
            path: ModulePath::new(),
            filename: ModulePath::new(),
            license: [0u8; 64],
            license_len: 0,
            author: [0u8; 128],
            author_len: 0,
            description: [0u8; 256],
            desc_len: 0,
            version: [0u8; 32],
            version_len: 0,
            depends: [ModuleName::new(); MAX_DEPS],
            dep_count: 0,
            aliases: [[0u8; 64]; MAX_ALIASES],
            alias_count: 0,
            params: [ModuleParam::new(); MAX_PARAMS],
            param_count: 0,
            firmware: [[0u8; 64]; 8],
            firmware_count: 0,
            srcversion: [0u8; 32],
            srcver_len: 0,
        }
    }
}

/// Dependency database entry
#[derive(Clone, Copy)]
struct DepEntry {
    name: ModuleName,
    deps: [ModuleName; MAX_DEPS],
    dep_count: usize,
}

impl DepEntry {
    fn new() -> Self {
        Self {
            name: ModuleName::new(),
            deps: [ModuleName::new(); MAX_DEPS],
            dep_count: 0,
        }
    }
}

/// Alias entry
#[derive(Clone, Copy)]
struct AliasEntry {
    alias: [u8; 64],
    alias_len: usize,
    module: ModuleName,
}

impl AliasEntry {
    fn new() -> Self {
        Self {
            alias: [0u8; 64],
            alias_len: 0,
            module: ModuleName::new(),
        }
    }
}

/// Configuration entry (from /etc/modprobe.d/)
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigAction {
    Alias,
    Options,
    Install,
    Remove,
    Blacklist,
    Softdep,
}

#[derive(Clone, Copy)]
struct ConfigEntry {
    action: ConfigAction,
    module: ModuleName,
    value: [u8; 256],
    value_len: usize,
}

impl ConfigEntry {
    fn new() -> Self {
        Self {
            action: ConfigAction::Alias,
            module: ModuleName::new(),
            value: [0u8; 256],
            value_len: 0,
        }
    }
}

// ── Options ────────────────────────────────────────────────────────────

struct ModprobeOpts {
    tool: Tool,
    module_name: ModuleName,
    module_path: ModulePath, // for insmod: full path to .ko
    remove: bool,            // -r for modprobe
    force: bool,             // -f
    verbose: bool,           // -v
    dry_run: bool,           // -n
    quiet: bool,             // -q
    show_depends: bool,      // -D / --show-depends
    first_time: bool,        // --first-time
    ignore_install: bool,    // -i
    all: bool,               // -a (all matching)
    params: [ModuleParam; MAX_PARAMS],
    param_count: usize,
    // depmod specific
    depmod_all: bool,          // -a for depmod
    depmod_quick: bool,        // -A
    depmod_config: ModulePath, // -C
    // modinfo specific
    modinfo_field: [u8; 64],
    modinfo_field_len: usize,
    modinfo_null: bool, // -0
}

impl ModprobeOpts {
    fn new(tool: Tool) -> Self {
        Self {
            tool,
            module_name: ModuleName::new(),
            module_path: ModulePath::new(),
            remove: false,
            force: false,
            verbose: false,
            dry_run: false,
            quiet: false,
            show_depends: false,
            first_time: false,
            ignore_install: false,
            all: false,
            params: [ModuleParam::new(); MAX_PARAMS],
            param_count: 0,
            depmod_all: false,
            depmod_quick: false,
            depmod_config: ModulePath::new(),
            modinfo_field: [0u8; 64],
            modinfo_field_len: 0,
            modinfo_null: false,
        }
    }
}

// ── String/Number Helpers ──────────────────────────────────────────────

unsafe fn cstr_to_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return b"";
    }
    let mut len = 0usize;
    // SAFETY: Walking null-terminated C string from kernel/libc
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len >= 4096 {
                break;
            }
        }
        core::slice::from_raw_parts(ptr, len)
    }
}

fn format_u64(val: u64, buf: &mut [u8]) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = val;
    let mut i = 0;
    while n > 0 {
        if let Some(slot) = tmp.get_mut(i) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        i += 1;
    }
    let len = i.min(buf.len());
    for j in 0..len {
        if let (Some(dst), Some(src)) = (buf.get_mut(j), tmp.get(i - 1 - j)) {
            *dst = *src;
        }
    }
    len
}

fn copy_bytes(dst: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let mut p = pos;
    for &c in src {
        if p < dst.len() {
            dst[p] = c;
            p += 1;
        }
    }
    p
}

fn pad_right(buf: &mut [u8], start: usize, width: usize) -> usize {
    let mut pos = start;
    while pos < width && pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    pos
}

fn pad_left_num(val: u64, buf: &mut [u8], width: usize) -> usize {
    let mut tmp = [0u8; 20];
    let n = format_u64(val, &mut tmp);
    let mut pos = 0;
    if n < width {
        let pad = width - n;
        while pos < pad && pos < buf.len() {
            buf[pos] = b' ';
            pos += 1;
        }
    }
    for j in 0..n {
        if pos < buf.len()
            && let Some(c) = tmp.get(j) {
                buf[pos] = *c;
                pos += 1;
            }
    }
    pos
}

/// Normalize module name: replace '-' with '_', strip path/extension
fn normalize_module_name(name: &[u8]) -> ModuleName {
    // Strip path: take only the last component
    let mut start = 0;
    for (i, &b) in name.iter().enumerate() {
        if b == b'/' {
            start = i + 1;
        }
    }
    let basename = &name[start..];

    // Strip .ko, .ko.gz, .ko.xz, .ko.zst extension
    let mut end = basename.len();
    if ends_with(basename, b".ko.zst") {
        end -= 7;
    } else if ends_with(basename, b".ko.xz") || ends_with(basename, b".ko.gz") {
        end -= 6;
    } else if ends_with(basename, b".ko") {
        end -= 3;
    }

    let stripped = &basename[..end];

    // Replace '-' with '_'
    let mut result = ModuleName::new();
    let copy_len = stripped.len().min(MAX_NAME_LEN);
    for (slot, &b) in result.data.iter_mut().zip(stripped.iter()).take(copy_len) {
        *slot = if b == b'-' { b'_' } else { b };
    }
    result.len = copy_len;
    result
}

fn ends_with(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    &haystack[haystack.len() - needle.len()..] == needle
}

fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    &haystack[..needle.len()] == needle
}

fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}

fn format_size(size_bytes: u64, buf: &mut [u8]) -> usize {
    let (val, suffix): (u64, &[u8]) = if size_bytes >= 1024 * 1024 {
        (size_bytes / (1024 * 1024), b"M")
    } else if size_bytes >= 1024 {
        (size_bytes / 1024, b"K")
    } else {
        (size_bytes, b"B")
    };
    let mut pos = format_u64(val, buf);
    pos = copy_bytes(buf, pos, suffix);
    pos
}

// ── Tool Detection ─────────────────────────────────────────────────────

fn detect_tool(argv0: &[u8]) -> Tool {
    // Find the last path component
    let mut start = 0;
    for (i, &b) in argv0.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            start = i + 1;
        }
    }
    let name = &argv0[start..];

    if starts_with(name, b"insmod") {
        Tool::Insmod
    } else if starts_with(name, b"rmmod") {
        Tool::Rmmod
    } else if starts_with(name, b"lsmod") {
        Tool::Lsmod
    } else if starts_with(name, b"modinfo") {
        Tool::Modinfo
    } else if starts_with(name, b"depmod") {
        Tool::Depmod
    } else {
        Tool::Modprobe
    }
}

// ── Argument Parsing ───────────────────────────────────────────────────

fn parse_args(argc: i32, argv: *const *const u8) -> Result<ModprobeOpts, i32> {
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let argv0 = if !args.is_empty() {
        unsafe { cstr_to_slice(args[0]) }
    } else {
        b"modprobe"
    };

    let tool = detect_tool(argv0);
    let mut opts = ModprobeOpts::new(tool);

    let mut i = 1;
    while i < args.len() {
        let arg = unsafe { cstr_to_slice(args[i]) };

        match tool {
            Tool::Lsmod => {
                // lsmod takes no arguments
                if arg == b"--help" || arg == b"-h" {
                    show_lsmod_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                }
            }
            Tool::Insmod => {
                if arg == b"--help" || arg == b"-h" {
                    show_insmod_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-f" || arg == b"--force" {
                    opts.force = true;
                } else if opts.module_path.len == 0 {
                    opts.module_path = ModulePath::from_bytes(arg);
                    opts.module_name = normalize_module_name(arg);
                } else {
                    // Module parameter: key=value or key
                    if opts.param_count < MAX_PARAMS {
                        parse_module_param(arg, &mut opts.params[opts.param_count]);
                        opts.param_count += 1;
                    }
                }
            }
            Tool::Rmmod => {
                if arg == b"--help" || arg == b"-h" {
                    show_rmmod_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-f" || arg == b"--force" {
                    opts.force = true;
                } else if arg == b"-w" || arg == b"--wait" {
                    // Wait for module to become unused (not implemented in stub)
                } else if !arg.is_empty() && arg[0] != b'-' {
                    opts.module_name = normalize_module_name(arg);
                }
            }
            Tool::Modinfo => {
                if arg == b"--help" || arg == b"-h" {
                    show_modinfo_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-F" || arg == b"--field" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"modinfo: -F requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    let flen = val.len().min(63);
                    opts.modinfo_field[..flen].copy_from_slice(&val[..flen]);
                    opts.modinfo_field_len = flen;
                } else if arg == b"-0" || arg == b"--null" {
                    opts.modinfo_null = true;
                } else if !arg.is_empty() && arg[0] != b'-' {
                    opts.module_name = normalize_module_name(arg);
                }
            }
            Tool::Depmod => {
                if arg == b"--help" || arg == b"-h" {
                    show_depmod_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-a" || arg == b"--all" {
                    opts.depmod_all = true;
                } else if arg == b"-A" {
                    opts.depmod_quick = true;
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-n" || arg == b"--dry-run" {
                    opts.dry_run = true;
                } else if arg == b"-C" || arg == b"--config" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"depmod: -C requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    opts.depmod_config = ModulePath::from_bytes(val);
                }
            }
            Tool::Modprobe => {
                if arg == b"--help" || arg == b"-h" {
                    show_modprobe_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-r" || arg == b"--remove" {
                    opts.remove = true;
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-n" || arg == b"--dry-run" || arg == b"--show" {
                    opts.dry_run = true;
                } else if arg == b"-q" || arg == b"--quiet" {
                    opts.quiet = true;
                } else if arg == b"-f" || arg == b"--force" {
                    opts.force = true;
                } else if arg == b"-D" || arg == b"--show-depends" {
                    opts.show_depends = true;
                } else if arg == b"--first-time" {
                    opts.first_time = true;
                } else if arg == b"-i" || arg == b"--ignore-install" {
                    opts.ignore_install = true;
                } else if arg == b"-a" || arg == b"--all" {
                    opts.all = true;
                } else if !arg.is_empty() && arg[0] != b'-' {
                    if opts.module_name.len == 0 {
                        opts.module_name = normalize_module_name(arg);
                    } else {
                        // Module parameter
                        if opts.param_count < MAX_PARAMS {
                            parse_module_param(arg, &mut opts.params[opts.param_count]);
                            opts.param_count += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }

    // Validate required arguments
    match tool {
        Tool::Lsmod => {}  // No args needed
        Tool::Depmod => {} // Defaults to -a behavior
        Tool::Insmod => {
            if opts.module_path.len == 0 {
                print_err(b"insmod: missing module path\n");
                return Err(1);
            }
        }
        Tool::Rmmod | Tool::Modinfo | Tool::Modprobe => {
            if opts.module_name.len == 0 && !opts.show_depends {
                let name = tool_name(tool);
                print_err(name);
                print_err(b": missing module name\n");
                return Err(1);
            }
        }
    }

    Ok(opts)
}

fn parse_module_param(arg: &[u8], param: &mut ModuleParam) {
    // Split on '='
    let eq_pos = arg.iter().position(|&b| b == b'=');

    if let Some(pos) = eq_pos {
        let name = &arg[..pos];
        let value = &arg[pos + 1..];
        let nlen = name.len().min(MAX_PARAM_LEN);
        param.name[..nlen].copy_from_slice(&name[..nlen]);
        param.name_len = nlen;
        let vlen = value.len().min(MAX_PARAM_LEN);
        param.value[..vlen].copy_from_slice(&value[..vlen]);
        param.value_len = vlen;
    } else {
        let nlen = arg.len().min(MAX_PARAM_LEN);
        param.name[..nlen].copy_from_slice(&arg[..nlen]);
        param.name_len = nlen;
        // Boolean parameter (no value)
        param.value[0] = b'1';
        param.value_len = 1;
    }
}

fn tool_name(tool: Tool) -> &'static [u8] {
    match tool {
        Tool::Modprobe => b"modprobe",
        Tool::Insmod => b"insmod",
        Tool::Rmmod => b"rmmod",
        Tool::Lsmod => b"lsmod",
        Tool::Modinfo => b"modinfo",
        Tool::Depmod => b"depmod",
    }
}

// ── Help Messages ──────────────────────────────────────────────────────

fn show_version(tool: Tool) {
    print_out(tool_name(tool));
    print_out(b" version 0.1.0 (OurOS)\n");
}

fn show_modprobe_help() {
    print_out(b"Usage: modprobe [options] module [parameters...]\n\n");
    print_out(b"Load or unload kernel modules with dependency resolution.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -r, --remove        Remove module (and deps if unused)\n");
    print_out(b"  -v, --verbose       Show actions being taken\n");
    print_out(b"  -n, --dry-run       Don't actually load/unload\n");
    print_out(b"  -q, --quiet         Suppress error messages\n");
    print_out(b"  -f, --force         Force load (skip version checks)\n");
    print_out(b"  -D, --show-depends  Show dependencies only\n");
    print_out(b"  --first-time        Fail if module already loaded\n");
    print_out(b"  -i, --ignore-install Ignore install/remove commands in config\n");
    print_out(b"  -a, --all           Load all matching modules\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

fn show_insmod_help() {
    print_out(b"Usage: insmod [options] module.ko [parameters...]\n\n");
    print_out(b"Insert a kernel module (no dependency resolution).\n\n");
    print_out(b"Options:\n");
    print_out(b"  -v, --verbose       Show actions being taken\n");
    print_out(b"  -f, --force         Force insert (skip checks)\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

fn show_rmmod_help() {
    print_out(b"Usage: rmmod [options] module\n\n");
    print_out(b"Remove a kernel module.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -v, --verbose       Show actions being taken\n");
    print_out(b"  -f, --force         Force removal\n");
    print_out(b"  -w, --wait          Wait for module to become unused\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

fn show_lsmod_help() {
    print_out(b"Usage: lsmod\n\n");
    print_out(b"List currently loaded kernel modules.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

fn show_modinfo_help() {
    print_out(b"Usage: modinfo [options] module\n\n");
    print_out(b"Show information about a kernel module.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -F, --field FIELD   Show only specified field\n");
    print_out(b"  -0, --null          Use NUL instead of newline\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

fn show_depmod_help() {
    print_out(b"Usage: depmod [options]\n\n");
    print_out(b"Generate module dependency database.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -a, --all           Probe all modules\n");
    print_out(b"  -A                  Only update if modules changed\n");
    print_out(b"  -v, --verbose       Show dependencies as they're resolved\n");
    print_out(b"  -n, --dry-run       Print to stdout instead of writing files\n");
    print_out(b"  -C, --config DIR    Use alternate config directory\n");
    print_out(b"  -h, --help          Show this help\n");
    print_out(b"  -V, --version       Show version\n");
}

// ── Simulated Module Database ──────────────────────────────────────────

// In a real OS, these would read /proc/modules, /lib/modules/$(uname -r)/,
// and issue init_module()/delete_module() syscalls.

fn get_loaded_modules() -> ([LoadedModule; 32], usize) {
    let mut modules = [LoadedModule::new(); 32];
    let mut count = 0;

    // Simulated loaded module list. Tuple shape:
    // (name, size in bytes, use count, slice of dependency names).
    type SimModuleFixture<'a> = (&'a [u8], u64, u32, &'a [&'a [u8]]);
    let sim_modules: &[SimModuleFixture] = &[
        (b"ext4", 884736, 2, &[]),
        (b"mbcache", 16384, 1, &[b"ext4"]),
        (b"jbd2", 131072, 1, &[b"ext4"]),
        (b"crc16", 16384, 1, &[b"ext4"]),
        (b"virtio_blk", 24576, 0, &[]),
        (b"virtio_net", 65536, 0, &[]),
        (b"virtio_pci", 32768, 2, &[b"virtio_blk", b"virtio_net"]),
        (b"virtio_ring", 40960, 1, &[b"virtio_pci"]),
        (b"virtio", 20480, 1, &[b"virtio_ring"]),
        (b"e1000", 196608, 0, &[]),
        (b"ahci", 45056, 0, &[]),
        (b"libahci", 40960, 1, &[b"ahci"]),
        (b"xhci_hcd", 327680, 0, &[]),
        (b"usbcore", 327680, 1, &[b"xhci_hcd"]),
        (b"usb_common", 16384, 1, &[b"usbcore"]),
    ];

    for (name, size, refcnt, used_by) in sim_modules {
        if count >= 32 {
            break;
        }
        modules[count].name = ModuleName::from_bytes(name);
        modules[count].size = *size;
        modules[count].refcount = *refcnt;
        modules[count].state = MOD_LIVE;
        for (j, dep) in used_by.iter().enumerate() {
            if j < 8 {
                modules[count].used_by[j] = ModuleName::from_bytes(dep);
                modules[count].used_by_count += 1;
            }
        }
        count += 1;
    }

    (modules, count)
}

fn get_module_info(name: &[u8]) -> ModuleInfo {
    let mut info = ModuleInfo::new();
    info.name = normalize_module_name(name);

    // Build path
    let mut path_buf = [0u8; MAX_PATH_LEN];
    let mut pos = copy_bytes(&mut path_buf, 0, MODULES_DIR);
    pos = copy_bytes(&mut path_buf, pos, b"/0.1.0/kernel/");
    pos = copy_bytes(&mut path_buf, pos, info.name.as_bytes());
    pos = copy_bytes(&mut path_buf, pos, b".ko");
    info.path = ModulePath::from_bytes(&path_buf[..pos]);
    info.filename = ModulePath::from_bytes(&path_buf[..pos]);

    // Simulated module metadata based on name
    let normalized = info.name.as_bytes();
    if starts_with(normalized, b"ext4") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"ext4 filesystem driver",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        info.depends[0] = ModuleName::from_bytes(b"mbcache");
        info.depends[1] = ModuleName::from_bytes(b"jbd2");
        info.depends[2] = ModuleName::from_bytes(b"crc16");
        info.dep_count = 3;
    } else if starts_with(normalized, b"virtio_net") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"Virtio network driver",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        info.depends[0] = ModuleName::from_bytes(b"virtio_pci");
        info.dep_count = 1;
        // Aliases
        set_alias(
            &mut info.aliases,
            &mut info.alias_count,
            b"pci:v00001AF4d00001000*",
        );
        set_alias(
            &mut info.aliases,
            &mut info.alias_count,
            b"virtio:d00000001v*",
        );
    } else if starts_with(normalized, b"virtio_blk") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"Virtio block driver",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        info.depends[0] = ModuleName::from_bytes(b"virtio_pci");
        info.dep_count = 1;
    } else if starts_with(normalized, b"virtio_pci") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"Virtio PCI bus driver",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        // Mirror the canonical chain in cmd_depmod's sim_modules so that
        // resolve_dependencies can walk virtio_net -> virtio_pci -> virtio_ring -> virtio.
        info.depends[0] = ModuleName::from_bytes(b"virtio_ring");
        info.dep_count = 1;
    } else if starts_with(normalized, b"virtio_ring") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"Virtio ring buffer",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        info.depends[0] = ModuleName::from_bytes(b"virtio");
        info.dep_count = 1;
    } else if starts_with(normalized, b"e1000") {
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(
            &mut info.description,
            &mut info.desc_len,
            b"Intel PRO/1000 Network Driver",
        );
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
        set_alias(
            &mut info.aliases,
            &mut info.alias_count,
            b"pci:v00008086d0000100E*",
        );
        set_alias(
            &mut info.aliases,
            &mut info.alias_count,
            b"pci:v00008086d0000100F*",
        );
        // Parameters
        if info.param_count < MAX_PARAMS {
            let p = &mut info.params[info.param_count];
            set_buf(&mut p.name, &mut p.name_len, b"debug");
            set_buf(
                &mut p.value,
                &mut p.value_len,
                b"int:Debug level (0=none,...,16=all)",
            );
            info.param_count += 1;
        }
    } else {
        // Generic module info
        set_field(&mut info.license, &mut info.license_len, b"GPL");
        set_field(&mut info.author, &mut info.author_len, b"OurOS Project");
        set_field(&mut info.description, &mut info.desc_len, b"Kernel module");
        set_field(&mut info.version, &mut info.version_len, b"0.1.0");
    }

    info
}

fn set_field(dst: &mut [u8], dst_len: &mut usize, src: &[u8]) {
    let len = src.len().min(dst.len());
    dst[..len].copy_from_slice(&src[..len]);
    *dst_len = len;
}

fn set_buf(dst: &mut [u8], dst_len: &mut usize, src: &[u8]) {
    let len = src.len().min(dst.len());
    dst[..len].copy_from_slice(&src[..len]);
    *dst_len = len;
}

fn set_alias(aliases: &mut [[u8; 64]; MAX_ALIASES], count: &mut usize, val: &[u8]) {
    if *count < MAX_ALIASES {
        let len = val.len().min(63);
        aliases[*count][..len].copy_from_slice(&val[..len]);
        *count += 1;
    }
}

// ── Module Dependency Resolution ───────────────────────────────────────

fn resolve_dependencies(name: &[u8], deps: &mut [ModuleName; 64], dep_count: &mut usize) {
    // Start with the module's direct dependencies, then recurse
    let info = get_module_info(name);
    for i in 0..info.dep_count {
        let dep_name = info.depends[i].as_bytes();
        // Check if already in list
        let found = deps
            .iter()
            .take(*dep_count)
            .any(|d| d.eq_bytes(dep_name));
        if !found && *dep_count < 64 {
            deps[*dep_count] = info.depends[i];
            *dep_count += 1;
            // Recurse
            resolve_dependencies(dep_name, deps, dep_count);
        }
    }
}

// ── Command Implementations ────────────────────────────────────────────

fn cmd_lsmod() -> i32 {
    let (modules, count) = get_loaded_modules();

    // Header
    print_out(b"Module                  Size  Used by\n");

    let mut buf = [0u8; 512];
    for m in modules.iter().take(count) {
        let mut pos = 0;

        // Module name (24 chars left-aligned)
        pos = copy_bytes(&mut buf, pos, m.name.as_bytes());
        pos = pad_right(&mut buf, pos, 24);

        // Size (right-aligned in 8 chars)
        pos += pad_left_num(m.size, &mut buf[pos..], 8);
        pos = copy_bytes(&mut buf, pos, b"  ");

        // Refcount
        pos += format_u64(m.refcount as u64, &mut buf[pos..]);

        // Used by list
        if m.used_by_count > 0 {
            pos = copy_bytes(&mut buf, pos, b" ");
            for j in 0..m.used_by_count {
                if j > 0 {
                    pos = copy_bytes(&mut buf, pos, b",");
                }
                pos = copy_bytes(&mut buf, pos, m.used_by[j].as_bytes());
            }
        }

        buf[pos] = b'\n';
        pos += 1;
        print_out(&buf[..pos]);
    }

    0
}

fn cmd_insmod(opts: &ModprobeOpts) -> i32 {
    if opts.verbose {
        print_out(b"insmod ");
        print_out(opts.module_path.as_bytes());
        for i in 0..opts.param_count {
            print_out(b" ");
            print_out(&opts.params[i].name[..opts.params[i].name_len]);
            if opts.params[i].value_len > 0 {
                print_out(b"=");
                print_out(&opts.params[i].value[..opts.params[i].value_len]);
            }
        }
        print_out(b"\n");
    }

    // Check if already loaded
    let (modules, count) = get_loaded_modules();
    if modules
        .iter()
        .take(count)
        .any(|m| m.name.eq_bytes(opts.module_name.as_bytes()))
    {
        print_err(b"insmod: ERROR: could not insert module ");
        print_err(opts.module_path.as_bytes());
        print_err(b": File exists\n");
        return 1;
    }

    // In a real implementation, we'd call init_module() syscall
    if opts.verbose {
        print_out(b"Module ");
        print_out(opts.module_name.as_bytes());
        print_out(b" loaded successfully\n");
    }

    0
}

fn cmd_rmmod(opts: &ModprobeOpts) -> i32 {
    if opts.verbose {
        print_out(b"rmmod ");
        print_out(opts.module_name.as_bytes());
        print_out(b"\n");
    }

    // Check if loaded
    let (modules, count) = get_loaded_modules();
    let found_mod = modules
        .iter()
        .take(count)
        .find(|m| m.name.eq_bytes(opts.module_name.as_bytes()));

    let Some(m) = found_mod else {
        print_err(b"rmmod: ERROR: Module ");
        print_err(opts.module_name.as_bytes());
        print_err(b" is not currently loaded\n");
        return 1;
    };

    // Check refcount
    if m.refcount > 0 && !opts.force {
        print_err(b"rmmod: ERROR: Module ");
        print_err(opts.module_name.as_bytes());
        print_err(b" is in use by: ");
        for (j, used) in m.used_by.iter().take(m.used_by_count).enumerate() {
            if j > 0 {
                print_err(b", ");
            }
            print_err(used.as_bytes());
        }
        print_err(b"\n");
        return 1;
    }

    // In a real implementation, we'd call delete_module() syscall
    if opts.verbose {
        print_out(b"Module ");
        print_out(opts.module_name.as_bytes());
        print_out(b" removed successfully\n");
    }

    0
}

fn cmd_modprobe(opts: &ModprobeOpts) -> i32 {
    if opts.remove {
        return cmd_modprobe_remove(opts);
    }

    // Show dependencies mode
    if opts.show_depends {
        return cmd_show_depends(opts);
    }

    // Check if already loaded (unless --first-time)
    let (modules, count) = get_loaded_modules();
    if modules
        .iter()
        .take(count)
        .any(|m| m.name.eq_bytes(opts.module_name.as_bytes()))
    {
        if opts.first_time {
            if !opts.quiet {
                print_err(b"modprobe: FATAL: Module ");
                print_err(opts.module_name.as_bytes());
                print_err(b" already in kernel.\n");
            }
            return 1;
        }
        // Already loaded, not an error
        return 0;
    }

    // Resolve dependencies
    let mut deps = [ModuleName::new(); 64];
    let mut dep_count = 0usize;
    resolve_dependencies(opts.module_name.as_bytes(), &mut deps, &mut dep_count);

    // Load dependencies first (in order)
    for i in (0..dep_count).rev() {
        // Check if dep is already loaded
        let dep_loaded = modules
            .iter()
            .take(count)
            .any(|m| m.name.eq_bytes(deps[i].as_bytes()));

        if !dep_loaded
            && (opts.verbose || opts.dry_run) {
                print_out(b"insmod ");
                print_out(MODULES_DIR);
                print_out(b"/0.1.0/kernel/");
                print_out(deps[i].as_bytes());
                print_out(b".ko\n");
            }
    }

    // Load the main module
    if opts.verbose || opts.dry_run {
        print_out(b"insmod ");
        print_out(MODULES_DIR);
        print_out(b"/0.1.0/kernel/");
        print_out(opts.module_name.as_bytes());
        print_out(b".ko");
        for i in 0..opts.param_count {
            print_out(b" ");
            print_out(&opts.params[i].name[..opts.params[i].name_len]);
            if opts.params[i].value_len > 0 {
                print_out(b"=");
                print_out(&opts.params[i].value[..opts.params[i].value_len]);
            }
        }
        print_out(b"\n");
    }

    if opts.dry_run {
        return 0;
    }

    // In a real implementation, we'd call init_module() for each
    0
}

fn cmd_modprobe_remove(opts: &ModprobeOpts) -> i32 {
    let (modules, count) = get_loaded_modules();

    // Find the module
    let refcount = match modules
        .iter()
        .take(count)
        .find(|m| m.name.eq_bytes(opts.module_name.as_bytes()))
    {
        Some(m) => m.refcount,
        None => {
            if !opts.quiet {
                print_err(b"modprobe: FATAL: Module ");
                print_err(opts.module_name.as_bytes());
                print_err(b" is not currently loaded.\n");
            }
            return 1;
        }
    };

    if refcount > 0 && !opts.force {
        if !opts.quiet {
            print_err(b"modprobe: FATAL: Module ");
            print_err(opts.module_name.as_bytes());
            print_err(b" is in use.\n");
        }
        return 1;
    }

    if opts.verbose || opts.dry_run {
        print_out(b"rmmod ");
        print_out(opts.module_name.as_bytes());
        print_out(b"\n");
    }

    // Also try to remove unused dependencies
    let info = get_module_info(opts.module_name.as_bytes());
    for i in 0..info.dep_count {
        let dep_name = info.depends[i].as_bytes();
        // Check if the dep has other users
        let dep_refcount = modules
            .iter()
            .take(count)
            .find(|m| m.name.eq_bytes(dep_name))
            .map_or(0u32, |m| m.refcount);
        // Would be unused after removing our module
        if dep_refcount <= 1
            && (opts.verbose || opts.dry_run) {
                print_out(b"rmmod ");
                print_out(dep_name);
                print_out(b"\n");
            }
    }

    0
}

fn cmd_show_depends(opts: &ModprobeOpts) -> i32 {
    let mut deps = [ModuleName::new(); 64];
    let mut dep_count = 0usize;
    resolve_dependencies(opts.module_name.as_bytes(), &mut deps, &mut dep_count);

    // Print in load order (reverse)
    for i in (0..dep_count).rev() {
        print_out(b"insmod ");
        print_out(MODULES_DIR);
        print_out(b"/0.1.0/kernel/");
        print_out(deps[i].as_bytes());
        print_out(b".ko\n");
    }
    print_out(b"insmod ");
    print_out(MODULES_DIR);
    print_out(b"/0.1.0/kernel/");
    print_out(opts.module_name.as_bytes());
    print_out(b".ko\n");

    0
}

fn cmd_modinfo(opts: &ModprobeOpts) -> i32 {
    let info = get_module_info(opts.module_name.as_bytes());
    let sep = if opts.modinfo_null {
        b"\0" as &[u8]
    } else {
        b"\n" as &[u8]
    };

    // If -F specified, only show that field
    if opts.modinfo_field_len > 0 {
        let field = &opts.modinfo_field[..opts.modinfo_field_len];
        if bytes_eq(field, b"filename") {
            print_out(info.filename.as_bytes());
            print_out(sep);
        } else if bytes_eq(field, b"license") {
            print_out(&info.license[..info.license_len]);
            print_out(sep);
        } else if bytes_eq(field, b"author") {
            print_out(&info.author[..info.author_len]);
            print_out(sep);
        } else if bytes_eq(field, b"description") {
            print_out(&info.description[..info.desc_len]);
            print_out(sep);
        } else if bytes_eq(field, b"version") {
            print_out(&info.version[..info.version_len]);
            print_out(sep);
        } else if bytes_eq(field, b"depends") {
            for i in 0..info.dep_count {
                if i > 0 {
                    print_out(b",");
                }
                print_out(info.depends[i].as_bytes());
            }
            print_out(sep);
        } else if bytes_eq(field, b"alias") {
            for i in 0..info.alias_count {
                // Find the actual length of the alias
                let mut alen = 0;
                while alen < 64 && info.aliases[i][alen] != 0 {
                    alen += 1;
                }
                print_out(&info.aliases[i][..alen]);
                print_out(sep);
            }
        } else if bytes_eq(field, b"parm") {
            for i in 0..info.param_count {
                print_out(&info.params[i].name[..info.params[i].name_len]);
                print_out(b":");
                print_out(&info.params[i].value[..info.params[i].value_len]);
                print_out(sep);
            }
        }
        return 0;
    }

    // Show all fields
    print_out(b"filename:       ");
    print_out(info.filename.as_bytes());
    print_out(b"\n");

    if info.license_len > 0 {
        print_out(b"license:        ");
        print_out(&info.license[..info.license_len]);
        print_out(b"\n");
    }

    if info.author_len > 0 {
        print_out(b"author:         ");
        print_out(&info.author[..info.author_len]);
        print_out(b"\n");
    }

    if info.desc_len > 0 {
        print_out(b"description:    ");
        print_out(&info.description[..info.desc_len]);
        print_out(b"\n");
    }

    if info.version_len > 0 {
        print_out(b"version:        ");
        print_out(&info.version[..info.version_len]);
        print_out(b"\n");
    }

    // Aliases
    for i in 0..info.alias_count {
        print_out(b"alias:          ");
        let mut alen = 0;
        while alen < 64 && info.aliases[i][alen] != 0 {
            alen += 1;
        }
        print_out(&info.aliases[i][..alen]);
        print_out(b"\n");
    }

    // Dependencies
    if info.dep_count > 0 {
        print_out(b"depends:        ");
        for i in 0..info.dep_count {
            if i > 0 {
                print_out(b",");
            }
            print_out(info.depends[i].as_bytes());
        }
        print_out(b"\n");
    }

    // Parameters
    for i in 0..info.param_count {
        print_out(b"parm:           ");
        print_out(&info.params[i].name[..info.params[i].name_len]);
        print_out(b":");
        print_out(&info.params[i].value[..info.params[i].value_len]);
        print_out(b"\n");
    }

    // Firmware
    for i in 0..info.firmware_count {
        print_out(b"firmware:       ");
        let mut flen = 0;
        while flen < 64 && info.firmware[i][flen] != 0 {
            flen += 1;
        }
        print_out(&info.firmware[i][..flen]);
        print_out(b"\n");
    }

    0
}

fn cmd_depmod(opts: &ModprobeOpts) -> i32 {
    if opts.verbose {
        print_out(b"depmod: Generating module dependency database...\n");
    }

    // Simulated module list for dependency generation
    let sim_modules: &[(&[u8], &[&[u8]])] = &[
        (b"ext4", &[b"mbcache", b"jbd2", b"crc16"]),
        (b"mbcache", &[]),
        (b"jbd2", &[]),
        (b"crc16", &[]),
        (b"virtio_net", &[b"virtio_pci"]),
        (b"virtio_blk", &[b"virtio_pci"]),
        (b"virtio_pci", &[b"virtio_ring"]),
        (b"virtio_ring", &[b"virtio"]),
        (b"virtio", &[]),
        (b"e1000", &[]),
        (b"ahci", &[b"libahci"]),
        (b"libahci", &[]),
        (b"xhci_hcd", &[b"usbcore"]),
        (b"usbcore", &[b"usb_common"]),
        (b"usb_common", &[]),
    ];

    if opts.dry_run || opts.verbose {
        // Print modules.dep format
        for (name, deps) in sim_modules {
            print_out(b"kernel/");
            print_out(name);
            print_out(b".ko:");
            for dep in deps.iter() {
                print_out(b" kernel/");
                print_out(dep);
                print_out(b".ko");
            }
            print_out(b"\n");
        }
    }

    if !opts.dry_run
        && opts.verbose {
            print_out(b"depmod: Writing ");
            print_out(MODULES_DIR);
            print_out(b"/0.1.0/modules.dep\n");
            print_out(b"depmod: Writing ");
            print_out(MODULES_DIR);
            print_out(b"/0.1.0/modules.alias\n");
            print_out(b"depmod: Writing ");
            print_out(MODULES_DIR);
            print_out(b"/0.1.0/modules.symbols\n");
        }

    0
}

// ── Main ───────────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let opts = match parse_args(argc, argv) {
        Ok(o) => o,
        Err(code) => return code,
    };

    match opts.tool {
        Tool::Lsmod => cmd_lsmod(),
        Tool::Insmod => cmd_insmod(&opts),
        Tool::Rmmod => cmd_rmmod(&opts),
        Tool::Modprobe => cmd_modprobe(&opts),
        Tool::Modinfo => cmd_modinfo(&opts),
        Tool::Depmod => cmd_depmod(&opts),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_module_name_simple() {
        let n = normalize_module_name(b"ext4");
        assert_eq!(n.as_bytes(), b"ext4");
    }

    #[test]
    fn test_normalize_module_name_with_path() {
        let n = normalize_module_name(b"/lib/modules/5.10/kernel/ext4.ko");
        assert_eq!(n.as_bytes(), b"ext4");
    }

    #[test]
    fn test_normalize_module_name_compressed() {
        let n = normalize_module_name(b"ext4.ko.gz");
        assert_eq!(n.as_bytes(), b"ext4");
        let n = normalize_module_name(b"ext4.ko.xz");
        assert_eq!(n.as_bytes(), b"ext4");
        let n = normalize_module_name(b"ext4.ko.zst");
        assert_eq!(n.as_bytes(), b"ext4");
    }

    #[test]
    fn test_normalize_module_name_dash_to_underscore() {
        let n = normalize_module_name(b"virtio-net");
        assert_eq!(n.as_bytes(), b"virtio_net");
    }

    #[test]
    fn test_detect_tool() {
        assert_eq!(detect_tool(b"modprobe"), Tool::Modprobe);
        assert_eq!(detect_tool(b"/sbin/insmod"), Tool::Insmod);
        assert_eq!(detect_tool(b"/usr/bin/rmmod"), Tool::Rmmod);
        assert_eq!(detect_tool(b"lsmod"), Tool::Lsmod);
        assert_eq!(detect_tool(b"/sbin/modinfo"), Tool::Modinfo);
        assert_eq!(detect_tool(b"depmod"), Tool::Depmod);
    }

    #[test]
    fn test_ends_with() {
        assert!(ends_with(b"hello.ko", b".ko"));
        assert!(ends_with(b"test.ko.gz", b".ko.gz"));
        assert!(!ends_with(b"test", b".ko"));
        assert!(!ends_with(b"ko", b".ko"));
    }

    #[test]
    fn test_starts_with() {
        assert!(starts_with(b"hello world", b"hello"));
        assert!(!starts_with(b"world", b"hello"));
        assert!(starts_with(b"ext4", b"ext4"));
    }

    #[test]
    fn test_get_loaded_modules() {
        let (modules, count) = get_loaded_modules();
        assert!(count > 0);
        // Verify ext4 is in the list
        let mut found_ext4 = false;
        for m in modules.iter().take(count) {
            if m.name.eq_bytes(b"ext4") {
                found_ext4 = true;
                assert!(m.size > 0);
                break;
            }
        }
        assert!(found_ext4);
    }

    #[test]
    fn test_get_module_info_ext4() {
        let info = get_module_info(b"ext4");
        assert_eq!(info.name.as_bytes(), b"ext4");
        assert!(info.dep_count > 0);
        assert!(info.license_len > 0);
    }

    #[test]
    fn test_resolve_dependencies() {
        let mut deps = [ModuleName::new(); 64];
        let mut dep_count = 0usize;
        resolve_dependencies(b"ext4", &mut deps, &mut dep_count);
        assert!(dep_count >= 3); // mbcache, jbd2, crc16
    }

    #[test]
    fn test_resolve_dependencies_chain() {
        let mut deps = [ModuleName::new(); 64];
        let mut dep_count = 0usize;
        resolve_dependencies(b"virtio_net", &mut deps, &mut dep_count);
        // virtio_net -> virtio_pci -> virtio_ring -> virtio
        assert!(dep_count >= 3);
    }

    #[test]
    fn test_module_name_eq() {
        let n1 = ModuleName::from_bytes(b"test");
        assert!(n1.eq_bytes(b"test"));
        assert!(!n1.eq_bytes(b"other"));
    }

    #[test]
    fn test_format_u64() {
        let mut buf = [0u8; 20];
        let n = format_u64(42, &mut buf);
        assert_eq!(&buf[..n], b"42");
    }

    #[test]
    fn test_format_u64_zero() {
        let mut buf = [0u8; 20];
        let n = format_u64(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn test_format_size() {
        let mut buf = [0u8; 20];
        let n = format_size(1024, &mut buf);
        assert_eq!(&buf[..n], b"1K");
        let n = format_size(1048576, &mut buf);
        assert_eq!(&buf[..n], b"1M");
    }

    #[test]
    fn test_parse_module_param_with_value() {
        let mut param = ModuleParam::new();
        parse_module_param(b"debug=5", &mut param);
        assert_eq!(&param.name[..param.name_len], b"debug");
        assert_eq!(&param.value[..param.value_len], b"5");
    }

    #[test]
    fn test_parse_module_param_boolean() {
        let mut param = ModuleParam::new();
        parse_module_param(b"debug", &mut param);
        assert_eq!(&param.name[..param.name_len], b"debug");
        assert_eq!(&param.value[..param.value_len], b"1");
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(tool_name(Tool::Modprobe), b"modprobe");
        assert_eq!(tool_name(Tool::Insmod), b"insmod");
        assert_eq!(tool_name(Tool::Rmmod), b"rmmod");
        assert_eq!(tool_name(Tool::Lsmod), b"lsmod");
        assert_eq!(tool_name(Tool::Modinfo), b"modinfo");
        assert_eq!(tool_name(Tool::Depmod), b"depmod");
    }

    #[test]
    fn test_module_info_e1000() {
        let info = get_module_info(b"e1000");
        assert_eq!(info.name.as_bytes(), b"e1000");
        assert!(info.alias_count > 0);
        assert!(info.param_count > 0);
    }

    #[test]
    fn test_bytes_eq() {
        assert!(bytes_eq(b"hello", b"hello"));
        assert!(!bytes_eq(b"hello", b"world"));
        assert!(!bytes_eq(b"hello", b"hell"));
    }
}
