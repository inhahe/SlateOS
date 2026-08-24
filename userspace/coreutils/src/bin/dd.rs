//! dd — convert and copy a file.
//!
//! Usage: dd [if=FILE] [of=FILE] [bs=N] [count=N] [skip=N] [seek=N]
//!   if=     input file (default: stdin)
//!   of=     output file (default: stdout)
//!   bs=     block size in bytes (default: 512)
//!   count=  number of blocks to copy
//!   skip=   skip N blocks at start of input
//!   seek=   skip N blocks at start of output
//!
//! N may carry a multiplier suffix — `c`=1, `w`=2, `b`=512, `k`/`K`=1024,
//! `kB`=1000, `M`=1048576, `MB`=1000000, and so on — and may be a product
//! written `2x512`. An unrecognised one is an error, never a silent zero.
//!
//! `conv=`, `status=`, `ibs=`/`obs=` and the `iflag=`/`oflag=` family are not
//! implemented, and are *rejected* rather than ignored. That is deliberate: a
//! `dd` that accepts `conv=notrunc` and truncates anyway is worse than one that
//! refuses the request, because the caller has no way to find out.
//!
//! Two properties here are about not destroying data, and both were once wrong
//! (see `known-issues.md` → `B-dd-DESTROYS-THE-OUTPUT-FILE-WHEN-seek-IS-GIVEN`):
//! the output is truncated only when `seek=` is absent, and every seek, skip,
//! write and flush failure is fatal rather than ignored.

use coreutils::diag;
use coreutils::quote::quoteaf_os;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process;
use std::time::Instant;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DdOperands {
    input_file: Option<String>,
    output_file: Option<String>,
    bs: usize,
    count: Option<usize>,
    skip: usize,
    seek: usize,
}

impl Default for DdOperands {
    fn default() -> Self {
        Self {
            input_file: None,
            output_file: None,
            bs: 512,
            count: None,
            skip: 0,
            seek: 0,
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let ops = match parse_operands(&args) {
        Ok(o) => o,
        Err(e) => {
            diag!("dd: {e}");
            process::exit(1);
        }
    };

    let bs = ops.bs;
    let mut reader = open_input(&ops, bs);
    let mut writer = open_output(&ops, bs);

    let start = Instant::now();
    let mut buf = vec![0u8; bs];
    let mut blocks_in: usize = 0;
    let mut blocks_out: usize = 0;
    let mut partial_in: usize = 0;
    let mut partial_out: usize = 0;
    let mut total_bytes: u64 = 0;

    loop {
        if let Some(c) = ops.count
            && blocks_in.saturating_add(partial_in) >= c
        {
            break;
        }

        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                diag!("dd: read error: {e}");
                process::exit(1);
            }
        };

        if n == bs {
            blocks_in = blocks_in.saturating_add(1);
        } else {
            partial_in = partial_in.saturating_add(1);
        }

        match writer.write_all(buf.get(..n).unwrap_or(&[])) {
            Ok(()) => {
                if n == bs {
                    blocks_out = blocks_out.saturating_add(1);
                } else {
                    partial_out = partial_out.saturating_add(1);
                }
                total_bytes = total_bytes.saturating_add(n as u64);
            }
            Err(e) => {
                diag!("dd: write error: {e}");
                process::exit(1);
            }
        }
    }

    // A failed flush loses the tail of the copy. Reporting success after that
    // is how a truncated file gets treated as a good one by whatever runs next.
    if let Err(e) = writer.flush() {
        diag!("dd: write error: {e}");
        process::exit(1);
    }
    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();

    diag!("{blocks_in}+{partial_in} records in");
    diag!("{blocks_out}+{partial_out} records out");
    diag!("{}", format_rate_line(total_bytes, secs));
}

/// Open the input and position it past `skip=` blocks.
///
/// A failure to skip is fatal. It used to be ignored — the code fell back to
/// the unskipped reader — which meant `skip=` silently copied from offset 0,
/// producing a file that is the wrong contents rather than an error.
fn open_input(ops: &DdOperands, bs: usize) -> Box<dyn Read> {
    let skip_bytes = (ops.skip as u64).saturating_mul(bs as u64);

    let Some(path) = &ops.input_file else {
        // stdin may be a pipe, which cannot seek, so the skipped bytes have to
        // actually be read. A short read is not end-of-input, so this counts
        // bytes rather than calls — the previous version counted calls and so
        // skipped less than asked whenever a pipe delivered a partial block.
        let mut stdin = io::stdin();
        let mut discarded: u64 = 0;
        let mut buf = vec![0u8; bs];
        while discarded < skip_bytes {
            let want = usize::try_from(skip_bytes.saturating_sub(discarded))
                .unwrap_or(bs)
                .min(bs);
            match stdin.read(buf.get_mut(..want).unwrap_or(&mut [])) {
                // Running out of input while skipping is not an error in dd;
                // the copy simply has nothing left to do.
                Ok(0) => break,
                Ok(n) => discarded = discarded.saturating_add(n as u64),
                Err(e) => {
                    diag!("dd: read error while skipping: {e}");
                    process::exit(1);
                }
            }
        }
        return Box::new(stdin);
    };

    let mut fh = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            diag!("dd: failed to open {}: {e}", quoteaf_os(path));
            process::exit(1);
        }
    };
    if skip_bytes > 0
        && let Err(e) = fh.seek(SeekFrom::Start(skip_bytes))
    {
        diag!(
            "dd: cannot skip to offset {skip_bytes} in {}: {e}",
            quoteaf_os(path)
        );
        process::exit(1);
    }
    Box::new(fh)
}

