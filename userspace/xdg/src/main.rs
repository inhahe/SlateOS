//! OurOS XDG File Association Tools
//!
//! Multi-personality binary providing `xdg-open`, `xdg-mime`, and `mimeopen`
//! functionality. The active mode is determined by `argv[0]`:
//!
//! - **xdg-open** (default): open a file or URL with the appropriate handler
//! - **xdg-mime**: query and manage MIME type associations
//! - **mimeopen**: open files with interactive handler selection
//!
//! # MIME detection
//!
//! Two complementary strategies are used:
//! 1. Extension-based lookup from a built-in database of 200+ mappings
//! 2. Magic-byte inspection of the first 512 bytes for 20+ binary signatures
//!
//! # Handler resolution
//!
//! Handlers are resolved by consulting `mimeapps.list` files (user then
//! system), then falling back to environment variables (`$BROWSER`,
//! `$EDITOR`).

use std::collections::HashMap;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Personality detection
// ============================================================================

/// Which tool personality this invocation should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    /// `xdg-open`: open a file/URL with the default handler.
    XdgOpen,
    /// `xdg-mime`: query/set MIME associations.
    XdgMime,
    /// `mimeopen`: open with optional interactive selection.
    MimeOpen,
}

/// Determine the personality from `argv[0]`.
fn detect_personality(argv0: &str) -> Personality {
    let base = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("xdg-open");

    match base {
        "xdg-mime" => Personality::XdgMime,
        "mimeopen" => Personality::MimeOpen,
        _ => Personality::XdgOpen,
    }
}

// ============================================================================
// MIME extension database (200+ entries)
// ============================================================================

/// Return the MIME type for a given lowercase file extension, or `None`.
fn mime_from_extension(ext: &str) -> Option<&'static str> {
    // Organised by category for maintainability.
    Some(match ext {
        // --- Text / source code ---
        "txt" | "text" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "jsonl" | "ndjson" => "application/x-ndjson",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/x-yaml",
        "html" | "htm" => "text/html",
        "xhtml" => "application/xhtml+xml",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "application/javascript",
        "ts" => "application/typescript",
        "tsx" => "text/x-tsx",
        "jsx" => "text/x-jsx",
        "py" | "pyw" => "text/x-python",
        "rs" => "text/x-rust",
        "c" => "text/x-c",
        "cpp" | "cxx" | "cc" | "c++" => "text/x-c++",
        "h" => "text/x-c-header",
        "hpp" | "hxx" | "hh" => "text/x-c++-header",
        "java" => "text/x-java",
        "go" => "text/x-go",
        "rb" => "text/x-ruby",
        "sh" | "bash" => "application/x-shellscript",
        "zsh" => "application/x-zsh",
        "fish" => "application/x-fish",
        "ps1" => "application/x-powershell",
        "toml" => "application/toml",
        "ini" => "text/x-ini",
        "conf" | "cfg" => "text/x-config",
        "log" => "text/x-log",
        "diff" => "text/x-diff",
        "patch" => "text/x-patch",
        "tex" | "latex" => "application/x-tex",
        "bib" => "application/x-bibtex",
        "sql" => "application/sql",
        "graphql" | "gql" => "application/graphql",
        "proto" => "text/x-protobuf",
        "asm" | "s" => "text/x-asm",
        "lisp" | "cl" | "el" => "text/x-lisp",
        "hs" | "lhs" => "text/x-haskell",
        "ml" | "mli" => "text/x-ocaml",
        "scala" => "text/x-scala",
        "kt" | "kts" => "text/x-kotlin",
        "swift" => "text/x-swift",
        "dart" => "text/x-dart",
        "lua" => "text/x-lua",
        "vim" => "text/x-vim",
        "cmake" => "text/x-cmake",
        "makefile" | "mk" => "text/x-makefile",
        "dockerfile" => "text/x-dockerfile",
        "r" => "text/x-r",
        "m" => "text/x-matlab",
        "php" => "application/x-php",
        "pl" | "pm" => "application/x-perl",
        "tcl" => "application/x-tcl",
        "erl" | "hrl" => "text/x-erlang",
        "ex" | "exs" => "text/x-elixir",
        "clj" | "cljs" => "text/x-clojure",
        "cs" => "text/x-csharp",
        "fs" | "fsx" => "text/x-fsharp",
        "vb" => "text/x-vb",
        "d" => "text/x-d",
        "nim" => "text/x-nim",
        "zig" => "text/x-zig",
        "v" => "text/x-vlang",
        "ada" | "adb" | "ads" => "text/x-ada",
        "pas" | "pp" => "text/x-pascal",
        "rst" => "text/x-rst",
        "org" => "text/x-org",
        "textile" => "text/x-textile",
        "asciidoc" | "adoc" => "text/x-asciidoc",

        // --- Images ---
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" | "dib" => "image/bmp",
        "svg" | "svgz" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "tiff" | "tif" => "image/tiff",
        "psd" => "image/vnd.adobe.photoshop",
        "raw" => "image/x-raw",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        "jxl" => "image/jxl",
        "xcf" => "image/x-xcf",
        "pcx" => "image/x-pcx",
        "tga" => "image/x-tga",
        "exr" => "image/x-exr",
        "qoi" => "image/x-qoi",

        // --- Audio ---
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "wma" => "audio/x-ms-wma",
        "opus" => "audio/opus",
        "mid" | "midi" => "audio/midi",
        "aiff" | "aif" => "audio/aiff",
        "ape" => "audio/x-ape",
        "mka" => "audio/x-matroska",
        "wv" => "audio/x-wavpack",
        "ra" | "ram" => "audio/x-realaudio",
        "amr" => "audio/amr",
        "ac3" => "audio/ac3",
        "spx" => "audio/x-speex",

        // --- Video ---
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mov" | "qt" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "webm" => "video/webm",
        "mpeg" | "mpg" | "mpe" => "video/mpeg",
        "3gp" | "3gpp" => "video/3gpp",
        "ogv" => "video/ogg",
        "m2ts" | "mts" => "video/mp2t",
        "vob" => "video/x-ms-vob",
        "asf" => "video/x-ms-asf",
        "rm" | "rmvb" => "video/x-real",

        // --- Documents ---
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "rtf" => "application/rtf",
        "epub" => "application/epub+zip",
        "mobi" => "application/x-mobipocket-ebook",
        "djvu" | "djv" => "image/vnd.djvu",
        "xps" => "application/vnd.ms-xpsdocument",
        "ps" | "eps" => "application/postscript",
        "cbz" => "application/vnd.comicbook+zip",
        "cbr" => "application/vnd.comicbook-rar",

        // --- Archives ---
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" | "gzip" => "application/gzip",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "deb" => "application/vnd.debian.binary-package",
        "rpm" => "application/x-rpm",
        "iso" => "application/x-iso9660-image",
        "zst" | "zstd" => "application/zstd",
        "lz" => "application/x-lzip",
        "lz4" => "application/x-lz4",
        "lzma" => "application/x-lzma",
        "cab" => "application/vnd.ms-cab-compressed",
        "cpio" => "application/x-cpio",
        "ar" | "a" => "application/x-archive",
        "snap" => "application/vnd.snap",
        "flatpak" => "application/vnd.flatpak",
        "appimage" => "application/x-appimage",
        "dmg" => "application/x-apple-diskimage",

        // --- Executables ---
        "exe" => "application/x-dosexec",
        "msi" => "application/x-msi",
        "run" => "application/x-executable",
        "bin" => "application/octet-stream",
        "elf" => "application/x-elf",
        "dll" | "so" => "application/x-sharedlib",
        "o" | "obj" => "application/x-object",
        "wasm" => "application/wasm",
        "class" => "application/java-vm",
        "pyc" | "pyo" => "application/x-python-bytecode",

        // --- Fonts ---
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "eot" => "application/vnd.ms-fontobject",

        // --- Data / databases ---
        "db" | "sqlite" | "sqlite3" => "application/x-sqlite3",
        "parquet" => "application/x-parquet",
        "avro" => "application/avro",
        "arrow" | "ipc" => "application/x-apache-arrow",

        // --- Miscellaneous ---
        "torrent" => "application/x-bittorrent",
        "desktop" => "application/x-desktop",
        "key" => "application/x-pem-key",
        "pem" => "application/x-pem-file",
        "cert" | "crt" | "cer" => "application/x-x509-ca-cert",
        "der" => "application/x-x509-ca-cert",
        "p12" | "pfx" => "application/x-pkcs12",
        "csr" => "application/x-pem-csr",
        "gpg" | "pgp" | "asc" => "application/pgp-encrypted",
        "sig" => "application/pgp-signature",
        "swf" => "application/x-shockwave-flash",
        "jar" => "application/java-archive",
        "war" | "ear" => "application/java-archive",
        "apk" => "application/vnd.android.package-archive",
        "ipa" => "application/x-ios-app",
        "rss" => "application/rss+xml",
        "atom" => "application/atom+xml",
        "ics" | "ical" => "text/calendar",
        "vcf" | "vcard" => "text/vcard",
        "m3u" | "m3u8" => "application/x-mpegurl",
        "pls" => "audio/x-scpls",
        "srt" | "sub" => "text/x-subtitle",
        "ass" | "ssa" => "text/x-ssa",
        "map" => "application/json",
        "wsdl" => "application/wsdl+xml",
        "xsd" => "application/xml",
        "dtd" => "application/xml-dtd",
        "manifest" => "text/cache-manifest",

        _ => return None,
    })
}

