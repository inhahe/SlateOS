//! Slate OS getopt/cksum/sync/printenv — shell scripting helpers
//!
//! Multi-personality binary detected via argv[0]:
//! - `getopt`: Parse command-line options for shell scripts
//! - `cksum`: Print CRC32 checksum and byte count
//! - `sync`: Flush filesystem buffers
//! - `printenv`: Print environment variables

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

// ── Personality detection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Getopt,
    Cksum,
    Sync,
    Printenv,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cksum" => Mode::Cksum,
        "sync" => Mode::Sync,
        "printenv" => Mode::Printenv,
        _ => Mode::Getopt,
    }
}

// ── getopt ─────────────────────────────────────────────────────────
// Enhanced getopt(1) compatible with util-linux getopt

fn run_getopt() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut optstring: Option<String> = None;
    let mut longopts: Vec<LongOpt> = Vec::new();
    let mut name = "getopt".to_string();
    let mut shell = "bash".to_string();
    let mut args_to_parse: Vec<String> = Vec::new();
    let mut alternative = false; // Allow long options with single dash
    let mut enhanced = false;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: getopt [OPTIONS] -- OPTSTRING PARAMETERS");
                eprintln!("   or: getopt [OPTIONS] -o OPTSTRING -- PARAMETERS");
                eprintln!();
                eprintln!("Parse command-line options.");
                eprintln!();
                eprintln!("  -a, --alternative  allow long options with single dash");
                eprintln!("  -l, --longoptions=OPTS  long options (comma-separated)");
                eprintln!("  -n, --name=PROGNAME  program name for errors");
                eprintln!("  -o, --options=OPTSTRING  short option string");
                eprintln!("  -s, --shell=SHELL  quoting for shell (bash, sh, csh, tcsh)");
                eprintln!("  -q, --quiet        suppress error messages");
                eprintln!("  -Q, --quiet-output suppress normal output");
                eprintln!("  -T, --test         test for enhanced getopt");
                process::exit(0);
            }
            "-T" | "--test" => {
                // Return 4 to indicate enhanced getopt
                process::exit(4);
            }
            "-o" | "--options" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-o' requires an argument".to_string());
                }
                optstring = Some(argv[i].clone());
                enhanced = true;
            }
            _ if argv[i].starts_with("--options=") => {
                optstring = Some(argv[i]["--options=".len()..].to_string());
                enhanced = true;
            }
            "-l" | "--longoptions" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-l' requires an argument".to_string());
                }
                longopts = parse_longopts(&argv[i]);
                enhanced = true;
            }
            _ if argv[i].starts_with("--longoptions=") => {
                longopts = parse_longopts(&argv[i]["--longoptions=".len()..]);
                enhanced = true;
            }
            "-n" | "--name" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-n' requires an argument".to_string());
                }
                name = argv[i].clone();
                enhanced = true;
            }
            _ if argv[i].starts_with("--name=") => {
                name = argv[i]["--name=".len()..].to_string();
                enhanced = true;
            }
            "-s" | "--shell" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-s' requires an argument".to_string());
                }
                shell = argv[i].clone();
                enhanced = true;
            }
            _ if argv[i].starts_with("--shell=") => {
                shell = argv[i]["--shell=".len()..].to_string();
                enhanced = true;
            }
            "-a" | "--alternative" => {
                alternative = true;
                enhanced = true;
            }
            "-q" | "--quiet" => {
                enhanced = true;
            }
            "-Q" | "--quiet-output" => {
                enhanced = true;
            }
            "--" => {
                i += 1;
                // Everything after -- is the optstring (if not set) and parameters
                if !enhanced && optstring.is_none() && i < argv.len() {
                    optstring = Some(argv[i].clone());
                    i += 1;
                }
                while i < argv.len() {
                    args_to_parse.push(argv[i].clone());
                    i += 1;
                }
                break;
            }
            _ => {
                if !enhanced && optstring.is_none() {
                    optstring = Some(argv[i].clone());
                } else {
                    args_to_parse.push(argv[i].clone());
                }
            }
        }
        i += 1;
    }

    let opts = optstring.unwrap_or_default();
    let result = parse_options(&opts, &longopts, &args_to_parse, &name, alternative);

    let quoted = quote_for_shell(&result, &shell);
    println!("{quoted}");

    Ok(())
}

