//! OurOS tput/reset/clear — terminal capability tools
//!
//! Multi-personality binary detected via argv[0]:
//! - `tput`: Query and set terminal capabilities
//! - `reset`: Reset terminal to sane state
//! - `clear`: Clear the terminal screen

#![allow(unexpected_cfgs)]

use std::env;
use std::io::{self, Write};
use std::process;

// ── Personality detection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Tput,
    Reset,
    Clear,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "reset" | "tset" => Mode::Reset,
        "clear" => Mode::Clear,
        _ => Mode::Tput,
    }
}

// ── Terminal capability database ───────────────────────────────────

/// Built-in terminfo-like capability database for common terminals.
/// We support xterm, vt100, linux, dumb, and ouros terminal types.
struct TermCaps {
    term_type: String,
}

impl TermCaps {
    fn new() -> Self {
        let term_type = env::var("TERM").unwrap_or_else(|_| "xterm".to_string());
        Self { term_type }
    }

    fn with_term(term: &str) -> Self {
        Self {
            term_type: term.to_string(),
        }
    }

    fn is_dumb(&self) -> bool {
        self.term_type == "dumb" || self.term_type.is_empty()
    }

    /// Get a string capability value
    fn get_string(&self, cap: &str) -> Option<String> {
        if self.is_dumb() {
            return match cap {
                "cr" => Some("\r".to_string()),
                "bel" => Some("\x07".to_string()),
                _ => None,
            };
        }

        // xterm/vt100/linux/ouros all share most ANSI capabilities
        match cap {
            // Cursor movement
            "clear" | "cl" => Some("\x1B[H\x1B[2J".to_string()),
            "home" | "ho" => Some("\x1B[H".to_string()),
            "cup" => Some("\x1B[%i%p1%d;%p2%dH".to_string()),
            "cuu1" | "up" => Some("\x1B[A".to_string()),
            "cud1" | "do" => Some("\x1B[B".to_string()),
            "cuf1" | "nd" => Some("\x1B[C".to_string()),
            "cub1" | "le" => Some("\x08".to_string()),
            "cr" => Some("\r".to_string()),
            "nel" => Some("\n".to_string()),

            // Scrolling
            "ind" | "sf" => Some("\n".to_string()),
            "ri" | "sr" => Some("\x1BM".to_string()),

            // Screen manipulation
            "ed" | "cd" => Some("\x1B[J".to_string()),    // Clear to end of screen
            "el" | "ce" => Some("\x1B[K".to_string()),    // Clear to end of line
            "el1" | "cb" => Some("\x1B[1K".to_string()),  // Clear to beginning of line
            "smcup" | "ti" => Some("\x1B[?1049h".to_string()), // Enter alternate screen
            "rmcup" | "te" => Some("\x1B[?1049l".to_string()), // Exit alternate screen

            // Text attributes
            "bold" | "md" => Some("\x1B[1m".to_string()),
            "dim" | "mh" => Some("\x1B[2m".to_string()),
            "smul" | "us" => Some("\x1B[4m".to_string()),
            "rmul" | "ue" => Some("\x1B[24m".to_string()),
            "blink" | "mb" => Some("\x1B[5m".to_string()),
            "rev" | "mr" => Some("\x1B[7m".to_string()),
            "smso" | "so" => Some("\x1B[7m".to_string()),
            "rmso" | "se" => Some("\x1B[27m".to_string()),
            "sgr0" | "me" => Some("\x1B[0m".to_string()),

            // Visibility
            "civis" | "vi" => Some("\x1B[?25l".to_string()),
            "cnorm" | "ve" => Some("\x1B[?25h".to_string()),
            "cvvis" | "vs" => Some("\x1B[?12h\x1B[?25h".to_string()),

            // Insert/delete
            "ich1" | "ic" => Some("\x1B[@".to_string()),
            "dch1" | "dc" => Some("\x1B[P".to_string()),
            "il1" | "al" => Some("\x1B[L".to_string()),
            "dl1" | "dl" => Some("\x1B[M".to_string()),

            // Colors
            "setaf" => Some("\x1B[3%p1%dm".to_string()),
            "setab" => Some("\x1B[4%p1%dm".to_string()),
            "op" => Some("\x1B[39;49m".to_string()),

            // Keypad
            "smkx" | "ks" => Some("\x1B[?1h\x1B=".to_string()),
            "rmkx" | "ke" => Some("\x1B[?1l\x1B>".to_string()),

            // Bell
            "bel" => Some("\x07".to_string()),

            // Tab
            "ht" => Some("\t".to_string()),

            // Save/restore cursor
            "sc" => Some("\x1B7".to_string()),
            "rc" => Some("\x1B8".to_string()),

            // Terminal reset
            "rs1" | "r1" => Some("\x1Bc".to_string()),  // Full reset
            "rs2" | "r2" => Some("\x1B[!p\x1B[?3;4l\x1B[4l\x1B>".to_string()),

            _ => None,
        }
    }

