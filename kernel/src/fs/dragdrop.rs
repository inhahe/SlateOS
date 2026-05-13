//! OLE-style drag-and-drop data transfer system.
//!
//! Implements a multi-format data transfer protocol for drag-and-drop
//! operations between applications. Sources offer data in multiple formats
//! (text, HTML, file paths, images, custom MIME types) and targets pick
//! the format they understand — exactly like Windows OLE drag-and-drop.
//!
//! ## Architecture
//!
//! ```text
//! Source application
//!   → dragdrop::begin_drag(session) — registers offered formats
//!   → kernel tracks active drag session
//!
//! Compositor
//!   → dragdrop::update_position(x, y) — tracks cursor during drag
//!   → dragdrop::current_session() — for rendering drag feedback
//!
//! Target application
//!   → dragdrop::query_formats() — check what's available
//!   → dragdrop::accept(format) — request data in preferred format
//!   → dragdrop::complete() / dragdrop::cancel()
//! ```
//!
//! ## Drop Zones (per design spec)
//!
//! File explorer drop behavior:
//! - Empty space in file list → copy/move to this directory
//! - On a folder → copy/move into that folder
//! - On a file → open file with that program (if executable)
//! - Clear visual indicator (highlighted drop zone) for user feedback
//!
//! ## Design Notes
//!
//! - One active drag session at a time (mouse-driven, single pointer).
//! - Data is lazily provided: source registers format list, actual data
//!   is fetched only when the target accepts a specific format.
//! - File drags record source paths + operation (copy/move).
//! - Cross-application drags go through kernel-managed session state.
//! - Same FormatData model as clipboard for consistency.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum data per format in a drag session (4 MiB).
const MAX_DATA_SIZE: usize = 4 * 1024 * 1024;

/// Maximum formats offered per drag session.
const MAX_FORMATS: usize = 8;

/// Maximum registered drop zones per application.
const MAX_DROP_ZONES: usize = 256;

/// Maximum number of files in a single file drag.
const MAX_DRAG_FILES: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Data format identifier for drag-and-drop transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragFormat {
    /// Plain UTF-8 text.
    PlainText,
    /// HTML-formatted text.
    Html,
    /// URI list (file paths, one per line).
    FilePaths,
    /// Raw RGBA image data.
    ImageRgba,
    /// PNG-encoded image.
    ImagePng,
    /// Serialized widget/object data (application-specific).
    Widget,
    /// Custom MIME type.
    Custom,
}

impl DragFormat {
    /// MIME type string.
    pub fn mime(self) -> &'static str {
        match self {
            Self::PlainText => "text/plain",
            Self::Html => "text/html",
            Self::FilePaths => "text/uri-list",
            Self::ImageRgba => "image/x-rgba",
            Self::ImagePng => "image/png",
            Self::Widget => "application/x-widget",
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
            "application/x-widget" => Self::Widget,
            _ => Self::Custom,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "text",
            Self::Html => "html",
            Self::FilePaths => "files",
            Self::ImageRgba => "rgba",
            Self::ImagePng => "png",
            Self::Widget => "widget",
            Self::Custom => "custom",
        }
    }
}

/// The intended file operation when dragging files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragFileOp {
    /// Copy files to the drop target location.
    Copy,
    /// Move files (delete from source after drop).
    Move,
    /// Create a link/symlink at the drop target.
    Link,
}

impl DragFileOp {
    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }
}

/// Visual feedback effect shown during drag-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropEffect {
    /// Drop not accepted here.
    None,
    /// Will copy data.
    Copy,
    /// Will move data.
    Move,
    /// Will create a link.
    Link,
}

/// What kind of target area the cursor is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZoneKind {
    /// Empty space in a file list (copy/move to directory).
    FileListEmpty,
    /// Over a folder entry (copy/move into it).
    FolderEntry,
    /// Over a file entry (open with, if target is executable).
    FileEntry,
    /// Over a text input field (paste text).
    TextInput,
    /// Over an image area (paste image).
    ImageArea,
    /// Custom application-defined drop zone.
    Custom,
}

