//! SlateOS shuf/factor/numfmt — randomization and number tools
//!
//! Multi-personality binary detected via argv[0]:
//! - `shuf`: Randomly permute lines / select random lines
//! - `factor`: Print prime factors of numbers
//! - `numfmt`: Convert numbers from/to human-readable format

#![allow(unexpected_cfgs)]

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
    Numfmt,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "factor" => Mode::Factor,
        "numfmt" => Mode::Numfmt,
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
                head_count = Some(argv[i].parse::<usize>()
                    .map_err(|_| format!("invalid count: '{}'", argv[i]))?);
            }
            _ if arg.starts_with("--head-count=") => {
                let val = &arg["--head-count=".len()..];
                head_count = Some(val.parse::<usize>()
                    .map_err(|_| format!("invalid count: '{val}'"))?);
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
                seed = Some(val.parse::<u64>()
                    .map_err(|_| format!("invalid seed: '{val}'"))?);
            }
            "--" => {
                i += 1;
                if i < argv.len() {
                    input_file = Some(argv[i].clone());
                }
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("unknown option '{arg}'"));
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
        Some(ref path) => Box::new(
            fs::File::create(path)
                .map_err(|e| format!("{path}: {e}"))?
        ),
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
        return Err(format!("invalid range: '{s}'"));
    }
    let lo = parts[0].parse::<i64>()
        .map_err(|_| format!("invalid range start: '{}'", parts[0]))?;
    let hi = parts[1].parse::<i64>()
        .map_err(|_| format!("invalid range end: '{}'", parts[1]))?;
    if lo > hi {
        return Err(format!("range start {lo} is greater than end {hi}"));
    }
    Ok((lo, hi))
}

