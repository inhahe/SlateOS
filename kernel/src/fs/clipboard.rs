//! System clipboard for cut/copy/paste operations.
//!
//! Provides a multi-format clipboard that applications use for data
//! exchange. Supports text, file paths (for file manager cut/copy),
//! image data, and custom MIME-typed data. The clipboard is shared
//! across all applications via kernel-managed storage.
//!
//! ## Architecture
//!
//! ```text
//! Application A (copy)
//!   → clipboard::set(formats)
//!   → stores data in kernel clipboard buffer
//!
//! Application B (paste)
//!   → clipboard::get(preferred_format)
//!   → receives data from buffer
//! ```
//!
//! ## Features
//!
//! - **Multi-format** — one copy can offer data in multiple formats
//! - **Text clipboard** — plain text and rich text
//! - **File operations** — cut/copy file paths for file manager
//! - **History** — configurable clipboard history (last N entries)
//! - **Notifications** — watchers notified on clipboard changes
//! - **Memory limits** — maximum per-entry and total clipboard size
//!
//! ## Design Notes
//!
//! - Maximum clipboard data size: 4 MiB per entry.
//! - History depth: 32 entries (configurable).
//! - Clipboard change sequence number for polling.
//! - File cut operations record source paths + cut flag.
//! - Thread-safe via spin::Mutex.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum data per clipboard entry (4 MiB).
const MAX_ENTRY_SIZE: usize = 4 * 1024 * 1024;

/// Maximum clipboard history depth.
const MAX_HISTORY: usize = 32;

/// Maximum formats per entry.
const MAX_FORMATS: usize = 8;

/// Maximum watchers.
const MAX_WATCHERS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Standard clipboard format identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain UTF-8 text.
    PlainText,
    /// HTML-formatted text.
    Html,
    /// File paths (for file manager copy/cut).
    FilePaths,
    /// Image data (raw RGBA pixels).
    ImageRgba,
    /// Image data (PNG encoded).
    ImagePng,
    /// Custom MIME type (identified by string).
    Custom,
}

impl Format {
    /// MIME type string.
    pub fn mime(self) -> &'static str {
        match self {
            Self::PlainText => "text/plain",
            Self::Html => "text/html",
            Self::FilePaths => "text/uri-list",
            Self::ImageRgba => "image/x-rgba",
            Self::ImagePng => "image/png",
            Self::Custom => "application/octet-stream",
        }
    }

    /// Parse from MIME type.
    pub fn from_mime(mime: &str) -> Self {
        match mime {
            "text/plain" => Self::PlainText,
            "text/html" => Self::Html,
            "text/uri-list" => Self::FilePaths,
            "image/x-rgba" => Self::ImageRgba,
            "image/png" => Self::ImagePng,
            _ => Self::Custom,
        }
    }

    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "Text",
            Self::Html => "HTML",
            Self::FilePaths => "Files",
            Self::ImageRgba => "Image (RGBA)",
            Self::ImagePng => "Image (PNG)",
            Self::Custom => "Custom",
        }
    }
}

/// File operation type for FilePaths format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    /// Files were copied (leave originals).
    Copy,
    /// Files were cut (delete originals after paste).
    Cut,
}

/// A single format+data pair in a clipboard entry.
#[derive(Debug, Clone)]
pub struct FormatData {
    /// Data format.
    pub format: Format,
    /// Custom MIME type (only used when format == Custom).
    pub custom_mime: String,
    /// Raw data bytes.
    pub data: Vec<u8>,
}

/// A clipboard entry (one copy operation can provide multiple formats).
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    /// Available formats with their data.
    pub formats: Vec<FormatData>,
    /// Source application/context.
    pub source: String,
    /// Timestamp when copied.
    pub timestamp_ns: u64,
    /// Sequence number.
    pub sequence: u64,
    /// For file operations: whether this is a cut or copy.
    pub file_op: Option<FileOp>,
}

