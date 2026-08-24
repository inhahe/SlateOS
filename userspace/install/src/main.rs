//! Slate OS install — copy files and set attributes
//!
//! GNU coreutils-compatible `install` command for copying files
//! with specified permissions, ownership, and directory creation.

#![allow(unexpected_cfgs)]

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ── Mode constants ─────────────────────────────────────────────────

/// What a target gets when no `-m` was given — a file, a `-d` directory, or the
/// directory `-t` names. GNU's `install.c` `DEFAULT_MODE`; note it is `0755` for
/// a *file* too, not the `0644` that `cp` would leave.
const DEFAULT_MODE: u32 = 0o755;

/// The mode of a directory `install` invents on the way to its target: the
/// parents `-D` creates above a file, the ancestors above a `-d` operand, and
/// the directory `-D -t` creates. GNU's `make_ancestor_dir` uses a fixed
/// `0755` here, and measured against GNU 9.4 it really is fixed — five umasks
/// (`000 022 077 002 027`) crossed with `-m 700` and `-m 'a=,+X'` all leave
/// every invented ancestor at `0755`. The `-m` mode reaches only the target
/// itself.
const ANCESTOR_MODE: u32 = 0o755;

/// The two modes one `-m` spec names: one for a file target, one for a
/// directory target.
///
/// They are separate because `X` asks a different question of each — `+X` is
/// `0111` on a directory and nothing on a file — so a single number cannot
/// serve both. GNU compiles the spec once and applies it twice for exactly this
/// reason (`mode_adjust (0, false, …)` and `mode_adjust (0, true, …)`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Modes {
    /// For a file target, and for the destination of a copy.
    file: u32,
    /// For a `-d` target.
    dir: u32,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            file: DEFAULT_MODE,
            dir: DEFAULT_MODE,
        }
    }
}

/// Compile a `-m` spec, or `None` if it is not a mode.
///
/// Base `0` and umask `0` are both GNU's, and both matter. Base `0` means the
/// spec is read as a description of the finished mode rather than as an edit to
/// something: `install -m u+w src d` is `0200`, not `0755 | 0200`, so a spec
/// can only ever be *less* permissive than the user spelled. Umask `0` means a
/// who-less clause is taken at its word — `install -m +s` is `6000` whatever the
/// umask — which is where `install` parts company with `mkdir -m` and
/// `mkfifo -m`, both of which do let the umask into such a clause. Measured
/// across five umasks; every answer was identical.
fn compile_mode(spec: &[u8]) -> Option<Modes> {
    let changes = modechange::compile(spec)?;
    Some(Modes {
        file: modechange::adjust(0, false, 0, &changes).mode,
        dir: modechange::adjust(0, true, 0, &changes).mode,
    })
}

/// A string as GNU's `quote()` renders it in a diagnostic: `‘zzz’`.
///
/// `install`'s own messages use straight quotes, but the invalid-mode one does
/// not — it comes from `error (…, _("invalid mode %s"), quote (…))`, and
/// quoting style here is a property of the individual message, not of the
/// utility.
fn quote(s: &str) -> String {
    format!("\u{2018}{s}\u{2019}")
}

// ── Argument parsing ───────────────────────────────────────────────

#[derive(Debug)]
struct Args {
    /// Copy files to target
    sources: Vec<String>,
    /// Target file or directory
    target: Option<String>,
    /// Create directories mode (-d)
    directory_mode: bool,
    /// Target is a directory (-t DIR)
    target_directory: Option<String>,
    /// Do not treat last arg as directory (-T)
    no_target_directory: bool,
    /// The modes `-m` resolved to, or the defaults when it was absent.
    modes: Modes,
    /// Owner (-o OWNER)
    owner: Option<String>,
    /// Group (-g GROUP)
    group: Option<String>,
    /// Backup existing files (-b)
    backup: bool,
    /// Backup suffix (-S SUFFIX)
    backup_suffix: String,
    /// Compare and don't copy if same (-C)
    compare: bool,
    /// Create leading directories (-D)
    create_dirs: bool,
    /// Preserve timestamps (-p)
    preserve_timestamps: bool,
    /// Strip symbols (-s)
    strip: bool,
    /// Strip program (--strip-program=PROG)
    strip_program: String,
    /// Verbose (-v)
    verbose: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            target: None,
            directory_mode: false,
            target_directory: None,
            no_target_directory: false,
            modes: Modes::default(),
            owner: None,
            group: None,
            backup: false,
            backup_suffix: "~".to_string(),
            compare: false,
            create_dirs: false,
            preserve_timestamps: false,
            strip: false,
            strip_program: "strip".to_string(),
            verbose: false,
        }
    }
}

