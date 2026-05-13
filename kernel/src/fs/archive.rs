//! Unified archive manager.
//!
//! Provides a single API for listing, extracting, and creating archives
//! across all supported formats: ZIP, TAR, CPIO, AR, RAR5, and 7z.
//!
//! ## Architecture
//!
//! ```text
//! archive::detect(data)   → ArchiveFormat
//! archive::list(data)     → Vec<ArchiveEntry>
//! archive::extract_all(data, "/dest")  → ExtractResult
//! archive::create(Zip, entries)        → Vec<u8>
//! ```
//!
//! Format detection uses magic bytes from the first few bytes of the
//! archive data.  All format-specific parsing is delegated to the
//! individual modules (zip, tar, cpio, ar, rar, sevenz).

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::Vfs;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Supported archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    Cpio,
    Ar,
    Rar,
    SevenZ,
}

impl ArchiveFormat {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Tar => "TAR",
            Self::Cpio => "CPIO",
            Self::Ar => "AR",
            Self::Rar => "RAR5",
            Self::SevenZ => "7z",
        }
    }

    /// Common file extensions.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Zip => &[".zip"],
            Self::Tar => &[".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz", ".tar.lz4", ".tar.zst"],
            Self::Cpio => &[".cpio"],
            Self::Ar => &[".a", ".ar", ".deb"],
            Self::Rar => &[".rar"],
            Self::SevenZ => &[".7z"],
        }
    }

    /// Whether this format supports creating archives.
    pub fn supports_create(self) -> bool {
        matches!(self, Self::Zip | Self::Tar | Self::Cpio | Self::Ar)
    }
}

/// Entry kind within an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// Unified archive entry metadata.
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    /// Name/path within the archive.
    pub name: String,
    /// Uncompressed file size.
    pub size: u64,
    /// Entry kind.
    pub kind: EntryKind,
    /// Modification time (Unix timestamp, seconds).
    pub mtime: u64,
    /// Unix permissions (0 if not available).
    pub mode: u32,
    /// UID (0 if not available).
    pub uid: u32,
    /// GID (0 if not available).
    pub gid: u32,
    /// Symlink target (empty if not a symlink).
    pub link_target: String,
}

/// Entry for creating an archive.
#[derive(Debug, Clone)]
pub struct CreateEntry {
    /// Name/path within the archive.
    pub name: String,
    /// File content (empty for directories).
    pub data: Vec<u8>,
    /// Entry kind.
    pub kind: EntryKind,
}

