//! OurOS File Type Identifier
//!
//! Determines file types by examining file contents (magic numbers and byte
//! patterns) rather than relying on file extensions. Modeled after the Unix
//! `file` command.
//!
//! # Usage
//!
//! ```text
//! file [options] <file...>
//! file -i document.pdf
//! file --json *.bin
//! file -f filelist.txt
//! ```

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

/// Number of bytes to read from the head of each file for identification.
const MAGIC_BUF_SIZE: usize = 8192;

// ============================================================================
// Configuration
// ============================================================================

/// Output format mode.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    /// Human-readable description (default).
    Description,
    /// MIME type string.
    Mime,
    /// JSON object per file.
    Json,
}

/// Parsed command-line options.
struct Options {
    /// Do not print the filename prefix.
    brief: bool,
    /// Output mode.
    mode: OutputMode,
    /// Show all matching types instead of just the first.
    keep_going: bool,
    /// Follow symbolic links.
    dereference: bool,
    /// Attempt to look inside compressed files.
    try_compressed: bool,
    /// Use NUL byte as output line terminator instead of newline.
    nul_terminate: bool,
    /// Files to identify.
    files: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            brief: false,
            mode: OutputMode::Description,
            keep_going: false,
            dereference: false,
            try_compressed: false,
            nul_terminate: false,
            files: Vec::new(),
        }
    }
}

// ============================================================================
// Identified file type
// ============================================================================

/// Result of identifying a file's type.
struct FileType {
    /// Human-readable description (e.g. "PNG image data, 800 x 600").
    description: String,
    /// MIME type (e.g. "image/png").
    mime: String,
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Print usage information and exit.
fn usage() -> ! {
    let msg = "\
Usage: file [options] <file...>

Determine file type by examining contents.

Options:
  -b, --brief           Do not prepend filenames to output
  -i, --mime            Output MIME type instead of description
  -k, --keep-going      Show all matching types, not just the first
  -L, --dereference     Follow symbolic links
  -z                    Try to look inside compressed files
  -f <namefile>         Read filenames from the given file
      --json            Output as JSON
  -0                    NUL-terminate output lines
  -h, --help            Show this help
  --                    End of options";
    eprintln!("{msg}");
    process::exit(0);
}

/// Parse command-line arguments into an `Options` struct.
fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options::default();
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') {
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if let Some(rest) = arg.strip_prefix("--") {
            match rest {
                "help" => usage(),
                "brief" => opts.brief = true,
                "mime" => opts.mode = OutputMode::Mime,
                "keep-going" => opts.keep_going = true,
                "dereference" => opts.dereference = true,
                "json" => opts.mode = OutputMode::Json,
                _ => return Err(format!("unknown option: --{rest}")),
            }
            i += 1;
            continue;
        }

        // Short options (may be grouped, e.g. -bik).
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'h' => usage(),
                'b' => opts.brief = true,
                'i' => opts.mode = OutputMode::Mime,
                'k' => opts.keep_going = true,
                'L' => opts.dereference = true,
                'z' => opts.try_compressed = true,
                '0' => opts.nul_terminate = true,
                'f' => {
                    // -f requires a value: remainder of this group or next arg.
                    let namefile = if j + 1 < chars.len() {
                        chars[j + 1..].iter().collect::<String>()
                    } else if i + 1 < args.len() {
                        i += 1;
                        args[i].clone()
                    } else {
                        return Err("option -f requires a filename".into());
                    };
                    let names = read_namefile(&namefile)?;
                    opts.files.extend(names);
                    // Consumed rest of group or next arg; advance.
                    j = chars.len();
                    continue;
                }
                c => return Err(format!("unknown option: -{c}")),
            }
            j += 1;
        }
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("no files specified (use -h for help)".into());
    }

    Ok(opts)
}

/// Read filenames from a namefile (one filename per line).
fn read_namefile(path: &str) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let reader = BufReader::new(file);
    let mut names = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("{path}: {e}"))?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            names.push(trimmed.to_string());
        }
    }
    Ok(names)
}