fn parse_args() -> Args {
    let argv: Vec<String> = env::args().collect();
    let mut args = Args::default();
    let mut positionals = Vec::new();

    // The `-m` spec is carried uncompiled until every operand has been counted,
    // because GNU checks the operands first: `install -m zzz` with no operands
    // is `missing file operand`, and `install -m zzz src` is `missing
    // destination file operand after 'src'` — the invalid mode is not mentioned
    // in either. A parser that validated the spec where it read it could not
    // produce that ordering. Last `-m` wins, as with any repeated option.
    let mut mode_spec: Option<String> = None;

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--version" => {
                println!("install (Slate OS) 0.1.0");
                process::exit(0);
            }
            "-d" | "--directory" => args.directory_mode = true,
            "-D" => args.create_dirs = true,
            "-v" | "--verbose" => args.verbose = true,
            "-C" | "--compare" => args.compare = true,
            "-p" | "--preserve-timestamps" => args.preserve_timestamps = true,
            "-s" | "--strip" => args.strip = true,
            "-b" | "--backup" => args.backup = true,
            "-T" | "--no-target-directory" => args.no_target_directory = true,
            "-m" | "--mode" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("install: option '-m' requires an argument");
                    process::exit(1);
                }
                mode_spec = Some(argv[i].clone());
            }
            _ if arg.starts_with("--mode=") => {
                mode_spec = Some(arg["--mode=".len()..].to_string());
            }
            "-o" | "--owner" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("install: option '-o' requires an argument");
                    process::exit(1);
                }
                args.owner = Some(argv[i].clone());
            }
            _ if arg.starts_with("--owner=") => {
                args.owner = Some(arg["--owner=".len()..].to_string());
            }
            "-g" | "--group" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("install: option '-g' requires an argument");
                    process::exit(1);
                }
                args.group = Some(argv[i].clone());
            }
            _ if arg.starts_with("--group=") => {
                args.group = Some(arg["--group=".len()..].to_string());
            }
            "-t" | "--target-directory" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("install: option '-t' requires an argument");
                    process::exit(1);
                }
                args.target_directory = Some(argv[i].clone());
            }
            _ if arg.starts_with("--target-directory=") => {
                args.target_directory = Some(arg["--target-directory=".len()..].to_string());
            }
            "-S" | "--suffix" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("install: option '-S' requires an argument");
                    process::exit(1);
                }
                args.backup_suffix = argv[i].clone();
                args.backup = true;
            }
            _ if arg.starts_with("--suffix=") => {
                args.backup_suffix = arg["--suffix=".len()..].to_string();
                args.backup = true;
            }
            _ if arg.starts_with("--strip-program=") => {
                args.strip_program = arg["--strip-program=".len()..].to_string();
            }
            "--" => {
                i += 1;
                while i < argv.len() {
                    positionals.push(argv[i].clone());
                    i += 1;
                }
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") => {
                // Combined short flags like -Dv
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'd' => args.directory_mode = true,
                        'D' => args.create_dirs = true,
                        'v' => args.verbose = true,
                        'C' => args.compare = true,
                        'p' => args.preserve_timestamps = true,
                        's' => args.strip = true,
                        'b' => args.backup = true,
                        'T' => args.no_target_directory = true,
                        'm' => {
                            // Rest of this arg or next arg is the mode
                            let rest: String = chars[j + 1..].iter().collect();
                            let mode_str = if rest.is_empty() {
                                i += 1;
                                if i >= argv.len() {
                                    eprintln!("install: option '-m' requires an argument");
                                    process::exit(1);
                                }
                                argv[i].clone()
                            } else {
                                rest
                            };
                            mode_spec = Some(mode_str);
                            j = chars.len(); // Consumed rest
                            continue;
                        }
                        'o' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let val = if rest.is_empty() {
                                i += 1;
                                if i >= argv.len() {
                                    eprintln!("install: option '-o' requires an argument");
                                    process::exit(1);
                                }
                                argv[i].clone()
                            } else {
                                rest
                            };
                            args.owner = Some(val);
                            j = chars.len();
                            continue;
                        }
                        'g' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let val = if rest.is_empty() {
                                i += 1;
                                if i >= argv.len() {
                                    eprintln!("install: option '-g' requires an argument");
                                    process::exit(1);
                                }
                                argv[i].clone()
                            } else {
                                rest
                            };
                            args.group = Some(val);
                            j = chars.len();
                            continue;
                        }
                        c => {
                            eprintln!("install: unknown option '-{c}'");
                            process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => positionals.push(arg.clone()),
        }
        i += 1;
    }

    // Operand arity first. GNU's one test is
    // `n_files <= !(dir_arg || target_directory)`: with `-d` or `-t` a single
    // operand suffices, otherwise a source and a destination are both required.
    // The missing-operand diagnostics name only the operands, never the mode,
    // which is why the `-m` spec is still uncompiled here.
    let needs_two = !(args.directory_mode || args.target_directory.is_some());
    if positionals.len() <= usize::from(needs_two) {
        if let Some(first) = positionals.first() {
            eprintln!(
                "install: missing destination file operand after {}",
                quoteaf_os(first)
            );
        } else {
            eprintln!("install: missing file operand");
        }
        usage_error();
    }

    if args.directory_mode {
        // -d: all positionals are directories to create
        args.sources = positionals;
    } else if args.target_directory.is_some() {
        // -t DIR: all positionals are sources
        args.sources = positionals;
    } else if args.no_target_directory {
        // -T: exactly src dest
        if let Some(extra) = positionals.get(2) {
            eprintln!("install: extra operand {}", quoteaf_os(extra));
            usage_error();
        }
        let mut it = positionals.into_iter();
        let (Some(src), Some(dst)) = (it.next(), it.next()) else {
            // Unreachable: the arity check above rejected a shorter list.
            eprintln!("install: missing file operand");
            usage_error();
        };
        args.sources = vec![src];
        args.target = Some(dst);
    } else {
        // Normal: last arg is target, rest are sources
        args.target = Some(positionals.pop().unwrap_or_default());
        args.sources = positionals;
    }

    // Only now is the mode compiled, so that a bad one cannot pre-empt a
    // missing operand.
    if let Some(spec) = mode_spec {
        match compile_mode(spec.as_bytes()) {
            Some(modes) => args.modes = modes,
            None => {
                eprintln!("install: invalid mode {}", quote(&spec));
                process::exit(1);
            }
        }
    }

    args
}

