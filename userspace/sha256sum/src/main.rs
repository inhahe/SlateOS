//! Multi-personality cryptographic checksum utility for OurOS.
//!
//! This binary detects the algorithm from `argv[0]`:
//!   - `md5sum`    -> MD5    (RFC 1321, 128-bit)
//!   - `sha1sum`   -> SHA-1  (FIPS 180-4, 160-bit)
//!   - `sha256sum` -> SHA-256 (FIPS 180-4, 256-bit)
//!   - `sha512sum` -> SHA-512 (FIPS 180-4, 512-bit)
//!
//! Supports GNU coreutils-compatible flags: `-c`, `-b`, `-t`, `--check`,
//! `--tag`, `--quiet`, `--status`, `--warn`, `--strict`, `--help`,
//! `--version`.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::process;

// ---------------------------------------------------------------------------
// Algorithm selection
// ---------------------------------------------------------------------------

/// Which hash algorithm to use, detected from argv[0].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Algorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Md5 => "md5sum",
            Self::Sha1 => "sha1sum",
            Self::Sha256 => "sha256sum",
            Self::Sha512 => "sha512sum",
        }
    }

    fn digest_len(self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha512 => 64,
        }
    }

    /// Compute the hash of `data` and return hex string.
    fn hash_bytes(self, data: &[u8]) -> String {
        match self {
            Self::Md5 => hex_encode(&md5(data)),
            Self::Sha1 => hex_encode(&sha1(data)),
            Self::Sha256 => hex_encode(&sha256(data)),
            Self::Sha512 => hex_encode(&sha512(data)),
        }
    }
}

/// Detect algorithm from the program name in argv[0].
fn detect_algorithm(argv0: &str) -> Algorithm {
    // Extract just the filename, stripping directory separators and extension.
    let name = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();

    if lower.contains("md5") {
        Algorithm::Md5
    } else if lower.contains("sha512") || lower.contains("sha-512") {
        Algorithm::Sha512
    } else if lower.contains("sha1") || lower.contains("sha-1") {
        Algorithm::Sha1
    } else {
        // Default: SHA-256 (the binary is named sha256sum)
        Algorithm::Sha256
    }
}

// ---------------------------------------------------------------------------
// Hex encoding
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0xf) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Options {
    algo: Algorithm,
    check: bool,
    binary: bool,
    tag: bool,
    quiet: bool,
    status: bool,
    warn: bool,
    strict: bool,
    files: Vec<String>,
}