// ============================================================================
// Magic-number identification
// ============================================================================

/// Read up to `MAGIC_BUF_SIZE` bytes from the beginning of a file.
fn read_magic_bytes(path: &str) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; MAGIC_BUF_SIZE];
    let mut total = 0;
    loop {
        if total >= buf.len() {
            break;
        }
        let n = file.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    buf.truncate(total);
    Ok(buf)
}

/// Check whether the buffer starts with the given byte sequence.
#[inline]
fn starts_with(buf: &[u8], magic: &[u8]) -> bool {
    buf.len() >= magic.len() && buf[..magic.len()] == *magic
}

/// Check whether `needle` appears at `offset` in `buf`.
#[inline]
fn has_at(buf: &[u8], offset: usize, needle: &[u8]) -> bool {
    if buf.len() < offset + needle.len() {
        return false;
    }
    buf[offset..offset + needle.len()] == *needle
}

/// Read a little-endian u16 from a buffer at the given offset.
fn read_u16_le(buf: &[u8], offset: usize) -> Option<u16> {
    if buf.len() < offset + 2 {
        return None;
    }
    Some(u16::from_le_bytes([buf[offset], buf[offset + 1]]))
}

/// Read a little-endian u32 from a buffer at the given offset.
fn read_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    if buf.len() < offset + 4 {
        return None;
    }
    Some(u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ]))
}

/// Read a big-endian u32 from a buffer at the given offset.
fn read_u32_be(buf: &[u8], offset: usize) -> Option<u32> {
    if buf.len() < offset + 4 {
        return None;
    }
    Some(u32::from_be_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ]))
}

// ============================================================================
// Individual format detectors
// ============================================================================

/// Detect ELF binaries.
fn detect_elf(buf: &[u8]) -> Option<FileType> {
    if !starts_with(buf, b"\x7fELF") {
        return None;
    }
    if buf.len() < 18 {
        return Some(FileType {
            description: "ELF (too short to parse)".into(),
            mime: "application/x-elf".into(),
        });
    }

    let class = match buf.get(4)? {
        1 => "32-bit",
        2 => "64-bit",
        _ => "unknown-class",
    };
    let endian = match buf.get(5)? {
        1 => "LSB",
        2 => "MSB",
        _ => "unknown-endian",
    };
    let etype = read_u16_le(buf, 16).unwrap_or(0);
    let type_str = match etype {
        0 => "no type",
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core file",
        _ => "unknown type",
    };
    let machine = read_u16_le(buf, 18).unwrap_or(0);
    let machine_str = match machine {
        0x03 => "Intel 80386",
        0x3E => "x86-64",
        0x28 => "ARM",
        0xB7 => "AArch64",
        0xF3 => "RISC-V",
        _ => "unknown arch",
    };

    Some(FileType {
        description: format!(
            "ELF {class} {endian} {type_str}, {machine_str}"
        ),
        mime: match etype {
            2 => "application/x-executable".into(),
            3 => "application/x-sharedlib".into(),
            1 => "application/x-object".into(),
            4 => "application/x-coredump".into(),
            _ => "application/x-elf".into(),
        },
    })
}

/// Detect PE/COFF (Windows executables).
fn detect_pe(buf: &[u8]) -> Option<FileType> {
    if !starts_with(buf, b"MZ") {
        return None;
    }
    // Read e_lfanew at offset 0x3C.
    let pe_offset = read_u32_le(buf, 0x3C)? as usize;
    if !has_at(buf, pe_offset, b"PE\0\0") {
        return Some(FileType {
            description: "MS-DOS executable".into(),
            mime: "application/x-dosexec".into(),
        });
    }

    // COFF header starts at pe_offset + 4.
    let coff_base = pe_offset + 4;
    let machine = read_u16_le(buf, coff_base)?;
    let machine_str = match machine {
        0x8664 => "x86-64",
        0x014C => "Intel 80386",
        0xAA64 => "Aarch64",
        _ => "unknown arch",
    };

    // Optional header magic at coff_base + 20.
    let opt_magic = read_u16_le(buf, coff_base + 20).unwrap_or(0);
    let pe_type = match opt_magic {
        0x10B => "PE32",
        0x20B => "PE32+",
        _ => "PE",
    };

    // Check characteristics for DLL.
    let characteristics = read_u16_le(buf, coff_base + 18).unwrap_or(0);
    let kind = if characteristics & 0x2000 != 0 {
        "DLL"
    } else {
        "executable"
    };

    Some(FileType {
        description: format!("{pe_type} {kind}, {machine_str}"),
        // PE files (both DLLs and executables) share the same MIME type.
        mime: "application/x-dosexec".into(),
    })
}