/// GNU's `usage (EXIT_FAILURE)`: the referral line, not the whole help text.
fn usage_error() -> ! {
    eprintln!("Try 'install --help' for more information.");
    process::exit(1);
}

fn print_usage() {
    eprintln!("Usage: install [OPTION]... [-T] SOURCE DEST");
    eprintln!("  or:  install [OPTION]... SOURCE... DIRECTORY");
    eprintln!("  or:  install [OPTION]... -t DIRECTORY SOURCE...");
    eprintln!("  or:  install [OPTION]... -d DIRECTORY...");
    eprintln!();
    eprintln!("Copy files and set attributes.");
    eprintln!();
    eprintln!("  -b              make a backup of each existing destination file");
    eprintln!("  -C, --compare   compare and don't copy if the same");
    eprintln!("  -d, --directory create all components of specified directories");
    eprintln!("  -D              create leading components of DEST, then copy SOURCE");
    eprintln!("  -g, --group=GROUP  set group ownership");
    eprintln!("  -m, --mode=MODE   set permission mode (as in chmod)");
    eprintln!("  -o, --owner=OWNER set ownership");
    eprintln!("  -p, --preserve-timestamps  apply access/mod times of SOURCE");
    eprintln!("  -s, --strip     strip symbol tables");
    eprintln!("  -S, --suffix=SUFFIX  override backup suffix");
    eprintln!("  -t, --target-directory=DIR  copy all SOURCE(s) into DIR");
    eprintln!("  -T, --no-target-directory  treat DEST as a normal file");
    eprintln!("  -v, --verbose   print the name of each installed file");
    eprintln!("  -h, --help      display this help");
}

// ── Ownership helpers ──────────────────────────────────────────────