    /// Get a numeric capability value
    fn get_num(&self, cap: &str) -> Option<i32> {
        if self.is_dumb() {
            return match cap {
                "cols" | "co" => Some(80),
                "lines" | "li" => Some(24),
                _ => None,
            };
        }

        match cap {
            "cols" | "co" => Some(get_terminal_cols()),
            "lines" | "li" => Some(get_terminal_lines()),
            "colors" => {
                if self.term_type.contains("256color") {
                    Some(256)
                } else if self.is_dumb() {
                    Some(0)
                } else {
                    Some(8)
                }
            }
            "pairs" => {
                if self.term_type.contains("256color") {
                    Some(65536)
                } else {
                    Some(64)
                }
            }
            "it" => Some(8), // Initial tab spacing
            _ => None,
        }
    }

    /// Get a boolean capability value
    fn get_bool(&self, cap: &str) -> bool {
        if self.is_dumb() {
            return false;
        }

        match cap {
            "am" => true,     // Auto-margin (line wrap)
            "bce" => true,    // Background color erase
            "bw" => false,    // Backspace wraps
            "eo" => true,     // Can erase overstrikes
            "hs" => false,    // Has status line
            "hz" => false,    // Hazeltine bug
            "km" => true,     // Has meta key
            "mir" => true,    // Move in insert mode
            "msgr" => true,   // Move in standout mode
            "xenl" => true,   // Newline glitch
            "xon" => true,    // Uses XON/XOFF
            "ccc" => self.term_type.contains("xterm"), // Can change colors
            _ => false,
        }
    }
}

// ── Terminal size detection ────────────────────────────────────────

fn get_terminal_cols() -> i32 {
    // Try COLUMNS env var first
    if let Ok(val) = env::var("COLUMNS") {
        if let Ok(n) = val.parse::<i32>() {
            return n;
        }
    }

    // Try ioctl on OurOS
    #[cfg(target_os = "ouros")]
    {
        if let Some(size) = get_terminal_size_ioctl() {
            return size.0;
        }
    }

    80 // Default
}

fn get_terminal_lines() -> i32 {
    // Try LINES env var first
    if let Ok(val) = env::var("LINES") {
        if let Ok(n) = val.parse::<i32>() {
            return n;
        }
    }

    #[cfg(target_os = "ouros")]
    {
        if let Some(size) = get_terminal_size_ioctl() {
            return size.1;
        }
    }

    24 // Default
}

#[cfg(target_os = "ouros")]
#[allow(dead_code)]
fn get_terminal_size_ctl() -> Option<(i32, i32)> {
    // OurOS ioctl to get terminal size
    // Returns (cols, lines) or None
    let mut cols: u16 = 0;
    let mut lines: u16 = 0;
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 16u64, // SYS_IOCTL
            in("rdi") 1u64,  // stdout fd
            in("rsi") 0x5413u64, // TIOCGWINSZ
            in("rdx") &mut [lines, cols, 0u16, 0u16] as *mut _ as u64,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    if ret == 0 && cols > 0 && lines > 0 {
        Some((cols as i32, lines as i32))
    } else {
        None
    }
}

// ── Parameterized string expansion ─────────────────────────────────