#[derive(Debug, Clone)]
struct LongOpt {
    name: String,
    has_arg: ArgReq,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ArgReq {
    None,
    Required,
    Optional,
}

fn parse_longopts(spec: &str) -> Vec<LongOpt> {
    let mut opts = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(name) = part.strip_suffix("::") {
            opts.push(LongOpt { name: name.to_string(), has_arg: ArgReq::Optional });
        } else if let Some(name) = part.strip_suffix(':') {
            opts.push(LongOpt { name: name.to_string(), has_arg: ArgReq::Required });
        } else {
            opts.push(LongOpt { name: part.to_string(), has_arg: ArgReq::None });
        }
    }
    opts
}

fn parse_options(
    optstring: &str,
    longopts: &[LongOpt],
    args: &[String],
    _name: &str,
    _alternative: bool,
) -> Vec<String> {
    let mut result = Vec::new();
    let opt_chars: Vec<char> = optstring.chars().collect();
    let mut i = 0;
    let mut non_option_args: Vec<String> = Vec::new();

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            i += 1;
            while i < args.len() {
                non_option_args.push(args[i].clone());
                i += 1;
            }
            break;
        }

        if arg.starts_with("--") && arg.len() > 2 {
            // Long option
            let rest = &arg[2..];
            let (opt_name, opt_val) = if let Some(eq_pos) = rest.find('=') {
                (&rest[..eq_pos], Some(rest[eq_pos + 1..].to_string()))
            } else {
                (rest, None)
            };

            if let Some(lo) = longopts.iter().find(|o| o.name == opt_name) {
                result.push(format!("--{}", lo.name));
                match lo.has_arg {
                    ArgReq::Required => {
                        if let Some(val) = opt_val {
                            result.push(val);
                        } else {
                            i += 1;
                            if i < args.len() {
                                result.push(args[i].clone());
                            }
                        }
                    }
                    ArgReq::Optional => {
                        if let Some(val) = opt_val {
                            result.push(val);
                        }
                    }
                    ArgReq::None => {}
                }
            } else {
                // Unknown long option
                eprintln!("getopt: unrecognized option '--{opt_name}'");
            }
            i += 1;
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            // Short options
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                // Find this option in optstring
                if let Some(pos) = opt_chars.iter().position(|&oc| oc == c) {
                    result.push(format!("-{c}"));
                    // Check if it takes an argument
                    let next_is_colon = pos + 1 < opt_chars.len() && opt_chars[pos + 1] == ':';
                    let next_is_double = next_is_colon && pos + 2 < opt_chars.len() && opt_chars[pos + 2] == ':';

                    if next_is_double {
                        // Optional argument (must be attached)
                        if j + 1 < chars.len() {
                            let val: String = chars[j + 1..].iter().collect();
                            result.push(val);
                            break;
                        }
                    } else if next_is_colon {
                        // Required argument
                        if j + 1 < chars.len() {
                            let val: String = chars[j + 1..].iter().collect();
                            result.push(val);
                            break;
                        } else {
                            i += 1;
                            if i < args.len() {
                                result.push(args[i].clone());
                            }
                        }
                    }
                } else {
                    eprintln!("getopt: invalid option -- '{c}'");
                }
                j += 1;
            }
            i += 1;
            continue;
        }

        // Non-option argument
        non_option_args.push(arg.clone());
        i += 1;
    }

    result.push("--".to_string());
    result.extend(non_option_args);
    result
}

fn quote_for_shell(parts: &[String], shell: &str) -> String {
    let is_csh = shell == "csh" || shell == "tcsh";
    let mut out = String::new();

    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        if part == "--" {
            out.push_str("--");
        } else if is_csh {
            // C-shell quoting
            out.push('\'');
            for c in part.chars() {
                if c == '\'' {
                    out.push_str("'\\''");
                } else {
                    out.push(c);
                }
            }
            out.push('\'');
        } else {
            // Bourne shell quoting
            out.push('\'');
            for c in part.chars() {
                if c == '\'' {
                    out.push_str("'\\''");
                } else {
                    out.push(c);
                }
            }
            out.push('\'');
        }
    }
    out
}

