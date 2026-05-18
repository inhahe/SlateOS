//! OurOS base64/base32/uuencode/uudecode — encoding/decoding tools
//!
//! Multi-personality binary detected via argv[0]:
//! - `base64`: RFC 4648 Base64 encode/decode
//! - `base32`: RFC 4648 Base32 encode/decode
//! - `uuencode`: Traditional uuencoding
//! - `uudecode`: Traditional uudecoding

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

// ── Personality detection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Base64,
    Base32,
    Uuencode,
    Uudecode,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "base32" => Mode::Base32,
        "uuencode" => Mode::Uuencode,
        "uudecode" => Mode::Uudecode,
        _ => Mode::Base64,
    }
}

// ── Base64 encoding/decoding (RFC 4648) ────────────────────────────

const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const B64_URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(data: &[u8], alphabet: &[u8; 64], wrap: usize, pad: bool) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4 + data.len() / 57 + 2);
    let mut col = 0usize;

    let mut i = 0;
    while i + 2 < data.len() {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        let b2 = data[i + 2] as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(alphabet[((triple >> 18) & 0x3F) as usize] as char);
        out.push(alphabet[((triple >> 12) & 0x3F) as usize] as char);
        out.push(alphabet[((triple >> 6) & 0x3F) as usize] as char);
        out.push(alphabet[(triple & 0x3F) as usize] as char);
        col += 4;
        if wrap > 0 && col >= wrap {
            out.push('\n');
            col = 0;
        }
        i += 3;
    }

    let rem = data.len() - i;
    if rem == 1 {
        let b0 = data[i] as u32;
        out.push(alphabet[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(alphabet[((b0 << 4) & 0x3F) as usize] as char);
        if pad {
            out.push('=');
            out.push('=');
        }
    } else if rem == 2 {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        out.push(alphabet[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(alphabet[(((b0 << 4) | (b1 >> 4)) & 0x3F) as usize] as char);
        out.push(alphabet[((b1 << 2) & 0x3F) as usize] as char);
        if pad {
            out.push('=');
        }
    }

    if wrap > 0 && col > 0 {
        out.push('\n');
    }
    out
}

fn b64_decode_table(alphabet: &[u8; 64]) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    for (i, &ch) in alphabet.iter().enumerate() {
        table[ch as usize] = i as u8;
    }
    table
}

fn b64_decode(input: &str, alphabet: &[u8; 64]) -> Result<Vec<u8>, String> {
    let table = b64_decode_table(alphabet);
    let mut buf = Vec::with_capacity(input.len() * 3 / 4);
    let mut accum: u32 = 0;
    let mut bits: u32 = 0;
    let mut pad_count = 0u32;

    for (pos, ch) in input.chars().enumerate() {
        if ch == '\n' || ch == '\r' || ch == ' ' || ch == '\t' {
            continue;
        }
        if ch == '=' {
            pad_count += 1;
            continue;
        }
        if pad_count > 0 {
            return Err(format!("invalid character after padding at position {pos}"));
        }
        let val = if (ch as u32) < 256 { table[ch as usize] } else { 0xFF };
        if val == 0xFF {
            return Err(format!("invalid character '{}' at position {pos}", ch));
        }
        accum = (accum << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((accum >> bits) as u8);
            accum &= (1 << bits) - 1;
        }
    }
    Ok(buf)
}

// ── Base32 encoding/decoding (RFC 4648) ────────────────────────────

const B32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const B32_HEX_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

fn b32_encode(data: &[u8], alphabet: &[u8; 32], wrap: usize, pad: bool) -> String {
    let mut out = String::with_capacity((data.len() + 4) / 5 * 8 + data.len() / 40 + 2);
    let mut col = 0usize;

    // Process 5 bytes at a time -> 8 base32 chars
    let mut i = 0;
    while i + 4 < data.len() {
        let b0 = data[i] as u64;
        let b1 = data[i + 1] as u64;
        let b2 = data[i + 2] as u64;
        let b3 = data[i + 3] as u64;
        let b4 = data[i + 4] as u64;
        let quint = (b0 << 32) | (b1 << 24) | (b2 << 16) | (b3 << 8) | b4;
        out.push(alphabet[((quint >> 35) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 30) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 25) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 20) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 15) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 10) & 0x1F) as usize] as char);
        out.push(alphabet[((quint >> 5) & 0x1F) as usize] as char);
        out.push(alphabet[(quint & 0x1F) as usize] as char);
        col += 8;
        if wrap > 0 && col >= wrap {
            out.push('\n');
            col = 0;
        }
        i += 5;
    }

    let rem = data.len() - i;
    if rem > 0 {
        // Pad remaining bytes to 5 with zeros
        let mut block = [0u8; 5];
        for j in 0..rem {
            block[j] = data[i + j];
        }
        let b0 = block[0] as u64;
        let b1 = block[1] as u64;
        let b2 = block[2] as u64;
        let b3 = block[3] as u64;
        let b4 = block[4] as u64;
        let quint = (b0 << 32) | (b1 << 24) | (b2 << 16) | (b3 << 8) | b4;

        // Number of base32 chars to output: ceil(rem*8/5)
        let chars_out = match rem {
            1 => 2, // 8 bits -> 2 chars
            2 => 4, // 16 bits -> 4 chars
            3 => 5, // 24 bits -> 5 chars
            4 => 7, // 32 bits -> 7 chars
            _ => 0,
        };
        let pad_chars = 8 - chars_out;

        for j in 0..chars_out {
            let shift = 35 - j * 5;
            out.push(alphabet[((quint >> shift) & 0x1F) as usize] as char);
        }
        if pad {
            for _ in 0..pad_chars {
                out.push('=');
            }
        }
    }

    if wrap > 0 && col > 0 {
        out.push('\n');
    }
    out
}