/// Open the output and position it past `seek=` blocks.
///
/// **Truncation has to be decided here, at open time.** This previously opened
/// with `truncate(true)` unconditionally and then, when `seek=` was given,
/// re-opened with `truncate(false)` — but the first open had already emptied
/// the file, so `dd if=part of=disk.img seek=N` destroyed everything already in
/// `disk.img` before writing at the offset. GNU passes `O_TRUNC` only when
/// `seek=` is absent (and `conv=notrunc` is not given), which is what the
/// `ops.seek == 0` below reproduces.
fn open_output(ops: &DdOperands, bs: usize) -> Box<dyn Write> {
    let seek_bytes = (ops.seek as u64).saturating_mul(bs as u64);

    let Some(path) = &ops.output_file else {
        return Box::new(io::stdout());
    };

    let mut fh = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(ops.seek == 0)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            diag!("dd: failed to open {}: {e}", quoteaf_os(path));
            process::exit(1);
        }
    };
    if seek_bytes > 0
        && let Err(e) = fh.seek(SeekFrom::Start(seek_bytes))
    {
        // Also previously ignored, which wrote the payload at offset 0 — over
        // the very data `seek=` was there to preserve.
        diag!(
            "dd: cannot seek to offset {seek_bytes} in {}: {e}",
            quoteaf_os(path)
        );
        process::exit(1);
    }
    Box::new(fh)
}

/// Parse dd's `key=value` operands.
fn parse_operands(args: &[String]) -> Result<DdOperands, String> {
    let mut ops = DdOperands::default();
    for arg in args {
        let Some((key, val)) = arg.split_once('=') else {
            return Err(format!("unrecognized argument: {arg}"));
        };
        match key {
            "if" => ops.input_file = Some(val.to_string()),
            "of" => ops.output_file = Some(val.to_string()),
            "bs" => {
                let n = parse_size(val)?;
                // A zero block size would make every read return `Ok(0)`, which
                // the copy loop reads as end-of-input: dd would report success
                // having copied nothing, after truncating the output.
                if n == 0 {
                    return Err(format!("invalid number: '{val}'"));
                }
                ops.bs = n;
            }
            "count" => ops.count = Some(parse_size(val)?),
            "skip" => ops.skip = parse_size(val)?,
            "seek" => ops.seek = parse_size(val)?,
            _ => return Err(format!("unknown operand: {key}")),
        }
    }
    Ok(ops)
}

/// Multiplier suffixes, in the order they are tried. Longest first: `kB` has to
/// be matched before `B` would be, and `MB` before `M`.
///
/// The pairs come from GNU dd. Note that the two-letter forms are powers of
/// 1000 and the one-letter forms are powers of 1024 — `bs=1MB` and `bs=1M`
/// differ by about 5%, which is exactly why silently reading an unrecognised
/// suffix as "0" was dangerous rather than merely unhelpful.
const SIZE_SUFFIXES: &[(&str, u64)] = &[
    ("kB", 1000),
    ("MB", 1000 * 1000),
    ("GB", 1000 * 1000 * 1000),
    ("TB", 1000 * 1000 * 1000 * 1000),
    ("k", 1024),
    ("K", 1024),
    ("m", 1024 * 1024),
    ("M", 1024 * 1024),
    ("g", 1024 * 1024 * 1024),
    ("G", 1024 * 1024 * 1024),
    ("T", 1024_u64.pow(4)),
    ("b", 512),
    ("c", 1),
    ("w", 2),
];