// ── cksum ──────────────────────────────────────────────────────────

/// CRC-32 using the standard polynomial (reversed: 0xEDB88320).
/// Available for future use (e.g., `cksum --algorithm=crc32`).
#[allow(dead_code)]
fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut crc = i;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
        table[i as usize] = crc;
    }
    table
}

fn posix_cksum(data: &[u8]) -> u32 {
    // POSIX cksum uses a different CRC than standard CRC-32.
    // It processes bytes MSB first with polynomial 0x04C11DB7.
    let mut crc: u32 = 0;

    for &byte in data {
        for bit in (0..8).rev() {
            let b = ((byte >> bit) & 1) as u32;
            if (crc >> 31) ^ b != 0 {
                crc = (crc << 1) ^ 0x04C11DB7;
            } else {
                crc <<= 1;
            }
        }
    }

    // Process length
    let mut len = data.len() as u64;
    while len > 0 {
        let byte = (len & 0xFF) as u8;
        for bit in (0..8).rev() {
            let b = ((byte >> bit) & 1) as u32;
            if (crc >> 31) ^ b != 0 {
                crc = (crc << 1) ^ 0x04C11DB7;
            } else {
                crc <<= 1;
            }
        }
        len >>= 8;
    }

    !crc
}

fn run_cksum() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut files: Vec<String> = Vec::new();

    for arg in &argv[1..] {
        match arg.as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: cksum [FILE]...");
                eprintln!("Print CRC checksum and byte count of each FILE.");
                process::exit(0);
            }
            _ => files.push(arg.clone()),
        }
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    for file in &files {
        let (data, display_name) = if file == "-" {
            let mut buf = Vec::new();
            io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("stdin: {e}"))?;
            (buf, None)
        } else {
            let data = fs::read(file)
                .map_err(|e| format!("{file}: {e}"))?;
            (data, Some(file.as_str()))
        };

        let checksum = posix_cksum(&data);
        let size = data.len();

        match display_name {
            Some(name) => println!("{checksum} {size} {name}"),
            None => println!("{checksum} {size}"),
        }
    }

    Ok(())
}

// ── sync ───────────────────────────────────────────────────────────

fn run_sync() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut data_only = false;
    let mut filesystem_only = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &argv[1..] {
        match arg.as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: sync [OPTION] [FILE]...");
                eprintln!("Flush file system buffers.");
                eprintln!();
                eprintln!("  -d, --data     sync only file data, no metadata");
                eprintln!("  -f, --file-system  sync filesystems containing files");
                process::exit(0);
            }
            "-d" | "--data" => data_only = true,
            "-f" | "--file-system" => filesystem_only = true,
            _ => files.push(arg.clone()),
        }
    }

    if files.is_empty() {
        // Sync everything
        #[cfg(target_os = "slateos")]
        {
            let ret: i64;
            unsafe {
                core::arch::asm!(
                    "syscall",
                    in("rax") 162u64, // SYS_SYNC
                    lateout("rax") ret,
                    lateout("rcx") _,
                    lateout("r11") _,
                );
            }
            if ret < 0 {
                return Err(format!("sync failed: error {}", -ret));
            }
        }
        #[cfg(not(target_os = "slateos"))]
        {
            let _ = (data_only, filesystem_only);
            // No-op on non-SlateOS
        }
    } else {
        // Sync specific files
        for file in &files {
            let f = fs::File::open(file)
                .map_err(|e| format!("{file}: {e}"))?;
            if data_only {
                f.sync_data().map_err(|e| format!("{file}: {e}"))?;
            } else {
                f.sync_all().map_err(|e| format!("{file}: {e}"))?;
            }
        }
    }

    Ok(())
}

// ── printenv ───────────────────────────────────────────────────────