// ============================================================================
// Magic byte detection
// ============================================================================

/// Minimum bytes needed for reliable magic detection.
const MAGIC_BUF_LEN: usize = 512;

/// Detect MIME type by inspecting magic bytes at the beginning of a buffer.
///
/// Returns `None` if no known signature matches.
fn mime_from_magic(buf: &[u8]) -> Option<&'static str> {
    if buf.is_empty() {
        return None;
    }

    // Fixed-offset signatures, checked longest-prefix first where ambiguous.
    if buf.starts_with(b"%PDF") {
        return Some("application/pdf");
    }
    if buf.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if buf.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if buf.starts_with(b"GIF87a") || buf.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if buf.len() >= 4 && buf[0] == b'R' && buf[1] == b'I' && buf[2] == b'F' && buf[3] == b'F' {
        // RIFF container: check sub-type at offset 8
        if buf.len() >= 12 {
            let sub = &buf[8..12];
            if sub == b"WAVE" {
                return Some("audio/wav");
            }
            if sub == b"AVI " {
                return Some("video/x-msvideo");
            }
            if sub == b"WEBP" {
                return Some("image/webp");
            }
        }
        return Some("application/x-riff");
    }
    if buf.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if buf.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    // MP3: ID3 tag header or MPEG frame sync
    if buf.starts_with(b"ID3") {
        return Some("audio/mpeg");
    }
    if buf.len() >= 2 && buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 {
        return Some("audio/mpeg");
    }
    if buf.starts_with(b"PK\x03\x04") {
        return Some("application/zip");
    }
    if buf.starts_with(&[0x1F, 0x8B]) {
        return Some("application/gzip");
    }
    if buf.starts_with(b"BZh") {
        return Some("application/x-bzip2");
    }
    if buf.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return Some("application/x-xz");
    }
    if buf.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some("application/x-7z-compressed");
    }
    // RAR: "Rar!\x1A\x07"
    if buf.len() >= 6
        && buf[0] == b'R'
        && buf[1] == b'a'
        && buf[2] == b'r'
        && buf[3] == b'!'
        && buf[4] == 0x1A
        && buf[5] == 0x07
    {
        return Some("application/vnd.rar");
    }
    if buf.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return Some("application/x-elf");
    }
    if buf.starts_with(b"MZ") {
        return Some("application/x-dosexec");
    }
    // Mach-O (32-bit, 64-bit, fat binary)
    if buf.len() >= 4 {
        let magic_u32 = u32::from_be_bytes([
            buf[0],
            buf[1],
            *buf.get(2).unwrap_or(&0),
            *buf.get(3).unwrap_or(&0),
        ]);
        if matches!(
            magic_u32,
            0xFEED_FACE | 0xFEED_FACF | 0xCAFE_BABE | 0xBEBA_FECA
        ) {
            return Some("application/x-mach-binary");
        }
    }
    if buf.starts_with(b"SQLite format 3") {
        return Some("application/x-sqlite3");
    }
    if buf.starts_with(b"\x00asm") {
        return Some("application/wasm");
    }
    // BMP
    if buf.starts_with(b"BM") && buf.len() >= 14 {
        return Some("image/bmp");
    }
    // TIFF (little-endian or big-endian)
    if (buf.starts_with(b"II\x2A\x00") || buf.starts_with(b"MM\x00\x2A")) && buf.len() >= 4 {
        return Some("image/tiff");
    }
    // tar: "ustar" at offset 257
    if buf.len() >= 262 && &buf[257..262] == b"ustar" {
        return Some("application/x-tar");
    }
    // FLAC in Ogg (already caught by OggS above, but useful for raw FLAC)
    // Zstandard
    if buf.len() >= 4 && buf[0] == 0x28 && buf[1] == 0xB5 && buf[2] == 0x2F && buf[3] == 0xFD {
        return Some("application/zstd");
    }
    // OpenType / TrueType
    if buf.starts_with(b"\x00\x01\x00\x00") && buf.len() >= 6 {
        return Some("font/ttf");
    }
    if buf.starts_with(b"OTTO") {
        return Some("font/otf");
    }
    if buf.starts_with(b"wOFF") {
        return Some("font/woff");
    }
    if buf.starts_with(b"wOF2") {
        return Some("font/woff2");
    }

    None
}

// ============================================================================
// Extension helpers
// ============================================================================

/// Extract the lowercase extension from a path, if any.
fn extract_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

// ============================================================================
// MIME type detection (combined strategy)
// ============================================================================

/// Detect the MIME type for a file, trying magic bytes first, then extension.
fn detect_mime_type(path: &str) -> String {
    // Try magic bytes from file contents.
    if let Ok(mut file) = File::open(path) {
        let mut buf = [0u8; MAGIC_BUF_LEN];
        if let Ok(n) = file.read(&mut buf)
            && let Some(mime) = mime_from_magic(&buf[..n]) {
                return mime.to_string();
            }
    }

    // Fall back to extension lookup.
    if let Some(ext) = extract_extension(path)
        && let Some(mime) = mime_from_extension(&ext) {
            return mime.to_string();
        }

    "application/octet-stream".to_string()
}

// ============================================================================
// URL scheme detection
// ============================================================================

/// Recognised URL schemes and their types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlScheme {
    Http,
    Https,
    Ftp,
    Ftps,
    Mailto,
    File,
    Ssh,
    Telnet,
    #[allow(dead_code)]
    Unknown,
}

/// Detect URL scheme from a string, if it looks like a URL.
fn detect_url_scheme(input: &str) -> Option<UrlScheme> {
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("http://") {
        Some(UrlScheme::Http)
    } else if lower.starts_with("https://") {
        Some(UrlScheme::Https)
    } else if lower.starts_with("ftp://") {
        Some(UrlScheme::Ftp)
    } else if lower.starts_with("ftps://") {
        Some(UrlScheme::Ftps)
    } else if lower.starts_with("mailto:") {
        Some(UrlScheme::Mailto)
    } else if lower.starts_with("file://") {
        Some(UrlScheme::File)
    } else if lower.starts_with("ssh://") {
        Some(UrlScheme::Ssh)
    } else if lower.starts_with("telnet://") {
        Some(UrlScheme::Telnet)
    } else {
        None
    }
}

/// Return a suitable MIME type hint for a URL scheme.
fn mime_for_scheme(scheme: UrlScheme) -> &'static str {
    match scheme {
        UrlScheme::Http | UrlScheme::Https => "x-scheme-handler/http",
        UrlScheme::Ftp | UrlScheme::Ftps => "x-scheme-handler/ftp",
        UrlScheme::Mailto => "x-scheme-handler/mailto",
        UrlScheme::File => "application/octet-stream",
        UrlScheme::Ssh => "x-scheme-handler/ssh",
        UrlScheme::Telnet => "x-scheme-handler/telnet",
        UrlScheme::Unknown => "application/octet-stream",
    }
}