fn parse_args(algo: Algorithm) -> Options {
    let mut opts = Options {
        algo,
        check: false,
        binary: false,
        tag: false,
        quiet: false,
        status: false,
        warn: false,
        strict: false,
        files: Vec::new(),
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    let mut past_double_dash = false;

    while i < args.len() {
        let arg = &args[i];

        if past_double_dash || !arg.starts_with('-') || arg == "-" {
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            past_double_dash = true;
            i += 1;
            continue;
        }

        match arg.as_str() {
            "-c" | "--check" => opts.check = true,
            "-b" | "--binary" => opts.binary = true,
            "-t" | "--text" => opts.binary = false,
            "--tag" => opts.tag = true,
            "--quiet" => opts.quiet = true,
            "--status" => opts.status = true,
            "-w" | "--warn" => opts.warn = true,
            "--strict" => opts.strict = true,
            "--help" => {
                print_help(algo);
                process::exit(0);
            }
            "--version" => {
                println!("{} (OurOS) 0.1.0", algo.command());
                process::exit(0);
            }
            other => {
                // Handle combined short flags like -bw
                if other.starts_with('-') && !other.starts_with("--") {
                    let mut valid = true;
                    for ch in other[1..].chars() {
                        match ch {
                            'c' => opts.check = true,
                            'b' => opts.binary = true,
                            't' => opts.binary = false,
                            'w' => opts.warn = true,
                            _ => {
                                eprintln!("{}: invalid option -- '{ch}'", algo.command());
                                eprintln!("Try '{} --help' for more information.", algo.command());
                                valid = false;
                                break;
                            }
                        }
                    }
                    if !valid {
                        process::exit(1);
                    }
                } else {
                    eprintln!("{}: unrecognized option '{other}'", algo.command());
                    eprintln!("Try '{} --help' for more information.", algo.command());
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    opts
}

fn print_help(algo: Algorithm) {
    let cmd = algo.command();
    println!("Usage: {cmd} [OPTION]... [FILE]...");
    println!(
        "Print or check {} ({}-bit) checksums.",
        algo.name(),
        algo.digest_len() * 8
    );
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("  -b, --binary   read in binary mode");
    println!("  -c, --check    read checksums from FILE and check them");
    println!("      --tag      create a BSD-style checksum");
    println!("  -t, --text     read in text mode (default)");
    println!();
    println!("The following five options are useful only when verifying checksums:");
    println!("      --quiet    don't print OK for each successfully verified file");
    println!("      --status   don't output anything, status code shows success");
    println!("  -w, --warn     warn about improperly formatted checksum lines");
    println!("      --strict   exit non-zero for improperly formatted checksum lines");
    println!();
    println!("      --help     display this help and exit");
    println!("      --version  output version information and exit");
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    let argv0 = env::args().next().unwrap_or_else(|| "sha256sum".into());
    let algo = detect_algorithm(&argv0);
    let opts = parse_args(algo);

    let exit_code = if opts.check {
        run_check(&opts)
    } else {
        run_hash(&opts)
    };

    process::exit(exit_code);
}

/// Hash mode: compute and print checksums.
fn run_hash(opts: &Options) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let files = if opts.files.is_empty() {
        vec!["-".to_string()]
    } else {
        opts.files.clone()
    };
    let mut exit_code = 0;

    for path in &files {
        let data = if path == "-" {
            let mut buf = Vec::new();
            if let Err(e) = io::stdin().read_to_end(&mut buf) {
                eprintln!("{}: -: {e}", opts.algo.command());
                exit_code = 1;
                continue;
            }
            buf
        } else {
            match File::open(path) {
                Ok(mut f) => {
                    let mut buf = Vec::new();
                    if let Err(e) = f.read_to_end(&mut buf) {
                        eprintln!("{}: {path}: {e}", opts.algo.command());
                        exit_code = 1;
                        continue;
                    }
                    buf
                }
                Err(e) => {
                    eprintln!("{}: {path}: {e}", opts.algo.command());
                    exit_code = 1;
                    continue;
                }
            }
        };

        let hash = opts.algo.hash_bytes(&data);

        if opts.tag {
            // BSD format: ALGO (filename) = hash
            let _ = writeln!(out, "{} ({path}) = {hash}", opts.algo.name());
        } else {
            // GNU format: hash  filename (or hash *filename for binary)
            let mode_char = if opts.binary { '*' } else { ' ' };
            let _ = writeln!(out, "{hash} {mode_char}{path}");
        }
    }

    exit_code
}

/// Check mode: read checksum file and verify.
fn run_check(opts: &Options) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let files = if opts.files.is_empty() {
        vec!["-".to_string()]
    } else {
        opts.files.clone()
    };

    let mut exit_code = 0;
    let mut total_checked: u64 = 0;
    let mut total_failed: u64 = 0;
    let mut total_bad_lines: u64 = 0;

    let expected_hex_len = opts.algo.digest_len() * 2;

    for checksum_file in &files {
        let reader: Box<dyn BufRead> = if checksum_file == "-" {
            Box::new(io::BufReader::new(io::stdin()))
        } else {
            match File::open(checksum_file) {
                Ok(f) => Box::new(io::BufReader::new(f)),
                Err(e) => {
                    let _ = writeln!(err, "{}: {checksum_file}: {e}", opts.algo.command());
                    exit_code = 1;
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    let _ = writeln!(err, "{}: read error: {e}", opts.algo.command());
                    exit_code = 1;
                    continue;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try parsing BSD format: ALGO (filename) = hash
            // Try parsing GNU format: hash  filename  or  hash *filename
            let parsed = parse_check_line(trimmed, expected_hex_len, opts.algo);
            let (expected_hash, filename) = match parsed {
                Some(pair) => pair,
                None => {
                    total_bad_lines += 1;
                    if opts.warn {
                        let _ = writeln!(
                            err,
                            "{}: {checksum_file}: improperly formatted checksum line",
                            opts.algo.command()
                        );
                    }
                    continue;
                }
            };

            total_checked += 1;

            // Read the file and compute hash
            let actual = if filename == "-" {
                let mut buf = Vec::new();
                match io::stdin().read_to_end(&mut buf) {
                    Ok(_) => opts.algo.hash_bytes(&buf),
                    Err(e) => {
                        let _ = writeln!(err, "{}: {filename}: {e}", opts.algo.command());
                        total_failed += 1;
                        if !opts.status {
                            let _ = writeln!(out, "{filename}: FAILED open or read");
                        }
                        continue;
                    }
                }
            } else {
                match File::open(&filename) {
                    Ok(mut f) => {
                        let mut buf = Vec::new();
                        match f.read_to_end(&mut buf) {
                            Ok(_) => opts.algo.hash_bytes(&buf),
                            Err(e) => {
                                let _ = writeln!(err, "{}: {filename}: {e}", opts.algo.command());
                                total_failed += 1;
                                if !opts.status {
                                    let _ = writeln!(out, "{filename}: FAILED open or read");
                                }
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(err, "{}: {filename}: {e}", opts.algo.command());
                        total_failed += 1;
                        if !opts.status {
                            let _ = writeln!(out, "{filename}: FAILED open or read");
                        }
                        continue;
                    }
                }
            };

            if actual.eq_ignore_ascii_case(&expected_hash) {
                if !opts.quiet && !opts.status {
                    let _ = writeln!(out, "{filename}: OK");
                }
            } else {
                total_failed += 1;
                if !opts.status {
                    let _ = writeln!(out, "{filename}: FAILED");
                }
            }
        }
    }

    if total_failed > 0 {
        if !opts.status {
            let _ = writeln!(
                err,
                "{}: WARNING: {total_failed} computed checksum(s) did NOT match",
                opts.algo.command()
            );
        }
        exit_code = 1;
    }

    if total_bad_lines > 0 && !opts.status {
        let _ = writeln!(
            err,
            "{}: WARNING: {total_bad_lines} line(s) are improperly formatted",
            opts.algo.command()
        );
    }

    if opts.strict && total_bad_lines > 0 {
        exit_code = 1;
    }

    if total_checked == 0 && exit_code == 0 {
        let _ = writeln!(err, "{}: no file was verified", opts.algo.command());
        exit_code = 1;
    }

    exit_code
}

/// Parse a single checksum line in either GNU or BSD format.
///
/// Returns `Some((hex_hash, filename))` on success, `None` on parse failure.
fn parse_check_line(
    line: &str,
    expected_hex_len: usize,
    algo: Algorithm,
) -> Option<(String, String)> {
    // Try BSD format first: "ALGO (filename) = hash"
    if let Some(rest) = line.strip_prefix(algo.name()) {
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix('(')
            && let Some(paren_end) = rest.rfind(')')
        {
            let filename = &rest[..paren_end];
            let after = rest[paren_end + 1..].trim_start();
            if let Some(hash_str) = after.strip_prefix('=') {
                let hash_str = hash_str.trim();
                if hash_str.len() == expected_hex_len
                    && hash_str.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Some((hash_str.to_ascii_lowercase(), filename.to_string()));
                }
            }
        }
    }

    // GNU format: "hash  filename" or "hash *filename"
    // The hash must be exactly expected_hex_len hex characters.
    if line.len() < expected_hex_len + 2 {
        return None;
    }

    let hash_str = &line[..expected_hex_len];
    if !hash_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let separator = &line[expected_hex_len..];
    // Must be two characters: space+space, space+*, or just two spaces
    if separator.len() < 2 {
        return None;
    }
    let sep_bytes = separator.as_bytes();
    if sep_bytes[0] != b' ' {
        return None;
    }
    // Second char can be space (text) or * (binary)
    let filename_start = if sep_bytes[1] == b'*' || sep_bytes[1] == b' ' {
        expected_hex_len + 2
    } else {
        return None;
    };

    let filename = &line[filename_start..];
    if filename.is_empty() {
        return None;
    }

    Some((hash_str.to_ascii_lowercase(), filename.to_string()))
}

// ===========================================================================
// MD5 Implementation (RFC 1321)
// ===========================================================================

/// Compute the MD5 hash of `data`, returning a 16-byte digest.
fn md5(data: &[u8]) -> Vec<u8> {
    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    let padded = md5_pad(data);

    // Per-round shift amounts
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    // Pre-computed constants: floor(2^32 * |sin(i+1)|)
    const K: [u32; 64] = [
        0xd76a_a478,
        0xe8c7_b756,
        0x2420_70db,
        0xc1bd_ceee,
        0xf57c_0faf,
        0x4787_c62a,
        0xa830_4613,
        0xfd46_9501,
        0x6980_98d8,
        0x8b44_f7af,
        0xffff_5bb1,
        0x895c_d7be,
        0x6b90_1122,
        0xfd98_7193,
        0xa679_438e,
        0x49b4_0821,
        0xf61e_2562,
        0xc040_b340,
        0x265e_5a51,
        0xe9b6_c7aa,
        0xd62f_105d,
        0x0244_1453,
        0xd8a1_e681,
        0xe7d3_fbc8,
        0x21e1_cde6,
        0xc337_07d6,
        0xf4d5_0d87,
        0x455a_14ed,
        0xa9e3_e905,
        0xfcef_a3f8,
        0x676f_02d9,
        0x8d2a_4c8a,
        0xfffa_3942,
        0x8771_f681,
        0x6d9d_6122,
        0xfde5_380c,
        0xa4be_ea44,
        0x4bde_cfa9,
        0xf6bb_4b60,
        0xbebf_bc70,
        0x289b_7ec6,
        0xeaa1_27fa,
        0xd4ef_3085,
        0x0488_1d05,
        0xd9d4_d039,
        0xe6db_99e5,
        0x1fa2_7cf8,
        0xc4ac_5665,
        0xf429_2244,
        0x432a_ff97,
        0xab94_23a7,
        0xfc93_a039,
        0x655b_59c3,
        0x8f0c_cc92,
        0xffef_f47d,
        0x8584_5dd1,
        0x6fa8_7e4f,
        0xfe2c_e6e0,
        0xa301_4314,
        0x4e08_11a1,
        0xf753_7e82,
        0xbd3a_f235,
        0x2ad7_d2bb,
        0xeb86_d391,
    ];

    for chunk in padded.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (j, word) in m.iter_mut().enumerate() {
            let base = j * 4;
            *word = u32::from_le_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0..64 {
            let (f, g_idx) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };

            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g_idx]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = Vec::with_capacity(16);
    result.extend_from_slice(&a0.to_le_bytes());
    result.extend_from_slice(&b0.to_le_bytes());
    result.extend_from_slice(&c0.to_le_bytes());
    result.extend_from_slice(&d0.to_le_bytes());
    result
}

/// Pad message for MD5 (bit 1 + zeros + 64-bit LE length).
fn md5_pad(data: &[u8]) -> Vec<u8> {
    let orig_len_bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());
    msg
}

// ===========================================================================
// SHA-1 Implementation (FIPS 180-4)
// ===========================================================================

/// Compute the SHA-1 hash of `data`, returning a 20-byte digest.
fn sha1(data: &[u8]) -> Vec<u8> {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let padded = sha_pad_32(data);

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (j, word) in w[..16].iter_mut().enumerate() {
            let base = j * 4;
            *word = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }

        for j in 16..80 {
            w[j] = (w[j - 3] ^ w[j - 8] ^ w[j - 14] ^ w[j - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
                _ => (b ^ c ^ d, 0xCA62_C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = Vec::with_capacity(20);
    result.extend_from_slice(&h0.to_be_bytes());
    result.extend_from_slice(&h1.to_be_bytes());
    result.extend_from_slice(&h2.to_be_bytes());
    result.extend_from_slice(&h3.to_be_bytes());
    result.extend_from_slice(&h4.to_be_bytes());
    result
}

// ===========================================================================
// SHA-256 Implementation (FIPS 180-4)
// ===========================================================================

/// Compute the SHA-256 hash of `data`, returning a 32-byte digest.
fn sha256(data: &[u8]) -> Vec<u8> {
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let padded = sha_pad_32(data);

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (j, word) in w[..16].iter_mut().enumerate() {
            let base = j * 4;
            *word = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }

        for j in 16..64 {
            let s0 = w[j - 15].rotate_right(7) ^ w[j - 15].rotate_right(18) ^ (w[j - 15] >> 3);
            let s1 = w[j - 2].rotate_right(17) ^ w[j - 2].rotate_right(19) ^ (w[j - 2] >> 10);
            w[j] = w[j - 16]
                .wrapping_add(s0)
                .wrapping_add(w[j - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut gv = h[6];
        let mut hv = h[7];

        for j in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & gv);
            let temp1 = hv
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[j])
                .wrapping_add(w[j]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hv = gv;
            gv = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(gv);
        h[7] = h[7].wrapping_add(hv);
    }

    let mut result = Vec::with_capacity(32);
    for val in &h {
        result.extend_from_slice(&val.to_be_bytes());
    }
    result
}

/// Shared padding for SHA-1 and SHA-256 (bit 1 + zeros + 64-bit BE length).
fn sha_pad_32(data: &[u8]) -> Vec<u8> {
    let orig_len_bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_be_bytes());
    msg
}

// ===========================================================================
// SHA-512 Implementation (FIPS 180-4)
// ===========================================================================

/// Compute the SHA-512 hash of `data`, returning a 64-byte digest.
fn sha512(data: &[u8]) -> Vec<u8> {
    let mut h: [u64; 8] = [
        0x6a09_e667_f3bc_c908,
        0xbb67_ae85_84ca_a73b,
        0x3c6e_f372_fe94_f82b,
        0xa54f_f53a_5f1d_36f1,
        0x510e_527f_ade6_82d1,
        0x9b05_688c_2b3e_6c1f,
        0x1f83_d9ab_fb41_bd6b,
        0x5be0_cd19_137e_2179,
    ];

    const K: [u64; 80] = [
        0x428a_2f98_d728_ae22,
        0x7137_4491_23ef_65cd,
        0xb5c0_fbcf_ec4d_3b2f,
        0xe9b5_dba5_8189_dbbc,
        0x3956_c25b_f348_b538,
        0x59f1_11f1_b605_d019,
        0x923f_82a4_af19_4f9b,
        0xab1c_5ed5_da6d_8118,
        0xd807_aa98_a303_0242,
        0x1283_5b01_4570_6fbe,
        0x2431_85be_4ee4_b28c,
        0x550c_7dc3_d5ff_b4e2,
        0x72be_5d74_f27b_896f,
        0x80de_b1fe_3b16_96b1,
        0x9bdc_06a7_25c7_1235,
        0xc19b_f174_cf69_2694,
        0xe49b_69c1_9ef1_4ad2,
        0xefbe_4786_384f_25e3,
        0x0fc1_9dc6_8b8c_d5b5,
        0x240c_a1cc_77ac_9c65,
        0x2de9_2c6f_592b_0275,
        0x4a74_84aa_6ea6_e483,
        0x5cb0_a9dc_bd41_fbd4,
        0x76f9_88da_8311_53b5,
        0x983e_5152_ee66_dfab,
        0xa831_c66d_2db4_3210,
        0xb003_27c8_98fb_213f,
        0xbf59_7fc7_beef_0ee4,
        0xc6e0_0bf3_3da8_8fc2,
        0xd5a7_9147_930a_a725,
        0x06ca_6351_e003_826f,
        0x1429_2967_0a0e_6e70,
        0x27b7_0a85_46d2_2ffc,
        0x2e1b_2138_5c26_c926,
        0x4d2c_6dfc_5ac4_2aed,
        0x5338_0d13_9d95_b3df,
        0x650a_7354_8baf_63de,
        0x766a_0abb_3c77_b2a8,
        0x81c2_c92e_47ed_aee6,
        0x9272_2c85_1482_353b,
        0xa2bf_e8a1_4cf1_0364,
        0xa81a_664b_bc42_3001,
        0xc24b_8b70_d0f8_9791,
        0xc76c_51a3_0654_be30,
        0xd192_e819_d6ef_5218,
        0xd699_0624_5565_a910,
        0xf40e_3585_5771_202a,
        0x106a_a070_32bb_d1b8,
        0x19a4_c116_b8d2_d0c8,
        0x1e37_6c08_5141_ab53,
        0x2748_774c_df8e_eb99,
        0x34b0_bcb5_e19b_48a8,
        0x391c_0cb3_c5c9_5a63,
        0x4ed8_aa4a_e341_8acb,
        0x5b9c_ca4f_7763_e373,
        0x682e_6ff3_d6b2_b8a3,
        0x748f_82ee_5def_b2fc,
        0x78a5_636f_4317_2f60,
        0x84c8_7814_a1f0_ab72,
        0x8cc7_0208_1a64_39ec,
        0x90be_fffa_2363_1e28,
        0xa450_6ceb_de82_bde9,
        0xbef9_a3f7_b2c6_7915,
        0xc671_78f2_e372_532b,
        0xca27_3ece_ea26_619c,
        0xd186_b8c7_21c0_c207,
        0xeada_7dd6_cde0_eb1e,
        0xf57d_4f7f_ee6e_d178,
        0x06f0_67aa_7217_6fba,
        0x0a63_7dc5_a2c8_98a6,
        0x113f_9804_bef9_0dae,
        0x1b71_0b35_131c_471b,
        0x28db_77f5_2304_7d84,
        0x32ca_ab7b_40c7_2493,
        0x3c9e_be0a_15c9_bebc,
        0x431d_67c4_9c10_0d4c,
        0x4cc5_d4be_cb3e_42b6,
        0x597f_299c_fc65_7e2a,
        0x5fcb_6fab_3ad6_faec,
        0x6c44_198c_4a47_5817,
    ];

    let padded = sha_pad_64(data);

    for chunk in padded.chunks_exact(128) {
        let mut w = [0u64; 80];
        for (j, word) in w[..16].iter_mut().enumerate() {
            let base = j * 8;
            *word = u64::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
                chunk[base + 4],
                chunk[base + 5],
                chunk[base + 6],
                chunk[base + 7],
            ]);
        }

        for j in 16..80 {
            let s0 = w[j - 15].rotate_right(1) ^ w[j - 15].rotate_right(8) ^ (w[j - 15] >> 7);
            let s1 = w[j - 2].rotate_right(19) ^ w[j - 2].rotate_right(61) ^ (w[j - 2] >> 6);
            w[j] = w[j - 16]
                .wrapping_add(s0)
                .wrapping_add(w[j - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut gv = h[6];
        let mut hv = h[7];

        for j in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & gv);
            let temp1 = hv
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[j])
                .wrapping_add(w[j]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hv = gv;
            gv = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(gv);
        h[7] = h[7].wrapping_add(hv);
    }

    let mut result = Vec::with_capacity(64);
    for val in &h {
        result.extend_from_slice(&val.to_be_bytes());
    }
    result
}

/// Padding for SHA-512 (bit 1 + zeros + 128-bit BE length).
/// For simplicity and correctness with data < 2^64 bytes, the high 64 bits
/// of the length are always zero.
fn sha_pad_64(data: &[u8]) -> Vec<u8> {
    let orig_len_bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    // SHA-512 uses 128-byte blocks; length field is 16 bytes at end
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    // 128-bit length: high 64 bits are 0 for messages < 2^64 bytes
    msg.extend_from_slice(&0u64.to_be_bytes());
    msg.extend_from_slice(&orig_len_bits.to_be_bytes());
    msg
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helper
    // -----------------------------------------------------------------------

    fn hash_hex(algo: Algorithm, input: &[u8]) -> String {
        algo.hash_bytes(input)
    }

    // -----------------------------------------------------------------------
    // MD5 test vectors (RFC 1321)
    // -----------------------------------------------------------------------

    #[test]
    fn md5_empty() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b""),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }

    #[test]
    fn md5_a() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b"a"),
            "0cc175b9c0f1b6a831c399e269772661"
        );
    }

    #[test]
    fn md5_abc() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn md5_message_digest() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
    }

    #[test]
    fn md5_alpha_lower() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn md5_alphanumeric() {
        assert_eq!(
            hash_hex(
                Algorithm::Md5,
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            ),
            "d174ab98d277d9f5a5611c2c9f419d9f"
        );
    }

    #[test]
    fn md5_numeric() {
        assert_eq!(
            hash_hex(
                Algorithm::Md5,
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    // -----------------------------------------------------------------------
    // SHA-1 test vectors (FIPS 180-4)
    // -----------------------------------------------------------------------

    #[test]
    fn sha1_empty() {
        assert_eq!(
            hash_hex(Algorithm::Sha1, b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn sha1_abc() {
        assert_eq!(
            hash_hex(Algorithm::Sha1, b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn sha1_long() {
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        assert_eq!(
            hash_hex(
                Algorithm::Sha1,
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            ),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn sha1_a_single() {
        assert_eq!(
            hash_hex(Algorithm::Sha1, b"a"),
            "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"
        );
    }

    #[test]
    fn sha1_two_block() {
        // "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
        assert_eq!(
            hash_hex(
                Algorithm::Sha1,
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "a49b2446a02c645bf419f995b67091253a04a259"
        );
    }

    #[test]
    fn sha1_million_a() {
        // 1,000,000 repetitions of 'a'
        let data = vec![b'a'; 1_000_000];
        assert_eq!(
            hash_hex(Algorithm::Sha1, &data),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    // -----------------------------------------------------------------------
    // SHA-256 test vectors (FIPS 180-4)
    // -----------------------------------------------------------------------

    #[test]
    fn sha256_empty() {
        assert_eq!(
            hash_hex(Algorithm::Sha256, b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            hash_hex(Algorithm::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_long() {
        assert_eq!(
            hash_hex(
                Algorithm::Sha256,
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            ),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_two_block() {
        assert_eq!(
            hash_hex(
                Algorithm::Sha256,
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    #[test]
    fn sha256_million_a() {
        let data = vec![b'a'; 1_000_000];
        assert_eq!(
            hash_hex(Algorithm::Sha256, &data),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_a_single() {
        assert_eq!(
            hash_hex(Algorithm::Sha256, b"a"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
    }

    // -----------------------------------------------------------------------
    // SHA-512 test vectors (FIPS 180-4)
    // -----------------------------------------------------------------------

    #[test]
    fn sha512_empty() {
        assert_eq!(
            hash_hex(Algorithm::Sha512, b""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn sha512_abc() {
        assert_eq!(
            hash_hex(Algorithm::Sha512, b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn sha512_long() {
        assert_eq!(
            hash_hex(
                Algorithm::Sha512,
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018\
             501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
    }

    #[test]
    fn sha512_a_single() {
        assert_eq!(
            hash_hex(Algorithm::Sha512, b"a"),
            "1f40fc92da241694750979ee6cf582f2d5d7d28e18335de05abc54d0560e0f53\
             02860c652bf08d560252aa5e74210546f369fbbbce8c12cfc7957b2652fe9a75"
        );
    }

    #[test]
    fn sha512_million_a() {
        let data = vec![b'a'; 1_000_000];
        assert_eq!(
            hash_hex(Algorithm::Sha512, &data),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
             de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    #[test]
    fn sha512_two_blocks_short() {
        assert_eq!(
            hash_hex(
                Algorithm::Sha512,
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            ),
            "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c335\
             96fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445"
        );
    }

    // -----------------------------------------------------------------------
    // Algorithm detection from argv[0]
    // -----------------------------------------------------------------------

    #[test]
    fn detect_md5sum() {
        assert_eq!(detect_algorithm("md5sum"), Algorithm::Md5);
        assert_eq!(detect_algorithm("/usr/bin/md5sum"), Algorithm::Md5);
        assert_eq!(detect_algorithm("C:\\bin\\md5sum.exe"), Algorithm::Md5);
    }

    #[test]
    fn detect_sha1sum() {
        assert_eq!(detect_algorithm("sha1sum"), Algorithm::Sha1);
        assert_eq!(detect_algorithm("/usr/bin/sha1sum"), Algorithm::Sha1);
    }

    #[test]
    fn detect_sha256sum() {
        assert_eq!(detect_algorithm("sha256sum"), Algorithm::Sha256);
        assert_eq!(
            detect_algorithm("/usr/local/bin/sha256sum"),
            Algorithm::Sha256
        );
    }

    #[test]
    fn detect_sha512sum() {
        assert_eq!(detect_algorithm("sha512sum"), Algorithm::Sha512);
        assert_eq!(detect_algorithm("sha512sum.exe"), Algorithm::Sha512);
    }

    #[test]
    fn detect_default_sha256() {
        // Unknown name defaults to SHA-256
        assert_eq!(detect_algorithm("hashutil"), Algorithm::Sha256);
        assert_eq!(detect_algorithm("checksum"), Algorithm::Sha256);
    }

    // -----------------------------------------------------------------------
    // Check-line parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_gnu_line_sha256() {
        let line = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  myfile.txt";
        let result = parse_check_line(line, 64, Algorithm::Sha256);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(name, "myfile.txt");
    }

    #[test]
    fn parse_gnu_line_binary() {
        let line = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad *binary.bin";
        let result = parse_check_line(line, 64, Algorithm::Sha256);
        assert!(result.is_some());
        let (_, name) = result.unwrap();
        assert_eq!(name, "binary.bin");
    }

    #[test]
    fn parse_bsd_line_sha256() {
        let line = "SHA256 (myfile.txt) = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let result = parse_check_line(line, 64, Algorithm::Sha256);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(name, "myfile.txt");
    }

    #[test]
    fn parse_bsd_line_md5() {
        let line = "MD5 (test.txt) = d41d8cd98f00b204e9800998ecf8427e";
        let result = parse_check_line(line, 32, Algorithm::Md5);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(name, "test.txt");
    }

    #[test]
    fn parse_gnu_line_md5() {
        let line = "d41d8cd98f00b204e9800998ecf8427e  empty.txt";
        let result = parse_check_line(line, 32, Algorithm::Md5);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(name, "empty.txt");
    }

    #[test]
    fn parse_invalid_line() {
        assert!(parse_check_line("not a valid checksum line", 64, Algorithm::Sha256).is_none());
    }

    #[test]
    fn parse_too_short() {
        assert!(parse_check_line("abcd  file", 64, Algorithm::Sha256).is_none());
    }

    #[test]
    fn parse_bad_hex() {
        let line = "zz7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  myfile.txt";
        assert!(parse_check_line(line, 64, Algorithm::Sha256).is_none());
    }

    #[test]
    fn parse_missing_separator() {
        let line = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015admyfile.txt";
        assert!(parse_check_line(line, 64, Algorithm::Sha256).is_none());
    }

    #[test]
    fn parse_sha1_gnu() {
        let line = "da39a3ee5e6b4b0d3255bfef95601890afd80709  empty";
        let result = parse_check_line(line, 40, Algorithm::Sha1);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(name, "empty");
    }

    #[test]
    fn parse_sha512_gnu() {
        let hash_str = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
                         47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
        let line = format!("{hash_str}  empty_file");
        let result = parse_check_line(&line, 128, Algorithm::Sha512);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(hash, hash_str);
        assert_eq!(name, "empty_file");
    }

    #[test]
    fn parse_bsd_sha512() {
        let hash_str = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
                         47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
        let line = format!("SHA512 (empty_file) = {hash_str}");
        let result = parse_check_line(&line, 128, Algorithm::Sha512);
        assert!(result.is_some());
        let (hash, name) = result.unwrap();
        assert_eq!(hash, hash_str);
        assert_eq!(name, "empty_file");
    }

    #[test]
    fn parse_case_insensitive_hash() {
        let line = "D41D8CD98F00B204E9800998ECF8427E  file.txt";
        let result = parse_check_line(line, 32, Algorithm::Md5);
        assert!(result.is_some());
        let (hash, _) = result.unwrap();
        // parse_check_line lowercases the hash
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn parse_filename_with_spaces() {
        let line = "d41d8cd98f00b204e9800998ecf8427e  my file with spaces.txt";
        let result = parse_check_line(line, 32, Algorithm::Md5);
        assert!(result.is_some());
        let (_, name) = result.unwrap();
        assert_eq!(name, "my file with spaces.txt");
    }

    #[test]
    fn parse_bsd_filename_with_parens() {
        let line = "SHA256 (file (copy).txt) = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let result = parse_check_line(line, 64, Algorithm::Sha256);
        assert!(result.is_some());
        let (_, name) = result.unwrap();
        assert_eq!(name, "file (copy).txt");
    }

    // -----------------------------------------------------------------------
    // Hex encoding
    // -----------------------------------------------------------------------

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn hex_encode_bytes() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x0a, 0xb3]), "00ff0ab3");
    }

    // -----------------------------------------------------------------------
    // Algorithm properties
    // -----------------------------------------------------------------------

    #[test]
    fn algo_digest_len() {
        assert_eq!(Algorithm::Md5.digest_len(), 16);
        assert_eq!(Algorithm::Sha1.digest_len(), 20);
        assert_eq!(Algorithm::Sha256.digest_len(), 32);
        assert_eq!(Algorithm::Sha512.digest_len(), 64);
    }

    #[test]
    fn algo_names() {
        assert_eq!(Algorithm::Md5.name(), "MD5");
        assert_eq!(Algorithm::Sha1.name(), "SHA1");
        assert_eq!(Algorithm::Sha256.name(), "SHA256");
        assert_eq!(Algorithm::Sha512.name(), "SHA512");
    }

    #[test]
    fn algo_commands() {
        assert_eq!(Algorithm::Md5.command(), "md5sum");
        assert_eq!(Algorithm::Sha1.command(), "sha1sum");
        assert_eq!(Algorithm::Sha256.command(), "sha256sum");
        assert_eq!(Algorithm::Sha512.command(), "sha512sum");
    }

    // -----------------------------------------------------------------------
    // Edge-case hash inputs
    // -----------------------------------------------------------------------

    #[test]
    fn md5_55_bytes() {
        // Exactly fills one block minus the length field
        let data = vec![0x41u8; 55];
        let h = hash_hex(Algorithm::Md5, &data);
        assert_eq!(h, "e38a93ffe074a99b3fed47dfbe37db21");
    }

    #[test]
    fn md5_56_bytes() {
        // Boundary: padding forces a second block
        let data = vec![0x41u8; 56];
        let h = hash_hex(Algorithm::Md5, &data);
        assert_eq!(h, "a2f3e2024931bd470555002aa5ccc010");
    }

    #[test]
    fn md5_64_bytes() {
        // Exactly one block before padding
        let data = vec![0x41u8; 64];
        let h = hash_hex(Algorithm::Md5, &data);
        assert_eq!(h, "d289a97565bc2d27ac8b8545a5ddba45");
    }

    #[test]
    fn sha256_55_bytes() {
        let data = vec![0x42u8; 55];
        let h = hash_hex(Algorithm::Sha256, &data);
        assert_eq!(
            h,
            "1eed5900533b34bb08a62c072a0b0a67058b181b53a8f6e14d3d88d1d78fbe2b"
        );
    }

    #[test]
    fn sha512_111_bytes() {
        // SHA-512 block boundary: 128 bytes minus 16 (length) minus 1 (0x80) = 111
        let data = vec![0x43u8; 111];
        let h = hash_hex(Algorithm::Sha512, &data);
        assert_eq!(
            h,
            "27d750da57a99ce7c6d04ca713e5cd960d77574e5e073d9df960afadc27d5b61\
             13623d9f81179b6f313523fa02c2dfe2a25c363a8f209aa4c41b4fa7e0260901"
        );
    }

    #[test]
    fn sha1_hello_world() {
        assert_eq!(
            hash_hex(Algorithm::Sha1, b"Hello, World!"),
            "0a0a9f2a6772942557ab5355d76af442f8f65e01"
        );
    }

    #[test]
    fn sha256_hello_world() {
        assert_eq!(
            hash_hex(Algorithm::Sha256, b"Hello, World!"),
            "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f"
        );
    }

    #[test]
    fn md5_hello_world() {
        assert_eq!(
            hash_hex(Algorithm::Md5, b"Hello, World!"),
            "65a8e27d8879283831b664bd8b7f0ad4"
        );
    }

    #[test]
    fn sha512_hello_world() {
        assert_eq!(
            hash_hex(Algorithm::Sha512, b"Hello, World!"),
            "374d794a95cdcfd8b35993185fef9ba368f160d8daf432d08ba9f1ed1e5abe6c\
             c69291e0fa2fe0006a52570ef18c19def4e617c33ce52ef0a6e5fbe318cb0387"
        );
    }
}