/// A clipboard change watcher callback ID.
pub type WatcherId = u64;

/// Watcher entry.
struct Watcher {
    id: WatcherId,
    label: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Current clipboard content (most recent entry).
static CURRENT: spin::Mutex<Option<ClipboardEntry>> = spin::Mutex::new(None);

/// Clipboard history.
static HISTORY: spin::Mutex<Vec<ClipboardEntry>> = spin::Mutex::new(Vec::new());

/// Registered watchers.
static WATCHERS: spin::Mutex<Vec<Watcher>> = spin::Mutex::new(Vec::new());

/// Sequence counter (monotonically increasing).
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Next watcher ID.
static NEXT_WATCHER_ID: AtomicU64 = AtomicU64::new(1);

/// Statistics.
static COPY_COUNT: AtomicU64 = AtomicU64::new(0);
static PASTE_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Copy (Set clipboard)
// ---------------------------------------------------------------------------

/// Set clipboard to plain text.
pub fn set_text(text: &str, source: &str) -> KernelResult<()> {
    let data = Vec::from(text.as_bytes());
    set_single(Format::PlainText, data, source, None)
}

/// Set clipboard to file paths.
///
/// `paths` is a list of file paths. `op` is Copy or Cut.
pub fn set_files(paths: &[&str], op: FileOp, source: &str) -> KernelResult<()> {
    // Encode as newline-separated path list.
    let mut data = String::new();
    for (i, path) in paths.iter().enumerate() {
        if i > 0 { data.push('\n'); }
        data.push_str(path);
    }
    set_single(Format::FilePaths, Vec::from(data.as_bytes()), source, Some(op))
}

/// Set clipboard with a single format.
fn set_single(format: Format, data: Vec<u8>, source: &str, file_op: Option<FileOp>) -> KernelResult<()> {
    if data.len() > MAX_ENTRY_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let now = crate::timekeeping::clock_monotonic();

    COPY_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_BYTES.fetch_add(data.len() as u64, Ordering::Relaxed);

    let entry = ClipboardEntry {
        formats: vec![FormatData {
            format,
            custom_mime: String::new(),
            data,
        }],
        source: String::from(source),
        timestamp_ns: now,
        sequence: seq,
        file_op,
    };

    // Archive current to history before replacing.
    {
        let mut current = CURRENT.lock();
        if let Some(old) = current.take() {
            let mut history = HISTORY.lock();
            if history.len() >= MAX_HISTORY {
                history.remove(0);
            }
            history.push(old);
        }
        *current = Some(entry);
    }

    Ok(())
}

/// Set clipboard with multiple formats simultaneously.
pub fn set_multi(formats: Vec<FormatData>, source: &str, file_op: Option<FileOp>) -> KernelResult<()> {
    if formats.len() > MAX_FORMATS {
        return Err(KernelError::InvalidArgument);
    }
    let total_size: usize = formats.iter().map(|f| f.data.len()).sum();
    if total_size > MAX_ENTRY_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let now = crate::timekeeping::clock_monotonic();

    COPY_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_BYTES.fetch_add(total_size as u64, Ordering::Relaxed);

    let entry = ClipboardEntry {
        formats,
        source: String::from(source),
        timestamp_ns: now,
        sequence: seq,
        file_op,
    };

    let mut current = CURRENT.lock();
    if let Some(old) = current.take() {
        let mut history = HISTORY.lock();
        if history.len() >= MAX_HISTORY {
            history.remove(0);
        }
        history.push(old);
    }
    *current = Some(entry);

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — Paste (Get clipboard)
// ---------------------------------------------------------------------------

/// Get clipboard as plain text.
pub fn get_text() -> Option<String> {
    PASTE_COUNT.fetch_add(1, Ordering::Relaxed);
    let current = CURRENT.lock();
    let entry = current.as_ref()?;

    // Look for PlainText format first.
    for fd in &entry.formats {
        if fd.format == Format::PlainText {
            return core::str::from_utf8(&fd.data).ok().map(String::from);
        }
    }

    // Fall back to any text format.
    for fd in &entry.formats {
        if fd.format == Format::Html || fd.format == Format::FilePaths {
            return core::str::from_utf8(&fd.data).ok().map(String::from);
        }
    }

    None
}

/// Get clipboard file paths.
pub fn get_files() -> Option<(Vec<String>, FileOp)> {
    PASTE_COUNT.fetch_add(1, Ordering::Relaxed);
    let current = CURRENT.lock();
    let entry = current.as_ref()?;
    let op = entry.file_op.unwrap_or(FileOp::Copy);

    for fd in &entry.formats {
        if fd.format == Format::FilePaths {
            let text = core::str::from_utf8(&fd.data).ok()?;
            let paths: Vec<String> = text.lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
            return Some((paths, op));
        }
    }

    None
}

/// Get clipboard data in a specific format.
pub fn get_format(format: Format) -> Option<Vec<u8>> {
    PASTE_COUNT.fetch_add(1, Ordering::Relaxed);
    let current = CURRENT.lock();
    let entry = current.as_ref()?;

    entry.formats.iter()
        .find(|fd| fd.format == format)
        .map(|fd| fd.data.clone())
}

/// Check which formats are available.
pub fn available_formats() -> Vec<Format> {
    let current = CURRENT.lock();
    match current.as_ref() {
        Some(entry) => entry.formats.iter().map(|fd| fd.format).collect(),
        None => Vec::new(),
    }
}

/// Get current sequence number (for polling changes).
pub fn sequence() -> u64 {
    SEQUENCE.load(Ordering::Relaxed)
}

/// Check if clipboard has content.
pub fn is_empty() -> bool {
    CURRENT.lock().is_none()
}

// ---------------------------------------------------------------------------
// Public API — History
// ---------------------------------------------------------------------------

/// Get clipboard history (oldest first).
pub fn history() -> Vec<ClipboardEntry> {
    HISTORY.lock().clone()
}

/// Get history entry by index (0 = oldest).
pub fn history_entry(index: usize) -> Option<ClipboardEntry> {
    let hist = HISTORY.lock();
    hist.get(index).cloned()
}

/// Restore a history entry to current clipboard.
pub fn restore_from_history(index: usize) -> KernelResult<()> {
    let entry = {
        let hist = HISTORY.lock();
        hist.get(index).cloned()
    };

    match entry {
        Some(e) => {
            set_multi(e.formats, &e.source, e.file_op)?;
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Get history depth.
pub fn history_count() -> usize {
    HISTORY.lock().len()
}

/// Clear history.
pub fn clear_history() {
    HISTORY.lock().clear();
}

// ---------------------------------------------------------------------------
// Public API — Watchers
// ---------------------------------------------------------------------------

/// Register a clipboard change watcher.
///
/// Returns a watcher ID for unregistration. In the current kernel-space
/// implementation, watchers are labels only — actual notification would
/// be via IPC in userspace.
pub fn watch(label: &str) -> KernelResult<WatcherId> {
    let mut watchers = WATCHERS.lock();
    if watchers.len() >= MAX_WATCHERS {
        return Err(KernelError::OutOfMemory);
    }
    let id = NEXT_WATCHER_ID.fetch_add(1, Ordering::Relaxed);
    watchers.push(Watcher {
        id,
        label: String::from(label),
    });
    Ok(id)
}

/// Unregister a watcher.
pub fn unwatch(id: WatcherId) -> bool {
    let mut watchers = WATCHERS.lock();
    let len_before = watchers.len();
    watchers.retain(|w| w.id != id);
    watchers.len() < len_before
}

/// List registered watchers.
pub fn list_watchers() -> Vec<(WatcherId, String)> {
    let watchers = WATCHERS.lock();
    watchers.iter().map(|w| (w.id, w.label.clone())).collect()
}

// ---------------------------------------------------------------------------
// Public API — Management
// ---------------------------------------------------------------------------

/// Clear the clipboard.
pub fn clear() {
    *CURRENT.lock() = None;
}

/// Clear everything (clipboard + history).
pub fn clear_all() {
    *CURRENT.lock() = None;
    HISTORY.lock().clear();
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get statistics.
pub fn stats() -> (u64, u64, u64, u64, usize, usize) {
    let hist_count = HISTORY.lock().len();
    let watcher_count = WATCHERS.lock().len();
    (
        COPY_COUNT.load(Ordering::Relaxed),
        PASTE_COUNT.load(Ordering::Relaxed),
        TOTAL_BYTES.load(Ordering::Relaxed),
        SEQUENCE.load(Ordering::Relaxed),
        hist_count,
        watcher_count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    COPY_COUNT.store(0, Ordering::Relaxed);
    PASTE_COUNT.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[clipboard] Running self-test...");

    test_text_copy_paste();
    test_file_copy();
    test_history();
    test_multi_format();
    test_watchers();
    test_clear();

    serial_println!("[clipboard] Self-test passed (6 tests).");
    Ok(())
}

fn test_text_copy_paste() {
    clear_all();

    set_text("Hello, world!", "test").unwrap();
    assert!(!is_empty());

    let text = get_text();
    assert_eq!(text.as_deref(), Some("Hello, world!"));

    let formats = available_formats();
    assert!(formats.contains(&Format::PlainText));

    clear_all();
    serial_println!("[clipboard]   text_copy_paste: ok");
}

fn test_file_copy() {
    clear_all();

    let paths = ["/home/user/file1.txt", "/home/user/file2.txt"];
    set_files(&paths, FileOp::Cut, "explorer").unwrap();

    let (files, op) = get_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], "/home/user/file1.txt");
    assert_eq!(op, FileOp::Cut);

    clear_all();
    serial_println!("[clipboard]   file_copy: ok");
}

fn test_history() {
    clear_all();

    set_text("first", "test").unwrap();
    set_text("second", "test").unwrap();
    set_text("third", "test").unwrap();

    // Current should be "third".
    assert_eq!(get_text().as_deref(), Some("third"));

    // History should have "first" and "second".
    assert_eq!(history_count(), 2);

    let hist = history();
    let first_text = core::str::from_utf8(&hist[0].formats[0].data).unwrap();
    assert_eq!(first_text, "first");

    clear_all();
    serial_println!("[clipboard]   history: ok");
}

fn test_multi_format() {
    clear_all();

    let text_data = Vec::from("Hello".as_bytes());
    let html_data = Vec::from("<b>Hello</b>".as_bytes());

    let formats = vec![
        FormatData {
            format: Format::PlainText,
            custom_mime: String::new(),
            data: text_data,
        },
        FormatData {
            format: Format::Html,
            custom_mime: String::new(),
            data: html_data,
        },
    ];

    set_multi(formats, "test", None).unwrap();

    let avail = available_formats();
    assert_eq!(avail.len(), 2);
    assert!(avail.contains(&Format::PlainText));
    assert!(avail.contains(&Format::Html));

    let text = get_text();
    assert_eq!(text.as_deref(), Some("Hello"));

    clear_all();
    serial_println!("[clipboard]   multi_format: ok");
}

fn test_watchers() {
    let id = watch("test_watcher").unwrap();
    let watchers = list_watchers();
    assert_eq!(watchers.len(), 1);
    assert_eq!(watchers[0].0, id);

    assert!(unwatch(id));
    assert!(list_watchers().is_empty());

    serial_println!("[clipboard]   watchers: ok");
}

fn test_clear() {
    set_text("data", "test").unwrap();
    assert!(!is_empty());

    clear();
    assert!(is_empty());

    serial_println!("[clipboard]   clear: ok");
}
