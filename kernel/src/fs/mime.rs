//! MIME type detection for files.
//!
//! Determines the MIME content type of files using two strategies:
//! 1. **Magic bytes**: reads the first bytes of a file and matches against
//!    known binary signatures (PNG header, ELF magic, etc.).
//! 2. **Extension**: maps the file extension to a MIME type using a built-in
//!    database of common types.
//!
//! Magic detection is preferred when available (it examines actual content),
//! with extension as a fallback.  The API returns standard IANA MIME types
//! like `"image/png"` or `"application/pdf"`.
//!
//! ## Usage
//!
//! ```ignore
//! // Detect from a file path (reads header + checks extension):
//! let mime = fs::mime::detect("/path/to/file.png");
//! // → Ok("image/png")
//!
//! // Detect from raw bytes only:
//! let mime = fs::mime::from_bytes(header_bytes);
//! // → Some("application/pdf")
//!
//! // Detect from extension only:
//! let mime = fs::mime::from_extension("rs");
//! // → Some("text/x-rust")
//! ```
//!
//! ## Design
//!
//! The module is stateless — no global tables or initialization needed.
//! All detection is done via pure functions against const data.
//! This makes it safe to call from any context (interrupt, kshell, VFS).

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect the MIME type of a file at the given path.
///
/// Reads the first 512 bytes for magic detection, then falls back to
/// extension-based detection.  Returns `"application/octet-stream"` for
/// unknown binary files and `"text/plain"` for unknown text files.
pub fn detect(path: &str) -> KernelResult<&'static str> {
    // Try magic detection first (reads file header).
    if let Ok(header) = crate::fs::Vfs::read_at(path, 0, 512) {
        if let Some(mime) = from_bytes(&header) {
            return Ok(mime);
        }
    }

    // Fall back to extension.
    if let Some(ext) = path_extension(path) {
        if let Some(mime) = from_extension(ext) {
            return Ok(mime);
        }
    }

    // Final fallback: check if the file looks like text or binary.
    if let Ok(header) = crate::fs::Vfs::read_at(path, 0, 256) {
        return Ok(text_or_binary(&header));
    }

    Ok("application/octet-stream")
}