/// Simple terminfo-style parameter expansion.
/// Supports: %d (decimal), %p1..%p9 (push param), %i (increment params 1 and 2)
fn expand_params(template: &str, params: &[i32]) -> String {
    let mut out = String::new();
    let mut stack: Vec<i32> = Vec::new();
    let mut params = params.to_vec();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut incremented = false;

    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            match chars[i + 1] {
                '%' => {
                    out.push('%');
                    i += 2;
                }
                'd' => {
                    let val = stack.pop().unwrap_or(0);
                    out.push_str(&val.to_string());
                    i += 2;
                }
                's' => {
                    let _val = stack.pop().unwrap_or(0);
                    // String parameter not commonly used; skip
                    i += 2;
                }
                'c' => {
                    let val = stack.pop().unwrap_or(0);
                    if val >= 0 && val < 128 {
                        out.push(val as u8 as char);
                    }
                    i += 2;
                }
                'i' => {
                    if !incremented {
                        if !params.is_empty() {
                            params[0] = params[0].saturating_add(1);
                        }
                        if params.len() > 1 {
                            params[1] = params[1].saturating_add(1);
                        }
                        incremented = true;
                    }
                    i += 2;
                }
                'p' if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() => {
                    let param_idx = (chars[i + 2] as u32 - '0' as u32) as usize;
                    let val = if param_idx > 0 && param_idx <= params.len() {
                        params[param_idx - 1]
                    } else {
                        0
                    };
                    stack.push(val);
                    i += 3;
                }
                _ => {
                    out.push('%');
                    i += 1;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// ── clear mode ─────────────────────────────────────────────────────

fn run_clear() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut term: Option<String> = None;
    let mut clear_scrollback = false;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: clear [-T term] [-x]");
                eprintln!("Clear the terminal screen.");
                eprintln!();
                eprintln!("  -T TERM   use TERM type instead of $TERM");
                eprintln!("  -x        also clear scrollback buffer");
                process::exit(0);
            }
            "-T" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-T' requires an argument".to_string());
                }
                term = Some(argv[i].clone());
            }
            "-x" => clear_scrollback = true,
            _ => {}
        }
        i += 1;
    }

    let caps = match term {
        Some(ref t) => TermCaps::with_term(t),
        None => TermCaps::new(),
    };

    let mut stdout = io::stdout();

    if caps.is_dumb() {
        // Dumb terminal: just print newlines
        for _ in 0..24 {
            writeln!(stdout).map_err(|e| format!("write: {e}"))?;
        }
    } else {
        // Send clear sequence
        if let Some(seq) = caps.get_string("clear") {
            stdout
                .write_all(seq.as_bytes())
                .map_err(|e| format!("write: {e}"))?;
        }

        // Clear scrollback if requested
        if clear_scrollback {
            stdout
                .write_all(b"\x1B[3J")
                .map_err(|e| format!("write: {e}"))?;
        }
    }

    stdout.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

// ── reset mode ─────────────────────────────────────────────────────

fn run_reset() -> Result<(), String> {
    let mut stdout = io::stdout();

    // Full terminal reset sequence:
    // 1. RIS (Reset to Initial State)
    stdout
        .write_all(b"\x1Bc")
        .map_err(|e| format!("write: {e}"))?;

    // 2. Clear screen
    stdout
        .write_all(b"\x1B[H\x1B[2J")
        .map_err(|e| format!("write: {e}"))?;

    // 3. Reset attributes
    stdout
        .write_all(b"\x1B[0m")
        .map_err(|e| format!("write: {e}"))?;

    // 4. Show cursor
    stdout
        .write_all(b"\x1B[?25h")
        .map_err(|e| format!("write: {e}"))?;

    // 5. Exit alternate screen if in one
    stdout
        .write_all(b"\x1B[?1049l")
        .map_err(|e| format!("write: {e}"))?;

    // 6. Reset keypad mode
    stdout
        .write_all(b"\x1B>")
        .map_err(|e| format!("write: {e}"))?;

    // 7. Set default tabs (every 8 columns)
    stdout
        .write_all(b"\x1B[3g") // Clear all tabs
        .map_err(|e| format!("write: {e}"))?;

    // 8. Reset character set
    stdout
        .write_all(b"\x1B(B") // ASCII charset
        .map_err(|e| format!("write: {e}"))?;

    // 9. Normal screen mode (exit reverse video)
    stdout
        .write_all(b"\x1B[?5l")
        .map_err(|e| format!("write: {e}"))?;

    stdout.flush().map_err(|e| format!("flush: {e}"))?;

    eprintln!("Terminal reset to sane state.");
    Ok(())
}

// ── tput mode ──────────────────────────────────────────────────────

