//! OurOS install — copy files and set attributes
//!
//! GNU coreutils-compatible `install` command for copying files
//! with specified permissions, ownership, and directory creation.

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ── Mode constants ─────────────────────────────────────────────────

const DEFAULT_MODE: u32 = 0o755;
const DEFAULT_FILE_MODE: u32 = 0o755; // install default, NOT 0o644

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
    /// File mode (-m MODE)
    mode: u32,
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
            mode: DEFAULT_FILE_MODE,
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

fn parse_mode(s: &str) -> Result<u32, String> {
    // Try octal first
    if let Some(stripped) = s.strip_prefix('0') {
        if stripped.is_empty() {
            return Ok(0);
        }
        return u32::from_str_radix(stripped, 8)
            .map_err(|_| format!("invalid mode: '{s}'"));
    }

    // Try as plain octal digits
    if s.chars().all(|c| c.is_ascii_digit()) {
        return u32::from_str_radix(s, 8)
            .map_err(|_| format!("invalid mode: '{s}'"));
    }

    // Symbolic mode parsing (simplified: u+rwx,g+rx,o+rx style)
    parse_symbolic_mode(s, DEFAULT_FILE_MODE)
}

fn parse_symbolic_mode(s: &str, base: u32) -> Result<u32, String> {
    let mut result = base;

    for clause in s.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }

        let mut chars = clause.chars().peekable();

        // Parse who: u, g, o, a
        let mut who_mask = 0u32;
        while let Some(&c) = chars.peek() {
            match c {
                'u' => { who_mask |= 0o700; chars.next(); }
                'g' => { who_mask |= 0o070; chars.next(); }
                'o' => { who_mask |= 0o007; chars.next(); }
                'a' => { who_mask |= 0o777; chars.next(); }
                _ => break,
            }
        }
        if who_mask == 0 {
            who_mask = 0o777; // Default: all
        }

        // Parse op: +, -, =
        let op = chars.next().ok_or_else(|| format!("invalid mode: '{s}'"))?;
        if op != '+' && op != '-' && op != '=' {
            return Err(format!("invalid mode operator: '{op}'"));
        }

        // Parse perms: r, w, x, s, t, X
        let mut perm_bits = 0u32;
        for c in chars {
            match c {
                'r' => perm_bits |= 0o444,
                'w' => perm_bits |= 0o222,
                'x' => perm_bits |= 0o111,
                'X' => perm_bits |= 0o111, // Treat X like x for simplicity
                's' => perm_bits |= 0o6000,
                't' => perm_bits |= 0o1000,
                _ => return Err(format!("invalid permission character: '{c}'")),
            }
        }

        let effective = perm_bits & who_mask;
        match op {
            '+' => result |= effective,
            '-' => result &= !effective,
            '=' => {
                result &= !who_mask;
                result |= effective;
            }
            _ => {}
        }
    }

    Ok(result & 0o7777)
}