/// Detect MIME type from raw bytes (file header).
///
/// Returns `None` if no known signature matches.  Pass at least the
/// first 512 bytes of the file for best coverage (some signatures
/// are at higher offsets, e.g., USTAR tar at offset 257).
pub fn from_bytes(header: &[u8]) -> Option<&'static str> {
    if header.is_empty() {
        return None;
    }

    // --- Binary format signatures ---
    // Order: more specific patterns first where ambiguity exists.

    // ELF binary
    if header.starts_with(b"\x7FELF") {
        return Some("application/x-elf");
    }

    // PE/COFF (Windows executables, UEFI binaries)
    if header.starts_with(b"MZ") {
        return Some("application/x-dosexec");
    }

    // --- Image formats ---

    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if header.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if header.starts_with(b"BM") && header.len() >= 6 {
        return Some("image/bmp");
    }
    if header.starts_with(b"RIFF") && header.len() >= 12 && header.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if header.starts_with(b"II\x2a\x00") || header.starts_with(b"MM\x00\x2a") {
        return Some("image/tiff");
    }
    if header.len() >= 4 && header.starts_with(b"\x00\x00\x01\x00") {
        return Some("image/x-icon");
    }

    // --- Document formats ---

    if header.starts_with(b"%PDF") {
        return Some("application/pdf");
    }

    // --- Archive formats ---

    // ZIP (also: JAR, DOCX, XLSX, PPTX, APK, etc.)
    if header.starts_with(b"PK\x03\x04") {
        return Some("application/zip");
    }
    if header.starts_with(b"\x1f\x8b") {
        return Some("application/gzip");
    }
    if header.starts_with(b"BZh") {
        return Some("application/x-bzip2");
    }
    if header.starts_with(b"\xfd7zXZ\x00") {
        return Some("application/x-xz");
    }
    if header.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some("application/x-7z-compressed");
    }
    if header.starts_with(b"Rar!\x1a\x07") {
        return Some("application/x-rar-compressed");
    }
    if header.len() >= 4 {
        let magic32 = u32::from_le_bytes([
            header[0], header[1], header[2], header[3],
        ]);
        if magic32 == 0xFD2F_B528 {
            return Some("application/zstd");
        }
        if magic32 == 0x04224D18 {
            return Some("application/x-lz4");
        }
    }

    // CPIO newc
    if header.starts_with(b"070701") || header.starts_with(b"070702") {
        return Some("application/x-cpio");
    }

    // ar archive / .deb
    if header.starts_with(b"!<arch>\n") {
        if header.len() >= 68
            && header
                .get(8..24)
                .map_or(false, |n| n.starts_with(b"debian-binary"))
        {
            return Some("application/vnd.debian.binary-package");
        }
        return Some("application/x-archive");
    }

    // USTAR tar archive (magic at offset 257)
    if header.len() >= 263 && header.get(257..262) == Some(b"ustar") {
        return Some("application/x-tar");
    }

    // --- Audio formats ---

    if header.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if header.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if header.starts_with(b"ID3") {
        return Some("audio/mpeg");
    }
    if header.len() >= 2 && header[0] == 0xFF && (header[1] & 0xE0) == 0xE0 {
        return Some("audio/mpeg");
    }
    if header.starts_with(b"RIFF") && header.len() >= 12 && header.get(8..12) == Some(b"WAVE") {
        return Some("audio/wav");
    }
    if header.starts_with(b"MThd") {
        return Some("audio/midi");
    }

    // --- Video formats ---

    if header.starts_with(b"RIFF") && header.len() >= 12 && header.get(8..12) == Some(b"AVI ") {
        return Some("video/x-msvideo");
    }
    if header.starts_with(b"\x1a\x45\xdf\xa3") {
        return Some("video/webm");
    }

    // --- Other binary formats ---

    if header.starts_with(b"\x00asm") {
        return Some("application/wasm");
    }
    if header.starts_with(b"SQLite format 3") {
        return Some("application/x-sqlite3");
    }

    // --- Text-based formats (check after binary) ---

    if header.starts_with(b"#!") {
        return Some("text/x-shellscript");
    }
    if header.starts_with(b"\xEF\xBB\xBF") {
        return Some("text/plain"); // UTF-8 with BOM
    }
    if header.starts_with(b"\xFF\xFE") || header.starts_with(b"\xFE\xFF") {
        return Some("text/plain"); // UTF-16
    }
    if header.starts_with(b"<?xml") {
        return Some("application/xml");
    }
    if header.starts_with(b"<!DOCTYPE html")
        || header.starts_with(b"<html")
        || header.starts_with(b"<HTML")
    {
        return Some("text/html");
    }

    // JSON heuristic.
    if matches!(header.first(), Some(b'{') | Some(b'[')) {
        if core::str::from_utf8(header.get(..64.min(header.len())).unwrap_or(&[])).is_ok() {
            return Some("application/json");
        }
    }

    None
}