/// A format + data pair offered by the drag source.
#[derive(Debug, Clone)]
pub struct DragData {
    /// Format of this data slice.
    pub format: DragFormat,
    /// Custom MIME (only when format == Custom).
    pub custom_mime: String,
    /// Raw data bytes (may be empty if lazy — filled on accept).
    pub data: Vec<u8>,
}

/// A registered drop zone within an application window.
#[derive(Debug, Clone)]
pub struct DropZone {
    /// Unique ID for this zone.
    pub id: u64,
    /// Owner application/window.
    pub owner: String,
    /// Human-readable label.
    pub label: String,
    /// Kind of zone (affects drop behavior).
    pub kind: DropZoneKind,
    /// Bounding rectangle: (x, y, width, height).
    pub bounds: (u32, u32, u32, u32),
    /// Accepted formats (empty = accept all).
    pub accepted_formats: Vec<DragFormat>,
    /// Whether this zone is currently active.
    pub active: bool,
}

/// Current state of a drag-and-drop session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    /// No drag in progress.
    Idle,
    /// Drag started, cursor moving.
    Dragging,
    /// Cursor is over a valid drop zone.
    OverTarget,
    /// Drop was accepted by target, transferring data.
    Dropping,
    /// Session completed successfully.
    Completed,
    /// Session was cancelled.
    Cancelled,
}

