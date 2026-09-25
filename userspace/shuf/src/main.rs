//! Slate OS shuf/factor — randomization and number tools
//!
//! Multi-personality binary detected via argv[0]:
//! - `shuf`: Randomly permute lines / select random lines
//! - `factor`: Print prime factors of numbers
//!
//! `numfmt` was a third personality, which no link ever reached. It is
//! `userspace/coreutils`'s own bin since 2026-09-25, a port of GNU's checked
//! against it, and a name belongs to the one program that does the job
//! (design-decisions.md §1005).

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;
use std::time::SystemTime;

// ── Personality detection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Shuf,
    Factor,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "factor" => Mode::Factor,
        _ => Mode::Shuf,
    }
}

// ── PRNG (xorshift64*) ────────────────────────────────────────────

struct Rng {
    state: u64,
}

impl Rng {
    fn new() -> Self {
        // Seed from system time + address of a stack variable for some entropy
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x12345678_9ABCDEF0);
        // Mix in a stack address for extra entropy
        let stack_val = &seed as *const u64 as u64;
        Self {
            state: seed ^ stack_val ^ 0x6A09E667F3BCC908,
        }
    }

    fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Generate a random number in [0, bound)
    fn next_bounded(&mut self, bound: u64) -> u64 {
        if bound <= 1 {
            return 0;
        }
        // Rejection sampling to avoid bias
        let threshold = (u64::MAX - bound + 1) % bound;
        loop {
            let r = self.next_u64();
            if r >= threshold {
                return r % bound;
            }
        }
    }
}

// ── shuf mode ──────────────────────────────────────────────────────

fn run_shuf() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut input_file: Option<String> = None;
    let mut head_count: Option<usize> = None;
    let mut echo_args: Vec<String> = Vec::new();
    let mut input_range: Option<(i64, i64)> = None;
    let mut zero_terminated = false;
    let mut repeat = false;
    let mut seed: Option<u64> = None;
    let mut output_file: Option<String> = None;

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: shuf [OPTION]... [FILE]");
                eprintln!("  or:  shuf -e [OPTION]... [ARG]...");
                eprintln!("  or:  shuf -i LO-HI [OPTION]...");
                eprintln!();
                eprintln!("Write a random permutation of input lines.");
                eprintln!();
                eprintln!("  -e, --echo       treat args as input lines");
                eprintln!("  -i, --input-range=LO-HI  use integer range as input");
                eprintln!("  -n, --head-count=COUNT  output at most COUNT lines");
                eprintln!("  -o, --output=FILE  write to FILE");
                eprintln!("  -r, --repeat     allow repeated output (with -n)");
                eprintln!("  -z, --zero-terminated  line delimiter is NUL");
                eprintln!("  --random-source=SEED  use SEED for randomness");
                process::exit(0);
            }
            "-e" | "--echo" => {
                // All remaining args (until next option) are the echo lines
                i += 1;
                while i < argv.len() {
                    if argv[i].starts_with('-') && argv[i] != "--" {
                        // This could be another option; push back
                        break;
                    }
                    if argv[i] == "--" {
                        i += 1;
                        while i < argv.len() {
                            echo_args.push(argv[i].clone());
                            i += 1;
                        }
                        break;
                    }
                    echo_args.push(argv[i].clone());
                    i += 1;
                }
                continue;
            }
            "-i" | "--input-range" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-i' requires an argument".to_string());
                }
                input_range = Some(parse_range(&argv[i])?);
            }
            _ if arg.starts_with("--input-range=") => {
                input_range = Some(parse_range(&arg["--input-range=".len()..])?);
            }
            "-n" | "--head-count" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-n' requires an argument".to_string());
                }
                head_count = Some(
                    argv[i]
                        .parse::<usize>()
                        .map_err(|_| format!("invalid count: {}", quoteaf_os(&argv[i])))?,
                );
            }
            _ if arg.starts_with("--head-count=") => {
                let val = &arg["--head-count=".len()..];
                head_count = Some(
                    val.parse::<usize>()
                        .map_err(|_| format!("invalid count: {}", quoteaf_os(val)))?,
                );
            }
            "-o" | "--output" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-o' requires an argument".to_string());
                }
                output_file = Some(argv[i].clone());
            }
            _ if arg.starts_with("--output=") => {
                output_file = Some(arg["--output=".len()..].to_string());
            }
            "-r" | "--repeat" => repeat = true,
            "-z" | "--zero-terminated" => zero_terminated = true,
            _ if arg.starts_with("--random-source=") => {
                let val = &arg["--random-source=".len()..];
                seed = Some(
                    val.parse::<u64>()
                        .map_err(|_| format!("invalid seed: {}", quoteaf_os(val)))?,
                );
            }
            "--" => {
                i += 1;
                if i < argv.len() {
                    input_file = Some(argv[i].clone());
                }
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("unknown option {}", quoteaf_os(arg)));
            }
            _ => {
                input_file = Some(arg.clone());
            }
        }
        i += 1;
    }

    // Build the input lines
    let mut lines: Vec<String> = if !echo_args.is_empty() {
        echo_args
    } else if let Some((lo, hi)) = input_range {
        (lo..=hi).map(|n| n.to_string()).collect()
    } else {
        // Read from file or stdin
        let delim = if zero_terminated { b'\0' } else { b'\n' };
        read_lines_delimited(input_file.as_deref(), delim)?
    };

    let mut rng = match seed {
        Some(s) => Rng::from_seed(s),
        None => Rng::new(),
    };

    let count = head_count.unwrap_or(lines.len());

    // Open output
    let stdout = io::stdout();
    let mut out: Box<dyn Write> = match output_file {
        Some(ref path) => Box::new(fs::File::create(path).map_err(|e| format!("{path}: {e}"))?),
        None => Box::new(stdout.lock()),
    };

    let line_end = if zero_terminated { b'\0' } else { b'\n' };

    if repeat {
        // Repeat mode: pick random with replacement
        if lines.is_empty() {
            return Err("no input lines for repeat mode".to_string());
        }
        for _ in 0..count {
            let idx = rng.next_bounded(lines.len() as u64) as usize;
            out.write_all(lines[idx].as_bytes())
                .map_err(|e| format!("write: {e}"))?;
            out.write_all(&[line_end])
                .map_err(|e| format!("write: {e}"))?;
        }
    } else {
        // Fisher-Yates shuffle
        let n = lines.len();
        for idx in (1..n).rev() {
            let j = rng.next_bounded((idx + 1) as u64) as usize;
            lines.swap(idx, j);
        }
        for line in lines.iter().take(count) {
            out.write_all(line.as_bytes())
                .map_err(|e| format!("write: {e}"))?;
            out.write_all(&[line_end])
                .map_err(|e| format!("write: {e}"))?;
        }
    }

    Ok(())
}

