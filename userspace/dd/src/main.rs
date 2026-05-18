//! OurOS Data Copy and Conversion Utility
//!
//! Copies and converts data between files or devices with configurable block
//! sizes, offsets, and inline transformations. Modeled after the POSIX `dd`
//! command with key=value argument syntax.
//!
//! # Usage
//!
//! ```text
//! dd if=/dev/sda of=disk.img bs=4K count=1024
//! dd if=input.txt of=output.txt conv=ucase
//! dd if=/dev/zero of=zeros.bin bs=1M count=10
//! dd if=data.bin bs=512 skip=2 count=4
//! dd if=input of=output bs=1K conv=swab,sync status=progress
//! ```

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// ============================================================================
// Global interrupt flag
// ============================================================================

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Install a Ctrl+C handler that sets the INTERRUPTED flag so the copy loop
/// can print final statistics before exiting.
fn install_signal_handler() {
    #[cfg(target_family = "unix")]
    {
        // SAFETY: We install a SIGINT handler (signal 2). The handler only
        // performs an atomic store, which is async-signal-safe.
        unsafe {
            libc_signal(2, signal_handler as *const () as usize);
        }
    }
}

/// Register a signal handler via the C library's `signal()` function.
///
/// # Safety
///
/// `handler` must be a valid function pointer suitable for use as a signal
/// handler (it may only call async-signal-safe functions).
#[cfg(target_family = "unix")]
unsafe fn libc_signal(signum: i32, handler: usize) {
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    // SAFETY: signal() is a standard POSIX function. signum is SIGINT (2),
    // a valid signal. handler points to a function that only does an atomic
    // store, which is async-signal-safe.
    unsafe {
        signal(signum, handler);
    }
}

/// Signal handler: sets the INTERRUPTED flag to true.
///
/// This function only performs an atomic store, which is async-signal-safe.
#[cfg(target_family = "unix")]
extern "C" fn signal_handler(_sig: i32) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

// ============================================================================
// Size suffix parsing
// ============================================================================

/// Parse a byte-count string with optional suffix.
///
/// Supported suffixes (case-sensitive):
/// - `c` = 1 (character)
/// - `w` = 2 (word)
/// - `b` = 512 (block)
/// - `K` = 1024
/// - `M` = 1024^2
/// - `G` = 1024^3
/// - `kB` = 1000
/// - `MB` = 1000^2
/// - `GB` = 1000^3
///
/// A bare number with no suffix is treated as bytes.
fn parse_size(s: &str) -> Result<u64, String> {
    if s.is_empty() {
        return Err("empty size value".to_string());
    }

    // Try two-character suffixes first (kB, MB, GB).
    if s.len() >= 3 {
        let (num_part, suffix) = s.split_at(s.len() - 2);
        let multiplier = match suffix {
            "kB" => Some(1_000u64),
            "MB" => Some(1_000_000u64),
            "GB" => Some(1_000_000_000u64),
            _ => None,
        };
        if let Some(mult) = multiplier {
            let n: u64 = num_part
                .parse()
                .map_err(|_| format!("invalid number: '{num_part}'"))?;
            return n
                .checked_mul(mult)
                .ok_or_else(|| format!("size overflow: '{s}'"));
        }
    }

    // Try single-character suffixes.
    if s.len() >= 2 {
        let last = s.as_bytes()[s.len() - 1];
        let multiplier = match last {
            b'c' => Some(1u64),
            b'w' => Some(2u64),
            b'b' => Some(512u64),
            b'K' => Some(1_024u64),
            b'M' => Some(1_024 * 1_024),
            b'G' => Some(1_024 * 1_024 * 1_024),
            _ => None,
        };
        if let Some(mult) = multiplier {
            let num_part = &s[..s.len() - 1];
            let n: u64 = num_part
                .parse()
                .map_err(|_| format!("invalid number: '{num_part}'"))?;
            return n
                .checked_mul(mult)
                .ok_or_else(|| format!("size overflow: '{s}'"));
        }
    }

    // No suffix -- plain number.
    s.parse::<u64>()
        .map_err(|_| format!("invalid number: '{s}'"))
}