fn run_tput() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut term: Option<String> = None;
    let mut caps_to_query: Vec<(String, Vec<String>)> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                print_tput_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("tput (OurOS) 0.1.0");
                process::exit(0);
            }
            "-T" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-T' requires an argument".to_string());
                }
                term = Some(argv[i].clone());
            }
            "-S" => {
                // Read capabilities from stdin
                let stdin = io::stdin();
                let mut line = String::new();
                while stdin.read_line(&mut line).map_err(|e| format!("read: {e}"))? > 0 {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        let parts: Vec<String> = trimmed.split_whitespace().map(String::from).collect();
                        if !parts.is_empty() {
                            let name = parts[0].clone();
                            let params = parts[1..].to_vec();
                            caps_to_query.push((name, params));
                        }
                    }
                    line.clear();
                }
            }
            _ if argv[i].starts_with('-') => {
                return Err(format!("unknown option '{}'", argv[i]));
            }
            _ => {
                let name = argv[i].clone();
                let mut params = Vec::new();
                // Collect numeric parameters that follow
                while i + 1 < argv.len() && argv[i + 1].parse::<i32>().is_ok() {
                    i += 1;
                    params.push(argv[i].clone());
                }
                caps_to_query.push((name, params));
            }
        }
        i += 1;
    }

    if caps_to_query.is_empty() {
        return Err("usage: tput [-T term] capname [params...]".to_string());
    }

    let caps = match term {
        Some(ref t) => TermCaps::with_term(t),
        None => TermCaps::new(),
    };

    let mut exit_code = 0;
    let mut stdout = io::stdout();

    for (cap_name, params) in &caps_to_query {
        match cap_name.as_str() {
            // Special commands
            "init" => {
                // Initialize terminal
                if let Some(seq) = caps.get_string("smkx") {
                    stdout.write_all(seq.as_bytes()).map_err(|e| format!("write: {e}"))?;
                }
            }
            "reset" => {
                run_reset()?;
            }
            "clear" => {
                run_clear()?;
            }
            "longname" => {
                let name = match caps.term_type.as_str() {
                    "xterm" | "xterm-256color" => "xterm terminal emulator",
                    "vt100" => "DEC VT100",
                    "linux" => "Linux console",
                    "dumb" => "dumb terminal",
                    "ouros" => "OurOS terminal",
                    other => other,
                };
                println!("{name}");
            }
            _ => {
                // Try boolean first
                if caps.get_bool(cap_name) {
                    // Boolean true: exit 0, no output
                    continue;
                }

                // Try numeric
                if let Some(val) = caps.get_num(cap_name) {
                    println!("{val}");
                    continue;
                }

                // Try string
                if let Some(template) = caps.get_string(cap_name) {
                    // Expand parameters
                    let int_params: Vec<i32> = params
                        .iter()
                        .filter_map(|p| p.parse::<i32>().ok())
                        .collect();

                    if int_params.is_empty() && !template.contains('%') {
                        // Simple string, output directly
                        stdout
                            .write_all(template.as_bytes())
                            .map_err(|e| format!("write: {e}"))?;
                    } else {
                        let expanded = expand_params(&template, &int_params);
                        stdout
                            .write_all(expanded.as_bytes())
                            .map_err(|e| format!("write: {e}"))?;
                    }
                    continue;
                }

                // Unknown capability
                eprintln!("tput: unknown terminfo capability '{cap_name}'");
                exit_code = 1;
            }
        }
    }

    stdout.flush().map_err(|e| format!("flush: {e}"))?;

    if exit_code != 0 {
        process::exit(exit_code);
    }
    Ok(())
}

fn print_tput_usage() {
    eprintln!("Usage: tput [-T term] capname [params...]");
    eprintln!("       tput [-T term] -S  (read capabilities from stdin)");
    eprintln!("       tput init");
    eprintln!("       tput reset");
    eprintln!("       tput clear");
    eprintln!("       tput longname");
    eprintln!();
    eprintln!("Query/set terminal capabilities.");
    eprintln!();
    eprintln!("Common capabilities:");
    eprintln!("  cols        number of columns");
    eprintln!("  lines       number of lines");
    eprintln!("  colors      number of colors");
    eprintln!("  bold        start bold mode");
    eprintln!("  sgr0        reset attributes");
    eprintln!("  setaf N     set foreground color N");
    eprintln!("  setab N     set background color N");
    eprintln!("  cup R C     move cursor to row R, column C");
    eprintln!("  clear       clear screen");
    eprintln!("  civis       hide cursor");
    eprintln!("  cnorm       show cursor");
    eprintln!("  smcup       enter alternate screen");
    eprintln!("  rmcup       exit alternate screen");
    eprintln!("  smul        start underline");
    eprintln!("  rmul        stop underline");
    eprintln!("  rev         start reverse video");
    eprintln!("  sc          save cursor position");
    eprintln!("  rc          restore cursor position");
}