// ============================================================================
// MIME type category helpers
// ============================================================================

/// Returns `true` if the MIME type denotes text or source code.
fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/javascript"
        || mime == "application/typescript"
        || mime == "application/x-shellscript"
        || mime == "application/toml"
        || mime == "application/sql"
}

/// Returns `true` if the MIME type denotes an image.
#[allow(dead_code)]
fn is_image_mime(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// Returns `true` if the MIME type denotes audio.
#[allow(dead_code)]
fn is_audio_mime(mime: &str) -> bool {
    mime.starts_with("audio/")
}

/// Returns `true` if the MIME type denotes video.
#[allow(dead_code)]
fn is_video_mime(mime: &str) -> bool {
    mime.starts_with("video/")
}

/// Returns `true` if the MIME type denotes an archive or compressed file.
#[allow(dead_code)]
fn is_archive_mime(mime: &str) -> bool {
    mime == "application/zip"
        || mime == "application/gzip"
        || mime == "application/x-tar"
        || mime == "application/x-bzip2"
        || mime == "application/x-xz"
        || mime == "application/x-7z-compressed"
        || mime == "application/vnd.rar"
        || mime == "application/zstd"
        || mime == "application/x-lzip"
        || mime == "application/x-lz4"
        || mime == "application/x-cpio"
        || mime == "application/x-archive"
}

/// Returns `true` if the MIME type denotes a document format.
#[allow(dead_code)]
fn is_document_mime(mime: &str) -> bool {
    mime == "application/pdf"
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.oasis.opendocument.")
        || mime == "application/msword"
        || mime == "application/vnd.ms-excel"
        || mime == "application/vnd.ms-powerpoint"
        || mime == "application/rtf"
        || mime == "application/epub+zip"
}

// ============================================================================
// INI-style parser (mimeapps.list and .desktop files)
// ============================================================================

/// Parsed section from an INI-style file.
#[derive(Debug, Clone)]
struct IniSection {
    name: String,
    entries: Vec<(String, String)>,
}

/// Parse an INI-style file into sections.
fn parse_ini(contents: &str) -> Vec<IniSection> {
    let mut sections = Vec::new();
    let mut current: Option<IniSection> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                if let Some(sec) = current.take() {
                    sections.push(sec);
                }
                current = Some(IniSection {
                    name: line[1..end].to_string(),
                    entries: Vec::new(),
                });
            }
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let value = line[eq_pos + 1..].trim().to_string();
            if let Some(ref mut sec) = current {
                sec.entries.push((key, value));
            }
        }
    }
    if let Some(sec) = current {
        sections.push(sec);
    }
    sections
}

// ============================================================================
// mimeapps.list handling
// ============================================================================