fn parse_range(s: &str) -> Result<(i64, i64), String> {
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(format!("invalid range: {}", quoteaf_os(s)));
    }
    let lo = parts[0]
        .parse::<i64>()
        .map_err(|_| format!("invalid range start: {}", quoteaf_os(parts[0])))?;
    let hi = parts[1]
        .parse::<i64>()
        .map_err(|_| format!("invalid range end: {}", quoteaf_os(parts[1])))?;
    if lo > hi {
        return Err(format!("range start {lo} is greater than end {hi}"));
    }
    Ok((lo, hi))
}

fn read_lines_delimited(file: Option<&str>, delim: u8) -> Result<Vec<String>, String> {
    let reader: Box<dyn BufRead> = match file {
        Some("-") | None => Box::new(io::stdin().lock()),
        Some(path) => {
            let f = fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
            Box::new(io::BufReader::new(f))
        }
    };

    let mut lines = Vec::new();
    let mut buf = Vec::new();
    let mut reader = reader;

    loop {
        buf.clear();
        let n = reader
            .read_until(delim, &mut buf)
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        // Strip delimiter
        if buf.last() == Some(&delim) {
            buf.pop();
        }
        // Also strip \r before \n if not zero-terminated
        if delim == b'\n' && buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if !buf.is_empty() || n > 1 {
            lines.push(String::from_utf8_lossy(&buf).to_string());
        }
    }

    Ok(lines)
}

// ── factor mode ────────────────────────────────────────────────────

fn run_factor() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() > 1 {
        // Factor command-line arguments
        for arg in &argv[1..] {
            if arg == "-h" || arg == "--help" {
                eprintln!("Usage: factor [NUMBER]...");
                eprintln!("Print the prime factors of each NUMBER.");
                process::exit(0);
            }
            let n = arg
                .parse::<u64>()
                .map_err(|_| format!("{} is not a valid number", quoteaf_os(arg)))?;
            print_factors(n);
        }
    } else {
        // Read from stdin
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.map_err(|e| format!("read: {e}"))?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let n = line
                .parse::<u64>()
                .map_err(|_| format!("{} is not a valid number", quoteaf_os(line)))?;
            print_factors(n);
        }
    }
    Ok(())
}

fn factorize(mut n: u64) -> Vec<u64> {
    let mut factors = Vec::new();

    if n <= 1 {
        return factors;
    }

    // Trial division by 2
    while n.is_multiple_of(2) {
        factors.push(2);
        n /= 2;
    }

    // Trial division by odd numbers
    let mut d = 3u64;
    while d.saturating_mul(d) <= n {
        while n.is_multiple_of(d) {
            factors.push(d);
            n /= d;
        }
        d += 2;
    }

    if n > 1 {
        factors.push(n);
    }

    factors
}