/// Parse one of dd's numeric operand values.
///
/// Returns an error on anything that is not a number, rather than the 0 this
/// used to return. Silently reading `bs=1MB` as `bs=0` meant dd truncated the
/// output file, copied nothing, and exited 0 — the input was intact but the
/// destination had been destroyed, and nothing said so.
///
/// `N x M` products are supported as GNU spells them, `bs=2x512`.
fn parse_size(s: &str) -> Result<usize, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("invalid number: ''".to_string());
    }
    let mut product: u64 = 1;
    for part in trimmed.split('x') {
        product = product
            .checked_mul(parse_size_factor(part)?)
            .ok_or_else(|| format!("number out of range: '{trimmed}'"))?;
    }
    usize::try_from(product).map_err(|_| format!("number out of range: '{trimmed}'"))
}

/// One factor of a size operand: digits with at most one multiplier suffix.
fn parse_size_factor(part: &str) -> Result<u64, String> {
    let bad = || format!("invalid number: '{part}'");
    if part.is_empty() {
        return Err(bad());
    }
    let (digits, multiplier) = SIZE_SUFFIXES
        .iter()
        .find_map(|&(suffix, mult)| part.strip_suffix(suffix).map(|d| (d, mult)))
        .unwrap_or((part, 1));
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    digits
        .parse::<u64>()
        .map_err(|_| bad())?
        .checked_mul(multiplier)
        .ok_or_else(|| format!("number out of range: '{part}'"))
}