/// Detect shell scripts (shebang lines).
fn detect_shebang(buf: &[u8]) -> Option<FileType> {
    if !starts_with(buf, b"#!") {
        return None;
    }
    // Extract the first line (up to newline or end of buffer, max 256 bytes).
    let limit = buf.len().min(256);
    let first_line_end = buf[..limit]
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(limit);
    let line = &buf[2..first_line_end];

    // Parse interpreter path.
    let line_str = core::str::from_utf8(line).unwrap_or("").trim();
    let interp = if let Some(rest) = line_str.strip_prefix("/usr/bin/env ") {
        rest.split_whitespace().next().unwrap_or("unknown")
    } else {
        // Take just the basename of the interpreter path.
        line_str
            .split_whitespace()
            .next()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or("unknown")
    };

    let mime = match interp {
        "python" | "python3" | "python2" => "text/x-python",
        "ruby" => "text/x-ruby",
        "perl" => "text/x-perl",
        "node" | "nodejs" => "application/javascript",
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ksh" | "csh"
        | "tcsh" => "text/x-shellscript",
        _ => "text/x-script",
    };

    Some(FileType {
        description: format!("{interp} script, ASCII text executable"),
        mime: mime.into(),
    })
}

/// Detect archive and compression formats.
fn detect_archive(buf: &[u8]) -> Option<FileType> {
    // tar: "ustar" at offset 257.
    if has_at(buf, 257, b"ustar") {
        return Some(FileType {
            description: "POSIX tar archive".into(),
            mime: "application/x-tar".into(),
        });
    }
    // zip
    if starts_with(buf, b"PK\x03\x04") {
        // Check for specific zip-based formats.
        if buf.len() >= 30 {
            let name_len = read_u16_le(buf, 26).unwrap_or(0) as usize;
            if buf.len() >= 30 + name_len {
                let name = &buf[30..30 + name_len];
                if name.starts_with(b"META-INF/") {
                    return Some(FileType {
                        description: "Java archive (JAR)".into(),
                        mime: "application/java-archive".into(),
                    });
                }
                if name == b"[Content_Types].xml" || name.starts_with(b"word/")
                {
                    return Some(FileType {
                        description: "Microsoft Office Open XML document"
                            .into(),
                        mime: "application/vnd.openxmlformats-officedocument\
                            .wordprocessingml.document"
                            .into(),
                    });
                }
            }
        }
        return Some(FileType {
            description: "Zip archive data".into(),
            mime: "application/zip".into(),
        });
    }
    // gzip
    if starts_with(buf, b"\x1f\x8b") {
        return Some(FileType {
            description: "gzip compressed data".into(),
            mime: "application/gzip".into(),
        });
    }
    // bzip2
    if starts_with(buf, b"BZ") && buf.len() >= 3 && buf[2] == b'h' {
        return Some(FileType {
            description: "bzip2 compressed data".into(),
            mime: "application/x-bzip2".into(),
        });
    }
    // xz
    if starts_with(buf, b"\xfd7zXZ\x00") {
        return Some(FileType {
            description: "XZ compressed data".into(),
            mime: "application/x-xz".into(),
        });
    }
    // zstd
    if starts_with(buf, b"\x28\xb5\x2f\xfd") {
        return Some(FileType {
            description: "Zstandard compressed data".into(),
            mime: "application/zstd".into(),
        });
    }
    // 7z
    if starts_with(buf, b"7z\xbc\xaf\x27\x1c") {
        return Some(FileType {
            description: "7-zip archive data".into(),
            mime: "application/x-7z-compressed".into(),
        });
    }
    // rar
    if starts_with(buf, b"Rar!") {
        return Some(FileType {
            description: "RAR archive data".into(),
            mime: "application/vnd.rar".into(),
        });
    }

    None
}