/// Look up a user in /etc/passwd, return UID
fn resolve_user(name: &str) -> Result<u32, String> {
    // Try numeric first
    if let Ok(uid) = name.parse::<u32>() {
        return Ok(uid);
    }

    let content =
        fs::read_to_string("/etc/passwd").map_err(|e| format!("cannot read /etc/passwd: {e}"))?;
    for line in content.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == name {
            return fields[2]
                .parse::<u32>()
                .map_err(|_| format!("invalid UID for user '{name}'"));
        }
    }
    Err(format!("unknown user '{name}'"))
}

/// Look up a group in /etc/group, return GID
fn resolve_group(name: &str) -> Result<u32, String> {
    // Try numeric first
    if let Ok(gid) = name.parse::<u32>() {
        return Ok(gid);
    }

    let content =
        fs::read_to_string("/etc/group").map_err(|e| format!("cannot read /etc/group: {e}"))?;
    for line in content.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == name {
            return fields[2]
                .parse::<u32>()
                .map_err(|_| format!("invalid GID for group '{name}'"));
        }
    }
    Err(format!("unknown group '{name}'"))
}

// ── Syscall wrappers ───────────────────────────────────────────────

#[allow(dead_code)]
fn sys_chmod(path: &str, mode: u32) -> io::Result<()> {
    #[cfg(target_os = "slateos")]
    {
        let path_bytes = path.as_bytes();
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 90u64, // SYS_CHMOD
                in("rdi") path_bytes.as_ptr() as u64,
                in("rsi") path_bytes.len() as u64,
                in("rdx") mode as u64,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        if ret < 0 {
            Err(io::Error::from_raw_os_error(-ret as i32))
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "slateos"))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

#[allow(dead_code)]
fn sys_chown(path: &str, uid: u32, gid: u32) -> io::Result<()> {
    #[cfg(target_os = "slateos")]
    {
        let path_bytes = path.as_bytes();
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 92u64, // SYS_CHOWN
                in("rdi") path_bytes.as_ptr() as u64,
                in("rsi") path_bytes.len() as u64,
                in("rdx") uid as u64,
                in("r10") gid as u64,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        if ret < 0 {
            Err(io::Error::from_raw_os_error(-ret as i32))
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "slateos"))]
    {
        let _ = (path, uid, gid);
        Ok(())
    }
}

// ── File comparison ────────────────────────────────────────────────

fn files_are_same(src: &Path, dst: &Path) -> bool {
    let src_meta = match fs::metadata(src) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let dst_meta = match fs::metadata(dst) {
        Ok(m) => m,
        Err(_) => return false,
    };

    if src_meta.len() != dst_meta.len() {
        return false;
    }

    // Compare contents
    let mut src_f = match fs::File::open(src) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut dst_f = match fs::File::open(dst) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut src_buf = [0u8; 8192];
    let mut dst_buf = [0u8; 8192];

    loop {
        let src_n = match src_f.read(&mut src_buf) {
            Ok(n) => n,
            Err(_) => return false,
        };
        let dst_n = match dst_f.read(&mut dst_buf) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if src_n != dst_n {
            return false;
        }
        if src_n == 0 {
            return true;
        }
        if src_buf[..src_n] != dst_buf[..dst_n] {
            return false;
        }
    }
}

// ── Directory creation ─────────────────────────────────────────────

/// Create every missing component of `path`, each at [`ANCESTOR_MODE`].
///
/// A component that already exists is left exactly as it is — its mode is not
/// touched. Measured: `chmod 700 mid; install -m 711 -d mid/deep/leaf` leaves
/// `mid` at `700`, gives the invented `deep` `755`, and gives `leaf` the `-m`
/// mode. This is GNU's `make_ancestor_dir`, which sets a mode only on a
/// directory it created.
///
/// Used for the parents `-D` builds above a file, the ancestors above a `-d`
/// operand, and the directory `-D -t` creates — never for a target itself.
fn create_ancestors(path: &Path, verbose: bool) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        create_ancestors(parent, verbose)?;
    }

    create_dir_at(path, ANCESTOR_MODE, verbose)
}

/// Create one directory and give it `mode`, reporting it under `-v`.
fn create_dir_at(path: &Path, mode: u32, verbose: bool) -> Result<(), String> {
    fs::create_dir(path)
        .map_err(|e| format!("cannot create directory '{}': {e}", path.display()))?;

    if verbose {
        println!("install: creating directory {}", quoteaf_os(path));
    }

    set_mode(path, mode)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    sys_chmod(&path.to_string_lossy(), mode)
        .map_err(|e| format!("cannot set mode on '{}': {e}", path.display()))
}