/// Format dd's final progress line based on total bytes and elapsed time.
// The u64 -> f64 casts lose precision above 2^53 bytes (8 PiB). This is a
// human-readable rate printed to one decimal place, so a copy large enough to
// hit that would still round to the same displayed figure.
#[allow(clippy::cast_precision_loss)]
fn format_rate_line(total_bytes: u64, secs: f64) -> String {
    if secs <= 0.0 {
        return format!("{total_bytes} bytes copied");
    }
    let rate = total_bytes as f64 / secs;
    if rate >= 1_000_000_000.0 {
        format!(
            "{total_bytes} bytes ({:.1} GB) copied, {secs:.6} s, {:.1} GB/s",
            total_bytes as f64 / 1e9,
            rate / 1e9
        )
    } else if rate >= 1_000_000.0 {
        format!(
            "{total_bytes} bytes ({:.1} MB) copied, {secs:.6} s, {:.1} MB/s",
            total_bytes as f64 / 1e6,
            rate / 1e6
        )
    } else {
        format!("{total_bytes} bytes copied, {secs:.6} s, {rate:.0} bytes/s")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_size_plain() {
        assert_eq!(parse_size("100").unwrap(), 100);
    }

    #[test]
    fn parse_size_k_suffix() {
        assert_eq!(parse_size("1k").unwrap(), 1024);
        assert_eq!(parse_size("4K").unwrap(), 4 * 1024);
    }

    #[test]
    fn parse_size_m_suffix() {
        assert_eq!(parse_size("1m").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("2M").unwrap(), 2 * 1024 * 1024);
    }

    #[test]
    fn parse_size_g_suffix() {
        assert_eq!(parse_size("1g").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
    }

    // The two-letter suffixes are powers of 1000, not 1024. `1MB` used to parse
    // as 0 because only the last byte was inspected and `B` matched nothing.
    #[test]
    fn parse_size_decimal_suffixes() {
        assert_eq!(parse_size("1kB").unwrap(), 1000);
        assert_eq!(parse_size("1MB").unwrap(), 1_000_000);
        assert_eq!(parse_size("2GB").unwrap(), 2_000_000_000);
    }

    #[test]
    fn parse_size_block_and_char_suffixes() {
        assert_eq!(parse_size("1b").unwrap(), 512);
        assert_eq!(parse_size("3c").unwrap(), 3);
        assert_eq!(parse_size("3w").unwrap(), 6);
    }

    #[test]
    fn parse_size_product() {
        assert_eq!(parse_size("2x512").unwrap(), 1024);
        assert_eq!(parse_size("2x3x4").unwrap(), 24);
    }

    // These four used to return 0 rather than an error. Returning 0 for `bs=`
    // meant "copy nothing into the file you just truncated", which is the
    // silent-data-loss path this whole set of tests exists to pin down.
    #[test]
    fn parse_size_empty_is_an_error() {
        assert!(parse_size("").unwrap_err().contains("invalid number"));
    }

    #[test]
    fn parse_size_garbage_is_an_error() {
        assert!(
            parse_size("notanumber")
                .unwrap_err()
                .contains("invalid number")
        );
    }

    #[test]
    fn parse_size_unknown_suffix_is_an_error() {
        // `1Q` is not a suffix we know; it must not silently become 1 or 0.
        assert!(parse_size("1Q").unwrap_err().contains("invalid number"));
    }

    #[test]
    fn parse_size_suffix_without_digits_is_an_error() {
        assert!(parse_size("k").unwrap_err().contains("invalid number"));
    }

    #[test]
    fn parse_size_negative_is_an_error() {
        assert!(parse_size("-1").unwrap_err().contains("invalid number"));
    }

    #[test]
    fn parse_size_overflow_is_an_error() {
        let err = parse_size("99999999999999999999G").unwrap_err();
        assert!(
            err.contains("invalid number") || err.contains("out of range"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_size_zero_plain() {
        assert_eq!(parse_size("0").unwrap(), 0);
    }

    #[test]
    fn parse_size_trims_whitespace() {
        assert_eq!(parse_size("  100  ").unwrap(), 100);
    }

    // A zero block size makes every read return Ok(0), which the copy loop
    // cannot distinguish from end-of-input.
    #[test]
    fn parse_operands_bs_zero_is_an_error() {
        let err = parse_operands(&s(&["bs=0"])).unwrap_err();
        assert!(err.contains("invalid number"), "got: {err}");
    }

    #[test]
    fn parse_operands_bad_number_is_an_error() {
        for arg in ["bs=1MBx", "count=x", "skip=zz", "seek=1Q"] {
            assert!(
                parse_operands(&s(&[arg])).is_err(),
                "{arg} should not parse"
            );
        }
    }

    // count= and skip= may legitimately be zero; only bs= may not.
    #[test]
    fn parse_operands_zero_count_and_skip_are_fine() {
        let o = parse_operands(&s(&["count=0", "skip=0", "seek=0"])).unwrap();
        assert_eq!(o.count, Some(0));
        assert_eq!(o.skip, 0);
        assert_eq!(o.seek, 0);
    }

    // The operands dd does not implement are rejected, not ignored. Accepting
    // `conv=notrunc` and truncating anyway would be worse than refusing it.
    #[test]
    fn parse_operands_unimplemented_conv_is_rejected() {
        for arg in ["conv=notrunc", "status=none", "ibs=512", "oflag=direct"] {
            let err = parse_operands(&s(&[arg])).unwrap_err();
            assert!(err.contains("unknown operand"), "{arg} gave: {err}");
        }
    }

    #[test]
    fn parse_operands_defaults() {
        let o = parse_operands(&s(&[])).unwrap();
        assert_eq!(o, DdOperands::default());
    }

    #[test]
    fn parse_operands_if_of() {
        let o = parse_operands(&s(&["if=a.bin", "of=b.bin"])).unwrap();
        assert_eq!(o.input_file.as_deref(), Some("a.bin"));
        assert_eq!(o.output_file.as_deref(), Some("b.bin"));
    }

    #[test]
    fn parse_operands_bs_count_skip_seek() {
        let o = parse_operands(&s(&["bs=4k", "count=10", "skip=1", "seek=2"])).unwrap();
        assert_eq!(o.bs, 4096);
        assert_eq!(o.count, Some(10));
        assert_eq!(o.skip, 1);
        assert_eq!(o.seek, 2);
    }

    #[test]
    fn parse_operands_unknown_operand_errors() {
        let err = parse_operands(&s(&["nope=1"])).unwrap_err();
        assert!(err.contains("unknown operand"));
    }

    #[test]
    fn parse_operands_no_equals_errors() {
        let err = parse_operands(&s(&["badarg"])).unwrap_err();
        assert!(err.contains("unrecognized"));
    }

    #[test]
    fn parse_operands_value_with_embedded_equals() {
        // split_once on '=' splits on the first '=' only.
        let o = parse_operands(&s(&["if=a=b.bin"])).unwrap();
        assert_eq!(o.input_file.as_deref(), Some("a=b.bin"));
    }

    #[test]
    fn rate_line_zero_elapsed() {
        assert_eq!(format_rate_line(1000, 0.0), "1000 bytes copied");
    }

    #[test]
    fn rate_line_negative_elapsed() {
        assert_eq!(format_rate_line(1000, -1.0), "1000 bytes copied");
    }

    #[test]
    fn rate_line_bytes_per_sec() {
        // 100 bytes in 1 second = 100 bytes/s
        let line = format_rate_line(100, 1.0);
        assert!(line.contains("100 bytes copied"));
        assert!(line.contains("1.000000 s"));
        assert!(line.contains("100 bytes/s"));
    }

    #[test]
    fn rate_line_mb_per_sec() {
        // 5 MB in 1 second = 5 MB/s.
        let line = format_rate_line(5_000_000, 1.0);
        assert!(line.contains("(5.0 MB)"));
        assert!(line.contains("5.0 MB/s"));
    }

    #[test]
    fn rate_line_gb_per_sec() {
        let line = format_rate_line(2_000_000_000, 1.0);
        assert!(line.contains("(2.0 GB)"));
        assert!(line.contains("2.0 GB/s"));
    }
}