fn parse_args() -> Args {
    let argv: Vec<String> = env::args().collect();
    let mut args = Args::default();
    let mut positionals = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--version" => {
                println!("install (OurOS) 0.1.0");
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
                args.mode = parse_mode(&argv[i]).unwrap_or_else(|e| {
                    eprintln!("install: {e}");
                    process::exit(1);
                });
            }
            _ if arg.starts_with("--mode=") => {
                let val = &arg["--mode=".len()..];
                args.mode = parse_mode(val).unwrap_or_else(|e| {
                    eprintln!("install: {e}");
                    process::exit(1);
                });
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
                            args.mode = parse_mode(&mode_str).unwrap_or_else(|e| {
                                eprintln!("install: {e}");
                                process::exit(1);
                            });
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

    if args.directory_mode {
        // -d: all positionals are directories to create
        args.sources = positionals;
    } else if let Some(ref _td) = args.target_directory {
        // -t DIR: all positionals are sources
        args.sources = positionals;
    } else if args.no_target_directory {
        // -T: exactly src dest
        if positionals.len() != 2 {
            eprintln!("install: with -T, exactly two arguments required");
            process::exit(1);
        }
        args.sources = vec![positionals[0].clone()];
        args.target = Some(positionals[1].clone());
    } else {
        // Normal: last arg is target, rest are sources
        if positionals.len() < 2 {
            if positionals.len() == 1 && args.create_dirs {
                // -D with single arg: create parent dirs, then... need a source
                eprintln!("install: missing destination file operand after '{}'", positionals[0]);
                process::exit(1);
            }
            eprintln!("install: missing file operand");
            process::exit(1);
        }
        args.target = Some(positionals.pop().unwrap_or_default());
        args.sources = positionals;
    }

    args
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

    let content = fs::read_to_string("/etc/passwd")
        .map_err(|e| format!("cannot read /etc/passwd: {e}"))?;
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

    let content = fs::read_to_string("/etc/group")
        .map_err(|e| format!("cannot read /etc/group: {e}"))?;
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
    #[cfg(target_os = "ouros")]
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
    #[cfg(not(target_os = "ouros"))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

#[allow(dead_code)]
fn sys_chown(path: &str, uid: u32, gid: u32) -> io::Result<()> {
    #[cfg(target_os = "ouros")]
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
    #[cfg(not(target_os = "ouros"))]
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

fn create_directory_with_parents(path: &Path, mode: u32, verbose: bool) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }

    // Create parent directories
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            create_directory_with_parents(parent, mode, verbose)?;
        }
    }

    fs::create_dir(path)
        .map_err(|e| format!("cannot create directory '{}': {e}", path.display()))?;

    if verbose {
        println!("install: creating directory '{}'", path.display());
    }

    // Set mode on OurOS
    sys_chmod(&path.to_string_lossy(), mode)
        .map_err(|e| format!("cannot set mode on '{}': {e}", path.display()))?;

    Ok(())
}

// ── Install file ───────────────────────────────────────────────────