/// Stores the contents of a single mimeapps.list file.
#[derive(Debug, Default)]
struct MimeApps {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

/// Parse a semicolon-separated list of desktop IDs.
fn parse_desktop_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Load and parse a mimeapps.list file.
fn load_mimeapps(path: &Path) -> Option<MimeApps> {
    let contents = fs::read_to_string(path).ok()?;
    let sections = parse_ini(&contents);
    let mut result = MimeApps::default();

    for sec in &sections {
        let target = match sec.name.as_str() {
            "Default Applications" => &mut result.defaults,
            "Added Associations" => &mut result.added,
            "Removed Associations" => &mut result.removed,
            _ => continue,
        };
        for (key, value) in &sec.entries {
            target.insert(key.clone(), parse_desktop_list(value));
        }
    }
    Some(result)
}

/// Look up the default handler for a MIME type in a mimeapps.list, respecting
/// the removed-associations blacklist.
fn lookup_handler_in(apps: &MimeApps, mime: &str) -> Option<String> {
    let removed = apps.removed.get(mime);

    // First check explicit defaults.
    if let Some(defaults) = apps.defaults.get(mime) {
        for desktop_id in defaults {
            let dominated = removed
                .map(|r| r.contains(desktop_id))
                .unwrap_or(false);
            if !dominated {
                return Some(desktop_id.clone());
            }
        }
    }

    // Then check added associations.
    if let Some(added) = apps.added.get(mime) {
        for desktop_id in added {
            let dominated = removed
                .map(|r| r.contains(desktop_id))
                .unwrap_or(false);
            if !dominated {
                return Some(desktop_id.clone());
            }
        }
    }

    None
}

// ============================================================================
// .desktop file handling
// ============================================================================

/// Key fields from a .desktop file's `[Desktop Entry]` section.
#[derive(Debug, Default, Clone)]
struct DesktopEntry {
    name: String,
    exec: String,
    mime_types: Vec<String>,
    terminal: bool,
    entry_type: String,
    icon: String,
    no_display: bool,
}

/// Parse a .desktop file.
fn parse_desktop_file(contents: &str) -> Option<DesktopEntry> {
    let sections = parse_ini(contents);
    let section = sections.iter().find(|s| s.name == "Desktop Entry")?;

    let mut entry = DesktopEntry::default();
    for (key, value) in &section.entries {
        match key.as_str() {
            "Name" => entry.name = value.clone(),
            "Exec" => entry.exec = value.clone(),
            "MimeType" => {
                entry.mime_types = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            "Terminal" => entry.terminal = value.eq_ignore_ascii_case("true"),
            "Type" => entry.entry_type = value.clone(),
            "Icon" => entry.icon = value.clone(),
            "NoDisplay" => entry.no_display = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    Some(entry)
}

/// Load a .desktop file from disk.
fn load_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let contents = fs::read_to_string(path).ok()?;
    parse_desktop_file(&contents)
}

// ============================================================================
// Exec field expansion
// ============================================================================

/// Expand a .desktop `Exec` value, replacing field codes with actual values.
///
/// Supported field codes (per freedesktop spec):
/// - `%f` — single file path
/// - `%F` — list of file paths (space-separated)
/// - `%u` — single URL
/// - `%U` — list of URLs (space-separated)
/// - `%i` — icon (expanded to `--icon <icon>` if set, empty otherwise)
/// - `%c` — translated name
/// - `%k` — path of the desktop file
/// - `%%` — literal `%`
///
/// Unknown `%x` codes are removed.
fn expand_exec(
    exec: &str,
    files: &[&str],
    icon: &str,
    name: &str,
    desktop_path: &str,
) -> String {
    let single = files.first().copied().unwrap_or("");
    let all = files.join(" ");

    let mut result = String::with_capacity(exec.len() + all.len());
    let mut chars = exec.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('f') => result.push_str(single),
                Some('F') => result.push_str(&all),
                Some('u') => result.push_str(single),
                Some('U') => result.push_str(&all),
                Some('i') => {
                    if !icon.is_empty() {
                        result.push_str("--icon ");
                        result.push_str(icon);
                    }
                }
                Some('c') => result.push_str(name),
                Some('k') => result.push_str(desktop_path),
                Some('%') => result.push('%'),
                Some(_other) => { /* unknown code — drop it */ }
                None => result.push('%'), // trailing %
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ============================================================================
// Handler resolution
// ============================================================================

/// Return the home directory, falling back to "/root".
fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

/// Standard search paths for mimeapps.list files, user-first.
fn mimeapps_search_paths() -> Vec<PathBuf> {
    let home = home_dir();
    vec![
        home.join(".config/mimeapps.list"),
        PathBuf::from("/etc/xdg/mimeapps.list"),
        home.join(".local/share/applications/mimeapps.list"),
        PathBuf::from("/usr/share/applications/mimeapps.list"),
    ]
}

/// Standard search directories for .desktop files, user-first.
fn desktop_search_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    vec![
        home.join(".local/share/applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ]
}

/// Find a .desktop file by its ID (e.g. "editor.desktop") on the search path.
fn find_desktop_file(desktop_id: &str) -> Option<PathBuf> {
    for dir in desktop_search_dirs() {
        let candidate = dir.join(desktop_id);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the default handler for a MIME type, checking user then system
/// mimeapps.list files.
fn resolve_handler(mime: &str) -> Option<String> {
    for path in mimeapps_search_paths() {
        if let Some(apps) = load_mimeapps(&path)
            && let Some(handler) = lookup_handler_in(&apps, mime) {
                return Some(handler);
            }
    }
    None
}

/// Build the full command line for launching a handler.
fn build_command(desktop_id: &str, target: &str) -> Option<(String, Vec<String>)> {
    let desktop_path = find_desktop_file(desktop_id)?;
    let entry = load_desktop_file(&desktop_path)?;
    let path_str = desktop_path.to_str().unwrap_or("");
    let expanded = expand_exec(
        &entry.exec,
        &[target],
        &entry.icon,
        &entry.name,
        path_str,
    );

    // Split the expanded command into program + args.
    let parts: Vec<String> = shell_split(&expanded);
    if parts.is_empty() {
        return None;
    }
    let program = parts[0].clone();
    let args = parts[1..].to_vec();
    Some((program, args))
}

/// Minimal shell-word splitting (handles single and double quotes).
fn shell_split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

// ============================================================================
// mimeapps.list writing (for xdg-mime default / mimeopen -d)
// ============================================================================

/// Set the default handler for a MIME type in the user's mimeapps.list.
fn set_default_handler(mime: &str, desktop_id: &str) -> io::Result<()> {
    let home = home_dir();
    let config_dir = home.join(".config");
    fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("mimeapps.list");

    // Load existing contents or start fresh.
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let sections = parse_ini(&existing);

    let mut out = String::new();

    // Write [Default Applications] first, updating the target mime type.
    out.push_str("[Default Applications]\n");
    let mut wrote_mime = false;
    for sec in &sections {
        if sec.name == "Default Applications" {
            for (key, value) in &sec.entries {
                if key == mime {
                    writeln!(out, "{}={}", key, desktop_id)
                        .expect("string write cannot fail");
                    wrote_mime = true;
                } else {
                    writeln!(out, "{}={}", key, value)
                        .expect("string write cannot fail");
                }
            }
        }
    }
    if !wrote_mime {
        writeln!(out, "{}={}", mime, desktop_id).expect("string write cannot fail");
    }
    out.push('\n');

    // Preserve other sections verbatim.
    for sec in &sections {
        if sec.name == "Default Applications" {
            continue;
        }
        writeln!(out, "[{}]", sec.name).expect("string write cannot fail");
        for (key, value) in &sec.entries {
            writeln!(out, "{}={}", key, value).expect("string write cannot fail");
        }
        out.push('\n');
    }

    fs::write(&path, out)?;
    Ok(())
}

// ============================================================================
// MIME type XML installation (xdg-mime install/uninstall)
// ============================================================================

/// Install a MIME type XML definition into the user's local database.
fn install_mime_xml(xml_path: &str) -> io::Result<()> {
    let home = home_dir();
    let dest_dir = home.join(".local/share/mime/packages");
    fs::create_dir_all(&dest_dir)?;

    let src = Path::new(xml_path);
    let file_name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no filename in path"))?;
    let dest = dest_dir.join(file_name);
    fs::copy(src, &dest)?;

    eprintln!(
        "Installed {} to {}",
        xml_path,
        dest.display()
    );
    Ok(())
}

/// Remove a MIME type XML definition from the user's local database.
fn uninstall_mime_xml(xml_path: &str) -> io::Result<()> {
    let home = home_dir();
    let src = Path::new(xml_path);
    let file_name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no filename in path"))?;
    let dest = home.join(".local/share/mime/packages").join(file_name);

    if dest.exists() {
        fs::remove_file(&dest)?;
        eprintln!("Removed {}", dest.display());
    } else {
        eprintln!("Not installed: {}", dest.display());
    }
    Ok(())
}

// ============================================================================
// Scan for desktop files that handle a given MIME type
// ============================================================================

/// Find all .desktop files that claim to handle the given MIME type.
fn find_handlers_for_mime(mime: &str) -> Vec<(String, PathBuf)> {
    let mut results = Vec::new();
    for dir in desktop_search_dirs() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                if let Some(de) = load_desktop_file(&path)
                    && de.mime_types.iter().any(|m| m == mime) {
                        let name = if de.name.is_empty() {
                            path.file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or("unknown")
                                .to_string()
                        } else {
                            de.name.clone()
                        };
                        results.push((name, path));
                    }
            }
        }
    }
    results
}

// ============================================================================
// Fallback handler from environment variables
// ============================================================================

/// Try environment-based fallbacks for a MIME type.
fn env_fallback_handler(mime: &str) -> Option<String> {
    if is_text_mime(mime) {
        if let Ok(editor) = env::var("EDITOR") {
            return Some(editor);
        }
        if let Ok(visual) = env::var("VISUAL") {
            return Some(visual);
        }
    }
    if (mime.starts_with("x-scheme-handler/http") || mime == "text/html")
        && let Ok(browser) = env::var("BROWSER") {
            return Some(browser);
        }
    None
}

// ============================================================================
// xdg-open implementation
// ============================================================================

/// Run the xdg-open personality.
fn run_xdg_open(args: &[String]) -> i32 {
    let mut verbose = false;
    let mut target: Option<&str> = None;

    let mut iter = args.iter();
    for arg in iter {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                print_xdg_open_usage();
                return 0;
            }
            "--version" => {
                println!("xdg-open 0.1.0 (OurOS)");
                return 0;
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("xdg-open: unknown option '{}'", other);
                    return 1;
                }
                target = Some(other);
            }
        }
    }

    let target = match target {
        Some(t) => t,
        None => {
            eprintln!("xdg-open: no file or URL specified");
            print_xdg_open_usage();
            return 1;
        }
    };

    // Check for URL scheme first.
    if let Some(scheme) = detect_url_scheme(target) {
        let scheme_mime = mime_for_scheme(scheme);
        if verbose {
            println!("URL scheme detected: {:?}", scheme);
            println!("Handler MIME: {}", scheme_mime);
        }
        return launch_for_mime(scheme_mime, target, verbose);
    }

    // File mode: detect MIME type.
    let mime = detect_mime_type(target);
    if verbose {
        println!("Detected MIME type: {}", mime);
    }
    launch_for_mime(&mime, target, verbose)
}

/// Attempt to launch the appropriate handler for a MIME type and target.
fn launch_for_mime(mime: &str, target: &str, verbose: bool) -> i32 {
    // Try mimeapps.list chain.
    if let Some(desktop_id) = resolve_handler(mime) {
        if verbose {
            println!("Handler: {}", desktop_id);
        }
        if let Some((program, args)) = build_command(&desktop_id, target) {
            if verbose {
                println!("Exec: {} {}", program, args.join(" "));
            }
            return exec_command(&program, &args);
        }
        eprintln!(
            "xdg-open: handler '{}' found but could not build command",
            desktop_id
        );
    }

    // Try environment fallback.
    if let Some(fallback) = env_fallback_handler(mime) {
        if verbose {
            println!("Fallback handler: {}", fallback);
        }
        return exec_command(&fallback, &[target.to_string()]);
    }

    eprintln!(
        "xdg-open: no handler found for '{}' (MIME: {})",
        target, mime
    );
    4 // freedesktop exit code: no handler
}

/// Execute an external command, returning its exit code.
fn exec_command(program: &str, args: &[String]) -> i32 {
    match std::process::Command::new(program).args(args).status() {
        Ok(status) => {
            status.code().unwrap_or(1)
        }
        Err(e) => {
            eprintln!("xdg-open: failed to execute '{}': {}", program, e);
            1
        }
    }
}

fn print_xdg_open_usage() {
    eprintln!(
        "\
Usage: xdg-open [options] <file|URL>

Open a file or URL with the appropriate application.

Options:
  -v, --verbose   Show what would be launched
  -h, --help      Show this help
  --version       Show version"
    );
}

// ============================================================================
// xdg-mime implementation
// ============================================================================

/// Run the xdg-mime personality.
fn run_xdg_mime(args: &[String]) -> i32 {
    if args.is_empty() {
        print_xdg_mime_usage();
        return 1;
    }

    match args[0].as_str() {
        "query" => run_xdg_mime_query(&args[1..]),
        "default" => run_xdg_mime_default(&args[1..]),
        "install" => run_xdg_mime_install(&args[1..]),
        "uninstall" => run_xdg_mime_uninstall(&args[1..]),
        "--help" | "-h" => {
            print_xdg_mime_usage();
            0
        }
        "--version" => {
            println!("xdg-mime 0.1.0 (OurOS)");
            0
        }
        other => {
            eprintln!("xdg-mime: unknown subcommand '{}'", other);
            print_xdg_mime_usage();
            1
        }
    }
}

fn run_xdg_mime_query(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("xdg-mime query: missing query type (filetype|default)");
        return 1;
    }
    match args[0].as_str() {
        "filetype" => {
            if args.len() < 2 {
                eprintln!("xdg-mime query filetype: missing file argument");
                return 1;
            }
            let mime = detect_mime_type(&args[1]);
            println!("{}", mime);
            0
        }
        "default" => {
            if args.len() < 2 {
                eprintln!("xdg-mime query default: missing MIME type argument");
                return 1;
            }
            match resolve_handler(&args[1]) {
                Some(handler) => {
                    println!("{}", handler);
                    0
                }
                None => {
                    // Not an error, just no handler configured.
                    1
                }
            }
        }
        other => {
            eprintln!("xdg-mime query: unknown query type '{}'", other);
            1
        }
    }
}