fn b32_decode_table(alphabet: &[u8; 32]) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    for (i, &ch) in alphabet.iter().enumerate() {
        table[ch as usize] = i as u8;
        // Also accept lowercase for standard base32
        if ch.is_ascii_uppercase() {
            table[(ch + 32) as usize] = i as u8;
        }
    }
    table
}

fn b32_decode(input: &str, alphabet: &[u8; 32]) -> Result<Vec<u8>, String> {
    let table = b32_decode_table(alphabet);
    let mut buf = Vec::with_capacity(input.len() * 5 / 8);
    let mut accum: u64 = 0;
    let mut bits: u32 = 0;

    for (pos, ch) in input.chars().enumerate() {
        if ch == '\n' || ch == '\r' || ch == ' ' || ch == '\t' || ch == '=' {
            continue;
        }
        let val = if (ch as u32) < 256 { table[ch as usize] } else { 0xFF };
        if val == 0xFF {
            return Err(format!("invalid character '{}' at position {pos}", ch));
        }
        accum = (accum << 5) | val as u64;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            buf.push((accum >> bits) as u8);
            accum &= (1u64 << bits) - 1;
        }
    }
    Ok(buf)
}

// ── UU encoding/decoding ───────────────────────────────────────────

fn uu_encode(data: &[u8], filename: &str, mode: u32) -> String {
    let mut out = String::new();
    out.push_str(&format!("begin {mode:o} {filename}\n"));

    for chunk in data.chunks(45) {
        // Length character
        out.push((chunk.len() as u8 + 32) as char);

        let mut i = 0;
        while i < chunk.len() {
            let b0 = chunk[i];
            let b1 = if i + 1 < chunk.len() { chunk[i + 1] } else { 0 };
            let b2 = if i + 2 < chunk.len() { chunk[i + 2] } else { 0 };

            out.push(((b0 >> 2) + 32) as char);
            out.push((((b0 << 4) & 0x30 | (b1 >> 4)) + 32) as char);
            out.push((((b1 << 2) & 0x3C | (b2 >> 6)) + 32) as char);
            out.push(((b2 & 0x3F) + 32) as char);
            i += 3;
        }
        out.push('\n');
    }

    // End: empty line (space = 0 length) + "end"
    out.push_str("`\n");
    out.push_str("end\n");
    out
}