/// Detect image formats.
fn detect_image(buf: &[u8]) -> Option<FileType> {
    // PNG
    if starts_with(buf, b"\x89PNG\r\n\x1a\n") {
        let mut desc = String::from("PNG image data");
        // IHDR chunk starts at offset 8 (4-byte length + 4-byte type), data
        // at offset 16: width(4) + height(4).
        if buf.len() >= 24 && has_at(buf, 12, b"IHDR")
            && let (Some(w), Some(h)) =
                (read_u32_be(buf, 16), read_u32_be(buf, 20))
            {
                desc = format!("PNG image data, {w} x {h}");
            }
        return Some(FileType {
            description: desc,
            mime: "image/png".into(),
        });
    }
    // JPEG
    if starts_with(buf, b"\xff\xd8\xff") {
        return Some(FileType {
            description: "JPEG image data".into(),
            mime: "image/jpeg".into(),
        });
    }
    // GIF
    if starts_with(buf, b"GIF87a") || starts_with(buf, b"GIF89a") {
        let mut desc = String::from("GIF image data");
        if buf.len() >= 10
            && let (Some(w), Some(h)) =
                (read_u16_le(buf, 6), read_u16_le(buf, 8))
            {
                desc = format!("GIF image data, {w} x {h}");
            }
        return Some(FileType {
            description: desc,
            mime: "image/gif".into(),
        });
    }
    // BMP
    if starts_with(buf, b"BM") && buf.len() >= 6 {
        return Some(FileType {
            description: "BMP image data".into(),
            mime: "image/bmp".into(),
        });
    }
    // WebP: RIFF....WEBP
    if starts_with(buf, b"RIFF") && has_at(buf, 8, b"WEBP") {
        return Some(FileType {
            description: "WebP image data".into(),
            mime: "image/webp".into(),
        });
    }
    // TIFF (little-endian or big-endian)
    if starts_with(buf, b"II\x2a\x00") {
        return Some(FileType {
            description: "TIFF image data, little-endian".into(),
            mime: "image/tiff".into(),
        });
    }
    if starts_with(buf, b"MM\x00\x2a") {
        return Some(FileType {
            description: "TIFF image data, big-endian".into(),
            mime: "image/tiff".into(),
        });
    }
    // ICO
    if starts_with(buf, b"\x00\x00\x01\x00") && buf.len() >= 6 {
        return Some(FileType {
            description: "MS Windows icon resource".into(),
            mime: "image/vnd.microsoft.icon".into(),
        });
    }

    None
}

/// Detect SVG (may start with `<svg` or `<?xml` containing `<svg`).
///
/// This is checked separately from other images because it requires text
/// scanning and could false-positive if checked too early.
fn detect_svg(buf: &[u8]) -> Option<FileType> {
    // Only examine text-like content (first few KB).
    let limit = buf.len().min(4096);
    let text = core::str::from_utf8(&buf[..limit]).ok()?;
    let lower = text.to_ascii_lowercase();
    if lower.contains("<svg") {
        return Some(FileType {
            description: "SVG Scalable Vector Graphics image".into(),
            mime: "image/svg+xml".into(),
        });
    }
    None
}