// ============================================================================
// Conversion flags
// ============================================================================

/// Bit-flags for the `conv=` option. Multiple conversions can be combined
/// with commas.
#[derive(Clone, Copy, Default)]
struct ConvFlags {
    ucase: bool,
    lcase: bool,
    notrunc: bool,
    sync: bool,
    noerror: bool,
    fsync: bool,
    swab: bool,
}

fn parse_conv(s: &str) -> Result<ConvFlags, String> {
    let mut flags = ConvFlags::default();
    for token in s.split(',') {
        match token {
            "ucase" => flags.ucase = true,
            "lcase" => flags.lcase = true,
            "notrunc" => flags.notrunc = true,
            "sync" => flags.sync = true,
            "noerror" => flags.noerror = true,
            "fsync" => flags.fsync = true,
            "swab" => flags.swab = true,
            "" => {} // trailing comma or double comma -- ignore
            other => return Err(format!("unknown conversion: '{other}'")),
        }
    }
    if flags.ucase && flags.lcase {
        return Err("conv=ucase and conv=lcase are mutually exclusive".to_string());
    }
    Ok(flags)
}

// ============================================================================
// Status level
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusLevel {
    /// Default: print final summary.
    Default,
    /// Suppress all informational output.
    None,
    /// Print final summary but suppress transfer statistics line.
    Noxfer,
    /// Periodic progress updates during the transfer.
    Progress,
}

fn parse_status(s: &str) -> Result<StatusLevel, String> {
    match s {
        "none" => Ok(StatusLevel::None),
        "noxfer" => Ok(StatusLevel::Noxfer),
        "progress" => Ok(StatusLevel::Progress),
        other => Err(format!("unknown status level: '{other}'")),
    }
}

// ============================================================================
// Options
// ============================================================================

struct Options {
    input_file: Option<String>,
    output_file: Option<String>,
    ibs: u64,
    obs: u64,
    count: Option<u64>,
    skip: u64,
    seek: u64,
    conv: ConvFlags,
    status: StatusLevel,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input_file: None,
            output_file: None,
            ibs: 512,
            obs: 512,
            count: None,
            skip: 0,
            seek: 0,
            conv: ConvFlags::default(),
            status: StatusLevel::Default,
        }
    }
}

fn print_usage() {
    eprintln!("Usage: dd [OPERAND]...");
    eprintln!();
    eprintln!("Operands:");
    eprintln!("  if=FILE       Read from FILE instead of stdin");
    eprintln!("  of=FILE       Write to FILE instead of stdout");
    eprintln!("  bs=BYTES      Read and write BYTES bytes at a time (default 512)");
    eprintln!("  ibs=BYTES     Read BYTES bytes at a time (default 512)");
    eprintln!("  obs=BYTES     Write BYTES bytes at a time (default 512)");
    eprintln!("  count=N       Copy only N input blocks");
    eprintln!("  skip=N        Skip N ibs-sized blocks at start of input");
    eprintln!("  seek=N        Skip N obs-sized blocks at start of output");
    eprintln!("  conv=CONVS    Comma-separated conversions:");
    eprintln!("                  ucase    - convert to uppercase");
    eprintln!("                  lcase    - convert to lowercase");
    eprintln!("                  notrunc  - do not truncate output file");
    eprintln!("                  sync     - pad input blocks with NULs to ibs");
    eprintln!("                  noerror  - continue after read errors");
    eprintln!("                  fsync    - fsync output file when done");
    eprintln!("                  swab     - swap every pair of bytes");
    eprintln!("  status=LEVEL  Output level: none, noxfer, progress");
    eprintln!();
    eprintln!("Size suffixes: c=1, w=2, b=512, K=1024, M=1024^2, G=1024^3");
    eprintln!("               kB=1000, MB=1000^2, GB=1000^3");
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = env::args().collect();
    let mut opts = Options::default();
    let mut bs_set = false;

    for arg in argv.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_usage();
            process::exit(0);
        }

        let Some((key, value)) = arg.split_once('=') else {
            return Err(format!("invalid operand: '{arg}' (expected key=value)"));
        };

        match key {
            "if" => opts.input_file = Some(value.to_string()),
            "of" => opts.output_file = Some(value.to_string()),
            "bs" => {
                let size = parse_size(value)
                    .map_err(|e| format!("invalid bs: {e}"))?;
                if size == 0 {
                    return Err("bs must be greater than 0".to_string());
                }
                opts.ibs = size;
                opts.obs = size;
                bs_set = true;
            }
            "ibs" => {
                let size = parse_size(value)
                    .map_err(|e| format!("invalid ibs: {e}"))?;
                if size == 0 {
                    return Err("ibs must be greater than 0".to_string());
                }
                opts.ibs = size;
                // Only override obs if bs was not explicitly set.
                if !bs_set {
                    // ibs/obs are independent; don't touch obs.
                }
            }
            "obs" => {
                let size = parse_size(value)
                    .map_err(|e| format!("invalid obs: {e}"))?;
                if size == 0 {
                    return Err("obs must be greater than 0".to_string());
                }
                opts.obs = size;
            }
            "count" => {
                let n = parse_size(value)
                    .map_err(|e| format!("invalid count: {e}"))?;
                opts.count = Some(n);
            }
            "skip" => {
                opts.skip = parse_size(value)
                    .map_err(|e| format!("invalid skip: {e}"))?;
            }
            "seek" => {
                opts.seek = parse_size(value)
                    .map_err(|e| format!("invalid seek: {e}"))?;
            }
            "conv" => {
                opts.conv = parse_conv(value)?;
            }
            "status" => {
                opts.status = parse_status(value)?;
            }
            other => {
                return Err(format!("unrecognized operand: '{other}={value}'"));
            }
        }
    }

    Ok(opts)
}