/// Result of extracting an archive.
#[derive(Debug, Clone, Default)]
pub struct ExtractResult {
    /// Files extracted.
    pub files_extracted: u64,
    /// Directories created.
    pub dirs_created: u64,
    /// Bytes written.
    pub bytes_written: u64,
    /// Non-fatal errors.
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static LISTS: AtomicU64 = AtomicU64::new(0);
static EXTRACTS: AtomicU64 = AtomicU64::new(0);
static CREATES: AtomicU64 = AtomicU64::new(0);

/// Get counters: (lists, extracts, creates).
pub fn stats() -> (u64, u64, u64) {
    (
        LISTS.load(Ordering::Relaxed),
        EXTRACTS.load(Ordering::Relaxed),
        CREATES.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detect archive format from data content using magic bytes.
pub fn detect(data: &[u8]) -> Option<ArchiveFormat> {
    if data.len() < 4 {
        return None;
    }

    // ZIP: PK\x03\x04 (local file header) or PK\x05\x06 (empty archive).
    if data.len() >= 4 && data[0] == b'P' && data[1] == b'K'
        && (data[2] == 3 || data[2] == 5) && (data[3] == 4 || data[3] == 6)
    {
        return Some(ArchiveFormat::Zip);
    }

    // RAR5: Rar!\x1a\x07\x01\x00
    if data.len() >= 8
        && data[0] == b'R' && data[1] == b'a' && data[2] == b'r'
        && data[3] == b'!' && data[4] == 0x1a && data[5] == 0x07
    {
        return Some(ArchiveFormat::Rar);
    }

    // 7z: 7z\xbc\xaf\x27\x1c
    if data.len() >= 6
        && data[0] == b'7' && data[1] == b'z'
        && data[2] == 0xbc && data[3] == 0xaf
        && data[4] == 0x27 && data[5] == 0x1c
    {
        return Some(ArchiveFormat::SevenZ);
    }

    // AR: "!<arch>\n"
    if data.len() >= 8 && &data[..8] == b"!<arch>\n" {
        return Some(ArchiveFormat::Ar);
    }

    // CPIO: "070701" or "070702" (SVR4 newc).
    if data.len() >= 6 && (&data[..6] == b"070701" || &data[..6] == b"070702") {
        return Some(ArchiveFormat::Cpio);
    }

    // TAR: check for USTAR magic at offset 257.
    if data.len() >= 263 && &data[257..262] == b"ustar" {
        return Some(ArchiveFormat::Tar);
    }

    // TAR fallback: check if first 512 bytes look like a tar header
    // (null-terminated filename at start, valid checksum).
    if data.len() >= 512 {
        // Check for null-terminated name field.
        let name_end = data[..100].iter().position(|&b| b == 0);
        if let Some(end) = name_end {
            if end > 0 && data[..end].iter().all(|&b| b >= 0x20 && b < 0x7f) {
                // Looks like printable ASCII filename — might be tar.
                return Some(ArchiveFormat::Tar);
            }
        }
    }

    None
}

/// Detect format from a file extension.
pub fn detect_from_extension(name: &str) -> Option<ArchiveFormat> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Some(ArchiveFormat::Zip)
    } else if lower.ends_with(".tar") || lower.ends_with(".tgz")
        || lower.ends_with(".tar.gz") || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tar.xz") || lower.ends_with(".tar.lz4")
        || lower.ends_with(".tar.zst")
    {
        Some(ArchiveFormat::Tar)
    } else if lower.ends_with(".cpio") {
        Some(ArchiveFormat::Cpio)
    } else if lower.ends_with(".a") || lower.ends_with(".ar") || lower.ends_with(".deb") {
        Some(ArchiveFormat::Ar)
    } else if lower.ends_with(".rar") {
        Some(ArchiveFormat::Rar)
    } else if lower.ends_with(".7z") {
        Some(ArchiveFormat::SevenZ)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// List entries in an archive (auto-detects format).
pub fn list(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let fmt = detect(data).ok_or(KernelError::NotSupported)?;
    list_format(data, fmt)
}

/// List entries in an archive with a specified format.
pub fn list_format(data: &[u8], fmt: ArchiveFormat) -> KernelResult<Vec<ArchiveEntry>> {
    let entries = match fmt {
        ArchiveFormat::Zip => list_zip(data)?,
        ArchiveFormat::Tar => list_tar(data)?,
        ArchiveFormat::Cpio => list_cpio(data)?,
        ArchiveFormat::Ar => list_ar(data)?,
        ArchiveFormat::Rar => list_rar(data)?,
        ArchiveFormat::SevenZ => list_7z(data)?,
    };

    LISTS.fetch_add(1, Ordering::Relaxed);
    Ok(entries)
}

fn list_zip(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let zip_entries = crate::fs::zip::parse(data)?;
    Ok(zip_entries.iter().map(|e| ArchiveEntry {
        name: e.name.clone(),
        size: e.uncompressed_size,
        kind: if e.is_dir { EntryKind::Directory } else { EntryKind::File },
        mtime: 0,
        mode: 0,
        uid: 0,
        gid: 0,
        link_target: String::new(),
    }).collect())
}

fn list_tar(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let tar_entries = crate::fs::tar::parse(data)?;
    Ok(tar_entries.iter().map(|e| {
        let kind = match e.kind {
            crate::fs::tar::EntryKind::File => EntryKind::File,
            crate::fs::tar::EntryKind::Directory => EntryKind::Directory,
            crate::fs::tar::EntryKind::Symlink => EntryKind::Symlink,
            _ => EntryKind::Other,
        };
        ArchiveEntry {
            name: e.name.clone(),
            size: e.size,
            kind,
            mtime: e.mtime,
            mode: e.mode,
            uid: e.uid,
            gid: e.gid,
            link_target: e.link_target.clone(),
        }
    }).collect())
}

fn list_cpio(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let cpio_entries = crate::fs::cpio::uncpio(data)?;
    Ok(cpio_entries.iter().map(|e| {
        let kind = match e.entry_type {
            crate::fs::cpio::CpioEntryType::File => EntryKind::File,
            crate::fs::cpio::CpioEntryType::Directory => EntryKind::Directory,
            crate::fs::cpio::CpioEntryType::Symlink => EntryKind::Symlink,
            _ => EntryKind::Other,
        };
        ArchiveEntry {
            name: e.name.clone(),
            size: e.data.len() as u64,
            kind,
            mtime: e.mtime as u64,
            mode: e.mode,
            uid: e.uid,
            gid: e.gid,
            link_target: e.link_target.clone(),
        }
    }).collect())
}

fn list_ar(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let ar_entries = crate::fs::ar::unar(data)?;
    Ok(ar_entries.iter().map(|e| ArchiveEntry {
        name: e.name.clone(),
        size: e.data.len() as u64,
        kind: EntryKind::File, // AR only has files.
        mtime: e.mtime,
        mode: e.mode,
        uid: e.uid,
        gid: e.gid,
        link_target: String::new(),
    }).collect())
}

fn list_rar(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let rar_entries = crate::fs::rar::parse(data)?;
    Ok(rar_entries.iter().map(|e| ArchiveEntry {
        name: e.name.clone(),
        size: e.unpacked_size,
        kind: if e.is_dir { EntryKind::Directory } else { EntryKind::File },
        mtime: e.mtime as u64,
        mode: 0,
        uid: 0,
        gid: 0,
        link_target: String::new(),
    }).collect())
}

fn list_7z(data: &[u8]) -> KernelResult<Vec<ArchiveEntry>> {
    let entries = crate::fs::sevenz::un7z(data)?;
    Ok(entries.iter().map(|e| ArchiveEntry {
        name: e.name.clone(),
        size: e.data.len() as u64,
        kind: if e.is_dir { EntryKind::Directory } else { EntryKind::File },
        mtime: 0,
        mode: 0,
        uid: 0,
        gid: 0,
        link_target: String::new(),
    }).collect())
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract a single entry from an archive by name.
pub fn extract_one(data: &[u8], name: &str) -> KernelResult<Vec<u8>> {
    let fmt = detect(data).ok_or(KernelError::NotSupported)?;
    extract_one_format(data, name, fmt)
}

/// Extract a single entry with specified format.
pub fn extract_one_format(data: &[u8], name: &str, fmt: ArchiveFormat) -> KernelResult<Vec<u8>> {
    match fmt {
        ArchiveFormat::Zip => {
            let entries = crate::fs::zip::parse(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            crate::fs::zip::extract_entry(data, entry)
        }
        ArchiveFormat::Tar => {
            let entries = crate::fs::tar::parse(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            crate::fs::tar::entry_data(data, entry).map(|s| s.to_vec())
        }
        ArchiveFormat::Cpio => {
            let entries = crate::fs::cpio::uncpio(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            Ok(entry.data.clone())
        }
        ArchiveFormat::Ar => {
            let entries = crate::fs::ar::unar(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            Ok(entry.data.clone())
        }
        ArchiveFormat::Rar => {
            let entries = crate::fs::rar::parse(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            crate::fs::rar::entry_data(data, entry).map(|s| s.to_vec())
        }
        ArchiveFormat::SevenZ => {
            let entries = crate::fs::sevenz::un7z(data)?;
            let entry = entries.iter().find(|e| e.name == name)
                .ok_or(KernelError::NotFound)?;
            Ok(entry.data.clone())
        }
    }
}

/// Extract all entries from an archive to a directory.
pub fn extract_all(data: &[u8], dest: &str) -> KernelResult<ExtractResult> {
    let fmt = detect(data).ok_or(KernelError::NotSupported)?;
    extract_all_format(data, dest, fmt)
}

/// Extract all entries with specified format.
pub fn extract_all_format(data: &[u8], dest: &str, fmt: ArchiveFormat) -> KernelResult<ExtractResult> {
    let entries = list_format(data, fmt)?;
    let mut result = ExtractResult::default();

    // Ensure destination exists.
    let _ = Vfs::mkdir(dest);

    // Create directories first (sorted for proper ordering).
    let mut dirs: Vec<&str> = entries.iter()
        .filter(|e| e.kind == EntryKind::Directory)
        .map(|e| e.name.as_str())
        .collect();
    dirs.sort();

    for dir in &dirs {
        let path = alloc::format!("{}/{}", dest, dir.trim_end_matches('/'));
        match Vfs::mkdir(&path) {
            Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
            Err(KernelError::AlreadyExists) => {}
            Err(e) => result.errors.push(alloc::format!("mkdir {}: {:?}", path, e)),
        }
    }

    // Extract files.
    for entry in &entries {
        if entry.kind != EntryKind::File {
            continue;
        }

        // Ensure parent directory exists.
        let path = alloc::format!("{}/{}", dest, entry.name);
        if let Some(last_slash) = path.rfind('/') {
            let parent = &path[..last_slash];
            if !parent.is_empty() {
                let _ = Vfs::mkdir(parent);
            }
        }

        match extract_one_format(data, &entry.name, fmt) {
            Ok(content) => {
                let bytes = content.len() as u64;
                match Vfs::write_file(&path, &content) {
                    Ok(()) => {
                        result.files_extracted = result.files_extracted.saturating_add(1);
                        result.bytes_written = result.bytes_written.saturating_add(bytes);
                    }
                    Err(e) => result.errors.push(alloc::format!("write {}: {:?}", path, e)),
                }
            }
            Err(e) => result.errors.push(alloc::format!("extract {}: {:?}", entry.name, e)),
        }
    }

    EXTRACTS.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[archive] Extract ({}): {} files, {} dirs, {} bytes, {} errors",
        fmt.label(), result.files_extracted, result.dirs_created,
        result.bytes_written, result.errors.len(),
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// Create an archive from entries.
pub fn create(fmt: ArchiveFormat, entries: &[CreateEntry]) -> KernelResult<Vec<u8>> {
    if !fmt.supports_create() {
        return Err(KernelError::NotSupported);
    }

    let data = match fmt {
        ArchiveFormat::Zip => create_zip(entries),
        ArchiveFormat::Tar => create_tar(entries),
        ArchiveFormat::Cpio => create_cpio(entries)?,
        ArchiveFormat::Ar => create_ar(entries)?,
        _ => return Err(KernelError::NotSupported),
    };

    CREATES.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[archive] Create ({}): {} entries, {} bytes",
        fmt.label(), entries.len(), data.len(),
    );

    Ok(data)
}

fn create_zip(entries: &[CreateEntry]) -> Vec<u8> {
    use crate::fs::zip::ZipWriteEntry;
    let zip_entries: Vec<ZipWriteEntry> = entries.iter().map(|e| {
        ZipWriteEntry {
            name: if e.kind == EntryKind::Directory && !e.name.ends_with('/') {
                alloc::format!("{}/", e.name)
            } else {
                e.name.clone()
            },
            data: e.data.clone(),
            store_only: false,
        }
    }).collect();
    crate::fs::zip::create(&zip_entries)
}

fn create_tar(entries: &[CreateEntry]) -> Vec<u8> {
    use crate::fs::tar::{TarWriteEntry, EntryKind as TarKind};
    let tar_entries: Vec<TarWriteEntry> = entries.iter().map(|e| {
        TarWriteEntry {
            name: if e.kind == EntryKind::Directory && !e.name.ends_with('/') {
                alloc::format!("{}/", e.name)
            } else {
                e.name.clone()
            },
            data: e.data.clone(),
            kind: match e.kind {
                EntryKind::File => TarKind::File,
                EntryKind::Directory => TarKind::Directory,
                EntryKind::Symlink => TarKind::Symlink,
                EntryKind::Other => TarKind::Other(0),
            },
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
            link_target: String::new(),
        }
    }).collect();
    crate::fs::tar::create(&tar_entries)
}

fn create_cpio(entries: &[CreateEntry]) -> KernelResult<Vec<u8>> {
    use crate::fs::cpio::{CpioEntry, CpioEntryType};
    let cpio_entries: Vec<CpioEntry> = entries.iter().map(|e| {
        CpioEntry {
            name: e.name.clone(),
            data: e.data.clone(),
            entry_type: match e.kind {
                EntryKind::File => CpioEntryType::File,
                EntryKind::Directory => CpioEntryType::Directory,
                EntryKind::Symlink => CpioEntryType::Symlink,
                EntryKind::Other => CpioEntryType::Unknown,
            },
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
            link_target: String::new(),
        }
    }).collect();
    crate::fs::cpio::mkcpio(&cpio_entries)
}

fn create_ar(entries: &[CreateEntry]) -> KernelResult<Vec<u8>> {
    use crate::fs::ar::ArEntry;
    let ar_entries: Vec<ArEntry> = entries.iter()
        .filter(|e| e.kind == EntryKind::File) // AR only supports files.
        .map(|e| {
            ArEntry {
                name: e.name.clone(),
                data: e.data.clone(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
            }
        }).collect();
    crate::fs::ar::mkar(&ar_entries)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[archive] Running self-test...");

    test_detect_zip();
    test_detect_tar();
    test_detect_cpio();
    test_detect_ar();
    test_detect_extension();
    test_zip_roundtrip();
    test_tar_roundtrip();
    test_stats();

    serial_println!("[archive] Self-test passed (8 tests).");
    Ok(())
}

fn test_detect_zip() {
    let data = [b'P', b'K', 3, 4, 0, 0, 0, 0];
    assert_eq!(detect(&data), Some(ArchiveFormat::Zip));
    serial_println!("[archive]   detect zip: ok");
}

fn test_detect_tar() {
    // USTAR magic at offset 257.
    let mut data = [0u8; 270];
    data[0] = b't'; data[1] = b'e'; data[2] = b's'; data[3] = b't';
    data[257] = b'u'; data[258] = b's'; data[259] = b't'; data[260] = b'a'; data[261] = b'r';
    assert_eq!(detect(&data), Some(ArchiveFormat::Tar));
    serial_println!("[archive]   detect tar: ok");
}

fn test_detect_cpio() {
    let data = b"070701001234";
    assert_eq!(detect(data), Some(ArchiveFormat::Cpio));
    serial_println!("[archive]   detect cpio: ok");
}

fn test_detect_ar() {
    let data = b"!<arch>\ntest";
    assert_eq!(detect(data), Some(ArchiveFormat::Ar));
    serial_println!("[archive]   detect ar: ok");
}

fn test_detect_extension() {
    assert_eq!(detect_from_extension("file.zip"), Some(ArchiveFormat::Zip));
    assert_eq!(detect_from_extension("file.tar.gz"), Some(ArchiveFormat::Tar));
    assert_eq!(detect_from_extension("pkg.deb"), Some(ArchiveFormat::Ar));
    assert_eq!(detect_from_extension("data.rar"), Some(ArchiveFormat::Rar));
    assert_eq!(detect_from_extension("data.7z"), Some(ArchiveFormat::SevenZ));
    assert_eq!(detect_from_extension("file.txt"), None);
    serial_println!("[archive]   detect extension: ok");
}

fn test_zip_roundtrip() {
    let entries = alloc::vec![
        CreateEntry {
            name: String::from("hello.txt"),
            data: b"Hello from archive!".to_vec(),
            kind: EntryKind::File,
        },
        CreateEntry {
            name: String::from("sub/"),
            data: Vec::new(),
            kind: EntryKind::Directory,
        },
    ];

    let archive = create(ArchiveFormat::Zip, &entries).expect("create zip");
    assert!(archive.len() > 0, "archive should have data");

    let listed = list(&archive).expect("list zip");
    assert!(listed.len() >= 1, "should list entries");
    assert!(listed.iter().any(|e| e.name == "hello.txt"), "should find hello.txt");

    let content = extract_one(&archive, "hello.txt").expect("extract");
    assert_eq!(&content, b"Hello from archive!");

    serial_println!("[archive]   zip roundtrip: ok");
}

fn test_tar_roundtrip() {
    let entries = alloc::vec![
        CreateEntry {
            name: String::from("data.txt"),
            data: b"TAR content".to_vec(),
            kind: EntryKind::File,
        },
    ];

    let archive = create(ArchiveFormat::Tar, &entries).expect("create tar");
    let listed = list(&archive).expect("list tar");
    assert!(listed.iter().any(|e| e.name.contains("data.txt")), "should find data.txt");

    let content = extract_one(&archive, "data.txt").expect("extract");
    assert_eq!(&content, b"TAR content");

    serial_println!("[archive]   tar roundtrip: ok");
}

fn test_stats() {
    let (lists, extracts, creates) = stats();
    assert!(lists > 0, "should have lists");
    assert!(creates > 0, "should have creates");
    let _ = extracts;

    serial_println!("[archive]   stats: ok");
}