fn run_xdg_mime_default(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("xdg-mime default: requires APP.desktop and MIME type");
        return 1;
    }
    let desktop_id = &args[0];
    let mime = &args[1];

    match set_default_handler(mime, desktop_id) {
        Ok(()) => {
            println!("Set {} as default for {}", desktop_id, mime);
            0
        }
        Err(e) => {
            eprintln!("xdg-mime default: {}", e);
            1
        }
    }
}

fn run_xdg_mime_install(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("xdg-mime install: missing XML file argument");
        return 1;
    }
    match install_mime_xml(&args[0]) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("xdg-mime install: {}", e);
            1
        }
    }
}

fn run_xdg_mime_uninstall(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("xdg-mime uninstall: missing XML file argument");
        return 1;
    }
    match uninstall_mime_xml(&args[0]) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("xdg-mime uninstall: {}", e);
            1
        }
    }
}

fn print_xdg_mime_usage() {
    eprintln!(
        "\
Usage: xdg-mime <command> [args...]

Commands:
  query filetype FILE      Print the MIME type of FILE
  query default MIME       Print the default handler for MIME type
  default APP.desktop MIME Set the default handler for MIME type
  install FILE.xml         Install a MIME type definition
  uninstall FILE.xml       Remove a MIME type definition
  --help                   Show this help
  --version                Show version"
    );
}

// ============================================================================
// mimeopen implementation
// ============================================================================

/// Run the mimeopen personality.
fn run_mimeopen(args: &[String]) -> i32 {
    let mut ask = false;
    let mut set_default = false;
    let mut no_open = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-a" | "--ask" => ask = true,
            "-d" | "--ask-default" => {
                ask = true;
                set_default = true;
            }
            "-n" | "--no-open" => no_open = true,
            "-h" | "--help" => {
                print_mimeopen_usage();
                return 0;
            }
            "--version" => {
                println!("mimeopen 0.1.0 (OurOS)");
                return 0;
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("mimeopen: unknown option '{}'", other);
                    return 1;
                }
                files.push(other.to_string());
            }
        }
    }

    if files.is_empty() {
        eprintln!("mimeopen: no file specified");
        print_mimeopen_usage();
        return 1;
    }

    let mut exit_code = 0;
    for file in &files {
        let mime = detect_mime_type(file);

        if no_open {
            println!("{}: {}", file, mime);
            continue;
        }

        if ask {
            let code = interactive_open(file, &mime, set_default);
            if code != 0 {
                exit_code = code;
            }
        } else {
            let code = launch_for_mime(&mime, file, false);
            if code != 0 {
                exit_code = code;
            }
        }
    }
    exit_code
}

/// Present an interactive handler selection menu.
fn interactive_open(file: &str, mime: &str, set_default: bool) -> i32 {
    let handlers = find_handlers_for_mime(mime);

    if handlers.is_empty() {
        eprintln!("No handlers found for MIME type '{}'", mime);
        eprintln!("Enter application command to use:");
    } else {
        eprintln!("Choose an application to open '{}' ({}):", file, mime);
        for (i, (name, path)) in handlers.iter().enumerate() {
            let desktop_id = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown");
            eprintln!("  {}) {} ({})", i + 1, name, desktop_id);
        }
        eprintln!("  0) Enter custom command");
    }

    eprint!("> ");
    let _ = io::stderr().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        eprintln!("mimeopen: failed to read input");
        return 1;
    }
    let input = input.trim();

    if let Ok(choice) = input.parse::<usize>() {
        if choice == 0 || handlers.is_empty() {
            eprint!("Enter command: ");
            let _ = io::stderr().flush();
            let mut cmd = String::new();
            if io::stdin().read_line(&mut cmd).is_err() {
                return 1;
            }
            return exec_command(cmd.trim(), &[file.to_string()]);
        }
        if choice > 0 && choice <= handlers.len() {
            let (_name, path) = &handlers[choice - 1];
            let desktop_id = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");

            if set_default
                && let Err(e) = set_default_handler(mime, desktop_id) {
                    eprintln!("Warning: could not set default: {}", e);
                }

            if let Some((program, args)) = build_command(desktop_id, file) {
                return exec_command(&program, &args);
            }
            eprintln!("mimeopen: could not build command for '{}'", desktop_id);
            return 1;
        }
    }

    // Treat input as a raw command.
    exec_command(input, &[file.to_string()])
}