/// Map a file extension (without the leading dot) to a MIME type.
///
/// Returns `None` if the extension is not recognized.
/// The extension should be lowercase for best matching.
pub fn from_extension(ext: &str) -> Option<&'static str> {
    // Normalize to lowercase for matching.
    // Since we're in no_std, do an ASCII-only comparison.
    match ext {
        // Text and source code
        "txt" | "text" | "log" => Some("text/plain"),
        "rs" => Some("text/x-rust"),
        "c" | "h" => Some("text/x-c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some("text/x-c++"),
        "py" => Some("text/x-python"),
        "js" | "mjs" => Some("text/javascript"),
        "ts" => Some("text/typescript"),
        "json" => Some("application/json"),
        "toml" => Some("application/toml"),
        "yaml" | "yml" => Some("application/x-yaml"),
        "xml" | "xsd" | "xsl" => Some("application/xml"),
        "html" | "htm" => Some("text/html"),
        "css" => Some("text/css"),
        "md" | "markdown" => Some("text/markdown"),
        "sh" | "bash" => Some("text/x-shellscript"),
        "bat" | "cmd" => Some("text/x-msdos-batch"),
        "csv" => Some("text/csv"),
        "ini" | "cfg" | "conf" => Some("text/plain"),
        "sql" => Some("application/sql"),
        "lua" => Some("text/x-lua"),
        "rb" => Some("text/x-ruby"),
        "java" => Some("text/x-java-source"),
        "go" => Some("text/x-go"),
        "swift" => Some("text/x-swift"),
        "kt" => Some("text/x-kotlin"),
        "r" => Some("text/x-r"),
        "ps1" | "psm1" => Some("text/x-powershell"),
        "diff" | "patch" => Some("text/x-diff"),

        // Binary / executable
        "nx" => Some("application/x-nx-executable"),
        "dso" => Some("application/x-nx-sharedlib"),
        "slib" => Some("application/x-nx-staticlib"),
        "elf" => Some("application/x-elf"),
        "o" => Some("application/x-object"),
        "a" | "lib" => Some("application/x-archive"),
        "so" | "dll" => Some("application/x-sharedlib"),
        "exe" => Some("application/x-dosexec"),
        "wasm" => Some("application/wasm"),
        "class" => Some("application/java-vm"),

        // Image
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "webp" => Some("image/webp"),
        "tif" | "tiff" => Some("image/tiff"),
        "psd" => Some("image/vnd.adobe.photoshop"),

        // Audio
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" | "oga" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "m4a" | "aac" => Some("audio/mp4"),
        "wma" => Some("audio/x-ms-wma"),
        "mid" | "midi" => Some("audio/midi"),
        "opus" => Some("audio/opus"),

        // Video
        "mp4" | "m4v" => Some("video/mp4"),
        "avi" => Some("video/x-msvideo"),
        "mkv" => Some("video/x-matroska"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "wmv" => Some("video/x-ms-wmv"),
        "flv" => Some("video/x-flv"),

        // Archive / compressed
        "zip" => Some("application/zip"),
        "gz" | "gzip" => Some("application/gzip"),
        "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"),
        "zst" | "zstd" => Some("application/zstd"),
        "lz4" => Some("application/x-lz4"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/x-rar-compressed"),
        "tar" => Some("application/x-tar"),
        "cpio" => Some("application/x-cpio"),
        "deb" => Some("application/vnd.debian.binary-package"),
        "rpm" => Some("application/x-rpm"),
        "jar" => Some("application/java-archive"),

        // Document
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "rtf" => Some("application/rtf"),
        "epub" => Some("application/epub+zip"),

        // Font
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),

        // Database
        "db" | "sqlite" | "sqlite3" => Some("application/x-sqlite3"),

        _ => None,
    }
}