fn print_factors(n: u64) {
    let factors = factorize(n);
    if factors.is_empty() {
        println!("{n}:");
    } else {
        let factor_strs: Vec<String> = factors.iter().map(|f| f.to_string()).collect();
        println!("{n}: {}", factor_strs.join(" "));
    }
}

// ── Main entry point ───────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv0 = env::args().next().unwrap_or_else(|| "shuf".to_string());
    let mode = detect_mode(&argv0);

    match mode {
        Mode::Shuf => run_shuf(),
        Mode::Factor => run_factor(),
    }
}

fn main() {
    if let Err(e) = run() {
        let prog = env::args().next().unwrap_or_else(|| "shuf".to_string());
        let name = prog.rsplit(['/', '\\']).next().unwrap_or(&prog);
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
    fn test_detect_shuf() {
        assert_eq!(detect_mode("shuf"), Mode::Shuf);
        assert_eq!(detect_mode("/usr/bin/shuf"), Mode::Shuf);
        assert_eq!(detect_mode("shuf.exe"), Mode::Shuf);
    }

    #[test]
    fn test_detect_factor() {
        assert_eq!(detect_mode("factor"), Mode::Factor);
        assert_eq!(detect_mode("/usr/bin/factor"), Mode::Factor);
        assert_eq!(detect_mode("C:\\bin\\factor.exe"), Mode::Factor);
    }

    /// `numfmt` is coreutils' bin now; this binary no longer answers to it,
    /// so the name falls to the default like any other it does not know.
    #[test]
    fn numfmt_is_not_a_personality_here() {
        assert_eq!(detect_mode("numfmt"), Mode::Shuf);
        assert_eq!(detect_mode("./numfmt"), Mode::Shuf);
    }

    #[test]
    fn test_detect_default() {
        assert_eq!(detect_mode("unknown"), Mode::Shuf);
    }

    // ── PRNG ──

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = Rng::from_seed(42);
        let mut rng2 = Rng::from_seed(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = Rng::from_seed(1);
        let mut rng2 = Rng::from_seed(2);
        // Very unlikely to produce same sequence
        let same = (0..10).all(|_| rng1.next_u64() == rng2.next_u64());
        assert!(!same);
    }

    #[test]
    fn test_rng_bounded() {
        let mut rng = Rng::from_seed(123);
        for _ in 0..1000 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_rng_bounded_one() {
        let mut rng = Rng::from_seed(42);
        assert_eq!(rng.next_bounded(1), 0);
    }

    #[test]
    fn test_rng_zero_seed_adjusted() {
        let rng = Rng::from_seed(0);
        assert_eq!(rng.state, 1); // Zero adjusted to 1
    }

    // ── Factorization ──

    #[test]
    fn test_factor_zero() {
        assert_eq!(factorize(0), vec![] as Vec<u64>);
    }

    #[test]
    fn test_factor_one() {
        assert_eq!(factorize(1), vec![] as Vec<u64>);
    }

    #[test]
    fn test_factor_prime() {
        assert_eq!(factorize(2), vec![2]);
        assert_eq!(factorize(3), vec![3]);
        assert_eq!(factorize(7), vec![7]);
        assert_eq!(factorize(13), vec![13]);
        assert_eq!(factorize(97), vec![97]);
    }

    #[test]
    fn test_factor_composite() {
        assert_eq!(factorize(4), vec![2, 2]);
        assert_eq!(factorize(6), vec![2, 3]);
        assert_eq!(factorize(12), vec![2, 2, 3]);
        assert_eq!(factorize(100), vec![2, 2, 5, 5]);
        assert_eq!(factorize(360), vec![2, 2, 2, 3, 3, 5]);
    }

    #[test]
    fn test_factor_large_prime() {
        assert_eq!(factorize(104729), vec![104729]); // Prime
    }

    #[test]
    fn test_factor_power_of_two() {
        assert_eq!(factorize(64), vec![2, 2, 2, 2, 2, 2]);
        assert_eq!(factorize(1024), vec![2, 2, 2, 2, 2, 2, 2, 2, 2, 2]);
    }

    #[test]
    fn test_factor_large_composite() {
        assert_eq!(factorize(2 * 3 * 5 * 7 * 11 * 13), vec![2, 3, 5, 7, 11, 13]);
    }

    // ── Range parsing (shuf -i) ──

    #[test]
    fn test_parse_range_valid() {
        assert_eq!(parse_range("1-10").unwrap(), (1, 10));
        assert_eq!(parse_range("0-100").unwrap(), (0, 100));
    }

    #[test]
    fn test_parse_range_invalid() {
        assert!(parse_range("10-1").is_err());
        assert!(parse_range("abc").is_err());
    }
}