fn print_mimeopen_usage() {
    eprintln!(
        "\
Usage: mimeopen [options] <file...>

Open files with the appropriate application.

Options:
  -a, --ask          Ask which application to use
  -d, --ask-default  Ask and set as default handler
  -n, --no-open      Print MIME type without opening
  -h, --help         Show this help
  --version          Show version"
    );
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::XdgOpen);

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match personality {
        Personality::XdgOpen => run_xdg_open(&rest),
        Personality::XdgMime => run_xdg_mime(&rest),
        Personality::MimeOpen => run_mimeopen(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Personality detection ---

    #[test]
    fn personality_xdg_open_default() {
        assert_eq!(detect_personality("xdg-open"), Personality::XdgOpen);
    }

    #[test]
    fn personality_xdg_open_path() {
        assert_eq!(
            detect_personality("/usr/bin/xdg-open"),
            Personality::XdgOpen
        );
    }

    #[test]
    fn personality_xdg_mime() {
        assert_eq!(detect_personality("xdg-mime"), Personality::XdgMime);
    }

    #[test]
    fn personality_xdg_mime_path() {
        assert_eq!(
            detect_personality("/usr/local/bin/xdg-mime"),
            Personality::XdgMime
        );
    }

    #[test]
    fn personality_mimeopen() {
        assert_eq!(detect_personality("mimeopen"), Personality::MimeOpen);
    }

    #[test]
    fn personality_mimeopen_path() {
        assert_eq!(
            detect_personality("/opt/bin/mimeopen"),
            Personality::MimeOpen
        );
    }

    #[test]
    fn personality_unknown_defaults_to_xdg_open() {
        assert_eq!(detect_personality("foobar"), Personality::XdgOpen);
    }

    // --- MIME type from extension ---

    #[test]
    fn ext_text_plain() {
        assert_eq!(mime_from_extension("txt"), Some("text/plain"));
    }

    #[test]
    fn ext_markdown() {
        assert_eq!(mime_from_extension("md"), Some("text/markdown"));
    }

    #[test]
    fn ext_json() {
        assert_eq!(mime_from_extension("json"), Some("application/json"));
    }

    #[test]
    fn ext_html() {
        assert_eq!(mime_from_extension("html"), Some("text/html"));
    }

    #[test]
    fn ext_css() {
        assert_eq!(mime_from_extension("css"), Some("text/css"));
    }

    #[test]
    fn ext_javascript() {
        assert_eq!(mime_from_extension("js"), Some("application/javascript"));
    }

    #[test]
    fn ext_rust() {
        assert_eq!(mime_from_extension("rs"), Some("text/x-rust"));
    }

    #[test]
    fn ext_python() {
        assert_eq!(mime_from_extension("py"), Some("text/x-python"));
    }

    #[test]
    fn ext_shell() {
        assert_eq!(mime_from_extension("sh"), Some("application/x-shellscript"));
    }

    #[test]
    fn ext_toml() {
        assert_eq!(mime_from_extension("toml"), Some("application/toml"));
    }

    #[test]
    fn ext_csv() {
        assert_eq!(mime_from_extension("csv"), Some("text/csv"));
    }

    #[test]
    fn ext_png() {
        assert_eq!(mime_from_extension("png"), Some("image/png"));
    }

    #[test]
    fn ext_jpeg() {
        assert_eq!(mime_from_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_from_extension("jpeg"), Some("image/jpeg"));
    }

    #[test]
    fn ext_gif() {
        assert_eq!(mime_from_extension("gif"), Some("image/gif"));
    }

    #[test]
    fn ext_svg() {
        assert_eq!(mime_from_extension("svg"), Some("image/svg+xml"));
    }

    #[test]
    fn ext_webp() {
        assert_eq!(mime_from_extension("webp"), Some("image/webp"));
    }

    #[test]
    fn ext_heic() {
        assert_eq!(mime_from_extension("heic"), Some("image/heic"));
    }

    #[test]
    fn ext_avif() {
        assert_eq!(mime_from_extension("avif"), Some("image/avif"));
    }

    #[test]
    fn ext_tiff() {
        assert_eq!(mime_from_extension("tiff"), Some("image/tiff"));
    }

    #[test]
    fn ext_mp3() {
        assert_eq!(mime_from_extension("mp3"), Some("audio/mpeg"));
    }

    #[test]
    fn ext_wav() {
        assert_eq!(mime_from_extension("wav"), Some("audio/wav"));
    }

    #[test]
    fn ext_flac() {
        assert_eq!(mime_from_extension("flac"), Some("audio/flac"));
    }

    #[test]
    fn ext_ogg() {
        assert_eq!(mime_from_extension("ogg"), Some("audio/ogg"));
    }

    #[test]
    fn ext_opus() {
        assert_eq!(mime_from_extension("opus"), Some("audio/opus"));
    }

    #[test]
    fn ext_mp4() {
        assert_eq!(mime_from_extension("mp4"), Some("video/mp4"));
    }

    #[test]
    fn ext_mkv() {
        assert_eq!(mime_from_extension("mkv"), Some("video/x-matroska"));
    }

    #[test]
    fn ext_avi() {
        assert_eq!(mime_from_extension("avi"), Some("video/x-msvideo"));
    }

    #[test]
    fn ext_webm() {
        assert_eq!(mime_from_extension("webm"), Some("video/webm"));
    }

    #[test]
    fn ext_mov() {
        assert_eq!(mime_from_extension("mov"), Some("video/quicktime"));
    }

    #[test]
    fn ext_pdf() {
        assert_eq!(mime_from_extension("pdf"), Some("application/pdf"));
    }

    #[test]
    fn ext_docx() {
        assert_eq!(
            mime_from_extension("docx"),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
    }

    #[test]
    fn ext_epub() {
        assert_eq!(mime_from_extension("epub"), Some("application/epub+zip"));
    }

    #[test]
    fn ext_zip() {
        assert_eq!(mime_from_extension("zip"), Some("application/zip"));
    }

    #[test]
    fn ext_tar() {
        assert_eq!(mime_from_extension("tar"), Some("application/x-tar"));
    }

    #[test]
    fn ext_gz() {
        assert_eq!(mime_from_extension("gz"), Some("application/gzip"));
    }

    #[test]
    fn ext_7z() {
        assert_eq!(
            mime_from_extension("7z"),
            Some("application/x-7z-compressed")
        );
    }

    #[test]
    fn ext_iso() {
        assert_eq!(
            mime_from_extension("iso"),
            Some("application/x-iso9660-image")
        );
    }

    #[test]
    fn ext_deb() {
        assert_eq!(
            mime_from_extension("deb"),
            Some("application/vnd.debian.binary-package")
        );
    }

    #[test]
    fn ext_ttf() {
        assert_eq!(mime_from_extension("ttf"), Some("font/ttf"));
    }

    #[test]
    fn ext_woff2() {
        assert_eq!(mime_from_extension("woff2"), Some("font/woff2"));
    }

    #[test]
    fn ext_desktop() {
        assert_eq!(
            mime_from_extension("desktop"),
            Some("application/x-desktop")
        );
    }

    #[test]
    fn ext_sqlite() {
        assert_eq!(
            mime_from_extension("sqlite"),
            Some("application/x-sqlite3")
        );
    }

    #[test]
    fn ext_torrent() {
        assert_eq!(
            mime_from_extension("torrent"),
            Some("application/x-bittorrent")
        );
    }

    #[test]
    fn ext_unknown_returns_none() {
        assert_eq!(mime_from_extension("xyzzy"), None);
    }

    #[test]
    fn ext_empty_returns_none() {
        assert_eq!(mime_from_extension(""), None);
    }

    // --- Magic byte detection ---

    #[test]
    fn magic_pdf() {
        assert_eq!(
            mime_from_magic(b"%PDF-1.7 something"),
            Some("application/pdf")
        );
    }

    #[test]
    fn magic_png() {
        let buf = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("image/png"));
    }

    #[test]
    fn magic_jpeg() {
        let buf = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("image/jpeg"));
    }

    #[test]
    fn magic_gif87a() {
        assert_eq!(mime_from_magic(b"GIF87a..."), Some("image/gif"));
    }

    #[test]
    fn magic_gif89a() {
        assert_eq!(mime_from_magic(b"GIF89a..."), Some("image/gif"));
    }

    #[test]
    fn magic_zip() {
        let buf = [b'P', b'K', 0x03, 0x04, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("application/zip"));
    }

    #[test]
    fn magic_gzip() {
        let buf = [0x1F, 0x8B, 0x08, 0, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("application/gzip"));
    }

    #[test]
    fn magic_bz2() {
        assert_eq!(mime_from_magic(b"BZh91AY"), Some("application/x-bzip2"));
    }

    #[test]
    fn magic_xz() {
        let buf = [0xFD, b'7', b'z', b'X', b'Z', 0x00, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("application/x-xz"));
    }

    #[test]
    fn magic_7z() {
        let buf = [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("application/x-7z-compressed"));
    }

    #[test]
    fn magic_elf() {
        let buf = [0x7F, b'E', b'L', b'F', 2, 1, 1, 0];
        assert_eq!(mime_from_magic(&buf), Some("application/x-elf"));
    }

    #[test]
    fn magic_pe() {
        assert_eq!(
            mime_from_magic(b"MZ\x90\x00\x03\x00"),
            Some("application/x-dosexec")
        );
    }

    #[test]
    fn magic_ogg() {
        assert_eq!(mime_from_magic(b"OggS\x00\x02"), Some("audio/ogg"));
    }

    #[test]
    fn magic_flac() {
        assert_eq!(mime_from_magic(b"fLaC\x00\x00"), Some("audio/flac"));
    }

    #[test]
    fn magic_mp3_id3() {
        assert_eq!(mime_from_magic(b"ID3\x04\x00"), Some("audio/mpeg"));
    }

    #[test]
    fn magic_mp3_sync() {
        let buf = [0xFF, 0xFB, 0x90, 0x00];
        assert_eq!(mime_from_magic(&buf), Some("audio/mpeg"));
    }

    #[test]
    fn magic_wav() {
        let buf = b"RIFF\x00\x00\x00\x00WAVEfmt ";
        assert_eq!(mime_from_magic(buf), Some("audio/wav"));
    }

    #[test]
    fn magic_avi() {
        let buf = b"RIFF\x00\x00\x00\x00AVI LIST";
        assert_eq!(mime_from_magic(buf), Some("video/x-msvideo"));
    }

    #[test]
    fn magic_webp() {
        let buf = b"RIFF\x00\x00\x00\x00WEBPVP8 ";
        assert_eq!(mime_from_magic(buf), Some("image/webp"));
    }

    #[test]
    fn magic_sqlite() {
        assert_eq!(
            mime_from_magic(b"SQLite format 3\x00"),
            Some("application/x-sqlite3")
        );
    }

    #[test]
    fn magic_wasm() {
        let buf = [0x00, b'a', b's', b'm', 0x01, 0x00, 0x00, 0x00];
        assert_eq!(mime_from_magic(&buf), Some("application/wasm"));
    }

    #[test]
    fn magic_tar_ustar() {
        let mut buf = [0u8; 512];
        buf[257] = b'u';
        buf[258] = b's';
        buf[259] = b't';
        buf[260] = b'a';
        buf[261] = b'r';
        assert_eq!(mime_from_magic(&buf), Some("application/x-tar"));
    }

    #[test]
    fn magic_empty_buffer() {
        assert_eq!(mime_from_magic(b""), None);
    }

    #[test]
    fn magic_unknown_bytes() {
        assert_eq!(mime_from_magic(b"\x01\x02\x03\x04\x05"), None);
    }

    #[test]
    fn magic_rar() {
        let buf = [b'R', b'a', b'r', b'!', 0x1A, 0x07, 0x01, 0x00];
        assert_eq!(mime_from_magic(&buf), Some("application/vnd.rar"));
    }

    #[test]
    fn magic_bmp() {
        let mut buf = [0u8; 16];
        buf[0] = b'B';
        buf[1] = b'M';
        assert_eq!(mime_from_magic(&buf), Some("image/bmp"));
    }

    #[test]
    fn magic_tiff_le() {
        let buf = [b'I', b'I', 0x2A, 0x00, 0, 0, 0, 0];
        assert_eq!(mime_from_magic(&buf), Some("image/tiff"));
    }

    // --- mimeapps.list parsing ---

    #[test]
    fn parse_mimeapps_defaults() {
        let content = "\
[Default Applications]
text/plain=editor.desktop
text/html=browser.desktop
image/png=imageviewer.desktop
";
        let sections = parse_ini(content);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "Default Applications");
        assert_eq!(sections[0].entries.len(), 3);
        assert_eq!(sections[0].entries[0], ("text/plain".into(), "editor.desktop".into()));
    }

    #[test]
    fn parse_mimeapps_added_associations() {
        let content = "\
[Added Associations]
text/plain=editor.desktop;nano.desktop;
";
        let sections = parse_ini(content);
        let entries = &sections[0].entries;
        assert_eq!(entries[0].1, "editor.desktop;nano.desktop;");
    }

    #[test]
    fn parse_mimeapps_removed_associations() {
        let content = "\
[Removed Associations]
text/plain=gedit.desktop;
";
        let sections = parse_ini(content);
        assert_eq!(sections[0].name, "Removed Associations");
    }

    #[test]
    fn parse_mimeapps_multiple_sections() {
        let content = "\
[Default Applications]
text/plain=editor.desktop

[Added Associations]
text/plain=editor.desktop;nano.desktop;

[Removed Associations]
text/plain=gedit.desktop;
";
        let sections = parse_ini(content);
        assert_eq!(sections.len(), 3);
    }

    #[test]
    fn desktop_list_parsing() {
        let list = parse_desktop_list("editor.desktop;nano.desktop;");
        assert_eq!(list, vec!["editor.desktop", "nano.desktop"]);
    }

    #[test]
    fn desktop_list_parsing_empty() {
        let list = parse_desktop_list("");
        assert!(list.is_empty());
    }

    #[test]
    fn desktop_list_parsing_single() {
        let list = parse_desktop_list("app.desktop");
        assert_eq!(list, vec!["app.desktop"]);
    }

    #[test]
    fn handler_lookup_finds_default() {
        let mut apps = MimeApps::default();
        apps.defaults
            .insert("text/plain".into(), vec!["editor.desktop".into()]);
        assert_eq!(
            lookup_handler_in(&apps, "text/plain"),
            Some("editor.desktop".into())
        );
    }

    #[test]
    fn handler_lookup_skips_removed() {
        let mut apps = MimeApps::default();
        apps.defaults
            .insert("text/plain".into(), vec!["editor.desktop".into()]);
        apps.removed
            .insert("text/plain".into(), vec!["editor.desktop".into()]);
        assert_eq!(lookup_handler_in(&apps, "text/plain"), None);
    }

    #[test]
    fn handler_lookup_falls_through_to_added() {
        let mut apps = MimeApps::default();
        apps.added
            .insert("text/plain".into(), vec!["nano.desktop".into()]);
        assert_eq!(
            lookup_handler_in(&apps, "text/plain"),
            Some("nano.desktop".into())
        );
    }

    #[test]
    fn handler_lookup_missing_mime() {
        let apps = MimeApps::default();
        assert_eq!(lookup_handler_in(&apps, "video/mp4"), None);
    }

    // --- Desktop file parsing ---

    #[test]
    fn parse_desktop_entry_basic() {
        let content = "\
[Desktop Entry]
Name=Text Editor
Exec=/usr/bin/editor %f
MimeType=text/plain;text/x-python;
Terminal=false
Type=Application
";
        let entry = parse_desktop_file(content).unwrap();
        assert_eq!(entry.name, "Text Editor");
        assert_eq!(entry.exec, "/usr/bin/editor %f");
        assert_eq!(entry.mime_types, vec!["text/plain", "text/x-python"]);
        assert!(!entry.terminal);
        assert_eq!(entry.entry_type, "Application");
    }

    #[test]
    fn parse_desktop_entry_terminal_true() {
        let content = "\
[Desktop Entry]
Name=Nano
Exec=nano %f
Terminal=true
Type=Application
";
        let entry = parse_desktop_file(content).unwrap();
        assert!(entry.terminal);
    }

    #[test]
    fn parse_desktop_entry_with_icon() {
        let content = "\
[Desktop Entry]
Name=Browser
Exec=/usr/bin/browser %u
Icon=web-browser
Type=Application
";
        let entry = parse_desktop_file(content).unwrap();
        assert_eq!(entry.icon, "web-browser");
    }

    #[test]
    fn parse_desktop_entry_no_display() {
        let content = "\
[Desktop Entry]
Name=Hidden
Exec=/usr/bin/hidden
NoDisplay=true
Type=Application
";
        let entry = parse_desktop_file(content).unwrap();
        assert!(entry.no_display);
    }

    #[test]
    fn parse_desktop_missing_desktop_entry_section() {
        let content = "\
[Some Other Section]
Name=Foo
Exec=bar
";
        assert!(parse_desktop_file(content).is_none());
    }

    // --- Exec field expansion ---

    #[test]
    fn expand_exec_percent_f() {
        let result = expand_exec("/usr/bin/editor %f", &["myfile.txt"], "", "", "");
        assert_eq!(result, "/usr/bin/editor myfile.txt");
    }

    #[test]
    fn expand_exec_percent_capital_f() {
        let result = expand_exec("/usr/bin/editor %F", &["a.txt", "b.txt"], "", "", "");
        assert_eq!(result, "/usr/bin/editor a.txt b.txt");
    }

    #[test]
    fn expand_exec_percent_u() {
        let result =
            expand_exec("/usr/bin/browser %u", &["https://example.com"], "", "", "");
        assert_eq!(result, "/usr/bin/browser https://example.com");
    }

    #[test]
    fn expand_exec_percent_capital_u() {
        let result = expand_exec(
            "/usr/bin/browser %U",
            &["https://a.com", "https://b.com"],
            "",
            "",
            "",
        );
        assert_eq!(result, "/usr/bin/browser https://a.com https://b.com");
    }

    #[test]
    fn expand_exec_percent_i_with_icon() {
        let result = expand_exec("app %i", &[], "my-icon", "", "");
        assert_eq!(result, "app --icon my-icon");
    }

    #[test]
    fn expand_exec_percent_i_no_icon() {
        let result = expand_exec("app %i", &[], "", "", "");
        assert_eq!(result, "app ");
    }

    #[test]
    fn expand_exec_percent_c() {
        let result = expand_exec("app --title %c", &[], "", "My App", "");
        assert_eq!(result, "app --title My App");
    }

    #[test]
    fn expand_exec_percent_k() {
        let result = expand_exec("app %k", &[], "", "", "/usr/share/app.desktop");
        assert_eq!(result, "app /usr/share/app.desktop");
    }

    #[test]
    fn expand_exec_literal_percent() {
        let result = expand_exec("echo 100%%", &[], "", "", "");
        assert_eq!(result, "echo 100%");
    }

    #[test]
    fn expand_exec_unknown_code_dropped() {
        let result = expand_exec("app %z file", &[], "", "", "");
        assert_eq!(result, "app  file");
    }

    #[test]
    fn expand_exec_no_codes() {
        let result = expand_exec("/usr/bin/app --flag", &["ignored"], "", "", "");
        assert_eq!(result, "/usr/bin/app --flag");
    }

    // --- URL scheme detection ---

    #[test]
    fn url_http() {
        assert_eq!(
            detect_url_scheme("http://example.com"),
            Some(UrlScheme::Http)
        );
    }

    #[test]
    fn url_https() {
        assert_eq!(
            detect_url_scheme("https://example.com"),
            Some(UrlScheme::Https)
        );
    }

    #[test]
    fn url_mailto() {
        assert_eq!(
            detect_url_scheme("mailto:user@example.com"),
            Some(UrlScheme::Mailto)
        );
    }

    #[test]
    fn url_ftp() {
        assert_eq!(
            detect_url_scheme("ftp://ftp.example.com"),
            Some(UrlScheme::Ftp)
        );
    }

    #[test]
    fn url_ssh() {
        assert_eq!(
            detect_url_scheme("ssh://server.example.com"),
            Some(UrlScheme::Ssh)
        );
    }

    #[test]
    fn url_file() {
        assert_eq!(
            detect_url_scheme("file:///tmp/test"),
            Some(UrlScheme::File)
        );
    }

    #[test]
    fn url_case_insensitive() {
        assert_eq!(
            detect_url_scheme("HTTP://EXAMPLE.COM"),
            Some(UrlScheme::Http)
        );
    }

    #[test]
    fn url_not_a_url() {
        assert_eq!(detect_url_scheme("/tmp/myfile.txt"), None);
    }

    #[test]
    fn url_relative_path() {
        assert_eq!(detect_url_scheme("myfile.txt"), None);
    }

    // --- Extension extraction ---

    #[test]
    fn extract_ext_simple() {
        assert_eq!(extract_extension("photo.png"), Some("png".into()));
    }

    #[test]
    fn extract_ext_uppercase() {
        assert_eq!(extract_extension("photo.PNG"), Some("png".into()));
    }

    #[test]
    fn extract_ext_double_dot() {
        assert_eq!(extract_extension("archive.tar.gz"), Some("gz".into()));
    }

    #[test]
    fn extract_ext_no_extension() {
        assert_eq!(extract_extension("Makefile"), None);
    }

    #[test]
    fn extract_ext_dotfile() {
        assert_eq!(extract_extension(".gitignore"), None);
    }

    #[test]
    fn extract_ext_path_with_dirs() {
        assert_eq!(
            extract_extension("/home/user/doc.pdf"),
            Some("pdf".into())
        );
    }

    // --- MIME type category helpers ---

    #[test]
    fn category_is_text() {
        assert!(is_text_mime("text/plain"));
        assert!(is_text_mime("text/html"));
        assert!(is_text_mime("application/json"));
        assert!(is_text_mime("application/javascript"));
        assert!(!is_text_mime("image/png"));
    }

    #[test]
    fn category_is_image() {
        assert!(is_image_mime("image/png"));
        assert!(is_image_mime("image/jpeg"));
        assert!(!is_image_mime("text/plain"));
    }

    #[test]
    fn category_is_audio() {
        assert!(is_audio_mime("audio/mpeg"));
        assert!(is_audio_mime("audio/flac"));
        assert!(!is_audio_mime("video/mp4"));
    }

    #[test]
    fn category_is_video() {
        assert!(is_video_mime("video/mp4"));
        assert!(is_video_mime("video/webm"));
        assert!(!is_video_mime("audio/mpeg"));
    }

    #[test]
    fn category_is_archive() {
        assert!(is_archive_mime("application/zip"));
        assert!(is_archive_mime("application/gzip"));
        assert!(is_archive_mime("application/x-7z-compressed"));
        assert!(!is_archive_mime("application/pdf"));
    }

    #[test]
    fn category_is_document() {
        assert!(is_document_mime("application/pdf"));
        assert!(is_document_mime("application/epub+zip"));
        assert!(is_document_mime(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
        assert!(!is_document_mime("text/plain"));
    }

    // --- INI parser edge cases ---

    #[test]
    fn ini_comments_ignored() {
        let content = "\
# this is a comment
[Section]
key=value
# another comment
";
        let sections = parse_ini(content);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].entries.len(), 1);
    }

    #[test]
    fn ini_empty_lines_ignored() {
        let content = "\n\n[Section]\n\nkey=value\n\n";
        let sections = parse_ini(content);
        assert_eq!(sections.len(), 1);
    }

    #[test]
    fn ini_value_with_equals() {
        let content = "[Section]\nkey=val=ue\n";
        let sections = parse_ini(content);
        assert_eq!(sections[0].entries[0].1, "val=ue");
    }

    #[test]
    fn ini_whitespace_trimmed() {
        let content = "[Section]\n  key  =  value  \n";
        let sections = parse_ini(content);
        assert_eq!(sections[0].entries[0].0, "key");
        assert_eq!(sections[0].entries[0].1, "value");
    }

    // --- Shell splitting ---

    #[test]
    fn shell_split_simple() {
        assert_eq!(shell_split("a b c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn shell_split_double_quotes() {
        assert_eq!(
            shell_split(r#"cmd "arg with spaces" end"#),
            vec!["cmd", "arg with spaces", "end"]
        );
    }

    #[test]
    fn shell_split_single_quotes() {
        assert_eq!(
            shell_split("cmd 'arg with spaces' end"),
            vec!["cmd", "arg with spaces", "end"]
        );
    }

    #[test]
    fn shell_split_escaped_space() {
        assert_eq!(shell_split(r"cmd arg\ 1 end"), vec!["cmd", "arg 1", "end"]);
    }

    #[test]
    fn shell_split_empty_input() {
        let result: Vec<String> = shell_split("");
        assert!(result.is_empty());
    }

    // --- Mime scheme mapping ---

    #[test]
    fn mime_for_http_scheme() {
        assert_eq!(mime_for_scheme(UrlScheme::Http), "x-scheme-handler/http");
    }

    #[test]
    fn mime_for_mailto_scheme() {
        assert_eq!(
            mime_for_scheme(UrlScheme::Mailto),
            "x-scheme-handler/mailto"
        );
    }

    // --- Binary detection edge cases ---

    #[test]
    fn magic_short_buffer_no_panic() {
        // Single byte should not panic or false-match.
        assert_eq!(mime_from_magic(&[0xFF]), None);
    }

    #[test]
    fn magic_two_byte_mz() {
        // Just "MZ" with no more data should still detect PE.
        assert_eq!(mime_from_magic(b"MZ"), Some("application/x-dosexec"));
    }

    // --- Zstd magic ---
    #[test]
    fn magic_zstd() {
        let buf = [0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00];
        assert_eq!(mime_from_magic(&buf), Some("application/zstd"));
    }

    // --- Font magic ---
    #[test]
    fn magic_otf() {
        assert_eq!(mime_from_magic(b"OTTO\x00\x10"), Some("font/otf"));
    }

    #[test]
    fn magic_woff() {
        assert_eq!(mime_from_magic(b"wOFF\x00\x01"), Some("font/woff"));
    }

    #[test]
    fn magic_woff2() {
        assert_eq!(mime_from_magic(b"wOF2\x00\x01"), Some("font/woff2"));
    }
}