fn read_lines_delimited(file: Option<&str>, delim: u8) -> Result<Vec<String>, String> {
    let reader: Box<dyn BufRead> = match file {
        Some("-") | None => Box::new(io::stdin().lock()),
        Some(path) => {
            let f = fs::File::open(path)
                .map_err(|e| format!("{path}: {e}"))?;
            Box::new(io::BufReader::new(f))
        }
    };

    let mut lines = Vec::new();
    let mut buf = Vec::new();
    let mut reader = reader;

    loop {
        buf.clear();
        let n = reader.read_until(delim, &mut buf)
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
            let n = arg.parse::<u64>()
                .map_err(|_| format!("'{arg}' is not a valid number"))?;
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
            let n = line.parse::<u64>()
                .map_err(|_| format!("'{line}' is not a valid number"))?;
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

// ── numfmt mode ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum NumfmtUnit {
    None,    // Raw number
    Auto,    // Auto-detect suffix
    Si,      // SI suffixes (K=1000, M=1000000, ...)
    Iec,     // IEC suffixes (K=1024, M=1048576, ...)
    IecI,    // IEC with 'i' suffix (Ki, Mi, Gi, ...)
}

fn run_numfmt() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut from_unit = NumfmtUnit::None;
    let mut to_unit = NumfmtUnit::None;
    let mut padding: Option<i32> = None;
    let mut round = "from-zero"; // from-zero, towards-zero, up, down, nearest
    let mut suffix: Option<String> = None;
    let mut format_str: Option<String> = None;
    let mut field = 1usize; // 1-indexed
    let mut delimiter: Option<char> = None;
    let mut header_lines = 0usize;
    let mut inputs: Vec<String> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: numfmt [OPTION]... [NUMBER]...");
                eprintln!("Convert numbers from/to human-readable strings.");
                eprintln!();
                eprintln!("  --from=UNIT     auto-scale input (none, auto, si, iec, iec-i)");
                eprintln!("  --to=UNIT       auto-scale output (none, auto, si, iec, iec-i)");
                eprintln!("  --padding=N     pad output to N characters");
                eprintln!("  --round=METHOD  from-zero, towards-zero, up, down, nearest");
                eprintln!("  --suffix=S      add/remove suffix S");
                eprintln!("  --format=FMT    printf-style format (%f, %g, %.Nf)");
                eprintln!("  --field=N       process Nth field (default: 1)");
                eprintln!("  -d, --delimiter=D  field delimiter");
                eprintln!("  --header[=N]    don't convert first N header lines");
                process::exit(0);
            }
            _ if arg.starts_with("--from=") => {
                from_unit = parse_unit(&arg["--from=".len()..])?;
            }
            _ if arg.starts_with("--to=") => {
                to_unit = parse_unit(&arg["--to=".len()..])?;
            }
            _ if arg.starts_with("--padding=") => {
                padding = Some(arg["--padding=".len()..].parse::<i32>()
                    .map_err(|_| format!("invalid padding: '{}'", &arg["--padding=".len()..]))?);
            }
            _ if arg.starts_with("--round=") => {
                round = match &arg["--round=".len()..] {
                    "from-zero" | "towards-zero" | "up" | "down" | "nearest" => {
                        // Leak a &str that lasts the program lifetime — acceptable for CLI
                        let s: &'static str = Box::leak(arg["--round=".len()..].to_string().into_boxed_str());
                        s
                    }
                    other => return Err(format!("invalid rounding method: '{other}'")),
                };
            }
            _ if arg.starts_with("--suffix=") => {
                suffix = Some(arg["--suffix=".len()..].to_string());
            }
            _ if arg.starts_with("--format=") => {
                format_str = Some(arg["--format=".len()..].to_string());
            }
            _ if arg.starts_with("--field=") => {
                field = arg["--field=".len()..].parse::<usize>()
                    .map_err(|_| format!("invalid field: '{}'", &arg["--field=".len()..]))?;
                if field == 0 {
                    return Err("field number must be >= 1".to_string());
                }
            }
            "-d" | "--delimiter" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-d' requires an argument".to_string());
                }
                delimiter = argv[i].chars().next();
            }
            _ if arg.starts_with("--delimiter=") || arg.starts_with("-d") => {
                let val = arg
                    .strip_prefix("--delimiter=")
                    .or_else(|| arg.strip_prefix("-d"))
                    .unwrap_or("");
                delimiter = val.chars().next();
            }
            _ if arg == "--header" => {
                header_lines = 1;
            }
            _ if arg.starts_with("--header=") => {
                header_lines = arg["--header=".len()..].parse::<usize>()
                    .map_err(|_| format!("invalid header count: '{}'", &arg["--header=".len()..]))?;
            }
            "--" => {
                i += 1;
                while i < argv.len() {
                    inputs.push(argv[i].clone());
                    i += 1;
                }
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("unknown option '{arg}'"));
            }
            _ => inputs.push(arg.clone()),
        }
        i += 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if !inputs.is_empty() {
        for input in &inputs {
            let result = convert_number(input, from_unit, to_unit, padding, round, suffix.as_deref(), format_str.as_deref())?;
            writeln!(out, "{result}").map_err(|e| format!("write: {e}"))?;
        }
    } else {
        // Read from stdin
        let stdin = io::stdin();
        let mut line_num = 0usize;
        for line in stdin.lock().lines() {
            let line = line.map_err(|e| format!("read: {e}"))?;
            line_num += 1;

            if line_num <= header_lines {
                writeln!(out, "{line}").map_err(|e| format!("write: {e}"))?;
                continue;
            }

            // Process specified field
            let delim = delimiter.unwrap_or(' ');
            let parts: Vec<&str> = line.split(delim).collect();
            let field_idx = field - 1;

            if field_idx < parts.len() {
                let mut result_parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
                let converted = convert_number(
                    parts[field_idx].trim(),
                    from_unit, to_unit, padding, round,
                    suffix.as_deref(), format_str.as_deref(),
                )?;
                result_parts[field_idx] = converted;
                let delim_str = String::from(delimiter.unwrap_or(' '));
                writeln!(out, "{}", result_parts.join(&delim_str))
                    .map_err(|e| format!("write: {e}"))?;
            } else {
                writeln!(out, "{line}").map_err(|e| format!("write: {e}"))?;
            }
        }
    }

    Ok(())
}