fn uu_decode(input: &str) -> Result<(Vec<u8>, String, u32), String> {
    let mut lines = input.lines();

    // Find begin line
    let begin_line = loop {
        match lines.next() {
            Some(line) if line.starts_with("begin ") => break line,
            Some(_) => continue,
            None => return Err("no 'begin' line found".to_string()),
        }
    };

    // Parse: begin MODE FILENAME
    let parts: Vec<&str> = begin_line.splitn(3, ' ').collect();
    if parts.len() < 3 {
        return Err("malformed begin line".to_string());
    }
    let mode = u32::from_str_radix(parts[1], 8)
        .map_err(|_| format!("invalid mode: {}", parts[1]))?;
    let filename = parts[2].to_string();

    let mut data = Vec::new();

    for line in lines {
        if line == "end" || line.is_empty() {
            break;
        }
        let bytes = line.as_bytes();
        if bytes.is_empty() {
            continue;
        }

        let length = if bytes[0] == b'`' { 0 } else { (bytes[0] - 32) as usize };
        if length == 0 {
            continue;
        }

        let mut i = 1;
        let mut decoded = 0;
        while decoded < length && i + 3 < bytes.len() {
            let c0 = bytes[i].wrapping_sub(32) & 0x3F;
            let c1 = bytes[i + 1].wrapping_sub(32) & 0x3F;
            let c2 = bytes[i + 2].wrapping_sub(32) & 0x3F;
            let c3 = bytes[i + 3].wrapping_sub(32) & 0x3F;

            if decoded < length {
                data.push((c0 << 2) | (c1 >> 4));
                decoded += 1;
            }
            if decoded < length {
                data.push((c1 << 4) | (c2 >> 2));
                decoded += 1;
            }
            if decoded < length {
                data.push((c2 << 6) | c3);
                decoded += 1;
            }
            i += 4;
        }
    }

    Ok((data, filename, mode))
}

// ── Argument parsing ───────────────────────────────────────────────

struct Args {
    mode: Mode,
    decode: bool,
    wrap: usize,
    ignore_garbage: bool,
    url_safe: bool,    // base64 URL-safe alphabet
    hex: bool,         // base32 hex alphabet
    no_pad: bool,      // omit padding
    input_files: Vec<String>,
    // uuencode specific
    uu_filename: Option<String>,
    uu_mode: u32,
    // uudecode specific
    output_file: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            mode: Mode::Base64,
            decode: false,
            wrap: 76,
            ignore_garbage: false,
            url_safe: false,
            hex: false,
            no_pad: false,
            input_files: Vec::new(),
            uu_filename: None,
            uu_mode: 0o644,
            output_file: None,
        }
    }
}

fn parse_args(mode: Mode) -> Args {
    let argv: Vec<String> = env::args().collect();
    let mut args = Args { mode, ..Args::default() };

    // Set default wrap by mode
    args.wrap = match mode {
        Mode::Base64 => 76,
        Mode::Base32 => 76,
        Mode::Uuencode | Mode::Uudecode => 0,
    };

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage(mode);
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("{} (OurOS) 0.1.0", match mode {
                    Mode::Base64 => "base64",
                    Mode::Base32 => "base32",
                    Mode::Uuencode => "uuencode",
                    Mode::Uudecode => "uudecode",
                });
                process::exit(0);
            }
            "-d" | "--decode" => args.decode = true,
            "-i" | "--ignore-garbage" => args.ignore_garbage = true,
            "-w" | "--wrap" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("{}: option '-w' requires an argument", argv[0]);
                    process::exit(1);
                }
                args.wrap = argv[i].parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("{}: invalid wrap size '{}'", argv[0], argv[i]);
                    process::exit(1);
                });
            }
            _ if arg.starts_with("--wrap=") => {
                let val = &arg["--wrap=".len()..];
                args.wrap = val.parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("{}: invalid wrap size '{val}'", argv[0]);
                    process::exit(1);
                });
            }
            "--url" | "--url-safe" => args.url_safe = true,
            "--hex" => args.hex = true,
            "--no-pad" => args.no_pad = true,
            "-o" | "--output" => {
                i += 1;
                if i >= argv.len() {
                    eprintln!("{}: option '-o' requires an argument", argv[0]);
                    process::exit(1);
                }
                args.output_file = Some(argv[i].clone());
            }
            _ if arg.starts_with("--output=") => {
                args.output_file = Some(arg["--output=".len()..].to_string());
            }
            "-m" if mode == Mode::Uuencode => {
                // Use base64 encoding for uuencode (like historical -m flag)
                args.mode = Mode::Base64;
            }
            "--" => {
                i += 1;
                while i < argv.len() {
                    args.input_files.push(argv[i].clone());
                    i += 1;
                }
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("{}: unknown option '{arg}'", argv[0]);
                process::exit(1);
            }
            _ => {
                args.input_files.push(arg.clone());
            }
        }
        i += 1;
    }

    // For uuencode: last positional arg is the encoded filename
    if mode == Mode::Uuencode && !args.input_files.is_empty() {
        // uuencode [file] name
        // If 2 args: file is input, name is the label
        // If 1 arg: stdin is input, arg is the label
        if args.input_files.len() >= 2 {
            args.uu_filename = Some(args.input_files.pop().unwrap_or_default());
        } else {
            args.uu_filename = Some(args.input_files[0].clone());
            args.input_files.clear(); // Read from stdin
        }
    }

    args
}