// ============================================================================
// Transfer statistics
// ============================================================================

struct Stats {
    full_blocks_in: u64,
    partial_blocks_in: u64,
    full_blocks_out: u64,
    partial_blocks_out: u64,
    bytes_copied: u64,
    start_time: Instant,
}

impl Stats {
    fn new() -> Self {
        Self {
            full_blocks_in: 0,
            partial_blocks_in: 0,
            full_blocks_out: 0,
            partial_blocks_out: 0,
            bytes_copied: 0,
            start_time: Instant::now(),
        }
    }

    fn record_read(&mut self, bytes_read: u64, block_size: u64) {
        if bytes_read == block_size {
            self.full_blocks_in = self.full_blocks_in.saturating_add(1);
        } else {
            self.partial_blocks_in = self.partial_blocks_in.saturating_add(1);
        }
    }

    fn record_write(&mut self, bytes_written: u64, block_size: u64) {
        self.bytes_copied = self.bytes_copied.saturating_add(bytes_written);
        if bytes_written == block_size {
            self.full_blocks_out = self.full_blocks_out.saturating_add(1);
        } else {
            self.partial_blocks_out = self.partial_blocks_out.saturating_add(1);
        }
    }

    fn elapsed_secs(&self) -> f64 {
        let elapsed = self.start_time.elapsed();
        elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 / 1_000_000_000.0
    }
}

// ============================================================================
// Human-readable formatting
// ============================================================================

/// Format a byte count as a human-readable SI string (e.g. "524 kB").
fn format_si(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        let gb = bytes as f64 / 1_000_000_000.0;
        format!("{gb:.1} GB")
    } else if bytes >= 1_000_000 {
        let mb = bytes as f64 / 1_000_000.0;
        format!("{mb:.1} MB")
    } else if bytes >= 1_000 {
        let kb = bytes as f64 / 1_000.0;
        format!("{kb:.0} kB")
    } else {
        format!("{bytes} B")
    }
}

/// Format a byte count as a human-readable IEC string (e.g. "512 KiB").
fn format_iec(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        let gib = bytes as f64 / 1_073_741_824.0;
        format!("{gib:.1} GiB")
    } else if bytes >= 1_048_576 {
        let mib = bytes as f64 / 1_048_576.0;
        format!("{mib:.1} MiB")
    } else if bytes >= 1_024 {
        let kib = bytes as f64 / 1_024.0;
        format!("{kib:.0} KiB")
    } else {
        format!("{bytes} B")
    }
}