fn parse_unit(s: &str) -> Result<NumfmtUnit, String> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Ok(NumfmtUnit::None),
        "auto" => Ok(NumfmtUnit::Auto),
        "si" => Ok(NumfmtUnit::Si),
        "iec" => Ok(NumfmtUnit::Iec),
        "iec-i" => Ok(NumfmtUnit::IecI),
        _ => Err(format!("invalid unit: '{s}'")),
    }
}

fn parse_number_with_suffix(s: &str, unit: NumfmtUnit) -> Result<f64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty input".to_string());
    }

    match unit {
        NumfmtUnit::None => {
            s.parse::<f64>().map_err(|_| format!("invalid number: '{s}'"))
        }
        NumfmtUnit::Auto | NumfmtUnit::Si | NumfmtUnit::Iec | NumfmtUnit::IecI => {
            // Strip trailing suffix and multiply
            let (num_str, multiplier) = extract_suffix(s, unit)?;
            let base: f64 = num_str.parse()
                .map_err(|_| format!("invalid number: '{num_str}'"))?;
            Ok(base * multiplier)
        }
    }
}

fn extract_suffix(s: &str, unit: NumfmtUnit) -> Result<(&str, f64), String> {
    if s.is_empty() {
        return Ok(("0", 1.0));
    }

    // Check for IEC 'i' suffix (Ki, Mi, etc.)
    let (s_trimmed, is_iec_suffix) = if s.ends_with('i') && s.len() >= 2 {
        (&s[..s.len() - 1], true)
    } else {
        (s, false)
    };

    let last = s_trimmed.as_bytes()[s_trimmed.len() - 1];
    let (num_part, suffix_char) = if last.is_ascii_alphabetic() {
        (&s_trimmed[..s_trimmed.len() - 1], Some(last))
    } else {
        (s_trimmed, None)
    };

    if num_part.is_empty() {
        return Err(format!("invalid number: '{s}'"));
    }

    let multiplier = match suffix_char {
        None => 1.0,
        Some(c) => {
            let use_iec = matches!(unit, NumfmtUnit::Iec | NumfmtUnit::IecI) || is_iec_suffix;
            let base: f64 = if use_iec { 1024.0 } else { 1000.0 };
            match c.to_ascii_uppercase() {
                b'K' => base,
                b'M' => base * base,
                b'G' => base * base * base,
                b'T' => base * base * base * base,
                b'P' => base * base * base * base * base,
                b'E' => base * base * base * base * base * base,
                _ => return Err(format!("invalid suffix: '{}'", c as char)),
            }
        }
    };

    Ok((num_part, multiplier))
}

fn format_number(value: f64, unit: NumfmtUnit, round_method: &str, padding: Option<i32>, format_str: Option<&str>) -> String {
    let formatted = match unit {
        NumfmtUnit::None => {
            if let Some(fmt) = format_str {
                apply_format(value, fmt)
            } else if value == value.floor() && value.abs() < 1e15 {
                format!("{}", value as i64)
            } else {
                format!("{value}")
            }
        }
        NumfmtUnit::Si => format_with_suffix(value, 1000.0, "", round_method, format_str),
        NumfmtUnit::Iec => format_with_suffix(value, 1024.0, "", round_method, format_str),
        NumfmtUnit::IecI => format_with_suffix(value, 1024.0, "i", round_method, format_str),
        NumfmtUnit::Auto => {
            // Auto for output: use SI
            format_with_suffix(value, 1000.0, "", round_method, format_str)
        }
    };

    // Apply padding
    match padding {
        Some(pad) if pad > 0 => {
            let width = pad as usize;
            if formatted.len() < width {
                format!("{formatted:>width$}")
            } else {
                formatted
            }
        }
        Some(pad) if pad < 0 => {
            let width = (-pad) as usize;
            if formatted.len() < width {
                format!("{formatted:<width$}")
            } else {
                formatted
            }
        }
        _ => formatted,
    }
}