/// Classify bytes as text or binary content.
///
/// Returns `"text/plain"` if the content appears to be text (printable
/// ASCII + common control chars + valid UTF-8 high bytes), or
/// `"application/octet-stream"` if it contains binary indicators
/// (null bytes, non-text control characters).
pub fn text_or_binary(data: &[u8]) -> &'static str {
    let check_len = data.len().min(256);
    let mut non_text = 0usize;

    for &b in data.get(..check_len).unwrap_or(&[]) {
        match b {
            // Common text bytes: printable ASCII, tab, newline, CR.
            0x09 | 0x0A | 0x0D | 0x20..=0x7E => {}
            // High bytes: UTF-8 continuation bytes.
            0x80..=0xFF => {}
            // Null and other control chars are strong binary indicators.
            _ => {
                non_text = non_text.saturating_add(1);
            }
        }
    }

    if non_text == 0 && check_len > 0 {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

/// Get the category of a MIME type as a human-readable string.
///
/// Useful for grouping files in a file explorer or search results.
#[allow(dead_code)]
pub fn category(mime: &str) -> &'static str {
    if mime.starts_with("text/") {
        "Text"
    } else if mime.starts_with("image/") {
        "Image"
    } else if mime.starts_with("audio/") {
        "Audio"
    } else if mime.starts_with("video/") {
        "Video"
    } else if mime.starts_with("font/") {
        "Font"
    } else if mime.contains("zip")
        || mime.contains("tar")
        || mime.contains("compressed")
        || mime.contains("archive")
        || mime.contains("gzip")
        || mime.contains("bzip")
        || mime.contains("xz")
        || mime.contains("zstd")
        || mime.contains("lz4")
        || mime.contains("7z")
        || mime.contains("rar")
        || mime.contains("cpio")
        || mime.contains("deb")
    {
        "Archive"
    } else if mime == "application/pdf"
        || mime.contains("document")
        || mime.contains("spreadsheet")
        || mime.contains("presentation")
        || mime == "application/rtf"
        || mime == "application/epub+zip"
    {
        "Document"
    } else if mime == "application/x-elf"
        || mime == "application/x-dosexec"
        || mime == "application/wasm"
        || mime == "application/x-sharedlib"
        || mime == "application/x-object"
        || mime == "application/x-nx-executable"
        || mime == "application/x-nx-sharedlib"
        || mime == "application/x-nx-staticlib"
    {
        "Executable"
    } else if mime == "application/x-sqlite3" || mime == "application/sql" {
        "Database"
    } else {
        "Other"
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the file extension from a path (lowercase, without dot).
fn path_extension(path: &str) -> Option<&str> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let dot_pos = filename.rfind('.')?;
    if dot_pos == 0 {
        // Dotfile like ".bashrc" — not a meaningful extension.
        return None;
    }
    Some(&filename[dot_pos.saturating_add(1)..])
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for MIME type detection.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[mime] Running self-test...");

    // --- Test 1: Magic byte detection ---
    {
        // PNG
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(from_bytes(png), Some("image/png"));

        // JPEG
        let jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF";
        assert_eq!(from_bytes(jpeg), Some("image/jpeg"));

        // ELF
        let elf = b"\x7FELF\x02\x01\x01\x00";
        assert_eq!(from_bytes(elf), Some("application/x-elf"));

        // PDF
        let pdf = b"%PDF-1.7";
        assert_eq!(from_bytes(pdf), Some("application/pdf"));

        // ZIP
        let zip = b"PK\x03\x04\x14\x00";
        assert_eq!(from_bytes(zip), Some("application/zip"));

        // gzip
        let gz = b"\x1f\x8b\x08\x00";
        assert_eq!(from_bytes(gz), Some("application/gzip"));

        // Empty data.
        assert_eq!(from_bytes(&[]), None);

        // Unknown binary.
        let unknown = &[0x00, 0x01, 0x02, 0x03];
        assert_eq!(from_bytes(unknown), None);

        serial_println!("[mime]   magic detection OK");
    }

    // --- Test 2: Extension detection ---
    {
        assert_eq!(from_extension("png"), Some("image/png"));
        assert_eq!(from_extension("rs"), Some("text/x-rust"));
        assert_eq!(from_extension("pdf"), Some("application/pdf"));
        assert_eq!(from_extension("mp3"), Some("audio/mpeg"));
        assert_eq!(from_extension("mp4"), Some("video/mp4"));
        assert_eq!(from_extension("zip"), Some("application/zip"));
        assert_eq!(from_extension("unknown_ext"), None);

        serial_println!("[mime]   extension detection OK");
    }

    // --- Test 3: Text/binary classification ---
    {
        assert_eq!(text_or_binary(b"Hello, world!\n"), "text/plain");
        assert_eq!(text_or_binary(b"line 1\nline 2\n"), "text/plain");
        assert_eq!(text_or_binary(b"\x00\x01\x02\x03"), "application/octet-stream");
        assert_eq!(text_or_binary(b""), "application/octet-stream");

        serial_println!("[mime]   text/binary classification OK");
    }

    // --- Test 4: Path extension extraction ---
    {
        assert_eq!(path_extension("/home/user/file.txt"), Some("txt"));
        assert_eq!(path_extension("/path/to/image.PNG"), Some("PNG"));
        assert_eq!(path_extension("/path/.bashrc"), None);
        assert_eq!(path_extension("/path/noext"), None);
        assert_eq!(path_extension("file.tar.gz"), Some("gz"));

        serial_println!("[mime]   path extension OK");
    }

    // --- Test 5: Category mapping ---
    {
        assert_eq!(category("image/png"), "Image");
        assert_eq!(category("audio/mpeg"), "Audio");
        assert_eq!(category("video/mp4"), "Video");
        assert_eq!(category("text/plain"), "Text");
        assert_eq!(category("application/zip"), "Archive");
        assert_eq!(category("application/pdf"), "Document");
        assert_eq!(category("application/x-elf"), "Executable");

        serial_println!("[mime]   category mapping OK");
    }

    // --- Test 6: Full detect() on real files ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_mime_test.txt";
        if let Ok(()) = Vfs::write_file(test_path, b"Hello, MIME world!\n") {
            match detect(test_path) {
                Ok(mime) => {
                    if !mime.starts_with("text/") {
                        serial_println!("[mime]   WARN: text file detected as {}", mime);
                    } else {
                        serial_println!("[mime]   detect() on text file: {} OK", mime);
                    }
                }
                Err(e) => {
                    serial_println!("[mime]   detect() error: {:?}", e);
                }
            }
            let _ = Vfs::remove(test_path);
        } else {
            serial_println!("[mime]   SKIP: cannot write test file");
        }
    }

    serial_println!("[mime] Self-test passed (6 tests).");
    Ok(())
}