/// Format a transfer speed as a human-readable string.
fn format_speed(bytes: u64, secs: f64) -> String {
    if secs <= 0.0 {
        return "Infinity B/s".to_string();
    }
    let bps = bytes as f64 / secs;
    if bps >= 1_000_000_000.0 {
        format!("{:.1} GB/s", bps / 1_000_000_000.0)
    } else if bps >= 1_000_000.0 {
        format!("{:.1} MB/s", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.1} kB/s", bps / 1_000.0)
    } else {
        format!("{bps:.0} B/s")
    }
}

/// Print the final transfer summary to stderr.
fn print_summary(stats: &Stats) {
    let elapsed = stats.elapsed_secs();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let _ = writeln!(
        err,
        "{}+{} records in",
        stats.full_blocks_in, stats.partial_blocks_in,
    );
    let _ = writeln!(
        err,
        "{}+{} records out",
        stats.full_blocks_out, stats.partial_blocks_out,
    );
    let _ = writeln!(
        err,
        "{} bytes ({}, {}) copied, {:.3} s, {}",
        stats.bytes_copied,
        format_si(stats.bytes_copied),
        format_iec(stats.bytes_copied),
        elapsed,
        format_speed(stats.bytes_copied, elapsed),
    );
}

/// Print a progress line (overwrites itself on the same line) to stderr.
fn print_progress(stats: &Stats) {
    let elapsed = stats.elapsed_secs();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let _ = write!(
        err,
        "\r{} bytes ({}, {}) copied, {:.3} s, {}",
        stats.bytes_copied,
        format_si(stats.bytes_copied),
        format_iec(stats.bytes_copied),
        elapsed,
        format_speed(stats.bytes_copied, elapsed),
    );
    let _ = err.flush();
}

// ============================================================================
// In-place data conversions
// ============================================================================

/// Apply the requested conversions to a data buffer in-place.
fn apply_conversions(buf: &mut [u8], conv: &ConvFlags) {
    // Byte-swap: swap every adjacent pair of bytes.
    if conv.swab {
        let pairs = buf.len() / 2;
        for i in 0..pairs {
            let a = i * 2;
            let b = a + 1;
            buf.swap(a, b);
        }
    }

    if conv.ucase {
        for byte in buf.iter_mut() {
            if *byte >= b'a' && *byte <= b'z' {
                *byte -= b'a' - b'A';
            }
        }
    } else if conv.lcase {
        for byte in buf.iter_mut() {
            if *byte >= b'A' && *byte <= b'Z' {
                *byte += b'a' - b'A';
            }
        }
    }
}

// ============================================================================
// I/O helpers
// ============================================================================

/// Wrapper around an input source (file or stdin).
enum Input {
    File(File),
    Stdin(io::Stdin),
}

impl Input {
    fn read_exact_or_short(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total = 0;
        while total < buf.len() {
            let n = match self {
                Input::File(f) => f.read(&mut buf[total..])?,
                Input::Stdin(s) => s.read(&mut buf[total..])?,
            };
            if n == 0 {
                break; // EOF
            }
            total += n;
        }
        Ok(total)
    }
}

/// Wrapper around an output destination (file or stdout).
enum Output {
    File(File),
    Stdout(io::Stdout),
}

impl Output {
    fn write_all_bytes(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            Output::File(f) => f.write_all(buf),
            Output::Stdout(s) => s.write_all(buf),
        }
    }

    fn flush_output(&mut self) -> io::Result<()> {
        match self {
            Output::File(f) => f.flush(),
            Output::Stdout(s) => s.flush(),
        }
    }

    fn sync_all(&mut self) -> io::Result<()> {
        match self {
            Output::File(f) => f.sync_all(),
            Output::Stdout(_) => Ok(()), // Cannot fsync stdout.
        }
    }
}