/// Detect document formats (PDF, HTML, XML).
fn detect_document(buf: &[u8]) -> Option<FileType> {
    // PDF
    if starts_with(buf, b"%PDF") {
        let mut desc = String::from("PDF document");
        // Try to extract version from "%PDF-X.Y".
        if buf.len() >= 8
            && let Ok(header) = core::str::from_utf8(&buf[..buf.len().min(16)])
                && let Some(ver) = header.strip_prefix("%PDF-") {
                    let ver_end = ver
                        .find(|c: char| !c.is_ascii_digit() && c != '.')
                        .unwrap_or(ver.len());
                    if ver_end > 0 {
                        desc = format!("PDF document, version {}", &ver[..ver_end]);
                    }
                }
        return Some(FileType {
            description: desc,
            mime: "application/pdf".into(),
        });
    }

    // HTML detection (case-insensitive).
    let limit = buf.len().min(1024);
    if let Ok(text) = core::str::from_utf8(&buf[..limit]) {
        let lower = text.trim_start().to_ascii_lowercase();
        if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            return Some(FileType {
                description: "HTML document, ASCII text".into(),
                mime: "text/html; charset=us-ascii".into(),
            });
        }
    }

    // XML (but not SVG or HTML -- SVG is handled separately).
    if starts_with(buf, b"<?xml") {
        return Some(FileType {
            description: "XML document text".into(),
            mime: "application/xml".into(),
        });
    }

    None
}

/// Detect media/audio formats.
fn detect_media(buf: &[u8]) -> Option<FileType> {
    // FLAC
    if starts_with(buf, b"fLaC") {
        return Some(FileType {
            description: "FLAC audio bitstream data".into(),
            mime: "audio/flac".into(),
        });
    }
    // OGG
    if starts_with(buf, b"OggS") {
        return Some(FileType {
            description: "Ogg data".into(),
            mime: "audio/ogg".into(),
        });
    }
    // MP3 with ID3 tag
    if starts_with(buf, b"ID3") {
        return Some(FileType {
            description: "Audio file with ID3 version 2 tag".into(),
            mime: "audio/mpeg".into(),
        });
    }
    // MP3 sync word
    if buf.len() >= 2 && buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 {
        return Some(FileType {
            description: "MPEG ADTS audio data".into(),
            mime: "audio/mpeg".into(),
        });
    }
    // WAV: RIFF....WAVE
    if starts_with(buf, b"RIFF") && has_at(buf, 8, b"WAVE") {
        return Some(FileType {
            description: "RIFF WAVE audio data".into(),
            mime: "audio/x-wav".into(),
        });
    }
    // MIDI
    if starts_with(buf, b"MThd") {
        return Some(FileType {
            description: "Standard MIDI data".into(),
            mime: "audio/midi".into(),
        });
    }
    // MP4/MOV: "ftyp" at offset 4.
    if has_at(buf, 4, b"ftyp") {
        // Read the brand at offset 8 (4 bytes).
        let brand = if buf.len() >= 12 {
            core::str::from_utf8(&buf[8..12]).unwrap_or("")
        } else {
            ""
        };
        let (desc, mime) = match brand {
            "isom" | "iso2" | "mp41" | "mp42" | "avc1" | "dash" => {
                ("ISO Media, MP4 Base Media", "video/mp4")
            }
            "M4A " => ("Apple MPEG-4 audio", "audio/mp4"),
            "M4V " => ("Apple MPEG-4 video", "video/mp4"),
            "qt  " => ("Apple QuickTime movie", "video/quicktime"),
            _ => ("ISO Media, MPEG-4 compatible", "video/mp4"),
        };
        return Some(FileType {
            description: desc.into(),
            mime: mime.into(),
        });
    }

    None
}