fn run_printenv() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut null_terminated = false;
    let mut vars: Vec<String> = Vec::new();

    for arg in &argv[1..] {
        match arg.as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: printenv [OPTION] [VARIABLE]...");
                eprintln!("Print the values of environment variables.");
                eprintln!();
                eprintln!("  -0, --null  end each line with NUL, not newline");
                process::exit(0);
            }
            "-0" | "--null" => null_terminated = true,
            _ => vars.push(arg.clone()),
        }
    }

    let end = if null_terminated { "\0" } else { "\n" };

    if vars.is_empty() {
        // Print all environment variables
        let mut env_vars: Vec<(String, String)> = env::vars().collect();
        env_vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in &env_vars {
            print!("{key}={value}{end}");
        }
    } else {
        let mut found_all = true;
        for var in &vars {
            match env::var(var) {
                Ok(value) => print!("{value}{end}"),
                Err(_) => found_all = false,
            }
        }
        if !found_all {
            process::exit(1);
        }
    }

    Ok(())
}

// ── Main ───────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv0 = env::args().next().unwrap_or_else(|| "getopt".to_string());
    let mode = detect_mode(&argv0);

    match mode {
        Mode::Getopt => run_getopt(),
        Mode::Cksum => run_cksum(),
        Mode::Sync => run_sync(),
        Mode::Printenv => run_printenv(),
    }
}