/// Create a `-d` target: ancestors at [`ANCESTOR_MODE`], the target itself at
/// `mode`.
///
/// The target's mode is set **whether or not it already existed**, which is the
/// one place `install -d` differs from `mkdir -p -m`: measured, `mkdir -m 711 e;
/// install -d e` leaves `e` at `755`, and `install -m 700 -d e` leaves it at
/// `700`. `install -d` is a statement about what the directory should be, not a
/// request to create it.
fn create_target_dir(path: &Path, mode: u32, verbose: bool) -> Result<(), String> {
    if path.exists() {
        return set_mode(path, mode);
    }

    if let Some(parent) = path.parent() {
        create_ancestors(parent, verbose)?;
    }

    create_dir_at(path, mode, verbose)
}

// ── Install file ───────────────────────────────────────────────────

fn install_file(src: &Path, dst: &Path, args: &Args) -> Result<(), String> {
    // Compare mode: skip if files are identical
    if args.compare && files_are_same(src, dst) {
        return Ok(());
    }

    // Backup existing file
    if args.backup && dst.exists() {
        let backup_path = format!("{}{}", dst.display(), args.backup_suffix);
        fs::rename(dst, &backup_path)
            .map_err(|e| format!("cannot backup '{}' to '{backup_path}': {e}", dst.display()))?;
    }

    // Create parent directories if -D
    if args.create_dirs
        && let Some(parent) = dst.parent()
    {
        create_ancestors(parent, args.verbose)?;
    }

    // Copy the file: read source, write to temp, rename
    // We copy to a temp name in the same directory, then rename,
    // to get atomic replacement behavior.
    let dst_dir = dst.parent().unwrap_or_else(|| Path::new("."));
    let temp_name = dst_dir.join(format!(
        ".install-tmp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    // Read source
    let data = fs::read(src).map_err(|e| format!("cannot read '{}': {e}", src.display()))?;

    // Write to temp
    let mut tmp_file =
        fs::File::create(&temp_name).map_err(|e| format!("cannot create temp file: {e}"))?;
    tmp_file
        .write_all(&data)
        .map_err(|e| format!("cannot write temp file: {e}"))?;
    drop(tmp_file);

    // Rename temp to destination
    // If rename fails (cross-device), fall back to copy+delete
    if fs::rename(&temp_name, dst).is_err() {
        fs::copy(&temp_name, dst)
            .map_err(|e| format!("cannot copy to '{}': {e}", dst.display()))?;
        let _ = fs::remove_file(&temp_name);
    }

    // Set permissions
    set_mode(dst, args.modes.file)?;

    // Set ownership
    if args.owner.is_some() || args.group.is_some() {
        let uid = match &args.owner {
            Some(o) => resolve_user(o)?,
            None => u32::MAX, // -1 means no change
        };
        let gid = match &args.group {
            Some(g) => resolve_group(g)?,
            None => u32::MAX,
        };
        sys_chown(&dst.to_string_lossy(), uid, gid)
            .map_err(|e| format!("cannot set ownership on '{}': {e}", dst.display()))?;
    }

    // Preserve timestamps
    if args.preserve_timestamps {
        // On Slate OS we'd copy atime/mtime from source via syscall.
        // For now, this is a placeholder that will work when the
        // utimensat syscall is available.
        #[cfg(target_os = "slateos")]
        {
            // TODO: implement utimensat call to copy timestamps
        }
    }

    // Strip symbols
    if args.strip {
        let status = std::process::Command::new(&args.strip_program)
            .arg(dst.as_os_str())
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "install: strip program {} failed with exit code {}",
                    quoteaf_os(&args.strip_program),
                    s.code().unwrap_or(-1)
                );
            }
            Err(e) => {
                eprintln!(
                    "install: cannot run strip program {}: {e}",
                    quoteaf_os(&args.strip_program)
                );
            }
        }
    }

    if args.verbose {
        println!("{} -> {}", quoteaf_os(src), quoteaf_os(dst));
    }

    Ok(())
}