/// Detect structured data formats (JSON, YAML, SQLite, TOML).
fn detect_data(buf: &[u8]) -> Option<FileType> {
    // SQLite (binary, check first).
    if starts_with(buf, b"SQLite format 3") {
        return Some(FileType {
            description: "SQLite 3.x database".into(),
            mime: "application/vnd.sqlite3".into(),
        });
    }

    // The remaining data formats are text-based; require valid UTF-8.
    let text = core::str::from_utf8(buf).ok()?;
    let trimmed = text.trim_start();

    // JSON: starts with { or [.
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Quick plausibility check: look for a quote after the opening brace.
        if let Some(after_brace) = trimmed.strip_prefix('{') {
            let rest = after_brace.trim_start();
            if rest.starts_with('"') || rest.starts_with('}') {
                return Some(FileType {
                    description: "JSON text data".into(),
                    mime: "application/json".into(),
                });
            }
        } else {
            // Array: check for a value after [.
            let rest = trimmed[1..].trim_start();
            if rest.starts_with('"')
                || rest.starts_with('{')
                || rest.starts_with('[')
                || rest.starts_with(']')
                || rest.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                || rest.starts_with("true")
                || rest.starts_with("false")
                || rest.starts_with("null")
            {
                return Some(FileType {
                    description: "JSON text data".into(),
                    mime: "application/json".into(),
                });
            }
        }
    }

    // YAML: starts with "---" or "%YAML".
    if trimmed.starts_with("---") || trimmed.starts_with("%YAML") {
        return Some(FileType {
            description: "YAML document text".into(),
            mime: "text/yaml".into(),
        });
    }

    // TOML: heuristic -- look for [section] headers and key = value patterns.
    if detect_toml_heuristic(trimmed) {
        return Some(FileType {
            description: "TOML configuration text".into(),
            mime: "application/toml".into(),
        });
    }

    None
}

/// Simple heuristic to detect TOML files.
///
/// Looks for lines matching `[section]` and `key = value` patterns.
fn detect_toml_heuristic(text: &str) -> bool {
    let mut has_section = false;
    let mut has_kvpair = false;
    let mut lines_checked = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            has_section = true;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            // Keys should be identifier-like.
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                has_kvpair = true;
            }
        }
        lines_checked += 1;
        if lines_checked > 30 {
            break;
        }
    }

    // Require both a section header and at least one key-value pair.
    has_section && has_kvpair
}

/// Detect compiled/binary formats (Java class, Mach-O, WASM).
fn detect_compiled(buf: &[u8]) -> Option<FileType> {
    // Java class file.
    if starts_with(buf, b"\xca\xfe\xba\xbe") {
        // Disambiguate from Mach-O fat binary: Java class has version in
        // bytes 4-7 where minor is typically small. A fat binary has a count
        // of architectures in bytes 4-7 (usually < 20).
        let major = read_u16_le(buf, 6).unwrap_or(0);
        // Java class major versions range roughly 45-67+. Fat binary arch
        // counts are usually 1-5.
        if major >= 44 {
            return Some(FileType {
                description: "compiled Java class data".into(),
                mime: "application/x-java-applet".into(),
            });
        }
        // Likely a Mach-O fat binary.
        return Some(FileType {
            description: "Mach-O universal binary".into(),
            mime: "application/x-mach-binary".into(),
        });
    }
    // Mach-O 32-bit.
    if starts_with(buf, b"\xfe\xed\xfa\xce") {
        return Some(FileType {
            description: "Mach-O 32-bit executable".into(),
            mime: "application/x-mach-binary".into(),
        });
    }
    // Mach-O 64-bit.
    if starts_with(buf, b"\xcf\xfa\xed\xfe") {
        return Some(FileType {
            description: "Mach-O 64-bit executable".into(),
            mime: "application/x-mach-binary".into(),
        });
    }
    // WebAssembly.
    if starts_with(buf, b"\x00asm") {
        return Some(FileType {
            description: "WebAssembly (wasm) binary module".into(),
            mime: "application/wasm".into(),
        });
    }

    None
}

// ============================================================================
// Text heuristics
// ============================================================================

/// Classification of text encoding detected via heuristics.
enum TextKind {
    /// Pure 7-bit ASCII text.
    Ascii,
    /// Valid UTF-8 text with multi-byte sequences.
    Utf8,
    /// Text with bytes > 127 that are not valid UTF-8 (likely ISO-8859).
    Iso8859,
    /// Binary data (contains NUL bytes or other non-text indicators).
    Binary,
}