fn format_with_suffix(value: f64, base: f64, iec_suffix: &str, round_method: &str, format_str: Option<&str>) -> String {
    let suffixes = ["", "K", "M", "G", "T", "P", "E"];
    let abs_val = value.abs();

    let mut idx = 0;
    let mut scaled = abs_val;
    while scaled >= base && idx + 1 < suffixes.len() {
        scaled /= base;
        idx += 1;
    }

    if value < 0.0 {
        scaled = -scaled;
    }

    // Apply rounding
    let rounded = apply_rounding(scaled, round_method);

    let num_str = if let Some(fmt) = format_str {
        apply_format(rounded, fmt)
    } else if rounded == rounded.floor() && rounded.abs() < 1e15 {
        format!("{}", rounded as i64)
    } else {
        // One decimal place for scaled values
        let s = format!("{rounded:.1}");
        // Strip trailing zero after decimal
        if s.contains('.') {
            let trimmed = s.trim_end_matches('0');
            trimmed.trim_end_matches('.').to_string()
        } else {
            s
        }
    };

    if idx == 0 {
        num_str
    } else {
        format!("{num_str}{}{iec_suffix}", suffixes[idx])
    }
}

fn apply_rounding(value: f64, method: &str) -> f64 {
    match method {
        "up" => value.ceil(),
        "down" => value.floor(),
        "towards-zero" => value.trunc(),
        "nearest" => value.round(),
        _ => {
            // from-zero: round away from zero
            if value >= 0.0 { value.ceil() } else { value.floor() }
        }
    }
}

fn apply_format(value: f64, fmt: &str) -> String {
    // Simple format parsing: %f, %.Nf, %g, %.Ng
    if fmt.contains("%.") {
        // Extract precision
        if let Some(start) = fmt.find("%.") {
            let rest = &fmt[start + 2..];
            let end = rest.find(['f', 'g', 'e']).unwrap_or(rest.len());
            if let Ok(prec) = rest[..end].parse::<usize>() {
                let spec = if end < rest.len() { rest.as_bytes()[end] } else { b'f' };
                let formatted = match spec {
                    b'g' => {
                        let s = format!("{value:.prec$}");
                        if s.contains('.') {
                            s.trim_end_matches('0').trim_end_matches('.').to_string()
                        } else {
                            s
                        }
                    }
                    b'e' => format!("{value:.prec$e}"),
                    _ => format!("{value:.prec$}"),
                };
                let prefix = &fmt[..start];
                let suffix_start = start + 2 + end + 1;
                let suffix_str = if suffix_start <= fmt.len() { &fmt[suffix_start..] } else { "" };
                return format!("{prefix}{formatted}{suffix_str}");
            }
        }
    }
    if fmt.contains("%f") {
        return fmt.replace("%f", &format!("{value:.6}"));
    }
    if fmt.contains("%g") {
        let s = format!("{value}");
        return fmt.replace("%g", &s);
    }
    format!("{value}")
}

fn convert_number(
    s: &str,
    from_unit: NumfmtUnit,
    to_unit: NumfmtUnit,
    padding: Option<i32>,
    round_method: &str,
    suffix: Option<&str>,
    format_str: Option<&str>,
) -> Result<String, String> {
    // Strip user suffix before parsing
    let input = if let Some(sfx) = suffix {
        if let Some(stripped) = s.strip_suffix(sfx) {
            stripped
        } else {
            s
        }
    } else {
        s
    };

    let value = parse_number_with_suffix(input, from_unit)?;
    let mut result = format_number(value, to_unit, round_method, padding, format_str);

    // Re-add suffix
    if let Some(sfx) = suffix {
        result.push_str(sfx);
    }

    Ok(result)
}

// ── Main entry point ───────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv0 = env::args().next().unwrap_or_else(|| "shuf".to_string());
    let mode = detect_mode(&argv0);

    match mode {
        Mode::Shuf => run_shuf(),
        Mode::Factor => run_factor(),
        Mode::Numfmt => run_numfmt(),
    }
}