// ── Main ───────────────────────────────────────────────────────────

fn main() {
    let argv0 = env::args().next().unwrap_or_else(|| "tput".to_string());
    let mode = detect_mode(&argv0);

    let result = match mode {
        Mode::Clear => run_clear(),
        Mode::Reset => run_reset(),
        Mode::Tput => run_tput(),
    };

    if let Err(e) = result {
        let name = argv0
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .unwrap_or(&argv0);
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
    fn test_detect_tput() {
        assert_eq!(detect_mode("tput"), Mode::Tput);
        assert_eq!(detect_mode("/usr/bin/tput"), Mode::Tput);
        assert_eq!(detect_mode("tput.exe"), Mode::Tput);
    }

    #[test]
    fn test_detect_clear() {
        assert_eq!(detect_mode("clear"), Mode::Clear);
        assert_eq!(detect_mode("/usr/bin/clear"), Mode::Clear);
    }

    #[test]
    fn test_detect_reset() {
        assert_eq!(detect_mode("reset"), Mode::Reset);
        assert_eq!(detect_mode("./reset.exe"), Mode::Reset);
    }

    #[test]
    fn test_detect_tset() {
        assert_eq!(detect_mode("tset"), Mode::Reset);
    }

    #[test]
    fn test_detect_default() {
        assert_eq!(detect_mode("unknown"), Mode::Tput);
    }

    // ── String capabilities ──

    #[test]
    fn test_cap_clear() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("clear"), Some("\x1B[H\x1B[2J".to_string()));
    }

    #[test]
    fn test_cap_home() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("home"), Some("\x1B[H".to_string()));
    }

    #[test]
    fn test_cap_bold() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("bold"), Some("\x1B[1m".to_string()));
    }

    #[test]
    fn test_cap_sgr0() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("sgr0"), Some("\x1B[0m".to_string()));
    }

    #[test]
    fn test_cap_smul() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("smul"), Some("\x1B[4m".to_string()));
    }

    #[test]
    fn test_cap_rmul() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("rmul"), Some("\x1B[24m".to_string()));
    }

    #[test]
    fn test_cap_rev() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("rev"), Some("\x1B[7m".to_string()));
    }

    #[test]
    fn test_cap_civis() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("civis"), Some("\x1B[?25l".to_string()));
    }

    #[test]
    fn test_cap_cnorm() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("cnorm"), Some("\x1B[?25h".to_string()));
    }

    #[test]
    fn test_cap_smcup() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("smcup"), Some("\x1B[?1049h".to_string()));
    }

    #[test]
    fn test_cap_rmcup() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("rmcup"), Some("\x1B[?1049l".to_string()));
    }

    #[test]
    fn test_cap_bel() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("bel"), Some("\x07".to_string()));
    }

    #[test]
    fn test_cap_sc_rc() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("sc"), Some("\x1B7".to_string()));
        assert_eq!(caps.get_string("rc"), Some("\x1B8".to_string()));
    }

    #[test]
    fn test_cap_setaf() {
        let caps = TermCaps::with_term("xterm");
        assert!(caps.get_string("setaf").is_some());
    }

    #[test]
    fn test_cap_unknown() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_string("nonexistent_cap"), None);
    }

    // ── Dumb terminal ──

    #[test]
    fn test_dumb_no_caps() {
        let caps = TermCaps::with_term("dumb");
        assert!(caps.is_dumb());
        assert_eq!(caps.get_string("clear"), None);
        assert_eq!(caps.get_string("bold"), None);
    }

    #[test]
    fn test_dumb_cr_and_bel() {
        let caps = TermCaps::with_term("dumb");
        assert_eq!(caps.get_string("cr"), Some("\r".to_string()));
        assert_eq!(caps.get_string("bel"), Some("\x07".to_string()));
    }

    #[test]
    fn test_dumb_numeric() {
        let caps = TermCaps::with_term("dumb");
        assert_eq!(caps.get_num("cols"), Some(80));
        assert_eq!(caps.get_num("lines"), Some(24));
    }

    #[test]
    fn test_dumb_booleans() {
        let caps = TermCaps::with_term("dumb");
        assert!(!caps.get_bool("am"));
        assert!(!caps.get_bool("km"));
    }

    // ── Numeric capabilities ──

    #[test]
    fn test_num_colors() {
        let caps = TermCaps::with_term("xterm-256color");
        assert_eq!(caps.get_num("colors"), Some(256));
    }

    #[test]
    fn test_num_colors_basic() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_num("colors"), Some(8));
    }

    #[test]
    fn test_num_pairs() {
        let caps = TermCaps::with_term("xterm-256color");
        assert_eq!(caps.get_num("pairs"), Some(65536));
    }

    #[test]
    fn test_num_it() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_num("it"), Some(8));
    }

    #[test]
    fn test_num_unknown() {
        let caps = TermCaps::with_term("xterm");
        assert_eq!(caps.get_num("nonexistent"), None);
    }

    // ── Boolean capabilities ──

    #[test]
    fn test_bool_am() {
        let caps = TermCaps::with_term("xterm");
        assert!(caps.get_bool("am"));
    }

    #[test]
    fn test_bool_bce() {
        let caps = TermCaps::with_term("xterm");
        assert!(caps.get_bool("bce"));
    }

    #[test]
    fn test_bool_ccc_xterm() {
        let caps = TermCaps::with_term("xterm");
        assert!(caps.get_bool("ccc"));
    }

    #[test]
    fn test_bool_ccc_vt100() {
        let caps = TermCaps::with_term("vt100");
        assert!(!caps.get_bool("ccc"));
    }

    #[test]
    fn test_bool_unknown() {
        let caps = TermCaps::with_term("xterm");
        assert!(!caps.get_bool("nonexistent"));
    }

    // ── Parameter expansion ──

    #[test]
    fn test_expand_no_params() {
        assert_eq!(expand_params("\x1B[H", &[]), "\x1B[H");
    }

    #[test]
    fn test_expand_percent_d() {
        assert_eq!(expand_params("\x1B[%p1%dm", &[3]), "\x1B[3m");
    }

    #[test]
    fn test_expand_cup() {
        // cup = "\x1B[%i%p1%d;%p2%dH"
        // %i increments both params by 1 (0-based to 1-based)
        let result = expand_params("\x1B[%i%p1%d;%p2%dH", &[5, 10]);
        assert_eq!(result, "\x1B[6;11H");
    }

    #[test]
    fn test_expand_setaf() {
        let result = expand_params("\x1B[3%p1%dm", &[1]);
        assert_eq!(result, "\x1B[31m");
    }

    #[test]
    fn test_expand_setab() {
        let result = expand_params("\x1B[4%p1%dm", &[4]);
        assert_eq!(result, "\x1B[44m");
    }

    #[test]
    fn test_expand_percent_literal() {
        assert_eq!(expand_params("100%%", &[]), "100%");
    }

    #[test]
    fn test_expand_missing_param() {
        // If param index exceeds available params, use 0
        let result = expand_params("%p3%d", &[1, 2]);
        assert_eq!(result, "0");
    }

    // ── Terminal type helpers ──

    #[test]
    fn test_term_with_custom_type() {
        let caps = TermCaps::with_term("linux");
        assert!(!caps.is_dumb());
        assert_eq!(caps.term_type, "linux");
    }

    #[test]
    fn test_empty_term_is_dumb() {
        let caps = TermCaps::with_term("");
        assert!(caps.is_dumb());
    }

    // ── Aliases ──

    #[test]
    fn test_alias_caps() {
        let caps = TermCaps::with_term("xterm");
        // Short names should work too
        assert_eq!(caps.get_string("cl"), caps.get_string("clear"));
        assert_eq!(caps.get_string("ho"), caps.get_string("home"));
        assert_eq!(caps.get_string("md"), caps.get_string("bold"));
        assert_eq!(caps.get_string("me"), caps.get_string("sgr0"));
        assert_eq!(caps.get_string("us"), caps.get_string("smul"));
        assert_eq!(caps.get_string("ue"), caps.get_string("rmul"));
        assert_eq!(caps.get_string("mr"), caps.get_string("rev"));
    }

    // ── Edge cases ──

    #[test]
    fn test_expand_empty() {
        assert_eq!(expand_params("", &[]), "");
    }

    #[test]
    fn test_expand_no_percent() {
        assert_eq!(expand_params("hello world", &[42]), "hello world");
    }
}