/// Classify buffer content by scanning byte patterns.
fn classify_text(buf: &[u8]) -> TextKind {
    if buf.is_empty() {
        return TextKind::Ascii;
    }

    let mut has_high_bytes = false;
    let mut i = 0;

    while i < buf.len() {
        let b = buf[i];

        // NUL byte is a strong binary indicator.
        if b == 0 {
            return TextKind::Binary;
        }

        // Control characters that are not typical in text files.
        if b < 0x08
            || (b > 0x0D && b < 0x1B)
            || (b > 0x1B && b < 0x20)
        {
            // Allow BEL(7), BS(8), HT(9), LF(10), VT(11), FF(12), CR(13),
            // ESC(27), and printable range. Everything else is suspicious.
            return TextKind::Binary;
        }

        if b > 127 {
            has_high_bytes = true;
            // Check for valid UTF-8 multi-byte sequences.
            let seq_len = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => return TextKind::Iso8859, // Invalid UTF-8 lead byte.
            };
            if i + seq_len > buf.len() {
                // Incomplete sequence at buffer end -- tolerate, but note the
                // high bytes.
                break;
            }
            // Verify continuation bytes.
            let mut valid = true;
            for j in 1..seq_len {
                if buf[i + j] & 0xC0 != 0x80 {
                    valid = false;
                    break;
                }
            }
            if !valid {
                return TextKind::Iso8859;
            }
            i += seq_len;
            continue;
        }

        i += 1;
    }

    if has_high_bytes {
        TextKind::Utf8
    } else {
        TextKind::Ascii
    }
}

/// Produce a `FileType` from text-heuristic classification.
fn text_type(buf: &[u8]) -> FileType {
    match classify_text(buf) {
        TextKind::Ascii => FileType {
            description: "ASCII text".into(),
            mime: "text/plain; charset=us-ascii".into(),
        },
        TextKind::Utf8 => FileType {
            description: "UTF-8 Unicode text".into(),
            mime: "text/plain; charset=utf-8".into(),
        },
        TextKind::Iso8859 => FileType {
            description: "ISO-8859 text".into(),
            mime: "text/plain; charset=iso-8859-1".into(),
        },
        TextKind::Binary => FileType {
            description: "data".into(),
            mime: "application/octet-stream".into(),
        },
    }
}

// ============================================================================
// Top-level identification
// ============================================================================

/// One magic-number detector's signature: takes the file's leading bytes,
/// returns `Some(FileType)` on match or `None` to skip.
type Detector = fn(&[u8]) -> Option<FileType>;

/// Ordered list of magic-number detectors. Checked in priority order; the
/// first match wins (unless `--keep-going` is set).
const DETECTORS: &[Detector] = &[
    detect_elf,
    detect_pe,
    detect_shebang,
    detect_archive,
    detect_image,
    detect_document,
    detect_svg,
    detect_media,
    detect_data,
    detect_compiled,
];

/// Identify the type of a file given its leading bytes.
///
/// Returns one or more `FileType` results. With `keep_going`, all matching
/// detectors contribute; otherwise only the first match is returned.
fn identify(buf: &[u8], keep_going: bool) -> Vec<FileType> {
    if buf.is_empty() {
        return vec![FileType {
            description: "empty".into(),
            mime: "application/x-empty".into(),
        }];
    }

    let mut results = Vec::new();

    for detector in DETECTORS {
        if let Some(ft) = detector(buf) {
            results.push(ft);
            if !keep_going {
                return results;
            }
        }
    }

    // Fall back to text heuristics if no magic matched (or in keep-going
    // mode, append text classification).
    if results.is_empty() {
        results.push(text_type(buf));
    }

    results
}

/// Identify a special file (directory, symlink, device, etc.) by its metadata.
fn identify_special(path: &str, dereference: bool) -> Option<FileType> {
    let meta = if dereference {
        fs::metadata(path).ok()?
    } else {
        fs::symlink_metadata(path).ok()?
    };

    let ft = meta.file_type();

    if !dereference && ft.is_symlink() {
        // Read the link target for display.
        let target = fs::read_link(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".into());
        return Some(FileType {
            description: format!("symbolic link to {target}"),
            mime: "inode/symlink".into(),
        });
    }
    if ft.is_dir() {
        return Some(FileType {
            description: "directory".into(),
            mime: "inode/directory".into(),
        });
    }

    // On Unix-like systems, check for special file types via the mode bits.
    // For portability, we use cfg attributes.
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if ft.is_block_device() {
            return Some(FileType {
                description: "block special device".into(),
                mime: "inode/blockdevice".into(),
            });
        }
        if ft.is_char_device() {
            return Some(FileType {
                description: "character special device".into(),
                mime: "inode/chardevice".into(),
            });
        }
        if ft.is_fifo() {
            return Some(FileType {
                description: "fifo (named pipe)".into(),
                mime: "inode/fifo".into(),
            });
        }
        if ft.is_socket() {
            return Some(FileType {
                description: "socket".into(),
                mime: "inode/socket".into(),
            });
        }
    }

    None
}