fn print_usage(mode: Mode) {
    match mode {
        Mode::Base64 => {
            eprintln!("Usage: base64 [OPTION]... [FILE]");
            eprintln!("Base64 encode or decode FILE, or stdin.");
            eprintln!();
            eprintln!("  -d, --decode       decode data");
            eprintln!("  -i, --ignore-garbage  ignore non-alphabet characters");
            eprintln!("  -w, --wrap=COLS    wrap lines at COLS (0 = no wrap, default 76)");
            eprintln!("  --url-safe         use URL-safe alphabet (- and _ instead of + and /)");
            eprintln!("  --no-pad           omit padding characters");
            eprintln!("  -h, --help         display this help");
        }
        Mode::Base32 => {
            eprintln!("Usage: base32 [OPTION]... [FILE]");
            eprintln!("Base32 encode or decode FILE, or stdin.");
            eprintln!();
            eprintln!("  -d, --decode       decode data");
            eprintln!("  -i, --ignore-garbage  ignore non-alphabet characters");
            eprintln!("  -w, --wrap=COLS    wrap lines at COLS (0 = no wrap, default 76)");
            eprintln!("  --hex              use base32hex alphabet (0-9, A-V)");
            eprintln!("  --no-pad           omit padding characters");
            eprintln!("  -h, --help         display this help");
        }
        Mode::Uuencode => {
            eprintln!("Usage: uuencode [file] name");
            eprintln!("Uuencode FILE (or stdin) with NAME as the encoded filename.");
            eprintln!();
            eprintln!("  -m                 use base64 encoding instead of uuencoding");
            eprintln!("  -h, --help         display this help");
        }
        Mode::Uudecode => {
            eprintln!("Usage: uudecode [OPTION]... [FILE]");
            eprintln!("Uudecode FILE (or stdin).");
            eprintln!();
            eprintln!("  -o FILE            write output to FILE instead of encoded name");
            eprintln!("  -h, --help         display this help");
        }
    }
}

// ── I/O helpers ────────────────────────────────────────────────────

fn read_input(files: &[String]) -> Result<Vec<u8>, String> {
    if files.is_empty() || (files.len() == 1 && files[0] == "-") {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("stdin: {e}"))?;
        Ok(buf)
    } else {
        let mut combined = Vec::new();
        for f in files {
            if f == "-" {
                io::stdin()
                    .read_to_end(&mut combined)
                    .map_err(|e| format!("stdin: {e}"))?;
            } else {
                let data = fs::read(f).map_err(|e| format!("{f}: {e}"))?;
                combined.extend_from_slice(&data);
            }
        }
        Ok(combined)
    }
}

fn write_output(data: &[u8], file: Option<&str>) -> Result<(), String> {
    match file {
        Some(path) => fs::write(path, data).map_err(|e| format!("{path}: {e}")),
        None => io::stdout()
            .write_all(data)
            .map_err(|e| format!("stdout: {e}")),
    }
}