fn install_file(
    src: &Path,
    dst: &Path,
    args: &Args,
) -> Result<(), String> {
    // Compare mode: skip if files are identical
    if args.compare && files_are_same(src, dst) {
        return Ok(());
    }

    // Backup existing file
    if args.backup && dst.exists() {
        let backup_path = format!("{}{}", dst.display(), args.backup_suffix);
        fs::rename(dst, &backup_path)
            .map_err(|e| format!(
                "cannot backup '{}' to '{backup_path}': {e}",
                dst.display()
            ))?;
    }

    // Create parent directories if -D
    if args.create_dirs {
        if let Some(parent) = dst.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                create_directory_with_parents(parent, DEFAULT_MODE, args.verbose)?;
            }
        }
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
    let data = fs::read(src)
        .map_err(|e| format!("cannot read '{}': {e}", src.display()))?;

    // Write to temp
    let mut tmp_file = fs::File::create(&temp_name)
        .map_err(|e| format!("cannot create temp file: {e}"))?;
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
    sys_chmod(&dst.to_string_lossy(), args.mode)
        .map_err(|e| format!("cannot set mode on '{}': {e}", dst.display()))?;

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
        // On OurOS we'd copy atime/mtime from source via syscall.
        // For now, this is a placeholder that will work when the
        // utimensat syscall is available.
        #[cfg(target_os = "ouros")]
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
                    "install: strip program '{}' failed with exit code {}",
                    args.strip_program,
                    s.code().unwrap_or(-1)
                );
            }
            Err(e) => {
                eprintln!(
                    "install: cannot run strip program '{}': {e}",
                    args.strip_program
                );
            }
        }
    }

    if args.verbose {
        println!("'{}' -> '{}'", src.display(), dst.display());
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
            create_directory_with_parents(&path, args.mode, args.verbose)?;
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
                create_directory_with_parents(td, DEFAULT_MODE, args.verbose)?;
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
        let dst_path = PathBuf::from(
            args.target
                .as_deref()
                .ok_or("missing destination")?,
        );
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

    // ── Mode parsing ──

    #[test]
    fn test_parse_mode_octal() {
        assert_eq!(parse_mode("0755").unwrap(), 0o755);
        assert_eq!(parse_mode("0644").unwrap(), 0o644);
        assert_eq!(parse_mode("0600").unwrap(), 0o600);
        assert_eq!(parse_mode("0777").unwrap(), 0o777);
    }

    #[test]
    fn test_parse_mode_no_leading_zero() {
        assert_eq!(parse_mode("755").unwrap(), 0o755);
        assert_eq!(parse_mode("644").unwrap(), 0o644);
    }

    #[test]
    fn test_parse_mode_zero() {
        assert_eq!(parse_mode("0").unwrap(), 0);
        assert_eq!(parse_mode("00").unwrap(), 0);
    }

    #[test]
    fn test_parse_mode_symbolic_user() {
        assert_eq!(parse_symbolic_mode("u+rwx", 0).unwrap(), 0o700);
        assert_eq!(parse_symbolic_mode("u+r", 0).unwrap(), 0o400);
        assert_eq!(parse_symbolic_mode("u+w", 0).unwrap(), 0o200);
        assert_eq!(parse_symbolic_mode("u+x", 0).unwrap(), 0o100);
    }

    #[test]
    fn test_parse_mode_symbolic_group() {
        assert_eq!(parse_symbolic_mode("g+rwx", 0).unwrap(), 0o070);
        assert_eq!(parse_symbolic_mode("g+rx", 0).unwrap(), 0o050);
    }

    #[test]
    fn test_parse_mode_symbolic_other() {
        assert_eq!(parse_symbolic_mode("o+rwx", 0).unwrap(), 0o007);
    }

    #[test]
    fn test_parse_mode_symbolic_all() {
        assert_eq!(parse_symbolic_mode("a+rwx", 0).unwrap(), 0o777);
        assert_eq!(parse_symbolic_mode("a+r", 0).unwrap(), 0o444);
    }

    #[test]
    fn test_parse_mode_symbolic_remove() {
        assert_eq!(parse_symbolic_mode("a-w", 0o777).unwrap(), 0o555);
        assert_eq!(parse_symbolic_mode("o-rwx", 0o777).unwrap(), 0o770);
    }

    #[test]
    fn test_parse_mode_symbolic_equals() {
        assert_eq!(parse_symbolic_mode("u=rwx", 0o777).unwrap(), 0o777);
        assert_eq!(parse_symbolic_mode("u=r", 0o777).unwrap(), 0o477);
    }

    #[test]
    fn test_parse_mode_symbolic_combined() {
        assert_eq!(parse_symbolic_mode("u+rwx,g+rx,o+r", 0).unwrap(), 0o754);
    }

    #[test]
    fn test_parse_mode_symbolic_default_who() {
        // No who specified = all
        assert_eq!(parse_symbolic_mode("+r", 0).unwrap(), 0o444);
    }

    #[test]
    fn test_parse_mode_symbolic_setuid() {
        let result = parse_symbolic_mode("u+s", 0).unwrap();
        assert_eq!(result & 0o4000, 0o4000);
    }

    #[test]
    fn test_parse_mode_symbolic_sticky() {
        let result = parse_symbolic_mode("+t", 0).unwrap();
        assert_eq!(result & 0o1000, 0o1000);
    }

    #[test]
    fn test_parse_mode_invalid() {
        assert!(parse_mode("abc").is_err());
        assert!(parse_mode("999").is_err());
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

    #[test]
    fn test_default_mode() {
        assert_eq!(DEFAULT_MODE, 0o755);
        assert_eq!(DEFAULT_FILE_MODE, 0o755);
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
        assert_eq!(args.mode, 0o755);
        assert!(args.owner.is_none());
        assert!(args.group.is_none());
        assert!(!args.backup);
        assert!(!args.compare);
        assert!(!args.create_dirs);
        assert!(!args.preserve_timestamps);
        assert!(!args.strip);
        assert!(!args.verbose);
    }

    // ── Symbolic mode edge cases ──

    #[test]
    fn test_symbolic_mode_ug() {
        assert_eq!(parse_symbolic_mode("ug+rx", 0).unwrap(), 0o550);
    }

    #[test]
    fn test_symbolic_mode_multiple_clauses() {
        assert_eq!(
            parse_symbolic_mode("u=rwx,g=rx,o=", 0).unwrap(),
            0o750
        );
    }

    #[test]
    fn test_symbolic_mode_empty_perms() {
        // "u=" clears user bits
        assert_eq!(parse_symbolic_mode("u=", 0o777).unwrap(), 0o077);
    }

    #[test]
    fn test_symbolic_mode_invalid_char() {
        assert!(parse_symbolic_mode("u+z", 0).is_err());
    }

    // ── Temp file naming ──

    #[test]
    fn test_temp_file_prefix() {
        let name = format!(".install-tmp-{}", 12345u64);
        assert!(name.starts_with(".install-tmp-"));
    }
}
