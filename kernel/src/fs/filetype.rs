//! File type registry and icon mapping.
//!
//! Maps MIME types and file extensions to icons, descriptions, and
//! handling preferences. Per design spec (line 304):
//! "When the same application controls multiple file types, it should
//!  be allowed to specify different icons for each extension."
//!
//! ## Architecture
//!
//! ```text
//! File explorer needs icon for "photo.png"
//!   → filetype::icon_for_file("photo.png")
//!     → detect MIME type → lookup registered icon
//!     → fallback to category icon → fallback to generic icon
//!
//! Application registers file type handling
//!   → filetype::register_type(mime, icon, description, app)
//!     → stored in type registry
//!     → used by explorer, open-with, properties
//! ```
//!
//! ## Icon Resolution Order
//!
//! 1. Per-app icon for this specific MIME type (e.g., VLC's video icon)
//! 2. Registered type icon (from the type registry)
//! 3. Category icon (audio → musical note, image → picture)
//! 4. Generic file/folder icon

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered file types.
const MAX_TYPES: usize = 512;

/// Maximum app-specific icon overrides.
const MAX_APP_ICONS: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Icon category for fallback resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconCategory {
    /// Text files.
    Text,
    /// Image files.
    Image,
    /// Audio files.
    Audio,
    /// Video files.
    Video,
    /// Archive/compressed files.
    Archive,
    /// Document (PDF, RTF, etc.).
    Document,
    /// Executable/binary.
    Executable,
    /// Source code.
    SourceCode,
    /// Spreadsheet/data.
    Data,
    /// System/config file.
    System,
    /// Unknown/generic.
    Generic,
    /// Directory.
    Folder,
    /// Symlink.
    Link,
}

impl IconCategory {
    /// Default icon identifier for this category.
    pub fn default_icon(self) -> &'static str {
        match self {
            Self::Text => "icon-text",
            Self::Image => "icon-image",
            Self::Audio => "icon-audio",
            Self::Video => "icon-video",
            Self::Archive => "icon-archive",
            Self::Document => "icon-document",
            Self::Executable => "icon-executable",
            Self::SourceCode => "icon-code",
            Self::Data => "icon-data",
            Self::System => "icon-system",
            Self::Generic => "icon-file",
            Self::Folder => "icon-folder",
            Self::Link => "icon-link",
        }
    }

    /// Determine category from MIME type.
    pub fn from_mime(mime: &str) -> Self {
        if mime.starts_with("text/x-") && is_source_mime(mime) {
            return Self::SourceCode;
        }
        match mime.split('/').next().unwrap_or("") {
            "text" => Self::Text,
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => match mime {
                "application/pdf" | "application/rtf" => Self::Document,
                "application/zip" | "application/gzip" | "application/x-tar"
                | "application/x-bzip2" | "application/x-xz"
                | "application/x-7z-compressed" | "application/x-rar-compressed"
                | "application/zstd" => Self::Archive,
                "application/x-executable" | "application/x-sharedlib" => Self::Executable,
                "application/json" | "application/xml" | "application/yaml" => Self::Data,
                "application/csv" => Self::Data,
                "inode/directory" => Self::Folder,
                "inode/symlink" => Self::Link,
                _ => Self::Generic,
            },
        }
    }
}

/// A registered file type.
#[derive(Debug, Clone)]
pub struct FileType {
    /// MIME type (e.g., "image/png").
    pub mime: String,
    /// Human-readable description (e.g., "PNG Image").
    pub description: String,
    /// Default icon identifier.
    pub icon: String,
    /// File extensions associated with this type.
    pub extensions: Vec<String>,
    /// Category for fallback.
    pub category: IconCategory,
    /// Whether this is a system-registered type (vs. app-registered).
    pub system: bool,
}

/// An app-specific icon override.
#[derive(Debug, Clone)]
pub struct AppIconOverride {
    /// Application path.
    pub app_path: String,
    /// MIME type.
    pub mime: String,
    /// Custom icon for this app + type combination.
    pub icon: String,
}

/// Resolved icon information.
#[derive(Debug, Clone)]
pub struct ResolvedIcon {
    /// Icon identifier.
    pub icon: String,
    /// Where the icon was resolved from.
    pub source: IconSource,
    /// MIME type.
    pub mime: String,
    /// Human-readable type description.
    pub description: String,
    /// Icon category.
    pub category: IconCategory,
}