fn main() {
    if let Err(e) = run() {
        let prog = env::args().next().unwrap_or_else(|| "shuf".to_string());
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

    #[test]
    fn test_detect_numfmt() {
        assert_eq!(detect_mode("numfmt"), Mode::Numfmt);
        assert_eq!(detect_mode("./numfmt"), Mode::Numfmt);
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

    // ── Number formatting (numfmt) ──

    #[test]
    fn test_parse_unit() {
        assert_eq!(parse_unit("none").unwrap(), NumfmtUnit::None);
        assert_eq!(parse_unit("si").unwrap(), NumfmtUnit::Si);
        assert_eq!(parse_unit("iec").unwrap(), NumfmtUnit::Iec);
        assert_eq!(parse_unit("iec-i").unwrap(), NumfmtUnit::IecI);
        assert_eq!(parse_unit("auto").unwrap(), NumfmtUnit::Auto);
        assert!(parse_unit("invalid").is_err());
    }

    #[test]
    fn test_parse_number_none() {
        assert_eq!(parse_number_with_suffix("42", NumfmtUnit::None).unwrap(), 42.0);
        assert_eq!(parse_number_with_suffix("3.25", NumfmtUnit::None).unwrap(), 3.25);
        assert_eq!(parse_number_with_suffix("-100", NumfmtUnit::None).unwrap(), -100.0);
    }

    #[test]
    fn test_parse_number_si() {
        assert_eq!(parse_number_with_suffix("1K", NumfmtUnit::Si).unwrap(), 1000.0);
        assert_eq!(parse_number_with_suffix("1M", NumfmtUnit::Si).unwrap(), 1_000_000.0);
        assert_eq!(parse_number_with_suffix("1G", NumfmtUnit::Si).unwrap(), 1_000_000_000.0);
        assert_eq!(parse_number_with_suffix("2.5K", NumfmtUnit::Si).unwrap(), 2500.0);
    }

    #[test]
    fn test_parse_number_iec() {
        assert_eq!(parse_number_with_suffix("1K", NumfmtUnit::Iec).unwrap(), 1024.0);
        assert_eq!(parse_number_with_suffix("1M", NumfmtUnit::Iec).unwrap(), 1_048_576.0);
        assert_eq!(parse_number_with_suffix("1G", NumfmtUnit::Iec).unwrap(), 1_073_741_824.0);
    }

    #[test]
    fn test_parse_number_iec_i() {
        assert_eq!(parse_number_with_suffix("1Ki", NumfmtUnit::IecI).unwrap(), 1024.0);
        assert_eq!(parse_number_with_suffix("1Mi", NumfmtUnit::IecI).unwrap(), 1_048_576.0);
    }

    #[test]
    fn test_format_number_none() {
        assert_eq!(format_number(42.0, NumfmtUnit::None, "from-zero", None, None), "42");
        assert_eq!(format_number(3.25, NumfmtUnit::None, "from-zero", None, None), "3.25");
    }

    #[test]
    fn test_format_number_si() {
        assert_eq!(format_number(1000.0, NumfmtUnit::Si, "from-zero", None, None), "1K");
        assert_eq!(format_number(1500.0, NumfmtUnit::Si, "from-zero", None, None), "2K");
        assert_eq!(format_number(1_000_000.0, NumfmtUnit::Si, "from-zero", None, None), "1M");
    }

    #[test]
    fn test_format_number_iec() {
        assert_eq!(format_number(1024.0, NumfmtUnit::Iec, "from-zero", None, None), "1K");
        assert_eq!(format_number(1_048_576.0, NumfmtUnit::Iec, "from-zero", None, None), "1M");
    }

    #[test]
    fn test_format_number_iec_i() {
        assert_eq!(format_number(1024.0, NumfmtUnit::IecI, "from-zero", None, None), "1Ki");
        assert_eq!(format_number(1_048_576.0, NumfmtUnit::IecI, "from-zero", None, None), "1Mi");
    }

    #[test]
    fn test_format_number_padding_right() {
        let result = format_number(42.0, NumfmtUnit::None, "from-zero", Some(10), None);
        assert_eq!(result.len(), 10);
        assert!(result.starts_with(' '));
    }

    #[test]
    fn test_format_number_padding_left() {
        let result = format_number(42.0, NumfmtUnit::None, "from-zero", Some(-10), None);
        assert_eq!(result.len(), 10);
        assert!(result.ends_with(' '));
    }

    // ── Rounding ──

    #[test]
    fn test_rounding_up() {
        assert_eq!(apply_rounding(1.1, "up"), 2.0);
        assert_eq!(apply_rounding(-1.1, "up"), -1.0);
    }

    #[test]
    fn test_rounding_down() {
        assert_eq!(apply_rounding(1.9, "down"), 1.0);
        assert_eq!(apply_rounding(-1.1, "down"), -2.0);
    }

    #[test]
    fn test_rounding_towards_zero() {
        assert_eq!(apply_rounding(1.9, "towards-zero"), 1.0);
        assert_eq!(apply_rounding(-1.9, "towards-zero"), -1.0);
    }

    #[test]
    fn test_rounding_nearest() {
        assert_eq!(apply_rounding(1.4, "nearest"), 1.0);
        assert_eq!(apply_rounding(1.5, "nearest"), 2.0);
    }

    #[test]
    fn test_rounding_from_zero() {
        assert_eq!(apply_rounding(1.1, "from-zero"), 2.0);
        assert_eq!(apply_rounding(-1.1, "from-zero"), -2.0);
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

    // ── Full conversion ──

    #[test]
    fn test_convert_si_to_none() {
        let result = convert_number("1K", NumfmtUnit::Si, NumfmtUnit::None, None, "from-zero", None, None).unwrap();
        assert_eq!(result, "1000");
    }

    #[test]
    fn test_convert_iec_to_none() {
        let result = convert_number("1K", NumfmtUnit::Iec, NumfmtUnit::None, None, "from-zero", None, None).unwrap();
        assert_eq!(result, "1024");
    }

    #[test]
    fn test_convert_none_to_si() {
        let result = convert_number("1000", NumfmtUnit::None, NumfmtUnit::Si, None, "from-zero", None, None).unwrap();
        assert_eq!(result, "1K");
    }

    #[test]
    fn test_convert_with_suffix() {
        let result = convert_number("1000B", NumfmtUnit::None, NumfmtUnit::Si, None, "from-zero", Some("B"), None).unwrap();
        assert_eq!(result, "1KB");
    }

    #[test]
    fn test_convert_empty_input() {
        assert!(convert_number("", NumfmtUnit::None, NumfmtUnit::None, None, "from-zero", None, None).is_err());
    }

    // ── Format string ──

    #[test]
    fn test_apply_format_fixed() {
        assert_eq!(apply_format(3.252_55, "%.2f"), "3.25");
        // 3.25255 is not exactly representable as f64 — it stores as
        // ~3.2525499999999998, so rounding to 4 decimals yields 3.2525,
        // not 3.2526. Use a binary-clean input for the .4f assertion.
        assert_eq!(apply_format(3.25, "%.4f"), "3.2500");
    }

    #[test]
    fn test_apply_format_general() {
        let result = apply_format(42.0, "%g");
        assert!(result.contains("42"));
    }

    #[test]
    fn test_apply_format_f() {
        let result = apply_format(3.25, "%f");
        assert!(result.starts_with("3.25"));
    }

    // ── Extract suffix ──

    #[test]
    fn test_extract_suffix_none() {
        let (num, mult) = extract_suffix("42", NumfmtUnit::Si).unwrap();
        assert_eq!(num, "42");
        assert_eq!(mult, 1.0);
    }

    #[test]
    fn test_extract_suffix_k() {
        let (num, mult) = extract_suffix("5K", NumfmtUnit::Si).unwrap();
        assert_eq!(num, "5");
        assert_eq!(mult, 1000.0);
    }

    #[test]
    fn test_extract_suffix_ki() {
        let (num, mult) = extract_suffix("5Ki", NumfmtUnit::IecI).unwrap();
        assert_eq!(num, "5");
        assert_eq!(mult, 1024.0);
    }

    #[test]
    fn test_extract_suffix_iec_k() {
        let (num, mult) = extract_suffix("5K", NumfmtUnit::Iec).unwrap();
        assert_eq!(num, "5");
        assert_eq!(mult, 1024.0);
    }

    #[test]
    fn test_format_small_number() {
        // Numbers below base should not get a suffix
        assert_eq!(format_number(500.0, NumfmtUnit::Si, "from-zero", None, None), "500");
        assert_eq!(format_number(100.0, NumfmtUnit::Iec, "from-zero", None, None), "100");
    }
}