// ── Main ───────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let args = parse_args();

    if args.directory_mode {
        // -d: create directories
        for dir in &args.sources {
            let path = PathBuf::from(dir);
            create_target_dir(&path, args.modes.dir, args.verbose)?;
        }
        return Ok(());
    }

    // Determine target directory
    let target_dir = if let Some(ref td) = args.target_directory {
        Some(PathBuf::from(td))
    } else if let Some(ref t) = args.target {
        let tp = PathBuf::from(t);
        if args.sources.len() > 1 || (tp.exists() && tp.is_dir() && !args.no_target_directory) {
            Some(tp)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(ref td) = target_dir {
        // Ensure target directory exists
        if !td.exists() {
            if args.create_dirs {
                // The directory `-D -t` invents is an ancestor, not the target:
                // measured, `install -D -m 700 -t nodir src` leaves `nodir` at
                // `755` and gives only the copied file the `-m` mode.
                create_ancestors(td, args.verbose)?;
            } else {
                return Err(format!(
                    "target directory '{}' does not exist",
                    td.display()
                ));
            }
        }

        // Install each source into the target directory
        for src in &args.sources {
            let src_path = PathBuf::from(src);
            let filename = src_path
                .file_name()
                .ok_or_else(|| format!("cannot determine filename from '{src}'"))?;
            let dst = td.join(filename);
            install_file(&src_path, &dst, &args)?;
        }
    } else {
        // Single file install: SOURCE -> DEST
        if args.sources.len() != 1 {
            return Err("too many source files for single-file install".to_string());
        }
        let src_path = PathBuf::from(&args.sources[0]);
        let dst_path = PathBuf::from(args.target.as_deref().ok_or("missing destination")?);
        install_file(&src_path, &dst_path, &args)?;
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("install: {e}");
        process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- Mode parsing --
    //
    // Every row below was measured against GNU coreutils 9.4 under five umasks
    // (000, 022, 077, 002, 027). Every answer was identical under all five,
    // which is the whole reason `compile_mode` passes umask 0: unlike
    // `mkdir -m` and `mkfifo -m`, no clause of an `install -m` spec is ever
    // masked, not even a who-less one.

    /// `-m` on a file target: `mode_adjust(0, false, 0, ...)`.
    fn file_mode(spec: &str) -> u32 {
        compile_mode(spec.as_bytes())
            .unwrap_or_else(|| panic!("GNU accepts {spec:?}"))
            .file
    }

    /// `-m` on a `-d` target: `mode_adjust(0, true, 0, ...)`.
    fn dir_mode(spec: &str) -> u32 {
        compile_mode(spec.as_bytes())
            .unwrap_or_else(|| panic!("GNU accepts {spec:?}"))
            .dir
    }

    #[test]
    fn an_octal_spec_is_the_mode_it_spells() {
        for (spec, want) in [
            ("0755", 0o755),
            ("0644", 0o644),
            ("755", 0o755),
            ("644", 0o644),
            ("0", 0),
            ("00", 0),
            ("2755", 0o2755),
            ("1777", 0o1777),
        ] {
            assert_eq!(file_mode(spec), want, "-m {spec} on a file");
            assert_eq!(dir_mode(spec), want, "-m {spec} on a directory");
        }
    }

    /// The defect this conversion existed to remove.
    ///
    /// The old parser started from `0755` and edited it, so `-m u+w` produced
    /// `0755`, not the `0200` the user asked for -- a mode *more* permissive
    /// than the spec, arrived at silently. GNU starts from 0: a `-m` spec
    /// describes the finished mode rather than editing a default.
    #[test]
    fn a_symbolic_spec_starts_from_zero_not_from_the_default() {
        assert_eq!(file_mode("u+w"), 0o200);
        assert_eq!(file_mode("u+rwx"), 0o700);
        assert_eq!(file_mode("g+rwx"), 0o070);
        assert_eq!(file_mode("o+rwx"), 0o007);
        assert_eq!(file_mode("a+rwx"), 0o777);
        assert_eq!(file_mode("+r"), 0o444);
        assert_eq!(file_mode("u+rwx,g+rx,o+r"), 0o754);
        assert_eq!(file_mode("u=rw,go="), 0o600);
        assert_eq!(file_mode("a=,u+w,g+r"), 0o240);
    }

    /// A who-less clause is *not* masked, which is where `install` parts
    /// company with `mkdir -m` and `mkfifo -m`.
    ///
    /// Under `umask 077`, `mkdir -m 'a=,+w' d` is `0200` but
    /// `install -m 'a=,+w' src d` is `0222`. Only the first passes a real
    /// umask to `adjust`.
    #[test]
    fn the_umask_never_reaches_an_install_mode() {
        assert_eq!(file_mode("a=,+w"), 0o222);
        assert_eq!(file_mode("+x"), 0o111);
        assert_eq!(file_mode("+s"), 0o6000);
        assert_eq!(file_mode("+t"), 0o1000);
        assert_eq!(file_mode("g+s"), 0o2000);
        assert_eq!(file_mode("u+s"), 0o4000);
    }

    /// `X` is why one spec has to yield two modes.
    ///
    /// The old parser mapped `'x' | 'X'` to the same bits "for simplicity",
    /// which made `install -m 'a=,+X' src f` produce `0111` where GNU produces
    /// `0`. `X` fires on a directory, or on a file that already has an execute
    /// bit -- and the base here is 0, so a file never does.
    #[test]
    fn capital_x_fires_on_a_directory_and_not_on_a_file() {
        assert_eq!(file_mode("a=,+X"), 0);
        assert_eq!(dir_mode("a=,+X"), 0o111);
        assert_eq!(file_mode("+X"), 0);
        assert_eq!(dir_mode("+X"), 0o111);
        // `x` is unconditional, so the two targets agree on it.
        assert_eq!(file_mode("+x"), 0o111);
        assert_eq!(dir_mode("+x"), 0o111);
    }

    /// The boundary between a mode GNU accepts and one it refuses.
    ///
    /// The two acceptances are the rows worth having: `+` names no bits at all
    /// and `=` clears every bit, and both come out `0` here because the base is
    /// 0. A parser written from intuition refuses them. The refusals are the
    /// other half of the old defect -- it skipped empty clauses, so `,` and
    /// `+r,` were silently accepted as "no change", i.e. mode `0755`.
    #[test]
    fn the_boundary_between_a_valid_and_an_invalid_mode() {
        for spec in ["+", "="] {
            assert_eq!(file_mode(spec), 0, "GNU accepts -m {spec} as mode 0");
            assert_eq!(dir_mode(spec), 0, "GNU accepts -m {spec} as mode 0");
        }
        for spec in ["zzz", "8", "u=q", "z+r", ",", "a", "+r,", "abc", "999", ""] {
            assert!(
                compile_mode(spec.as_bytes()).is_none(),
                "GNU refuses -m {spec:?}"
            );
        }
    }

    /// GNU's wording, which is not this utility's usual wording.
    ///
    /// `install`'s other diagnostics use straight quotes; this one comes from
    /// `quote()` and uses curly ones, and it carries no colon after "mode".
    #[test]
    fn the_invalid_mode_diagnostic_matches_gnu() {
        assert_eq!(
            format!("install: invalid mode {}", quote("zzz")),
            "install: invalid mode \u{2018}zzz\u{2019}"
        );
        assert_eq!(
            format!("install: invalid mode {}", quote("")),
            "install: invalid mode \u{2018}\u{2019}"
        );
    }

    /// No `-m` means `0755` for a file as well as for a directory.
    ///
    /// Not `0644`: `install` exists to place executables, and measured, a
    /// `0700` source copied with no `-m` lands at `0755` -- the source's mode
    /// does not leak into the destination either.
    #[test]
    fn the_default_mode_is_0755_for_both_targets() {
        let d = Modes::default();
        assert_eq!(d.file, 0o755);
        assert_eq!(d.dir, 0o755);
        assert_eq!(DEFAULT_MODE, 0o755);
        assert_eq!(Args::default().modes, d);
    }

    /// An invented ancestor is `0755` whatever `-m` said.
    ///
    /// `install -D -m 711 src pp/qq/rr` gives `pp` and `qq` `0755` and only
    /// `rr` `0711`; `install -m 711 -d mid/deep/leaf` gives `deep` `0755` and
    /// only `leaf` `0711`. The old code handed the `-m` mode to every parent it
    /// created, all the way up.
    #[test]
    fn an_invented_ancestor_ignores_the_requested_mode() {
        assert_eq!(ANCESTOR_MODE, 0o755);
        assert_ne!(ANCESTOR_MODE, file_mode("711"));
    }

    // ── File comparison ──

    #[test]
    fn test_files_same_nonexistent() {
        assert!(!files_are_same(
            Path::new("/nonexistent/a"),
            Path::new("/nonexistent/b")
        ));
    }

    // ── Directory creation helpers ──

    /// A scratch directory named for this process, so two lanes building at
    /// once cannot collide. Removed by the test that made it.
    fn scratch(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("install-test-{tag}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    /// `create_ancestors` builds the whole chain and stops at the target's
    /// parent; it never creates the target.
    #[test]
    fn create_ancestors_builds_the_chain_and_nothing_more() {
        let root = scratch("ancestors");
        let deep = root.join("a").join("b").join("c");

        create_ancestors(&deep, false).expect("create the chain");
        assert!(deep.is_dir());
        assert!(root.join("a").is_dir());
        assert!(root.join("a").join("b").is_dir());

        // Idempotent: an existing chain is not an error, and nothing below it
        // is disturbed.
        let marker = deep.join("marker");
        fs::write(&marker, b"x").expect("write marker");
        create_ancestors(&deep, false).expect("second call is a no-op");
        assert!(marker.is_file());

        fs::remove_dir_all(&root).expect("clean up");
    }

    /// `create_target_dir` succeeds on a directory that already exists.
    ///
    /// The old code returned early on `path.exists()`, so an existing `-d`
    /// target never had its mode set at all. GNU sets it either way --
    /// `mkdir -m 711 e; install -d e` leaves `e` at `0755` -- so the existing
    /// case has to reach the chmod, not skip the function.
    #[test]
    fn create_target_dir_accepts_a_directory_that_already_exists() {
        let root = scratch("target");
        let target = root.join("x").join("y");

        create_target_dir(&target, 0o711, false).expect("create the target");
        assert!(target.is_dir());

        create_target_dir(&target, 0o700, false).expect("existing target is not an error");
        assert!(target.is_dir());

        fs::remove_dir_all(&root).expect("clean up");
    }

    // ── User/Group resolution ──

    #[test]
    fn test_resolve_user_numeric() {
        assert_eq!(resolve_user("0").unwrap(), 0);
        assert_eq!(resolve_user("1000").unwrap(), 1000);
        assert_eq!(resolve_user("65534").unwrap(), 65534);
    }

    #[test]
    fn test_resolve_group_numeric() {
        assert_eq!(resolve_group("0").unwrap(), 0);
        assert_eq!(resolve_group("100").unwrap(), 100);
    }

    // ── Backup suffix ──

    #[test]
    fn test_default_backup_suffix() {
        let args = Args::default();
        assert_eq!(args.backup_suffix, "~");
    }

    #[test]
    fn test_default_strip_program() {
        let args = Args::default();
        assert_eq!(args.strip_program, "strip");
    }

    // ── Path operations ──

    #[test]
    fn test_filename_extraction() {
        let path = PathBuf::from("/usr/bin/program");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "program");
    }

    #[test]
    fn test_filename_from_relative() {
        let path = PathBuf::from("./src/main.rs");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "main.rs");
    }

    #[test]
    fn test_parent_dir() {
        let path = PathBuf::from("/usr/local/bin/prog");
        assert_eq!(path.parent().unwrap(), Path::new("/usr/local/bin"));
    }

    // ── Args defaults ──

    #[test]
    fn test_args_defaults() {
        let args = Args::default();
        assert!(!args.directory_mode);
        assert!(!args.no_target_directory);
        assert_eq!(args.modes, Modes::default());
        assert!(args.owner.is_none());
        assert!(args.group.is_none());
        assert!(!args.backup);
        assert!(!args.compare);
        assert!(!args.create_dirs);
        assert!(!args.preserve_timestamps);
        assert!(!args.strip);
        assert!(!args.verbose);
    }

    // -- Symbolic mode edge cases --

    /// Several `who` letters before one operator, and several clauses.
    ///
    /// `u=` with an empty permission list clears the user triad, which is the
    /// clause the old parser's `if clause.is_empty()` skip came closest to
    /// breaking: `u=` is not empty, but `` is, and both had to be told apart.
    #[test]
    fn several_who_letters_and_several_clauses() {
        assert_eq!(file_mode("ug+rx"), 0o550);
        assert_eq!(file_mode("u=rwx,g=rx,o="), 0o750);
        assert_eq!(file_mode("a=rwx,u="), 0o077);
        assert!(compile_mode(b"u+z").is_none());
    }

    // ── Temp file naming ──

    #[test]
    fn test_temp_file_prefix() {
        let name = format!(".install-tmp-{}", 12345u64);
        assert!(name.starts_with(".install-tmp-"));
    }
}