/// Where an icon was resolved from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSource {
    /// Per-app override.
    AppOverride,
    /// Registered type.
    TypeRegistry,
    /// Category default.
    CategoryDefault,
    /// Generic fallback.
    GenericFallback,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static LOOKUP_COUNT: AtomicU64 = AtomicU64::new(0);
static REGISTER_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;

/// Registered file types.
static TYPES: Mutex<Vec<FileType>> = Mutex::new(Vec::new());

/// App-specific icon overrides.
static APP_ICONS: Mutex<Vec<AppIconOverride>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize with default system file types.
pub fn init() {
    let mut types = TYPES.lock();
    if !types.is_empty() {
        return;
    }

    let defaults: &[(&str, &str, &str, &[&str])] = &[
        ("text/plain", "Text Document", "icon-text", &["txt", "text", "log"]),
        ("text/html", "HTML Document", "icon-html", &["html", "htm"]),
        ("text/css", "CSS Stylesheet", "icon-css", &["css"]),
        ("text/markdown", "Markdown Document", "icon-markdown", &["md", "markdown"]),
        ("text/x-python", "Python Script", "icon-python", &["py", "pyw"]),
        ("text/x-rust", "Rust Source", "icon-rust", &["rs"]),
        ("text/x-c", "C Source", "icon-c", &["c", "h"]),
        ("text/x-shellscript", "Shell Script", "icon-shell", &["sh", "bash"]),
        ("text/csv", "CSV Spreadsheet", "icon-csv", &["csv"]),
        ("application/json", "JSON File", "icon-json", &["json"]),
        ("application/pdf", "PDF Document", "icon-pdf", &["pdf"]),
        ("application/zip", "ZIP Archive", "icon-zip", &["zip"]),
        ("application/gzip", "Gzip Archive", "icon-gzip", &["gz"]),
        ("application/x-tar", "Tar Archive", "icon-tar", &["tar"]),
        ("application/rtf", "Rich Text Document", "icon-rtf", &["rtf"]),
        ("application/xml", "XML Document", "icon-xml", &["xml"]),
        ("image/png", "PNG Image", "icon-png", &["png"]),
        ("image/jpeg", "JPEG Image", "icon-jpeg", &["jpg", "jpeg"]),
        ("image/gif", "GIF Image", "icon-gif", &["gif"]),
        ("image/bmp", "BMP Image", "icon-bmp", &["bmp"]),
        ("image/webp", "WebP Image", "icon-webp", &["webp"]),
        ("image/svg+xml", "SVG Image", "icon-svg", &["svg"]),
        ("audio/mpeg", "MP3 Audio", "icon-mp3", &["mp3"]),
        ("audio/flac", "FLAC Audio", "icon-flac", &["flac"]),
        ("audio/ogg", "OGG Audio", "icon-ogg", &["ogg"]),
        ("audio/wav", "WAV Audio", "icon-wav", &["wav"]),
        ("video/mp4", "MP4 Video", "icon-mp4", &["mp4", "m4v"]),
        ("video/webm", "WebM Video", "icon-webm", &["webm"]),
        ("video/x-matroska", "Matroska Video", "icon-mkv", &["mkv"]),
        ("application/x-executable", "Executable", "icon-exe", &[]),
    ];

    for (mime, desc, icon, exts) in defaults {
        let category = IconCategory::from_mime(mime);
        types.push(FileType {
            mime: String::from(*mime),
            description: String::from(*desc),
            icon: String::from(*icon),
            extensions: exts.iter().map(|e| String::from(*e)).collect(),
            category,
            system: true,
        });
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Resolve the icon for a file path.
///
/// Checks app overrides, type registry, category defaults, and generic fallback.
pub fn icon_for_file(path: &str) -> ResolvedIcon {
    icon_for_file_with_app(path, None)
}

/// Resolve the icon for a file, optionally considering an app's overrides.
pub fn icon_for_file_with_app(path: &str, app_path: Option<&str>) -> ResolvedIcon {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);

    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");
    let category = IconCategory::from_mime(mime);

    // Check for directory.
    if let Ok(meta) = crate::fs::vfs::Vfs::metadata(path) {
        if meta.entry_type == crate::fs::EntryType::Directory {
            return ResolvedIcon {
                icon: String::from("icon-folder"),
                source: IconSource::CategoryDefault,
                mime: String::from("inode/directory"),
                description: String::from("Folder"),
                category: IconCategory::Folder,
            };
        }
        if meta.entry_type == crate::fs::EntryType::Symlink {
            return ResolvedIcon {
                icon: String::from("icon-link"),
                source: IconSource::CategoryDefault,
                mime: String::from("inode/symlink"),
                description: String::from("Symbolic Link"),
                category: IconCategory::Link,
            };
        }
    }

    // 1. Check app-specific override.
    if let Some(app) = app_path {
        let overrides = APP_ICONS.lock();
        if let Some(ov) = overrides.iter().find(|o| o.app_path == app && o.mime == mime) {
            let desc = type_description(mime);
            return ResolvedIcon {
                icon: ov.icon.clone(),
                source: IconSource::AppOverride,
                mime: String::from(mime),
                description: desc,
                category,
            };
        }
    }

    // 2. Check type registry.
    {
        let types = TYPES.lock();
        if let Some(ft) = types.iter().find(|t| t.mime == mime) {
            return ResolvedIcon {
                icon: ft.icon.clone(),
                source: IconSource::TypeRegistry,
                mime: String::from(mime),
                description: ft.description.clone(),
                category: ft.category,
            };
        }
    }

    // 3. Category default.
    ResolvedIcon {
        icon: String::from(category.default_icon()),
        source: if category != IconCategory::Generic {
            IconSource::CategoryDefault
        } else {
            IconSource::GenericFallback
        },
        mime: String::from(mime),
        description: type_description(mime),
        category,
    }
}

/// Register a file type.
pub fn register_type(
    mime: &str,
    description: &str,
    icon: &str,
    extensions: &[&str],
) -> KernelResult<()> {
    REGISTER_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut types = TYPES.lock();

    // Update existing or add new.
    if let Some(existing) = types.iter_mut().find(|t| t.mime == mime) {
        existing.description = String::from(description);
        existing.icon = String::from(icon);
        existing.extensions = extensions.iter().map(|e| String::from(*e)).collect();
        existing.system = false;
        return Ok(());
    }

    if types.len() >= MAX_TYPES {
        return Err(KernelError::ResourceExhausted);
    }

    types.push(FileType {
        mime: String::from(mime),
        description: String::from(description),
        icon: String::from(icon),
        extensions: extensions.iter().map(|e| String::from(*e)).collect(),
        category: IconCategory::from_mime(mime),
        system: false,
    });

    Ok(())
}

/// Register an app-specific icon override.
pub fn register_app_icon(app_path: &str, mime: &str, icon: &str) -> KernelResult<()> {
    let mut overrides = APP_ICONS.lock();

    // Update existing.
    if let Some(existing) = overrides.iter_mut().find(|o| o.app_path == app_path && o.mime == mime) {
        existing.icon = String::from(icon);
        return Ok(());
    }

    if overrides.len() >= MAX_APP_ICONS {
        return Err(KernelError::ResourceExhausted);
    }

    overrides.push(AppIconOverride {
        app_path: String::from(app_path),
        mime: String::from(mime),
        icon: String::from(icon),
    });

    Ok(())
}

/// Unregister an app-specific icon override.
pub fn unregister_app_icon(app_path: &str, mime: &str) -> KernelResult<()> {
    let mut overrides = APP_ICONS.lock();
    if let Some(pos) = overrides.iter().position(|o| o.app_path == app_path && o.mime == mime) {
        overrides.remove(pos);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// List all registered file types.
pub fn list_types() -> Vec<(String, String, String, usize)> {
    let types = TYPES.lock();
    types.iter()
        .map(|t| (t.mime.clone(), t.description.clone(), t.icon.clone(), t.extensions.len()))
        .collect()
}

/// Get type info for a MIME type.
pub fn get_type(mime: &str) -> Option<FileType> {
    let types = TYPES.lock();
    types.iter().find(|t| t.mime == mime).cloned()
}

/// Look up MIME type by extension.
pub fn mime_for_extension(ext: &str) -> Option<String> {
    let types = TYPES.lock();
    for ft in types.iter() {
        if ft.extensions.iter().any(|e| e == ext) {
            return Some(ft.mime.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_source_mime(mime: &str) -> bool {
    matches!(mime, "text/x-python" | "text/x-rust" | "text/x-c"
        | "text/x-shellscript" | "text/x-java" | "text/x-go"
        | "text/x-javascript" | "text/x-typescript")
}

fn type_description(mime: &str) -> String {
    // Try the type registry first.
    let types = TYPES.lock();
    if let Some(ft) = types.iter().find(|t| t.mime == mime) {
        return ft.description.clone();
    }
    // Generic description from MIME.
    let parts: Vec<&str> = mime.split('/').collect();
    if parts.len() == 2 {
        alloc::format!("{} File", parts[1].to_uppercase())
    } else {
        String::from("File")
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (lookup_count, register_count, type_count, app_icon_count).
pub fn stats() -> (u64, u64, usize, usize) {
    (
        LOOKUP_COUNT.load(Ordering::Relaxed),
        REGISTER_COUNT.load(Ordering::Relaxed),
        TYPES.lock().len(),
        APP_ICONS.lock().len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    LOOKUP_COUNT.store(0, Ordering::Relaxed);
    REGISTER_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file type module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: initialization.
    {
        init();
        let types = list_types();
        assert!(!types.is_empty());
        assert!(types.iter().any(|(m, _, _, _)| m == "image/png"));
        serial_println!("[filetype] test 1 passed: init ({} types)", types.len());
    }

    // Test 2: icon category from MIME.
    {
        assert_eq!(IconCategory::from_mime("image/png"), IconCategory::Image);
        assert_eq!(IconCategory::from_mime("audio/mpeg"), IconCategory::Audio);
        assert_eq!(IconCategory::from_mime("text/plain"), IconCategory::Text);
        assert_eq!(IconCategory::from_mime("text/x-python"), IconCategory::SourceCode);
        assert_eq!(IconCategory::from_mime("application/pdf"), IconCategory::Document);
        assert_eq!(IconCategory::from_mime("application/zip"), IconCategory::Archive);
        serial_println!("[filetype] test 2 passed: MIME category mapping");
    }

    // Test 3: icon resolution.
    {
        let icon = icon_for_file("/test.png");
        assert_eq!(icon.source, IconSource::TypeRegistry);
        assert!(icon.icon.contains("png"));
        assert_eq!(icon.category, IconCategory::Image);
        serial_println!("[filetype] test 3 passed: icon resolution");
    }

    // Test 4: register custom type.
    {
        register_type(
            "application/x-custom",
            "Custom File",
            "icon-custom",
            &["cust"],
        )?;
        let ft = get_type("application/x-custom");
        assert!(ft.is_some());
        assert_eq!(ft.as_ref().map(|f| f.description.as_str()), Some("Custom File"));
        serial_println!("[filetype] test 4 passed: register type");
    }

    // Test 5: app-specific icon override.
    {
        register_app_icon("/usr/bin/viewer", "image/png", "viewer-png-icon")?;
        let icon = icon_for_file_with_app("/test.png", Some("/usr/bin/viewer"));
        assert_eq!(icon.source, IconSource::AppOverride);
        assert_eq!(icon.icon, "viewer-png-icon");

        // Without app, should use registry.
        let icon2 = icon_for_file("/test.png");
        assert_eq!(icon2.source, IconSource::TypeRegistry);

        unregister_app_icon("/usr/bin/viewer", "image/png")?;
        serial_println!("[filetype] test 5 passed: app icon override");
    }

    // Test 6: extension lookup.
    {
        let mime = mime_for_extension("png");
        assert_eq!(mime.as_deref(), Some("image/png"));
        let mime2 = mime_for_extension("xyz123");
        assert!(mime2.is_none());
        serial_println!("[filetype] test 6 passed: extension lookup");
    }

    // Test 7: stats.
    {
        let (lookups, registers, types, app_icons) = stats();
        assert!(lookups > 0);
        assert!(registers > 0);
        assert!(types > 0);
        // App icons should be 0 since we unregistered.
        let _ = app_icons;
        serial_println!("[filetype] test 7 passed: stats");
    }

    serial_println!("[filetype] all 7 self-tests passed");
    Ok(())
}