fn main() {
    if let Err(e) = run() {
        let prog = env::args().next().unwrap_or_else(|| "getopt".to_string());
        let name = prog
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&prog);
        eprintln!("{name}: {e}");
        process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality detection ──

    #[test]
    fn test_detect_getopt() {
        assert_eq!(detect_mode("getopt"), Mode::Getopt);
        assert_eq!(detect_mode("/usr/bin/getopt"), Mode::Getopt);
    }

    #[test]
    fn test_detect_cksum() {
        assert_eq!(detect_mode("cksum"), Mode::Cksum);
        assert_eq!(detect_mode("/bin/cksum"), Mode::Cksum);
    }

    #[test]
    fn test_detect_sync() {
        assert_eq!(detect_mode("sync"), Mode::Sync);
    }

    #[test]
    fn test_detect_printenv() {
        assert_eq!(detect_mode("printenv"), Mode::Printenv);
    }

    #[test]
    fn test_detect_default() {
        assert_eq!(detect_mode("unknown"), Mode::Getopt);
    }

    // ── Longopt parsing ──

    #[test]
    fn test_parse_longopts() {
        let opts = parse_longopts("verbose,output:,debug::");
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].name, "verbose");
        assert_eq!(opts[0].has_arg, ArgReq::None);
        assert_eq!(opts[1].name, "output");
        assert_eq!(opts[1].has_arg, ArgReq::Required);
        assert_eq!(opts[2].name, "debug");
        assert_eq!(opts[2].has_arg, ArgReq::Optional);
    }

    #[test]
    fn test_parse_longopts_empty() {
        let opts = parse_longopts("");
        assert!(opts.is_empty());
    }

    #[test]
    fn test_parse_longopts_single() {
        let opts = parse_longopts("help");
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].name, "help");
        assert_eq!(opts[0].has_arg, ArgReq::None);
    }

    // ── Option parsing ──

    #[test]
    fn test_parse_short_options() {
        let result = parse_options("abc:", &[], &[
            "-a".to_string(), "-b".to_string(), "-c".to_string(), "value".to_string()
        ], "test", false);
        assert!(result.contains(&"-a".to_string()));
        assert!(result.contains(&"-b".to_string()));
        assert!(result.contains(&"-c".to_string()));
        assert!(result.contains(&"value".to_string()));
    }

    #[test]
    fn test_parse_combined_short() {
        let result = parse_options("ab", &[], &["-ab".to_string()], "test", false);
        assert!(result.contains(&"-a".to_string()));
        assert!(result.contains(&"-b".to_string()));
    }

    #[test]
    fn test_parse_long_options() {
        let longopts = parse_longopts("verbose,output:");
        let result = parse_options("", &longopts, &[
            "--verbose".to_string(), "--output".to_string(), "file.txt".to_string()
        ], "test", false);
        assert!(result.contains(&"--verbose".to_string()));
        assert!(result.contains(&"--output".to_string()));
        assert!(result.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_parse_long_option_with_equals() {
        let longopts = parse_longopts("output:");
        let result = parse_options("", &longopts, &["--output=file.txt".to_string()], "test", false);
        assert!(result.contains(&"--output".to_string()));
        assert!(result.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_parse_separator() {
        let result = parse_options("a", &[], &[
            "-a".to_string(), "--".to_string(), "non-opt".to_string()
        ], "test", false);
        let sep_pos = result.iter().position(|s| s == "--").unwrap_or(0);
        assert!(sep_pos > 0);
        assert!(result[sep_pos + 1..].contains(&"non-opt".to_string()));
    }

    #[test]
    fn test_parse_non_option_args() {
        let result = parse_options("a", &[], &[
            "-a".to_string(), "file1".to_string(), "file2".to_string()
        ], "test", false);
        let sep_pos = result.iter().position(|s| s == "--").unwrap_or(0);
        let non_opts = &result[sep_pos + 1..];
        assert!(non_opts.contains(&"file1".to_string()));
        assert!(non_opts.contains(&"file2".to_string()));
    }

    // ── Shell quoting ──

    #[test]
    fn test_quote_simple() {
        let parts = vec!["-a".to_string(), "--".to_string(), "file".to_string()];
        let quoted = quote_for_shell(&parts, "bash");
        assert!(quoted.contains("'-a'"));
        assert!(quoted.contains("--"));
        assert!(quoted.contains("'file'"));
    }

    #[test]
    fn test_quote_with_spaces() {
        let parts = vec!["hello world".to_string()];
        let quoted = quote_for_shell(&parts, "bash");
        assert_eq!(quoted, "'hello world'");
    }

    #[test]
    fn test_quote_with_single_quote() {
        let parts = vec!["it's".to_string()];
        let quoted = quote_for_shell(&parts, "bash");
        assert!(quoted.contains("'\\''"));
    }

    // ── POSIX cksum ──

    #[test]
    fn test_cksum_empty() {
        let crc = posix_cksum(b"");
        assert_eq!(crc, 4294967295); // Known CRC for empty input
    }

    #[test]
    fn test_cksum_single_byte() {
        let crc = posix_cksum(b"a");
        // The POSIX cksum of "a" is a specific value
        assert_ne!(crc, 0);
    }

    #[test]
    fn test_cksum_deterministic() {
        let data = b"Hello, World!";
        let crc1 = posix_cksum(data);
        let crc2 = posix_cksum(data);
        assert_eq!(crc1, crc2);
    }

    #[test]
    fn test_cksum_different_data() {
        let crc1 = posix_cksum(b"hello");
        let crc2 = posix_cksum(b"world");
        assert_ne!(crc1, crc2);
    }

    // ── CRC32 table ──

    #[test]
    fn test_crc32_table_size() {
        let table = crc32_table();
        assert_eq!(table.len(), 256);
        assert_eq!(table[0], 0); // CRC of 0 is 0
    }

    #[test]
    fn test_crc32_table_nonzero() {
        let table = crc32_table();
        // Most entries should be nonzero
        let nonzero_count = table.iter().filter(|&&v| v != 0).count();
        assert!(nonzero_count > 200);
    }

    // ── Edge cases ──

    #[test]
    fn test_parse_empty_args() {
        let result = parse_options("", &[], &[], "test", false);
        assert_eq!(result, vec!["--".to_string()]);
    }

    #[test]
    fn test_parse_only_separator() {
        let result = parse_options("", &[], &["--".to_string()], "test", false);
        assert_eq!(result, vec!["--".to_string()]);
    }

    #[test]
    fn test_longopts_trailing_comma() {
        let opts = parse_longopts("help,verbose,");
        assert_eq!(opts.len(), 2);
    }

    #[test]
    fn test_quote_separator() {
        let parts = vec!["--".to_string()];
        let quoted = quote_for_shell(&parts, "bash");
        assert_eq!(quoted, "--");
    }

    #[test]
    fn test_quote_empty_parts() {
        let parts: Vec<String> = Vec::new();
        let quoted = quote_for_shell(&parts, "bash");
        assert_eq!(quoted, "");
    }
}