// ============================================================================
// Output formatting
// ============================================================================

/// Write a JSON-escaped string to the output (no surrounding quotes).
fn write_json_escaped(out: &mut impl Write, s: &str) -> io::Result<()> {
    for ch in s.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32)?,
            c => write!(out, "{c}")?,
        }
    }
    Ok(())
}

/// Emit one file's result.
fn emit_result(
    out: &mut impl Write,
    opts: &Options,
    path: &str,
    types: &[FileType],
) -> io::Result<()> {
    let terminator = if opts.nul_terminate { '\0' } else { '\n' };

    match opts.mode {
        OutputMode::Json => {
            write!(out, "{{\"filename\":\"")?;
            write_json_escaped(out, path)?;
            write!(out, "\",")?;
            if types.len() == 1 {
                write!(out, "\"type\":\"")?;
                write_json_escaped(out, &types[0].description)?;
                write!(out, "\",\"mime\":\"")?;
                write_json_escaped(out, &types[0].mime)?;
                write!(out, "\"")?;
            } else {
                write!(out, "\"types\":[")?;
                for (idx, ft) in types.iter().enumerate() {
                    if idx > 0 {
                        write!(out, ",")?;
                    }
                    write!(out, "{{\"type\":\"")?;
                    write_json_escaped(out, &ft.description)?;
                    write!(out, "\",\"mime\":\"")?;
                    write_json_escaped(out, &ft.mime)?;
                    write!(out, "\"}}")?;
                }
                write!(out, "]")?;
            }
            write!(out, "}}{terminator}")?;
        }
        OutputMode::Mime => {
            let mime_str: String = types
                .iter()
                .map(|ft| ft.mime.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            if opts.brief {
                write!(out, "{mime_str}{terminator}")?;
            } else {
                write!(out, "{path}: {mime_str}{terminator}")?;
            }
        }
        OutputMode::Description => {
            let desc_str: String = types
                .iter()
                .map(|ft| ft.description.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if opts.brief {
                write!(out, "{desc_str}{terminator}")?;
            } else {
                write!(out, "{path}: {desc_str}{terminator}")?;
            }
        }
    }
    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

/// Process a single file path and emit its type.
fn process_file(
    out: &mut impl Write,
    opts: &Options,
    path: &str,
) -> io::Result<()> {
    // Check for special files first (directory, symlink, device, etc.).
    if let Some(ft) = identify_special(path, opts.dereference) {
        emit_result(out, opts, path, &[ft])?;
        return Ok(());
    }

    // Read magic bytes.
    match read_magic_bytes(path) {
        Ok(buf) => {
            let types = identify(&buf, opts.keep_going);
            emit_result(out, opts, path, &types)?;
        }
        Err(e) => {
            // Report the error inline, same as real `file` does.
            let terminator = if opts.nul_terminate { '\0' } else { '\n' };
            if opts.brief {
                write!(out, "cannot open: {e}{terminator}")?;
            } else {
                write!(out, "{path}: cannot open: {e}{terminator}")?;
            }
        }
    }

    Ok(())
}

fn run() -> Result<(), String> {
    let opts = parse_args()?;
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for path in &opts.files {
        process_file(&mut out, &opts, path)
            .map_err(|e| format!("{path}: {e}"))?;
    }

    out.flush().map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

fn main() {
    if let Err(msg) = run() {
        eprintln!("file: {msg}");
        process::exit(1);
    }
}