// ============================================================================
// Core copy loop
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;

    install_signal_handler();

    // --- Open input ---
    let mut input = match &opts.input_file {
        Some(path) => {
            let f = File::open(path)
                .map_err(|e| format!("failed to open '{}': {}", path, e))?;
            Input::File(f)
        }
        None => Input::Stdin(io::stdin()),
    };

    // --- Open output ---
    let mut output = match &opts.output_file {
        Some(path) => {
            let mut open_opts = OpenOptions::new();
            open_opts.write(true).create(true);
            if !opts.conv.notrunc && opts.seek == 0 {
                open_opts.truncate(true);
            }
            let f = open_opts
                .open(path)
                .map_err(|e| format!("failed to open '{}': {}", path, e))?;
            Output::File(f)
        }
        None => Output::Stdout(io::stdout()),
    };

    // --- Skip input blocks ---
    if opts.skip > 0 {
        let skip_bytes = opts.skip.checked_mul(opts.ibs)
            .ok_or_else(|| "skip * ibs overflow".to_string())?;

        // Try seeking first; fall back to reading and discarding.
        let seeked = if let Input::File(ref mut f) = input {
            f.seek(SeekFrom::Current(skip_bytes as i64)).is_ok()
        } else {
            false
        };

        if !seeked {
            let mut discard_buf = vec![0u8; opts.ibs.min(65536) as usize];
            let mut remaining = skip_bytes;
            while remaining > 0 {
                let to_read = remaining.min(discard_buf.len() as u64) as usize;
                let n = input
                    .read_exact_or_short(&mut discard_buf[..to_read])
                    .map_err(|e| format!("error skipping input: {e}"))?;
                if n == 0 {
                    break; // EOF before finishing skip -- not an error.
                }
                remaining = remaining.saturating_sub(n as u64);
            }
        }
    }

    // --- Seek in output ---
    if opts.seek > 0 {
        let seek_bytes = opts.seek.checked_mul(opts.obs)
            .ok_or_else(|| "seek * obs overflow".to_string())?;

        let seeked = if let Output::File(ref mut f) = output {
            f.seek(SeekFrom::Start(seek_bytes)).is_ok()
        } else {
            false
        };

        // If seek failed (e.g. stdout), write NUL padding.
        if !seeked {
            let mut pad = vec![0u8; opts.obs.min(65536) as usize];
            let mut remaining = seek_bytes;
            while remaining > 0 {
                let to_write = remaining.min(pad.len() as u64) as usize;
                output
                    .write_all_bytes(&mut pad[..to_write])
                    .map_err(|e| format!("error seeking output: {e}"))?;
                remaining = remaining.saturating_sub(to_write as u64);
            }
        }
    }

    // --- Allocate buffers ---
    let ibs = opts.ibs as usize;
    let obs = opts.obs as usize;
    let mut in_buf = vec![0u8; ibs];

    // When ibs != obs, we need an intermediate output accumulator buffer.
    // Data is read in ibs-sized chunks, accumulated, then flushed in
    // obs-sized chunks.
    let needs_reblock = ibs != obs;
    let mut out_accum: Vec<u8> = if needs_reblock {
        Vec::with_capacity(obs)
    } else {
        Vec::new()
    };

    let mut stats = Stats::new();
    let mut blocks_read: u64 = 0;
    let mut last_progress = Instant::now();

    // --- Main copy loop ---
    loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }

        // Check block count limit.
        if let Some(max) = opts.count {
            if blocks_read >= max {
                break;
            }
        }

        // Read one input block.
        let bytes_read = match input.read_exact_or_short(&mut in_buf) {
            Ok(n) => n,
            Err(e) => {
                if opts.conv.noerror {
                    let stderr = io::stderr();
                    let mut err = stderr.lock();
                    let _ = writeln!(err, "dd: read error: {e}");
                    // With noerror+sync, fill the block with NULs.
                    if opts.conv.sync {
                        in_buf.fill(0);
                        ibs
                    } else {
                        // Skip this block.
                        blocks_read = blocks_read.saturating_add(1);
                        continue;
                    }
                } else {
                    return Err(format!("read error: {e}"));
                }
            }
        };

        if bytes_read == 0 {
            break; // EOF
        }

        blocks_read = blocks_read.saturating_add(1);
        stats.record_read(bytes_read as u64, opts.ibs);

        let data_end = bytes_read;

        // conv=sync: pad short reads to ibs with NUL bytes.
        let data = if opts.conv.sync && bytes_read < ibs {
            // Zero the padding region.
            in_buf[bytes_read..ibs].fill(0);
            &mut in_buf[..ibs]
        } else {
            &mut in_buf[..data_end]
        };

        // Apply in-place conversions.
        apply_conversions(data, &opts.conv);

        // --- Write output ---
        if needs_reblock {
            // Accumulate into out_accum, flush in obs-sized chunks.
            out_accum.extend_from_slice(data);

            while out_accum.len() >= obs {
                let chunk: Vec<u8> = out_accum.drain(..obs).collect();
                output
                    .write_all_bytes(&chunk)
                    .map_err(|e| format!("write error: {e}"))?;
                stats.record_write(obs as u64, opts.obs);
            }
        } else {
            // ibs == obs: write directly.
            output
                .write_all_bytes(data)
                .map_err(|e| format!("write error: {e}"))?;
            stats.record_write(data.len() as u64, opts.obs);
        }

        // Periodic progress output.
        if opts.status == StatusLevel::Progress {
            let now = Instant::now();
            if now.duration_since(last_progress).as_millis() >= 1000 {
                print_progress(&stats);
                last_progress = now;
            }
        }
    }

    // Flush any remaining data in the reblock accumulator.
    if needs_reblock && !out_accum.is_empty() {
        let remaining = out_accum.len();
        output
            .write_all_bytes(&out_accum)
            .map_err(|e| format!("write error: {e}"))?;
        stats.record_write(remaining as u64, opts.obs);
        out_accum.clear();
    }

    // Flush output.
    output
        .flush_output()
        .map_err(|e| format!("flush error: {e}"))?;

    // conv=fsync: force data to disk.
    if opts.conv.fsync {
        output
            .sync_all()
            .map_err(|e| format!("fsync error: {e}"))?;
    }

    // Print final statistics.
    match opts.status {
        StatusLevel::None => {}
        StatusLevel::Noxfer => {
            let stderr = io::stderr();
            let mut err = stderr.lock();
            let _ = writeln!(
                err,
                "{}+{} records in",
                stats.full_blocks_in, stats.partial_blocks_in,
            );
            let _ = writeln!(
                err,
                "{}+{} records out",
                stats.full_blocks_out, stats.partial_blocks_out,
            );
        }
        StatusLevel::Default | StatusLevel::Progress => {
            if opts.status == StatusLevel::Progress {
                // Clear the progress line before printing the final summary.
                let stderr = io::stderr();
                let mut err = stderr.lock();
                let _ = write!(err, "\r\x1b[K");
            }
            print_summary(&stats);
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("dd: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Size parsing ---

    #[test]
    fn parse_size_bare_number() {
        assert_eq!(parse_size("512").unwrap(), 512);
        assert_eq!(parse_size("0").unwrap(), 0);
        assert_eq!(parse_size("1").unwrap(), 1);
    }

    #[test]
    fn parse_size_suffix_c() {
        assert_eq!(parse_size("10c").unwrap(), 10);
    }

    #[test]
    fn parse_size_suffix_w() {
        assert_eq!(parse_size("10w").unwrap(), 20);
    }

    #[test]
    fn parse_size_suffix_b() {
        assert_eq!(parse_size("2b").unwrap(), 1024);
    }

    #[test]
    fn parse_size_suffix_k() {
        assert_eq!(parse_size("4K").unwrap(), 4096);
    }

    #[test]
    fn parse_size_suffix_m() {
        assert_eq!(parse_size("1M").unwrap(), 1_048_576);
    }

    #[test]
    fn parse_size_suffix_g() {
        assert_eq!(parse_size("2G").unwrap(), 2_147_483_648);
    }

    #[test]
    fn parse_size_suffix_kb() {
        assert_eq!(parse_size("1kB").unwrap(), 1_000);
    }

    #[test]
    fn parse_size_suffix_mb() {
        assert_eq!(parse_size("1MB").unwrap(), 1_000_000);
    }

    #[test]
    fn parse_size_suffix_gb() {
        assert_eq!(parse_size("1GB").unwrap(), 1_000_000_000);
    }

    #[test]
    fn parse_size_empty_is_error() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn parse_size_invalid_suffix() {
        assert!(parse_size("10X").is_err());
    }

    #[test]
    fn parse_size_non_numeric() {
        assert!(parse_size("abc").is_err());
    }

    // --- Conversion flag parsing ---

    #[test]
    fn parse_conv_single() {
        let c = parse_conv("ucase").unwrap();
        assert!(c.ucase);
        assert!(!c.lcase);
    }

    #[test]
    fn parse_conv_multiple() {
        let c = parse_conv("notrunc,sync,noerror").unwrap();
        assert!(c.notrunc);
        assert!(c.sync);
        assert!(c.noerror);
        assert!(!c.ucase);
    }

    #[test]
    fn parse_conv_swab() {
        let c = parse_conv("swab").unwrap();
        assert!(c.swab);
    }

    #[test]
    fn parse_conv_fsync() {
        let c = parse_conv("fsync").unwrap();
        assert!(c.fsync);
    }

    #[test]
    fn parse_conv_ucase_lcase_conflict() {
        assert!(parse_conv("ucase,lcase").is_err());
    }

    #[test]
    fn parse_conv_unknown() {
        assert!(parse_conv("badconv").is_err());
    }

    #[test]
    fn parse_conv_empty_tokens() {
        // Trailing comma should not cause an error.
        let c = parse_conv("ucase,").unwrap();
        assert!(c.ucase);
    }

    // --- Status level parsing ---

    #[test]
    fn parse_status_valid() {
        assert_eq!(parse_status("none").unwrap(), StatusLevel::None);
        assert_eq!(parse_status("noxfer").unwrap(), StatusLevel::Noxfer);
        assert_eq!(parse_status("progress").unwrap(), StatusLevel::Progress);
    }

    #[test]
    fn parse_status_invalid() {
        assert!(parse_status("verbose").is_err());
    }

    // --- Conversions ---

    #[test]
    fn apply_ucase() {
        let conv = ConvFlags { ucase: true, ..ConvFlags::default() };
        let mut data = b"hello WORLD 123".to_vec();
        apply_conversions(&mut data, &conv);
        assert_eq!(&data, b"HELLO WORLD 123");
    }

    #[test]
    fn apply_lcase() {
        let conv = ConvFlags { lcase: true, ..ConvFlags::default() };
        let mut data = b"HELLO world 123".to_vec();
        apply_conversions(&mut data, &conv);
        assert_eq!(&data, b"hello world 123");
    }

    #[test]
    fn apply_swab_even() {
        let conv = ConvFlags { swab: true, ..ConvFlags::default() };
        let mut data = vec![0x01, 0x02, 0x03, 0x04];
        apply_conversions(&mut data, &conv);
        assert_eq!(data, vec![0x02, 0x01, 0x04, 0x03]);
    }

    #[test]
    fn apply_swab_odd() {
        let conv = ConvFlags { swab: true, ..ConvFlags::default() };
        let mut data = vec![0x01, 0x02, 0x03];
        apply_conversions(&mut data, &conv);
        // Only the first pair is swapped; the trailing byte is untouched.
        assert_eq!(data, vec![0x02, 0x01, 0x03]);
    }

    #[test]
    fn apply_swab_empty() {
        let conv = ConvFlags { swab: true, ..ConvFlags::default() };
        let mut data: Vec<u8> = vec![];
        apply_conversions(&mut data, &conv);
        assert!(data.is_empty());
    }

    #[test]
    fn apply_no_conversions() {
        let conv = ConvFlags::default();
        let mut data = b"unchanged".to_vec();
        let original = data.clone();
        apply_conversions(&mut data, &conv);
        assert_eq!(data, original);
    }

    #[test]
    fn apply_swab_then_ucase() {
        // swab is applied before ucase.
        let conv = ConvFlags { swab: true, ucase: true, ..ConvFlags::default() };
        let mut data = b"abcd".to_vec();
        apply_conversions(&mut data, &conv);
        // After swab: b"badc", after ucase: b"BADC"
        assert_eq!(&data, b"BADC");
    }

    // --- Formatting ---

    #[test]
    fn format_si_bytes() {
        assert_eq!(format_si(0), "0 B");
        assert_eq!(format_si(999), "999 B");
    }

    #[test]
    fn format_si_kb() {
        assert_eq!(format_si(1_000), "1 kB");
        assert_eq!(format_si(524_288), "524 kB");
    }

    #[test]
    fn format_si_mb() {
        assert_eq!(format_si(1_000_000), "1.0 MB");
        assert_eq!(format_si(10_500_000), "10.5 MB");
    }

    #[test]
    fn format_si_gb() {
        assert_eq!(format_si(1_000_000_000), "1.0 GB");
    }

    #[test]
    fn format_iec_bytes() {
        assert_eq!(format_iec(0), "0 B");
        assert_eq!(format_iec(1023), "1023 B");
    }

    #[test]
    fn format_iec_kib() {
        assert_eq!(format_iec(1_024), "1 KiB");
        assert_eq!(format_iec(524_288), "512 KiB");
    }

    #[test]
    fn format_iec_mib() {
        assert_eq!(format_iec(1_048_576), "1.0 MiB");
    }

    #[test]
    fn format_iec_gib() {
        assert_eq!(format_iec(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn format_speed_zero_time() {
        assert_eq!(format_speed(1000, 0.0), "Infinity B/s");
    }

    #[test]
    fn format_speed_bps() {
        assert_eq!(format_speed(500, 1.0), "500 B/s");
    }

    #[test]
    fn format_speed_kbps() {
        assert_eq!(format_speed(5_000, 1.0), "5.0 kB/s");
    }

    #[test]
    fn format_speed_mbps() {
        assert_eq!(format_speed(10_000_000, 1.0), "10.0 MB/s");
    }

    #[test]
    fn format_speed_gbps() {
        assert_eq!(format_speed(2_000_000_000, 1.0), "2.0 GB/s");
    }

    // --- Statistics tracking ---

    #[test]
    fn stats_initial() {
        let stats = Stats::new();
        assert_eq!(stats.full_blocks_in, 0);
        assert_eq!(stats.partial_blocks_in, 0);
        assert_eq!(stats.full_blocks_out, 0);
        assert_eq!(stats.partial_blocks_out, 0);
        assert_eq!(stats.bytes_copied, 0);
    }

    #[test]
    fn stats_record_full_read() {
        let mut stats = Stats::new();
        stats.record_read(512, 512);
        assert_eq!(stats.full_blocks_in, 1);
        assert_eq!(stats.partial_blocks_in, 0);
    }

    #[test]
    fn stats_record_partial_read() {
        let mut stats = Stats::new();
        stats.record_read(100, 512);
        assert_eq!(stats.full_blocks_in, 0);
        assert_eq!(stats.partial_blocks_in, 1);
    }

    #[test]
    fn stats_record_full_write() {
        let mut stats = Stats::new();
        stats.record_write(512, 512);
        assert_eq!(stats.full_blocks_out, 1);
        assert_eq!(stats.partial_blocks_out, 0);
        assert_eq!(stats.bytes_copied, 512);
    }

    #[test]
    fn stats_record_partial_write() {
        let mut stats = Stats::new();
        stats.record_write(100, 512);
        assert_eq!(stats.full_blocks_out, 0);
        assert_eq!(stats.partial_blocks_out, 1);
        assert_eq!(stats.bytes_copied, 100);
    }

    #[test]
    fn stats_cumulative_writes() {
        let mut stats = Stats::new();
        stats.record_write(512, 512);
        stats.record_write(512, 512);
        stats.record_write(200, 512);
        assert_eq!(stats.full_blocks_out, 2);
        assert_eq!(stats.partial_blocks_out, 1);
        assert_eq!(stats.bytes_copied, 1224);
    }
}