/// A complete drag-and-drop session.
#[derive(Debug, Clone)]
pub struct DragSession {
    /// Unique session ID.
    pub id: u64,
    /// Source application identifier.
    pub source: String,
    /// Current state.
    pub state: DragState,
    /// Offered data formats (format list known at start).
    pub offered_formats: Vec<DragFormat>,
    /// Actual data for each format (populated lazily or eagerly).
    pub data: Vec<DragData>,
    /// For file drags: the intended operation.
    pub file_op: Option<DragFileOp>,
    /// For file drags: source file paths.
    pub file_paths: Vec<String>,
    /// Current cursor position (x, y).
    pub cursor: (i32, i32),
    /// The drop zone currently under cursor (if any).
    pub current_zone: Option<u64>,
    /// Visual feedback effect.
    pub effect: DropEffect,
    /// Timestamp when drag started (ns).
    pub started_ns: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
static ZONE_COUNTER: AtomicU64 = AtomicU64::new(1);
static DRAG_COUNT: AtomicU64 = AtomicU64::new(0);
static DROP_COUNT: AtomicU64 = AtomicU64::new(0);
static CANCEL_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

static ACTIVE_SESSION: spin::Mutex<Option<DragSession>> = spin::Mutex::new(None);
static DROP_ZONES: spin::Mutex<Vec<DropZone>> = spin::Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Session management
// ---------------------------------------------------------------------------

/// Begin a new drag-and-drop session.
///
/// The source declares which formats it can provide. Data for each format
/// can be supplied eagerly (now) or lazily (when the target accepts).
pub fn begin_drag(
    source: &str,
    offered_formats: &[DragFormat],
    data: Vec<DragData>,
    file_op: Option<DragFileOp>,
    file_paths: &[&str],
) -> KernelResult<u64> {
    if offered_formats.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if offered_formats.len() > MAX_FORMATS {
        return Err(KernelError::InvalidArgument);
    }
    if file_paths.len() > MAX_DRAG_FILES {
        return Err(KernelError::InvalidArgument);
    }
    // Validate data sizes.
    for d in &data {
        if d.data.len() > MAX_DATA_SIZE {
            return Err(KernelError::InvalidArgument);
        }
    }

    let mut session_guard = ACTIVE_SESSION.lock();
    if let Some(ref s) = *session_guard {
        if s.state == DragState::Dragging || s.state == DragState::OverTarget {
            // Another drag is already active — reject.
            return Err(KernelError::WouldBlock);
        }
    }

    let id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let session = DragSession {
        id,
        source: String::from(source),
        state: DragState::Dragging,
        offered_formats: offered_formats.to_vec(),
        data,
        file_op,
        file_paths: file_paths.iter().map(|p| String::from(*p)).collect(),
        cursor: (0, 0),
        current_zone: None,
        effect: DropEffect::None,
        started_ns: now,
    };

    *session_guard = Some(session);
    DRAG_COUNT.fetch_add(1, Ordering::Relaxed);

    Ok(id)
}

/// Begin a file drag (convenience wrapper).
pub fn begin_file_drag(
    source: &str,
    paths: &[&str],
    op: DragFileOp,
) -> KernelResult<u64> {
    if paths.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Build URI list data for the FilePaths format.
    let mut uri_data = String::new();
    for p in paths {
        uri_data.push_str(p);
        uri_data.push('\n');
    }

    let data = vec![DragData {
        format: DragFormat::FilePaths,
        custom_mime: String::new(),
        data: uri_data.into_bytes(),
    }];

    begin_drag(source, &[DragFormat::FilePaths], data, Some(op), paths)
}

/// Begin a text drag (convenience wrapper).
pub fn begin_text_drag(source: &str, text: &str) -> KernelResult<u64> {
    let data = vec![DragData {
        format: DragFormat::PlainText,
        custom_mime: String::new(),
        data: Vec::from(text.as_bytes()),
    }];

    begin_drag(source, &[DragFormat::PlainText], data, None, &[])
}

/// Update the cursor position during a drag.
///
/// Returns the drop zone ID if the cursor is over one, and the
/// visual feedback effect.
pub fn update_position(x: i32, y: i32) -> Option<(u64, DropEffect)> {
    let mut session_guard = ACTIVE_SESSION.lock();
    let session = session_guard.as_mut()?;

    if session.state != DragState::Dragging && session.state != DragState::OverTarget {
        return None;
    }

    session.cursor = (x, y);

    // Check all zones for hit-test.
    let zones = DROP_ZONES.lock();
    let mut hit_zone: Option<(u64, DropEffect)> = None;

    for zone in zones.iter() {
        if !zone.active {
            continue;
        }
        let (zx, zy, zw, zh) = zone.bounds;
        let x_u = x as u32;
        let y_u = y as u32;
        if x >= 0 && y >= 0 && x_u >= zx && x_u < zx.saturating_add(zw)
            && y_u >= zy && y_u < zy.saturating_add(zh)
        {
            // Check format compatibility.
            let compatible = zone.accepted_formats.is_empty()
                || session.offered_formats.iter()
                    .any(|f| zone.accepted_formats.contains(f));

            if compatible {
                let effect = determine_effect(session, zone);
                hit_zone = Some((zone.id, effect));
                break; // First matching zone wins (front-to-back).
            }
        }
    }

    if let Some((zone_id, effect)) = hit_zone {
        session.state = DragState::OverTarget;
        session.current_zone = Some(zone_id);
        session.effect = effect;
        Some((zone_id, effect))
    } else {
        session.state = DragState::Dragging;
        session.current_zone = None;
        session.effect = DropEffect::None;
        None
    }
}

/// Determine the visual feedback effect for a zone.
fn determine_effect(session: &DragSession, zone: &DropZone) -> DropEffect {
    match zone.kind {
        DropZoneKind::FileListEmpty | DropZoneKind::FolderEntry => {
            match session.file_op {
                Some(DragFileOp::Copy) => DropEffect::Copy,
                Some(DragFileOp::Move) => DropEffect::Move,
                Some(DragFileOp::Link) => DropEffect::Link,
                None => DropEffect::Copy,
            }
        }
        DropZoneKind::FileEntry => DropEffect::Link,
        DropZoneKind::TextInput | DropZoneKind::ImageArea => DropEffect::Copy,
        DropZoneKind::Custom => DropEffect::Copy,
    }
}

/// Query available formats in the current drag session.
pub fn query_formats() -> Vec<DragFormat> {
    let session_guard = ACTIVE_SESSION.lock();
    match session_guard.as_ref() {
        Some(s) => s.offered_formats.clone(),
        None => Vec::new(),
    }
}

/// Accept the drop and retrieve data in the specified format.
///
/// This completes the drag session. The target gets the data and the
/// session transitions to `Completed`.
pub fn accept(format: DragFormat) -> KernelResult<Vec<u8>> {
    let mut session_guard = ACTIVE_SESSION.lock();
    let session = session_guard.as_mut()
        .ok_or(KernelError::NotFound)?;

    if session.state != DragState::OverTarget && session.state != DragState::Dragging {
        return Err(KernelError::InvalidArgument);
    }

    if !session.offered_formats.contains(&format) {
        return Err(KernelError::InvalidArgument);
    }

    session.state = DragState::Dropping;

    // Find data for the requested format.
    let data = session.data.iter()
        .find(|d| d.format == format)
        .map(|d| d.data.clone())
        .unwrap_or_default();

    let byte_count = data.len() as u64;
    TOTAL_BYTES.fetch_add(byte_count, Ordering::Relaxed);
    DROP_COUNT.fetch_add(1, Ordering::Relaxed);

    session.state = DragState::Completed;

    Ok(data)
}

/// Accept a file drop and retrieve paths + operation.
pub fn accept_files() -> KernelResult<(Vec<String>, DragFileOp)> {
    let mut session_guard = ACTIVE_SESSION.lock();
    let session = session_guard.as_mut()
        .ok_or(KernelError::NotFound)?;

    if session.state != DragState::OverTarget && session.state != DragState::Dragging {
        return Err(KernelError::InvalidArgument);
    }

    let op = session.file_op.ok_or(KernelError::InvalidArgument)?;
    let paths = session.file_paths.clone();

    session.state = DragState::Completed;
    DROP_COUNT.fetch_add(1, Ordering::Relaxed);

    Ok((paths, op))
}

/// Cancel the current drag session.
pub fn cancel() -> bool {
    let mut session_guard = ACTIVE_SESSION.lock();
    if let Some(ref mut s) = *session_guard {
        if s.state == DragState::Dragging || s.state == DragState::OverTarget {
            s.state = DragState::Cancelled;
            CANCEL_COUNT.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }
    false
}

/// Complete and clear the session (after accept or cancel).
pub fn finish() {
    let mut session_guard = ACTIVE_SESSION.lock();
    if let Some(ref s) = *session_guard {
        if s.state == DragState::Completed || s.state == DragState::Cancelled {
            *session_guard = None;
        }
    }
}

/// Get a snapshot of the current drag session (for rendering).
pub fn current_session() -> Option<DragSession> {
    ACTIVE_SESSION.lock().clone()
}

/// Check if a drag is in progress.
pub fn is_dragging() -> bool {
    let session_guard = ACTIVE_SESSION.lock();
    match session_guard.as_ref() {
        Some(s) => s.state == DragState::Dragging || s.state == DragState::OverTarget,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Drop zone registration
// ---------------------------------------------------------------------------

/// Register a new drop zone.
pub fn register_zone(
    owner: &str,
    label: &str,
    kind: DropZoneKind,
    bounds: (u32, u32, u32, u32),
    accepted_formats: &[DragFormat],
) -> KernelResult<u64> {
    let mut zones = DROP_ZONES.lock();
    if zones.len() >= MAX_DROP_ZONES {
        return Err(KernelError::OutOfMemory);
    }

    let id = ZONE_COUNTER.fetch_add(1, Ordering::Relaxed);
    zones.push(DropZone {
        id,
        owner: String::from(owner),
        label: String::from(label),
        kind,
        bounds,
        accepted_formats: accepted_formats.to_vec(),
        active: true,
    });

    Ok(id)
}

/// Unregister a drop zone by ID.
pub fn unregister_zone(id: u64) -> bool {
    let mut zones = DROP_ZONES.lock();
    let before = zones.len();
    zones.retain(|z| z.id != id);
    zones.len() < before
}

/// Unregister all drop zones owned by a specific owner.
pub fn unregister_owner_zones(owner: &str) -> usize {
    let mut zones = DROP_ZONES.lock();
    let before = zones.len();
    zones.retain(|z| z.owner != owner);
    before.saturating_sub(zones.len())
}

/// Set active/inactive state for a drop zone.
pub fn set_zone_active(id: u64, active: bool) -> bool {
    let mut zones = DROP_ZONES.lock();
    for z in zones.iter_mut() {
        if z.id == id {
            z.active = active;
            return true;
        }
    }
    false
}

/// List all registered drop zones.
pub fn list_zones() -> Vec<DropZone> {
    DROP_ZONES.lock().clone()
}

/// Get a specific zone by ID.
pub fn get_zone(id: u64) -> Option<DropZone> {
    DROP_ZONES.lock().iter().find(|z| z.id == id).cloned()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (drags, drops, cancels, total_bytes, zone_count).
pub fn stats() -> (u64, u64, u64, u64, usize) {
    let zone_count = DROP_ZONES.lock().len();
    (
        DRAG_COUNT.load(Ordering::Relaxed),
        DROP_COUNT.load(Ordering::Relaxed),
        CANCEL_COUNT.load(Ordering::Relaxed),
        TOTAL_BYTES.load(Ordering::Relaxed),
        zone_count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    DRAG_COUNT.store(0, Ordering::Relaxed);
    DROP_COUNT.store(0, Ordering::Relaxed);
    CANCEL_COUNT.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the drag-and-drop subsystem.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: text drag session lifecycle.
    {
        let id = begin_text_drag("test", "hello world")?;
        assert!(id > 0);
        assert!(is_dragging());
        let fmts = query_formats();
        assert_eq!(fmts.len(), 1);
        assert_eq!(fmts[0], DragFormat::PlainText);
        let data = accept(DragFormat::PlainText)?;
        assert_eq!(core::str::from_utf8(&data).unwrap_or(""), "hello world");
        assert!(!is_dragging());
        finish();
        serial_println!("[dragdrop] test 1 passed: text drag lifecycle");
    }

    // Test 2: file drag session.
    {
        let paths = ["/home/user/doc.txt", "/home/user/img.png"];
        let id = begin_file_drag("explorer", &paths, DragFileOp::Copy)?;
        assert!(id > 0);
        assert!(is_dragging());
        let (files, op) = accept_files()?;
        assert_eq!(files.len(), 2);
        assert_eq!(op, DragFileOp::Copy);
        finish();
        serial_println!("[dragdrop] test 2 passed: file drag lifecycle");
    }

    // Test 3: cancel drag.
    {
        let _id = begin_text_drag("test", "cancel me")?;
        assert!(is_dragging());
        assert!(cancel());
        assert!(!is_dragging());
        finish();
        serial_println!("[dragdrop] test 3 passed: cancel drag");
    }

    // Test 4: drop zone registration.
    {
        let zone_id = register_zone(
            "explorer", "file-list",
            DropZoneKind::FileListEmpty,
            (0, 0, 800, 600),
            &[DragFormat::FilePaths],
        )?;
        assert!(zone_id > 0);
        let zones = list_zones();
        let found = zones.iter().any(|z| z.id == zone_id);
        assert!(found);
        assert!(unregister_zone(zone_id));
        serial_println!("[dragdrop] test 4 passed: zone registration");
    }

    // Test 5: position tracking + zone hit test.
    {
        let zone_id = register_zone(
            "explorer", "drop-area",
            DropZoneKind::FileListEmpty,
            (100, 100, 400, 300),
            &[],
        )?;
        let _id = begin_text_drag("test", "drag data")?;

        // Cursor outside zone.
        let result = update_position(50, 50);
        assert!(result.is_none());

        // Cursor inside zone.
        let result = update_position(200, 200);
        assert!(result.is_some());
        if let Some((zid, _effect)) = result {
            assert_eq!(zid, zone_id);
        }

        cancel();
        finish();
        unregister_zone(zone_id);
        serial_println!("[dragdrop] test 5 passed: position + hit test");
    }

    // Test 6: reject duplicate drag session.
    {
        let _id1 = begin_text_drag("test", "first")?;
        let result = begin_text_drag("test", "second");
        assert!(result.is_err());
        cancel();
        finish();
        serial_println!("[dragdrop] test 6 passed: reject duplicate session");
    }

    // Test 7: stats tracking.
    {
        let (drags, drops, cancels, _, _) = stats();
        assert!(drags > 0);
        assert!(drops > 0);
        assert!(cancels > 0);
        serial_println!("[dragdrop] test 7 passed: stats tracking");
    }

    serial_println!("[dragdrop] all 7 self-tests passed");
    Ok(())
}