// ── Main entry point ───────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv0 = env::args().next().unwrap_or_else(|| "base64".to_string());
    let mode = detect_mode(&argv0);
    let args = parse_args(mode);

    match args.mode {
        Mode::Base64 => {
            let alphabet = if args.url_safe {
                B64_URL_ALPHABET
            } else {
                B64_ALPHABET
            };

            if args.decode {
                let input = read_input(&args.input_files)?;
                let text = String::from_utf8(input)
                    .map_err(|_| "input is not valid text".to_string())?;

                let cleaned = if args.ignore_garbage {
                    let table = b64_decode_table(alphabet);
                    text.chars()
                        .filter(|&c| c == '=' || c == '\n' || c == '\r'
                            || ((c as u32) < 256 && table[c as usize] != 0xFF))
                        .collect::<String>()
                } else {
                    text
                };

                let decoded = b64_decode(&cleaned, alphabet)?;
                write_output(&decoded, args.output_file.as_deref())?;
            } else {
                let data = read_input(&args.input_files)?;
                let encoded = b64_encode(&data, alphabet, args.wrap, !args.no_pad);
                let mut out = encoded.as_bytes().to_vec();
                // Ensure trailing newline
                if !out.is_empty() && out[out.len() - 1] != b'\n' {
                    out.push(b'\n');
                }
                write_output(&out, args.output_file.as_deref())?;
            }
        }
        Mode::Base32 => {
            let alphabet = if args.hex {
                B32_HEX_ALPHABET
            } else {
                B32_ALPHABET
            };

            if args.decode {
                let input = read_input(&args.input_files)?;
                let text = String::from_utf8(input)
                    .map_err(|_| "input is not valid text".to_string())?;

                let cleaned = if args.ignore_garbage {
                    let table = b32_decode_table(alphabet);
                    text.chars()
                        .filter(|&c| c == '=' || c == '\n' || c == '\r'
                            || ((c as u32) < 256 && table[c as usize] != 0xFF))
                        .collect::<String>()
                } else {
                    text
                };

                let decoded = b32_decode(&cleaned, alphabet)?;
                write_output(&decoded, args.output_file.as_deref())?;
            } else {
                let data = read_input(&args.input_files)?;
                let encoded = b32_encode(&data, alphabet, args.wrap, !args.no_pad);
                let mut out = encoded.as_bytes().to_vec();
                if !out.is_empty() && out[out.len() - 1] != b'\n' {
                    out.push(b'\n');
                }
                write_output(&out, args.output_file.as_deref())?;
            }
        }
        Mode::Uuencode => {
            let data = read_input(&args.input_files)?;
            let filename = args
                .uu_filename
                .as_deref()
                .unwrap_or("/dev/stdout");
            let encoded = uu_encode(&data, filename, args.uu_mode);
            write_output(encoded.as_bytes(), args.output_file.as_deref())?;
        }
        Mode::Uudecode => {
            let input = read_input(&args.input_files)?;
            let text = String::from_utf8(input)
                .map_err(|_| "input is not valid text".to_string())?;
            let (decoded, filename, _mode) = uu_decode(&text)?;

            let out_path = args.output_file.as_deref().unwrap_or(&filename);
            if out_path == "/dev/stdout" || out_path == "-" {
                write_output(&decoded, None)?;
            } else {
                write_output(&decoded, Some(out_path))?;
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let prog = env::args().next().unwrap_or_else(|| "base64".to_string());
        let name = prog
            .rsplit(|c| c == '/' || c == '\\')
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
    fn test_detect_base64() {
        assert_eq!(detect_mode("base64"), Mode::Base64);
        assert_eq!(detect_mode("/usr/bin/base64"), Mode::Base64);
        assert_eq!(detect_mode("base64.exe"), Mode::Base64);
    }

    #[test]
    fn test_detect_base32() {
        assert_eq!(detect_mode("base32"), Mode::Base32);
        assert_eq!(detect_mode("/usr/bin/base32"), Mode::Base32);
        assert_eq!(detect_mode("C:\\bin\\base32.exe"), Mode::Base32);
    }

    #[test]
    fn test_detect_uuencode() {
        assert_eq!(detect_mode("uuencode"), Mode::Uuencode);
        assert_eq!(detect_mode("/bin/uuencode"), Mode::Uuencode);
    }

    #[test]
    fn test_detect_uudecode() {
        assert_eq!(detect_mode("uudecode"), Mode::Uudecode);
        assert_eq!(detect_mode("./uudecode.exe"), Mode::Uudecode);
    }

    #[test]
    fn test_detect_unknown_defaults_base64() {
        assert_eq!(detect_mode("foobar"), Mode::Base64);
        assert_eq!(detect_mode(""), Mode::Base64);
    }

    // ── Base64 encode ──

    #[test]
    fn test_b64_encode_empty() {
        assert_eq!(b64_encode(b"", B64_ALPHABET, 0, true), "");
    }

    #[test]
    fn test_b64_encode_one_byte() {
        assert_eq!(b64_encode(b"M", B64_ALPHABET, 0, true), "TQ==");
    }

    #[test]
    fn test_b64_encode_two_bytes() {
        assert_eq!(b64_encode(b"Ma", B64_ALPHABET, 0, true), "TWE=");
    }

    #[test]
    fn test_b64_encode_three_bytes() {
        assert_eq!(b64_encode(b"Man", B64_ALPHABET, 0, true), "TWFu");
    }

    #[test]
    fn test_b64_encode_hello_world() {
        assert_eq!(
            b64_encode(b"Hello, World!", B64_ALPHABET, 0, true),
            "SGVsbG8sIFdvcmxkIQ=="
        );
    }

    #[test]
    fn test_b64_encode_rfc4648_vectors() {
        // RFC 4648 test vectors
        assert_eq!(b64_encode(b"", B64_ALPHABET, 0, true), "");
        assert_eq!(b64_encode(b"f", B64_ALPHABET, 0, true), "Zg==");
        assert_eq!(b64_encode(b"fo", B64_ALPHABET, 0, true), "Zm8=");
        assert_eq!(b64_encode(b"foo", B64_ALPHABET, 0, true), "Zm9v");
        assert_eq!(b64_encode(b"foob", B64_ALPHABET, 0, true), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba", B64_ALPHABET, 0, true), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar", B64_ALPHABET, 0, true), "Zm9vYmFy");
    }

    #[test]
    fn test_b64_encode_no_padding() {
        assert_eq!(b64_encode(b"f", B64_ALPHABET, 0, false), "Zg");
        assert_eq!(b64_encode(b"fo", B64_ALPHABET, 0, false), "Zm8");
    }

    #[test]
    fn test_b64_encode_wrap() {
        let data = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";
        let encoded = b64_encode(data, B64_ALPHABET, 76, true);
        for line in encoded.lines() {
            assert!(line.len() <= 76);
        }
    }

    #[test]
    fn test_b64_url_safe() {
        // Bytes that produce + and / in standard base64
        let data = [0xFB, 0xFF, 0xFE];
        let standard = b64_encode(&data, B64_ALPHABET, 0, true);
        let url_safe = b64_encode(&data, B64_URL_ALPHABET, 0, true);
        assert!(standard.contains('+') || standard.contains('/'));
        assert!(!url_safe.contains('+'));
        assert!(!url_safe.contains('/'));
    }

    // ── Base64 decode ──

    #[test]
    fn test_b64_decode_empty() {
        assert_eq!(b64_decode("", B64_ALPHABET).unwrap(), b"");
    }

    #[test]
    fn test_b64_decode_rfc4648_vectors() {
        assert_eq!(b64_decode("Zg==", B64_ALPHABET).unwrap(), b"f");
        assert_eq!(b64_decode("Zm8=", B64_ALPHABET).unwrap(), b"fo");
        assert_eq!(b64_decode("Zm9v", B64_ALPHABET).unwrap(), b"foo");
        assert_eq!(b64_decode("Zm9vYg==", B64_ALPHABET).unwrap(), b"foob");
        assert_eq!(b64_decode("Zm9vYmE=", B64_ALPHABET).unwrap(), b"fooba");
        assert_eq!(b64_decode("Zm9vYmFy", B64_ALPHABET).unwrap(), b"foobar");
    }

    #[test]
    fn test_b64_decode_with_newlines() {
        assert_eq!(
            b64_decode("SGVs\nbG8s\nIFdv\ncmxk\nIQ==", B64_ALPHABET).unwrap(),
            b"Hello, World!"
        );
    }

    #[test]
    fn test_b64_decode_no_padding() {
        assert_eq!(b64_decode("Zg", B64_ALPHABET).unwrap(), b"f");
        assert_eq!(b64_decode("Zm8", B64_ALPHABET).unwrap(), b"fo");
    }

    #[test]
    fn test_b64_decode_invalid_char() {
        assert!(b64_decode("Zg==$", B64_ALPHABET).is_err());
    }

    #[test]
    fn test_b64_roundtrip() {
        let data = b"The quick brown fox jumps over the lazy dog!";
        let encoded = b64_encode(data, B64_ALPHABET, 0, true);
        let decoded = b64_decode(&encoded, B64_ALPHABET).unwrap();
        assert_eq!(&decoded, data);
    }

    #[test]
    fn test_b64_roundtrip_binary() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = b64_encode(&data, B64_ALPHABET, 0, true);
        let decoded = b64_decode(&encoded, B64_ALPHABET).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b64_url_roundtrip() {
        let data = b"Hello+World/Foo";
        let encoded = b64_encode(data, B64_URL_ALPHABET, 0, true);
        let decoded = b64_decode(&encoded, B64_URL_ALPHABET).unwrap();
        assert_eq!(&decoded, data);
    }

    // ── Base32 encode ──

    #[test]
    fn test_b32_encode_empty() {
        assert_eq!(b32_encode(b"", B32_ALPHABET, 0, true), "");
    }

    #[test]
    fn test_b32_encode_rfc4648_vectors() {
        assert_eq!(b32_encode(b"", B32_ALPHABET, 0, true), "");
        assert_eq!(b32_encode(b"f", B32_ALPHABET, 0, true), "MY======");
        assert_eq!(b32_encode(b"fo", B32_ALPHABET, 0, true), "MZXQ====");
        assert_eq!(b32_encode(b"foo", B32_ALPHABET, 0, true), "MZXW6===");
        assert_eq!(b32_encode(b"foob", B32_ALPHABET, 0, true), "MZXW6YQ=");
        assert_eq!(b32_encode(b"fooba", B32_ALPHABET, 0, true), "MZXW6YTB");
        assert_eq!(b32_encode(b"foobar", B32_ALPHABET, 0, true), "MZXW6YTBOI======");
    }

    #[test]
    fn test_b32_encode_no_padding() {
        assert_eq!(b32_encode(b"f", B32_ALPHABET, 0, false), "MY");
        assert_eq!(b32_encode(b"fo", B32_ALPHABET, 0, false), "MZXQ");
    }

    #[test]
    fn test_b32_hex_encode() {
        // base32hex uses 0-9, A-V
        assert_eq!(b32_encode(b"f", B32_HEX_ALPHABET, 0, true), "CO======");
        assert_eq!(b32_encode(b"foobar", B32_HEX_ALPHABET, 0, true), "CPNMUOJ1E8======");
    }

    // ── Base32 decode ──

    #[test]
    fn test_b32_decode_rfc4648_vectors() {
        assert_eq!(b32_decode("", B32_ALPHABET).unwrap(), b"");
        assert_eq!(b32_decode("MY======", B32_ALPHABET).unwrap(), b"f");
        assert_eq!(b32_decode("MZXQ====", B32_ALPHABET).unwrap(), b"fo");
        assert_eq!(b32_decode("MZXW6===", B32_ALPHABET).unwrap(), b"foo");
        assert_eq!(b32_decode("MZXW6YQ=", B32_ALPHABET).unwrap(), b"foob");
        assert_eq!(b32_decode("MZXW6YTB", B32_ALPHABET).unwrap(), b"fooba");
        assert_eq!(b32_decode("MZXW6YTBOI======", B32_ALPHABET).unwrap(), b"foobar");
    }

    #[test]
    fn test_b32_decode_lowercase() {
        assert_eq!(b32_decode("mzxw6===", B32_ALPHABET).unwrap(), b"foo");
    }

    #[test]
    fn test_b32_decode_invalid() {
        assert!(b32_decode("1234====", B32_ALPHABET).is_err());
    }

    #[test]
    fn test_b32_roundtrip() {
        let data = b"Hello, World!";
        let encoded = b32_encode(data, B32_ALPHABET, 0, true);
        let decoded = b32_decode(&encoded, B32_ALPHABET).unwrap();
        assert_eq!(&decoded, data);
    }

    #[test]
    fn test_b32_roundtrip_binary() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = b32_encode(&data, B32_ALPHABET, 0, true);
        let decoded = b32_decode(&encoded, B32_ALPHABET).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b32_hex_roundtrip() {
        let data = b"test data";
        let encoded = b32_encode(data, B32_HEX_ALPHABET, 0, true);
        let decoded = b32_decode(&encoded, B32_HEX_ALPHABET).unwrap();
        assert_eq!(&decoded, data);
    }

    // ── UU encode/decode ──

    #[test]
    fn test_uu_encode_empty() {
        let encoded = uu_encode(b"", "test.txt", 0o644);
        assert!(encoded.starts_with("begin 644 test.txt\n"));
        assert!(encoded.ends_with("end\n"));
    }

    #[test]
    fn test_uu_encode_short() {
        let encoded = uu_encode(b"Cat", "cat.txt", 0o644);
        assert!(encoded.contains("begin 644 cat.txt"));
        assert!(encoded.contains("end"));
    }

    #[test]
    fn test_uu_roundtrip_short() {
        let data = b"Hello!";
        let encoded = uu_encode(data, "hello.txt", 0o755);
        let (decoded, filename, mode) = uu_decode(&encoded).unwrap();
        assert_eq!(&decoded, data);
        assert_eq!(filename, "hello.txt");
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn test_uu_roundtrip_binary() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = uu_encode(&data, "binary.bin", 0o600);
        let (decoded, _, _) = uu_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_uu_roundtrip_long() {
        // Data longer than 45 bytes (multiple lines)
        let data = b"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.";
        let encoded = uu_encode(data, "long.txt", 0o644);
        let (decoded, _, _) = uu_decode(&encoded).unwrap();
        assert_eq!(&decoded, data);
    }

    #[test]
    fn test_uu_decode_no_begin() {
        assert!(uu_decode("no begin line here\nend\n").is_err());
    }

    #[test]
    fn test_uu_decode_malformed_begin() {
        assert!(uu_decode("begin\n").is_err());
    }

    #[test]
    fn test_uu_mode_parsing() {
        let encoded = uu_encode(b"x", "f", 0o777);
        let (_, _, mode) = uu_decode(&encoded).unwrap();
        assert_eq!(mode, 0o777);
    }

    // ── Edge cases ──

    #[test]
    fn test_b64_all_zeros() {
        let data = vec![0u8; 10];
        let encoded = b64_encode(&data, B64_ALPHABET, 0, true);
        let decoded = b64_decode(&encoded, B64_ALPHABET).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b64_all_ones() {
        let data = vec![0xFFu8; 10];
        let encoded = b64_encode(&data, B64_ALPHABET, 0, true);
        let decoded = b64_decode(&encoded, B64_ALPHABET).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b32_all_zeros() {
        let data = vec![0u8; 10];
        let encoded = b32_encode(&data, B32_ALPHABET, 0, true);
        let decoded = b32_decode(&encoded, B32_ALPHABET).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b64_wrap_zero() {
        let data = b"Hello, World! This is a longer message for testing.";
        let encoded = b64_encode(data, B64_ALPHABET, 0, true);
        assert!(!encoded.contains('\n'));
    }

    #[test]
    fn test_b64_decode_tabs_spaces() {
        let encoded = "Z g = =";
        let decoded = b64_decode(encoded, B64_ALPHABET).unwrap();
        assert_eq!(&decoded, b"f");
    }

    #[test]
    fn test_b64_single_byte_values() {
        for byte in 0..=255u8 {
            let data = [byte];
            let encoded = b64_encode(&data, B64_ALPHABET, 0, true);
            let decoded = b64_decode(&encoded, B64_ALPHABET).unwrap();
            assert_eq!(decoded, data, "roundtrip failed for byte {byte}");
        }
    }

    #[test]
    fn test_b32_single_byte_values() {
        for byte in 0..=255u8 {
            let data = [byte];
            let encoded = b32_encode(&data, B32_ALPHABET, 0, true);
            let decoded = b32_decode(&encoded, B32_ALPHABET).unwrap();
            assert_eq!(decoded, data, "roundtrip failed for byte {byte}");
        }
    }

    #[test]
    fn test_uu_encode_mode_octal() {
        let encoded = uu_encode(b"x", "test", 0o644);
        assert!(encoded.starts_with("begin 644 test\n"));
    }

    #[test]
    fn test_b64_decode_table_coverage() {
        let table = b64_decode_table(B64_ALPHABET);
        // Verify all 64 characters map to their index
        for (i, &ch) in B64_ALPHABET.iter().enumerate() {
            assert_eq!(table[ch as usize], i as u8);
        }
        // Verify non-alphabet characters are 0xFF
        assert_eq!(table[b'!' as usize], 0xFF);
        assert_eq!(table[b'@' as usize], 0xFF);
    }

    #[test]
    fn test_b32_decode_table_coverage() {
        let table = b32_decode_table(B32_ALPHABET);
        for (i, &ch) in B32_ALPHABET.iter().enumerate() {
            assert_eq!(table[ch as usize], i as u8);
        }
    }
}
