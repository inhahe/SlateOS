//! SlateOS File Association Manager — GUI for File Type Associations
//!
//! A graphical application for managing which applications open which file
//! types. Provides browsing by category, search/filter, default-app assignment,
//! an "Open With" dialog mockup, and import/export of association configs.
//!
//! Uses the guitk library for rendering. Dark theme (Catppuccin Mocha).

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 700.0;
const SIDEBAR_WIDTH: f32 = 180.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const DETAILS_PANEL_WIDTH: f32 = 280.0;
const ROW_HEIGHT: f32 = 32.0;
const TABLE_HEADER_HEIGHT: f32 = 30.0;
const PADDING: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_WIDTH: f32 = 100.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const SEARCH_WIDTH: f32 = 260.0;
const SEARCH_HEIGHT: f32 = 30.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 34.0;
const DIALOG_WIDTH: f32 = 400.0;
const DIALOG_HEIGHT: f32 = 360.0;
const DIALOG_APP_ROW_HEIGHT: f32 = 36.0;

// ============================================================================
// File type categories
// ============================================================================

/// Categories for grouping file types in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileCategory {
    Documents,
    Images,
    Audio,
    Video,
    Archives,
    Code,
    Other,
}

impl FileCategory {
    const ALL: &[Self] = &[
        Self::Documents,
        Self::Images,
        Self::Audio,
        Self::Video,
        Self::Archives,
        Self::Code,
        Self::Other,
    ];

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::Images => "Images",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archives => "Archives",
            Self::Code => "Code",
            Self::Other => "Other",
        }
    }

    /// Short icon/glyph for the sidebar.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Documents => "D",
            Self::Images => "I",
            Self::Audio => "A",
            Self::Video => "V",
            Self::Archives => "Z",
            Self::Code => "C",
            Self::Other => "?",
        }
    }

    /// Accent color for this category.
    pub fn color(self) -> Color {
        match self {
            Self::Documents => COLOR_BLUE,
            Self::Images => COLOR_GREEN,
            Self::Audio => COLOR_PEACH,
            Self::Video => COLOR_RED,
            Self::Archives => COLOR_YELLOW,
            Self::Code => COLOR_MAUVE,
            Self::Other => COLOR_SUBTEXT0,
        }
    }

    /// Parse a category from a string label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "documents" => Some(Self::Documents),
            "images" => Some(Self::Images),
            "audio" => Some(Self::Audio),
            "video" => Some(Self::Video),
            "archives" => Some(Self::Archives),
            "code" => Some(Self::Code),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

// ============================================================================
// FileType — describes a single file type (extension)
// ============================================================================

/// Metadata for a file type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileType {
    /// File extension without leading dot (e.g. "txt", "png").
    pub extension: String,
    /// MIME type string (e.g. "text/plain").
    pub mime_type: String,
    /// Human-readable description (e.g. "Plain Text Document").
    pub description: String,
    /// ID of the default application to open this type, if any.
    pub default_app_id: Option<String>,
}

impl FileType {
    /// Create a new file type.
    pub fn new(extension: &str, mime_type: &str, description: &str) -> Self {
        Self {
            extension: extension.to_string(),
            mime_type: mime_type.to_string(),
            description: description.to_string(),
            default_app_id: None,
        }
    }

    /// Create a new file type with a default app assigned.
    pub fn with_default_app(
        extension: &str,
        mime_type: &str,
        description: &str,
        app_id: &str,
    ) -> Self {
        Self {
            extension: extension.to_string(),
            mime_type: mime_type.to_string(),
            description: description.to_string(),
            default_app_id: Some(app_id.to_string()),
        }
    }
}

// ============================================================================
// AppInfo — describes an application that can open files
// ============================================================================

/// Metadata for an application registered to open file types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    /// Unique application identifier (e.g. "textedit").
    pub id: String,
    /// Human-readable display name (e.g. "Text Editor").
    pub name: String,
    /// Path to the application executable.
    pub exec_path: String,
    /// File extensions this app supports.
    pub supported_extensions: Vec<String>,
    /// Icon asset ID for the app.
    pub icon_id: u64,
}

impl AppInfo {
    /// Create a new app info entry.
    pub fn new(id: &str, name: &str, exec_path: &str, extensions: &[&str], icon_id: u64) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            exec_path: exec_path.to_string(),
            supported_extensions: extensions.iter().map(|e| e.to_string()).collect(),
            icon_id,
        }
    }

    /// Check whether this app supports a given extension.
    pub fn supports_extension(&self, ext: &str) -> bool {
        let ext_lower = ext.to_lowercase();
        self.supported_extensions
            .iter()
            .any(|e| e.to_lowercase() == ext_lower)
    }
}

// ============================================================================
// Association — maps an extension to an app
// ============================================================================

/// A single file-type-to-application association.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Association {
    /// The file extension (without dot).
    pub extension: String,
    /// The application ID assigned to open this extension.
    pub app_id: String,
}

impl Association {
    /// Create a new association.
    pub fn new(extension: &str, app_id: &str) -> Self {
        Self {
            extension: extension.to_string(),
            app_id: app_id.to_string(),
        }
    }

    /// Serialize to a config line: `extension=app_id`.
    pub fn to_config_line(&self) -> String {
        let mut buf = String::new();
        buf.push_str(&self.extension);
        buf.push('=');
        buf.push_str(&self.app_id);
        buf
    }

    /// Parse from a config line: `extension=app_id`.
    /// Returns `None` if the line is malformed.
    pub fn from_config_line(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let (ext, app) = trimmed.split_once('=')?;
        let ext = ext.trim();
        let app = app.trim();
        if ext.is_empty() || app.is_empty() {
            return None;
        }
        Some(Self::new(ext, app))
    }
}

// ============================================================================
// Error type
// ============================================================================

/// Errors for association operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssocError {
    /// The requested file type was not found in the registry.
    FileTypeNotFound(String),
    /// The requested application was not found.
    AppNotFound(String),
    /// The app does not support the given extension.
    UnsupportedExtension { app_id: String, extension: String },
    /// An association already exists (when adding duplicates).
    AlreadyExists(String),
    /// Config parse error at a given line number.
    ParseError { line_number: usize, detail: String },
}

impl core::fmt::Display for AssocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FileTypeNotFound(ext) => write!(f, "File type not found: .{ext}"),
            Self::AppNotFound(id) => write!(f, "Application not found: {id}"),
            Self::UnsupportedExtension { app_id, extension } => {
                write!(f, "App '{app_id}' does not support .{extension}")
            }
            Self::AlreadyExists(ext) => write!(f, "Association already exists for .{ext}"),
            Self::ParseError {
                line_number,
                detail,
            } => {
                write!(f, "Parse error at line {line_number}: {detail}")
            }
        }
    }
}

// ============================================================================
// AssociationRegistry — central registry for all associations
// ============================================================================

/// The central registry managing file types, applications, and their associations.
pub struct AssociationRegistry {
    /// All known file types, keyed by extension (lowercase).
    pub file_types: BTreeMap<String, FileType>,
    /// All known applications, keyed by app ID.
    pub apps: BTreeMap<String, AppInfo>,
    /// Current associations: extension -> app_id.
    pub associations: BTreeMap<String, Association>,
    /// Category assignments: extension -> category.
    pub categories: BTreeMap<String, FileCategory>,
}

impl Default for AssociationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AssociationRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            file_types: BTreeMap::new(),
            apps: BTreeMap::new(),
            associations: BTreeMap::new(),
            categories: BTreeMap::new(),
        }
    }

    /// Create a registry pre-populated with built-in file types and apps.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.add_default_file_types();
        reg.add_default_apps();
        reg.assign_default_associations();
        reg
    }

    // -- File type management ------------------------------------------------

    /// Register a file type. Overwrites if the extension already exists.
    pub fn register_file_type(&mut self, ft: FileType, category: FileCategory) {
        let ext = ft.extension.to_lowercase();
        self.categories.insert(ext.clone(), category);
        self.file_types.insert(ext, ft);
    }

    /// Remove a file type by extension. Also removes its association.
    pub fn remove_file_type(&mut self, extension: &str) -> Result<FileType, AssocError> {
        let ext = extension.to_lowercase();
        self.associations.remove(&ext);
        self.categories.remove(&ext);
        self.file_types
            .remove(&ext)
            .ok_or(AssocError::FileTypeNotFound(ext))
    }

    /// Get a file type by extension.
    pub fn get_file_type(&self, extension: &str) -> Option<&FileType> {
        self.file_types.get(&extension.to_lowercase())
    }

    /// Get the category for a file extension.
    pub fn get_category(&self, extension: &str) -> FileCategory {
        self.categories
            .get(&extension.to_lowercase())
            .copied()
            .unwrap_or(FileCategory::Other)
    }

    /// Return all file types belonging to a given category.
    pub fn file_types_by_category(&self, category: FileCategory) -> Vec<&FileType> {
        self.file_types
            .values()
            .filter(|ft| {
                self.categories
                    .get(&ft.extension.to_lowercase())
                    .copied()
                    .unwrap_or(FileCategory::Other)
                    == category
            })
            .collect()
    }

    /// Return all registered extensions sorted alphabetically.
    pub fn all_extensions(&self) -> Vec<String> {
        self.file_types.keys().cloned().collect()
    }

    /// Return the total count of registered file types.
    pub fn file_type_count(&self) -> usize {
        self.file_types.len()
    }

    // -- App management ------------------------------------------------------

    /// Register an application.
    pub fn register_app(&mut self, app: AppInfo) {
        self.apps.insert(app.id.clone(), app);
    }

    /// Remove an application. Also clears any associations pointing to it.
    pub fn remove_app(&mut self, app_id: &str) -> Result<AppInfo, AssocError> {
        // Remove associations that reference this app.
        let to_remove: Vec<String> = self
            .associations
            .iter()
            .filter(|(_, a)| a.app_id == app_id)
            .map(|(k, _)| k.clone())
            .collect();
        for ext in to_remove {
            self.associations.remove(&ext);
        }
        self.apps
            .remove(app_id)
            .ok_or_else(|| AssocError::AppNotFound(app_id.to_string()))
    }

    /// Get an application by ID.
    pub fn get_app(&self, app_id: &str) -> Option<&AppInfo> {
        self.apps.get(app_id)
    }

    /// Return all apps that support a given extension.
    pub fn apps_for_extension(&self, extension: &str) -> Vec<&AppInfo> {
        let ext_lower = extension.to_lowercase();
        self.apps
            .values()
            .filter(|app| app.supports_extension(&ext_lower))
            .collect()
    }

    /// Return the total count of registered apps.
    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    // -- Association management ----------------------------------------------

    /// Set (or replace) the default app for a file extension.
    /// Validates that the file type exists, the app exists, and the app
    /// supports the extension.
    pub fn set_default_app(&mut self, extension: &str, app_id: &str) -> Result<(), AssocError> {
        let ext = extension.to_lowercase();

        if !self.file_types.contains_key(&ext) {
            return Err(AssocError::FileTypeNotFound(ext));
        }
        let app = self
            .apps
            .get(app_id)
            .ok_or_else(|| AssocError::AppNotFound(app_id.to_string()))?;
        if !app.supports_extension(&ext) {
            return Err(AssocError::UnsupportedExtension {
                app_id: app_id.to_string(),
                extension: ext,
            });
        }

        // Update the association map.
        self.associations
            .insert(ext.clone(), Association::new(&ext, app_id));

        // Also update the file type's default_app_id.
        if let Some(ft) = self.file_types.get_mut(&ext) {
            ft.default_app_id = Some(app_id.to_string());
        }

        Ok(())
    }

    /// Remove the association for a given extension (reset to no default).
    pub fn clear_association(&mut self, extension: &str) -> Result<(), AssocError> {
        let ext = extension.to_lowercase();
        if !self.file_types.contains_key(&ext) {
            return Err(AssocError::FileTypeNotFound(ext));
        }
        self.associations.remove(&ext);
        if let Some(ft) = self.file_types.get_mut(&ext) {
            ft.default_app_id = None;
        }
        Ok(())
    }

    /// Get the current default app for a file extension.
    pub fn get_default_app(&self, extension: &str) -> Option<&AppInfo> {
        let ext = extension.to_lowercase();
        self.associations
            .get(&ext)
            .and_then(|a| self.apps.get(&a.app_id))
    }

    /// Return the count of active associations.
    pub fn association_count(&self) -> usize {
        self.associations.len()
    }

    /// Reset all associations to the built-in defaults.
    pub fn reset_to_defaults(&mut self) {
        self.associations.clear();
        for ft in self.file_types.values_mut() {
            ft.default_app_id = None;
        }
        self.assign_default_associations();
    }

    // -- Search and filter ---------------------------------------------------

    /// Search file types by extension or description (case-insensitive substring).
    pub fn search(&self, query: &str) -> Vec<&FileType> {
        let q = query.to_lowercase();
        self.file_types
            .values()
            .filter(|ft| {
                ft.extension.to_lowercase().contains(&q)
                    || ft.description.to_lowercase().contains(&q)
                    || ft.mime_type.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Search and also filter to a specific category.
    pub fn search_in_category(&self, query: &str, category: FileCategory) -> Vec<&FileType> {
        let q = query.to_lowercase();
        self.file_types
            .values()
            .filter(|ft| {
                self.get_category(&ft.extension) == category
                    && (ft.extension.to_lowercase().contains(&q)
                        || ft.description.to_lowercase().contains(&q)
                        || ft.mime_type.to_lowercase().contains(&q))
            })
            .collect()
    }

    // -- Import / Export (line-based config) ----------------------------------

    /// Export all associations to a line-based config string.
    /// Format: `extension=app_id` per line, with a header comment.
    pub fn export_config(&self) -> String {
        let mut out = String::from("# Slate OS File Associations\n");
        for (ext, assoc) in &self.associations {
            out.push_str(ext);
            out.push('=');
            out.push_str(&assoc.app_id);
            out.push('\n');
        }
        out
    }

    /// Import associations from a line-based config string.
    /// Skips blank lines and comment lines (starting with `#`).
    /// Returns a list of errors for lines that failed to parse or apply.
    pub fn import_config(&mut self, config: &str) -> Vec<AssocError> {
        let mut errors = Vec::new();
        for (idx, line) in config.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match Association::from_config_line(trimmed) {
                Some(assoc) => {
                    if let Err(e) = self.set_default_app(&assoc.extension, &assoc.app_id) {
                        errors.push(e);
                    }
                }
                None => {
                    errors.push(AssocError::ParseError {
                        line_number: idx.wrapping_add(1),
                        detail: String::from("invalid format, expected extension=app_id"),
                    });
                }
            }
        }
        errors
    }

    // -- Built-in data -------------------------------------------------------

    /// Populate with 30+ built-in file types covering all categories.
    fn add_default_file_types(&mut self) {
        // (extension, mime_type, description, category)
        let defaults: &[(&str, &str, &str, FileCategory)] = &[
            // Documents
            (
                "txt",
                "text/plain",
                "Plain Text Document",
                FileCategory::Documents,
            ),
            (
                "pdf",
                "application/pdf",
                "PDF Document",
                FileCategory::Documents,
            ),
            (
                "doc",
                "application/msword",
                "Word Document",
                FileCategory::Documents,
            ),
            (
                "docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "Word Document (OOXML)",
                FileCategory::Documents,
            ),
            (
                "xls",
                "application/vnd.ms-excel",
                "Excel Spreadsheet",
                FileCategory::Documents,
            ),
            (
                "xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "Excel Spreadsheet (OOXML)",
                FileCategory::Documents,
            ),
            (
                "pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "PowerPoint Presentation",
                FileCategory::Documents,
            ),
            (
                "odt",
                "application/vnd.oasis.opendocument.text",
                "OpenDocument Text",
                FileCategory::Documents,
            ),
            (
                "rtf",
                "application/rtf",
                "Rich Text Format",
                FileCategory::Documents,
            ),
            (
                "csv",
                "text/csv",
                "Comma-Separated Values",
                FileCategory::Documents,
            ),
            // Images
            ("png", "image/png", "PNG Image", FileCategory::Images),
            ("jpg", "image/jpeg", "JPEG Image", FileCategory::Images),
            ("jpeg", "image/jpeg", "JPEG Image", FileCategory::Images),
            ("gif", "image/gif", "GIF Image", FileCategory::Images),
            ("bmp", "image/bmp", "Bitmap Image", FileCategory::Images),
            (
                "svg",
                "image/svg+xml",
                "SVG Vector Image",
                FileCategory::Images,
            ),
            ("webp", "image/webp", "WebP Image", FileCategory::Images),
            ("ico", "image/x-icon", "Icon File", FileCategory::Images),
            // Audio
            ("mp3", "audio/mpeg", "MP3 Audio", FileCategory::Audio),
            ("wav", "audio/wav", "WAV Audio", FileCategory::Audio),
            ("flac", "audio/flac", "FLAC Audio", FileCategory::Audio),
            ("ogg", "audio/ogg", "OGG Audio", FileCategory::Audio),
            ("m4a", "audio/mp4", "M4A Audio", FileCategory::Audio),
            // Video
            ("mp4", "video/mp4", "MP4 Video", FileCategory::Video),
            (
                "mkv",
                "video/x-matroska",
                "Matroska Video",
                FileCategory::Video,
            ),
            ("avi", "video/x-msvideo", "AVI Video", FileCategory::Video),
            ("webm", "video/webm", "WebM Video", FileCategory::Video),
            (
                "mov",
                "video/quicktime",
                "QuickTime Video",
                FileCategory::Video,
            ),
            // Archives
            (
                "zip",
                "application/zip",
                "ZIP Archive",
                FileCategory::Archives,
            ),
            (
                "tar",
                "application/x-tar",
                "Tar Archive",
                FileCategory::Archives,
            ),
            (
                "gz",
                "application/gzip",
                "Gzip Archive",
                FileCategory::Archives,
            ),
            (
                "7z",
                "application/x-7z-compressed",
                "7-Zip Archive",
                FileCategory::Archives,
            ),
            (
                "rar",
                "application/vnd.rar",
                "RAR Archive",
                FileCategory::Archives,
            ),
            // Code
            ("rs", "text/x-rust", "Rust Source", FileCategory::Code),
            ("py", "text/x-python", "Python Source", FileCategory::Code),
            (
                "js",
                "text/javascript",
                "JavaScript Source",
                FileCategory::Code,
            ),
            (
                "ts",
                "text/typescript",
                "TypeScript Source",
                FileCategory::Code,
            ),
            ("html", "text/html", "HTML Document", FileCategory::Code),
            ("css", "text/css", "CSS Stylesheet", FileCategory::Code),
            ("json", "application/json", "JSON Data", FileCategory::Code),
            ("xml", "application/xml", "XML Document", FileCategory::Code),
            (
                "toml",
                "application/toml",
                "TOML Config",
                FileCategory::Code,
            ),
            ("yaml", "text/yaml", "YAML Config", FileCategory::Code),
            ("c", "text/x-c", "C Source", FileCategory::Code),
            ("cpp", "text/x-c++", "C++ Source", FileCategory::Code),
            ("h", "text/x-c", "C/C++ Header", FileCategory::Code),
            // Other
            ("log", "text/plain", "Log File", FileCategory::Other),
            ("ini", "text/plain", "INI Config File", FileCategory::Other),
            (
                "iso",
                "application/x-iso9660-image",
                "Disc Image",
                FileCategory::Other,
            ),
            (
                "bin",
                "application/octet-stream",
                "Binary File",
                FileCategory::Other,
            ),
        ];

        for (ext, mime, desc, category) in defaults {
            self.register_file_type(FileType::new(ext, mime, desc), *category);
        }
    }

    /// Populate with 10+ built-in applications.
    fn add_default_apps(&mut self) {
        let apps: &[(&str, &str, &str, &[&str], u64)] = &[
            (
                "textedit",
                "Text Editor",
                "/usr/bin/textedit",
                &[
                    "txt", "rs", "py", "js", "ts", "html", "css", "json", "xml", "toml", "yaml",
                    "c", "cpp", "h", "log", "ini", "csv", "rtf", "odt",
                ],
                1,
            ),
            ("pdfviewer", "PDF Viewer", "/usr/bin/pdfviewer", &["pdf"], 2),
            (
                "photoviewer",
                "Photo Viewer",
                "/usr/bin/photoviewer",
                &["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico"],
                3,
            ),
            (
                "musicplayer",
                "Music Player",
                "/usr/bin/musicplayer",
                &["mp3", "wav", "flac", "ogg", "m4a"],
                4,
            ),
            (
                "videoplayer",
                "Video Player",
                "/usr/bin/videoplayer",
                &["mp4", "mkv", "avi", "webm", "mov"],
                5,
            ),
            (
                "archiver",
                "Archive Manager",
                "/usr/bin/archiver",
                &["zip", "tar", "gz", "7z", "rar"],
                6,
            ),
            (
                "browser",
                "Web Browser",
                "/usr/bin/browser",
                &["html", "svg", "json", "xml", "pdf"],
                7,
            ),
            (
                "office",
                "Office Suite",
                "/usr/bin/office",
                &["doc", "docx", "xls", "xlsx", "pptx", "odt", "rtf", "csv"],
                8,
            ),
            (
                "codeeditor",
                "Code Editor",
                "/usr/bin/codeeditor",
                &[
                    "txt", "rs", "py", "js", "ts", "html", "css", "json", "xml", "toml", "yaml",
                    "c", "cpp", "h",
                ],
                9,
            ),
            (
                "fileexplorer",
                "File Explorer",
                "/usr/bin/fileexplorer",
                &["iso", "bin", "zip", "tar", "gz", "7z", "rar"],
                10,
            ),
            (
                "imageeditor",
                "Image Editor",
                "/usr/bin/imageeditor",
                &["png", "jpg", "jpeg", "bmp", "webp", "svg"],
                11,
            ),
            (
                "hexeditor",
                "Hex Editor",
                "/usr/bin/hexeditor",
                &["bin", "iso"],
                12,
            ),
        ];

        for (id, name, path, exts, icon) in apps {
            self.register_app(AppInfo::new(id, name, path, exts, *icon));
        }
    }

    /// Assign sensible default associations after file types and apps are loaded.
    fn assign_default_associations(&mut self) {
        // Maps category to its primary default app ID.
        let category_defaults: &[(FileCategory, &str)] = &[
            (FileCategory::Documents, "textedit"),
            (FileCategory::Images, "photoviewer"),
            (FileCategory::Audio, "musicplayer"),
            (FileCategory::Video, "videoplayer"),
            (FileCategory::Archives, "archiver"),
            (FileCategory::Code, "codeeditor"),
            (FileCategory::Other, "textedit"),
        ];

        // Specific overrides that take precedence over the category default.
        let specific_overrides: &[(&str, &str)] = &[
            ("pdf", "pdfviewer"),
            ("doc", "office"),
            ("docx", "office"),
            ("xls", "office"),
            ("xlsx", "office"),
            ("pptx", "office"),
            ("odt", "office"),
            ("rtf", "office"),
            ("csv", "office"),
            ("iso", "fileexplorer"),
            ("bin", "hexeditor"),
        ];

        // First pass: assign by category.
        let extensions: Vec<(String, FileCategory)> = self
            .file_types
            .keys()
            .map(|ext| {
                let cat = self.get_category(ext);
                (ext.clone(), cat)
            })
            .collect();

        for (ext, cat) in &extensions {
            for (def_cat, app_id) in category_defaults {
                if cat == def_cat {
                    // Only assign if the app actually supports this extension.
                    if let Some(app) = self.apps.get(*app_id)
                        && app.supports_extension(ext)
                    {
                        self.associations
                            .insert(ext.clone(), Association::new(ext, app_id));
                        if let Some(ft) = self.file_types.get_mut(ext) {
                            ft.default_app_id = Some(app_id.to_string());
                        }
                    }
                    break;
                }
            }
        }

        // Second pass: apply specific overrides.
        for (ext, app_id) in specific_overrides {
            let ext_lower = ext.to_lowercase();
            if self.file_types.contains_key(&ext_lower)
                && let Some(app) = self.apps.get(*app_id)
                && app.supports_extension(&ext_lower)
            {
                self.associations
                    .insert(ext_lower.clone(), Association::new(&ext_lower, app_id));
                if let Some(ft) = self.file_types.get_mut(&ext_lower) {
                    ft.default_app_id = Some(app_id.to_string());
                }
            }
        }
    }
}

// ============================================================================
// UI state
// ============================================================================

/// Which view/dialog is currently active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveDialog {
    /// No dialog open; main view is active.
    None,
    /// "Open With" dialog for a given extension index.
    OpenWith,
    /// Add New File Type dialog.
    AddFileType,
}

/// Full UI state for the file association manager.
pub struct FileAssocUI {
    /// The underlying association registry.
    pub registry: AssociationRegistry,
    /// Currently selected category in the sidebar (None = show all).
    pub selected_category: Option<FileCategory>,
    /// Current search query string.
    pub search_query: String,
    /// Index of the selected file type in the current filtered list.
    pub selected_index: Option<usize>,
    /// Scroll offset for the file type list.
    pub scroll_offset: f32,
    /// Currently active dialog.
    pub active_dialog: ActiveDialog,
    /// In the "Open With" dialog, which app is selected.
    pub dialog_selected_app: Option<usize>,
    /// "Always use this app" checkbox state in the "Open With" dialog.
    pub dialog_always_use: bool,
    /// The extension that the "Open With" dialog is targeting.
    pub dialog_target_ext: String,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
}

impl Default for FileAssocUI {
    fn default() -> Self {
        Self::new()
    }
}

impl FileAssocUI {
    /// Create a new UI state with default registry.
    pub fn new() -> Self {
        Self {
            registry: AssociationRegistry::with_defaults(),
            selected_category: None,
            search_query: String::new(),
            selected_index: None,
            scroll_offset: 0.0,
            active_dialog: ActiveDialog::None,
            dialog_selected_app: None,
            dialog_always_use: false,
            dialog_target_ext: String::new(),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
        }
    }

    /// Return the list of file types matching the current filter/search.
    pub fn filtered_file_types(&self) -> Vec<&FileType> {
        let base: Vec<&FileType> = match self.selected_category {
            Some(cat) => self.registry.file_types_by_category(cat),
            None => self.registry.file_types.values().collect(),
        };

        if self.search_query.is_empty() {
            return base;
        }

        let q = self.search_query.to_lowercase();
        base.into_iter()
            .filter(|ft| {
                ft.extension.to_lowercase().contains(&q)
                    || ft.description.to_lowercase().contains(&q)
                    || ft.mime_type.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Open the "Open With" dialog for the currently selected file type.
    pub fn open_open_with_dialog(&mut self) {
        if let Some(idx) = self.selected_index {
            let filtered = self.filtered_file_types();
            if let Some(ft) = filtered.get(idx) {
                self.dialog_target_ext = ft.extension.clone();
                self.active_dialog = ActiveDialog::OpenWith;
                self.dialog_selected_app = Some(0);
                self.dialog_always_use = false;
            }
        }
    }

    /// Confirm the "Open With" dialog selection.
    pub fn confirm_open_with(&mut self) -> Result<(), AssocError> {
        if self.active_dialog != ActiveDialog::OpenWith {
            return Ok(());
        }

        let ext = self.dialog_target_ext.clone();
        let compatible = self.registry.apps_for_extension(&ext);

        if let Some(sel_idx) = self.dialog_selected_app
            && let Some(app) = compatible.get(sel_idx)
        {
            let app_id = app.id.clone();
            if self.dialog_always_use {
                self.registry.set_default_app(&ext, &app_id)?;
            }
        }

        self.active_dialog = ActiveDialog::None;
        Ok(())
    }

    /// Select a category in the sidebar.
    pub fn select_category(&mut self, category: Option<FileCategory>) {
        self.selected_category = category;
        self.selected_index = None;
        self.scroll_offset = 0.0;
    }

    /// Set the search query and reset selection.
    pub fn set_search_query(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.selected_index = None;
        self.scroll_offset = 0.0;
    }

    /// Select a file type row by index.
    pub fn select_file_type(&mut self, index: usize) {
        let count = self.filtered_file_types().len();
        if index < count {
            self.selected_index = Some(index);
        }
    }

    /// Get the currently selected file type, if any.
    pub fn selected_file_type(&self) -> Option<&FileType> {
        let idx = self.selected_index?;
        let filtered = self.filtered_file_types();
        filtered.get(idx).copied()
    }

    // -- Rendering -----------------------------------------------------------

    /// Render the full UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut rt = RenderTree::new();

        // Background
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut rt);
        self.render_sidebar(&mut rt);
        self.render_table(&mut rt);
        self.render_details_panel(&mut rt);

        if self.active_dialog == ActiveDialog::OpenWith {
            self.render_open_with_dialog(&mut rt);
        }

        rt
    }

    /// Render the top toolbar with search bar and action buttons.
    fn render_toolbar(&self, rt: &mut RenderTree) {
        // Toolbar background
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        rt.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: String::from("File Associations"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search bar background
        let search_x = SIDEBAR_WIDTH + PADDING;
        rt.push(RenderCommand::FillRect {
            x: search_x,
            y: 7.0,
            width: SEARCH_WIDTH,
            height: SEARCH_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Search placeholder or query text
        let search_text = if self.search_query.is_empty() {
            String::from("Search by extension or description...")
        } else {
            self.search_query.clone()
        };
        let search_text_color = if self.search_query.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };
        rt.push(RenderCommand::Text {
            x: search_x + 8.0,
            y: 15.0,
            text: search_text,
            color: search_text_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SEARCH_WIDTH - 16.0),
        });

        // "Reset Defaults" button
        let reset_x = self.window_width - BUTTON_WIDTH - PADDING;
        rt.push(RenderCommand::FillRect {
            x: reset_x,
            y: 7.0,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: reset_x + 10.0,
            y: 15.0,
            text: String::from("Reset Defaults"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 20.0),
        });

        // "Export" button
        let export_x = reset_x - BUTTON_WIDTH - PADDING;
        rt.push(RenderCommand::FillRect {
            x: export_x,
            y: 7.0,
            width: BUTTON_WIDTH - 20.0,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: export_x + 14.0,
            y: 15.0,
            text: String::from("Export"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the category sidebar.
    fn render_sidebar(&self, rt: &mut RenderTree) {
        let sidebar_y = TOOLBAR_HEIGHT;
        let sidebar_h = self.window_height - TOOLBAR_HEIGHT;

        // Sidebar background
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: SIDEBAR_WIDTH,
            height: sidebar_h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // "All Types" item
        let all_selected = self.selected_category.is_none();
        let all_bg = if all_selected {
            COLOR_SURFACE0
        } else {
            COLOR_MANTLE
        };
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: SIDEBAR_WIDTH,
            height: SIDEBAR_ITEM_HEIGHT,
            color: all_bg,
            corner_radii: CornerRadii::ZERO,
        });
        rt.push(RenderCommand::Text {
            x: PADDING + 24.0,
            y: sidebar_y + 10.0,
            text: String::from("All Types"),
            color: if all_selected { COLOR_BLUE } else { COLOR_TEXT },
            font_size: FONT_SIZE,
            font_weight: if all_selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(SIDEBAR_WIDTH - 34.0),
        });

        // Category items
        let mut y = sidebar_y + SIDEBAR_ITEM_HEIGHT + 8.0;

        // Section label
        rt.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: String::from("CATEGORIES"),
            color: COLOR_OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 20.0;

        for cat in FileCategory::ALL {
            let is_selected = self.selected_category == Some(*cat);
            let bg = if is_selected {
                COLOR_SURFACE0
            } else {
                COLOR_MANTLE
            };

            rt.push(RenderCommand::FillRect {
                x: 0.0,
                y,
                width: SIDEBAR_WIDTH,
                height: SIDEBAR_ITEM_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            // Category icon circle
            rt.push(RenderCommand::FillRect {
                x: PADDING,
                y: y + 7.0,
                width: 20.0,
                height: 20.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(10.0),
            });
            rt.push(RenderCommand::Text {
                x: PADDING + 5.0,
                y: y + 10.0,
                text: String::from(cat.icon()),
                color: COLOR_BASE,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Category label
            let label_color = if is_selected { COLOR_BLUE } else { COLOR_TEXT };
            rt.push(RenderCommand::Text {
                x: PADDING + 28.0,
                y: y + 10.0,
                text: String::from(cat.label()),
                color: label_color,
                font_size: FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 48.0),
            });

            // Count badge
            let count = self.registry.file_types_by_category(*cat).len();
            let count_str = format!("{count}");
            rt.push(RenderCommand::Text {
                x: SIDEBAR_WIDTH - 30.0,
                y: y + 10.0,
                text: count_str,
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            y += SIDEBAR_ITEM_HEIGHT;
        }
    }

    /// Render the main file type table.
    fn render_table(&self, rt: &mut RenderTree) {
        let table_x = SIDEBAR_WIDTH;
        let table_y = TOOLBAR_HEIGHT;
        let table_w = self.window_width - SIDEBAR_WIDTH - DETAILS_PANEL_WIDTH;
        let table_h = self.window_height - TOOLBAR_HEIGHT;

        // Table background
        rt.push(RenderCommand::FillRect {
            x: table_x,
            y: table_y,
            width: table_w,
            height: table_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Column headers
        rt.push(RenderCommand::FillRect {
            x: table_x,
            y: table_y,
            width: table_w,
            height: TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let col_ext_x = table_x + PADDING;
        let col_desc_x = table_x + 80.0;
        let col_mime_x = table_x + 240.0;
        let col_app_x = table_x + 400.0;
        let header_y = table_y + 9.0;

        rt.push(RenderCommand::Text {
            x: col_ext_x,
            y: header_y,
            text: String::from("Ext"),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        rt.push(RenderCommand::Text {
            x: col_desc_x,
            y: header_y,
            text: String::from("Description"),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        rt.push(RenderCommand::Text {
            x: col_mime_x,
            y: header_y,
            text: String::from("MIME Type"),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        rt.push(RenderCommand::Text {
            x: col_app_x,
            y: header_y,
            text: String::from("Default App"),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Separator line
        rt.push(RenderCommand::Line {
            x1: table_x,
            y1: table_y + TABLE_HEADER_HEIGHT,
            x2: table_x + table_w,
            y2: table_y + TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE1,
            width: 1.0,
        });

        // Clip to table area
        let rows_y = table_y + TABLE_HEADER_HEIGHT;
        let rows_h = table_h - TABLE_HEADER_HEIGHT;
        rt.push(RenderCommand::PushClip {
            x: table_x,
            y: rows_y,
            width: table_w,
            height: rows_h,
        });

        let filtered = self.filtered_file_types();
        let mut y = rows_y - self.scroll_offset;

        for (i, ft) in filtered.iter().enumerate() {
            if y + ROW_HEIGHT < rows_y {
                y += ROW_HEIGHT;
                continue;
            }
            if y > rows_y + rows_h {
                break;
            }

            let is_selected = self.selected_index == Some(i);
            let row_bg = if is_selected {
                COLOR_SURFACE1
            } else if i % 2 == 0 {
                COLOR_BASE
            } else {
                COLOR_SURFACE0
            };

            rt.push(RenderCommand::FillRect {
                x: table_x,
                y,
                width: table_w,
                height: ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let text_y = y + 9.0;

            // Extension with colored dot
            let cat = self.registry.get_category(&ft.extension);
            rt.push(RenderCommand::FillRect {
                x: col_ext_x,
                y: y + 11.0,
                width: 8.0,
                height: 8.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            let mut ext_display = String::from(".");
            ext_display.push_str(&ft.extension);
            rt.push(RenderCommand::Text {
                x: col_ext_x + 14.0,
                y: text_y,
                text: ext_display,
                color: if is_selected { COLOR_BLUE } else { COLOR_TEXT },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Description
            rt.push(RenderCommand::Text {
                x: col_desc_x,
                y: text_y,
                text: ft.description.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });

            // MIME type
            rt.push(RenderCommand::Text {
                x: col_mime_x,
                y: text_y,
                text: ft.mime_type.clone(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });

            // Default app name
            let app_name = ft
                .default_app_id
                .as_ref()
                .and_then(|id| self.registry.get_app(id))
                .map(|a| a.name.clone())
                .unwrap_or_else(|| String::from("(none)"));
            let app_color = if ft.default_app_id.is_some() {
                COLOR_GREEN
            } else {
                COLOR_OVERLAY0
            };
            rt.push(RenderCommand::Text {
                x: col_app_x,
                y: text_y,
                text: app_name,
                color: app_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(table_w - (col_app_x - table_x) - PADDING),
            });

            y += ROW_HEIGHT;
        }

        rt.push(RenderCommand::PopClip);

        // "No results" message
        if filtered.is_empty() {
            rt.push(RenderCommand::Text {
                x: table_x + table_w / 2.0 - 50.0,
                y: rows_y + 40.0,
                text: String::from("No matching file types"),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render the right-side details panel for the selected file type.
    fn render_details_panel(&self, rt: &mut RenderTree) {
        let panel_x = self.window_width - DETAILS_PANEL_WIDTH;
        let panel_y = TOOLBAR_HEIGHT;
        let panel_h = self.window_height - TOOLBAR_HEIGHT;

        // Panel background
        rt.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: DETAILS_PANEL_WIDTH,
            height: panel_h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line
        rt.push(RenderCommand::Line {
            x1: panel_x,
            y1: panel_y,
            x2: panel_x,
            y2: panel_y + panel_h,
            color: COLOR_SURFACE1,
            width: 1.0,
        });

        let x = panel_x + PADDING;
        let mut y = panel_y + PADDING;

        // Panel title
        rt.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Details"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 28.0;

        let filtered = self.filtered_file_types();
        let selected_ft = self.selected_index.and_then(|i| filtered.get(i).copied());

        match selected_ft {
            None => {
                rt.push(RenderCommand::Text {
                    x,
                    y,
                    text: String::from("Select a file type to see details"),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(DETAILS_PANEL_WIDTH - 2.0 * PADDING),
                });
            }
            Some(ft) => {
                // Extension badge
                let cat = self.registry.get_category(&ft.extension);
                rt.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: 60.0,
                    height: 28.0,
                    color: cat.color(),
                    corner_radii: CornerRadii::all(4.0),
                });
                let mut badge_text = String::from(".");
                badge_text.push_str(&ft.extension);
                rt.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: y + 7.0,
                    text: badge_text,
                    color: COLOR_BASE,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                y += 40.0;

                // Description
                self.render_detail_row(rt, x, y, "Description", &ft.description);
                y += 24.0;

                // MIME type
                self.render_detail_row(rt, x, y, "MIME Type", &ft.mime_type);
                y += 24.0;

                // Category
                self.render_detail_row(rt, x, y, "Category", cat.label());
                y += 24.0;

                // Default app
                let app_name = ft
                    .default_app_id
                    .as_ref()
                    .and_then(|id| self.registry.get_app(id))
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| String::from("(none)"));
                self.render_detail_row(rt, x, y, "Default App", &app_name);
                y += 36.0;

                // "Change Default" button
                rt.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: DETAILS_PANEL_WIDTH - 2.0 * PADDING,
                    height: BUTTON_HEIGHT,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
                rt.push(RenderCommand::Text {
                    x: x + (DETAILS_PANEL_WIDTH - 2.0 * PADDING) / 2.0 - 40.0,
                    y: y + 8.0,
                    text: String::from("Open With..."),
                    color: COLOR_BASE,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                y += BUTTON_HEIGHT + 8.0;

                // "Clear Association" button
                rt.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: DETAILS_PANEL_WIDTH - 2.0 * PADDING,
                    height: BUTTON_HEIGHT,
                    color: COLOR_RED,
                    corner_radii: CornerRadii::all(4.0),
                });
                rt.push(RenderCommand::Text {
                    x: x + (DETAILS_PANEL_WIDTH - 2.0 * PADDING) / 2.0 - 50.0,
                    y: y + 8.0,
                    text: String::from("Clear Association"),
                    color: COLOR_BASE,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                y += BUTTON_HEIGHT + 16.0;

                // Compatible apps list
                rt.push(RenderCommand::Text {
                    x,
                    y,
                    text: String::from("Compatible Apps"),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                y += 20.0;

                let compatible = self.registry.apps_for_extension(&ft.extension);
                if compatible.is_empty() {
                    rt.push(RenderCommand::Text {
                        x,
                        y,
                        text: String::from("No compatible apps"),
                        color: COLOR_OVERLAY0,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                } else {
                    for app in &compatible {
                        let is_default = ft.default_app_id.as_deref() == Some(&app.id);
                        let name_color = if is_default { COLOR_GREEN } else { COLOR_TEXT };

                        rt.push(RenderCommand::FillRect {
                            x,
                            y,
                            width: DETAILS_PANEL_WIDTH - 2.0 * PADDING,
                            height: 24.0,
                            color: if is_default {
                                COLOR_SURFACE0
                            } else {
                                COLOR_MANTLE
                            },
                            corner_radii: CornerRadii::all(3.0),
                        });
                        rt.push(RenderCommand::Text {
                            x: x + 8.0,
                            y: y + 5.0,
                            text: app.name.clone(),
                            color: name_color,
                            font_size: FONT_SIZE_SMALL,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(DETAILS_PANEL_WIDTH - 2.0 * PADDING - 16.0),
                        });

                        y += 26.0;
                    }
                }
            }
        }
    }

    /// Helper: render a label+value detail row.
    fn render_detail_row(&self, rt: &mut RenderTree, x: f32, y: f32, label: &str, value: &str) {
        rt.push(RenderCommand::Text {
            x,
            y,
            text: String::from(label),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        rt.push(RenderCommand::Text {
            x: x + 90.0,
            y,
            text: String::from(value),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(DETAILS_PANEL_WIDTH - 90.0 - 2.0 * PADDING),
        });
    }

    /// Render the "Open With" modal dialog.
    fn render_open_with_dialog(&self, rt: &mut RenderTree) {
        // Dim overlay
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: Color::rgba(0, 0, 0, 128),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog panel
        let dx = (self.window_width - DIALOG_WIDTH) / 2.0;
        let dy = (self.window_height - DIALOG_HEIGHT) / 2.0;

        // Shadow
        rt.push(RenderCommand::BoxShadow {
            x: dx,
            y: dy,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title bar
        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: DIALOG_WIDTH,
            height: 40.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });
        let mut dialog_title = String::from("Open With — .");
        dialog_title.push_str(&self.dialog_target_ext);
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 12.0,
            text: dialog_title,
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // App list
        let compatible = self.registry.apps_for_extension(&self.dialog_target_ext);
        let list_y = dy + 48.0;

        rt.push(RenderCommand::PushClip {
            x: dx,
            y: list_y,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT - 48.0 - 60.0,
        });

        let mut ay = list_y;
        for (i, app) in compatible.iter().enumerate() {
            let is_selected = self.dialog_selected_app == Some(i);
            let row_bg = if is_selected {
                COLOR_BLUE
            } else {
                COLOR_SURFACE0
            };
            let text_color = if is_selected { COLOR_BASE } else { COLOR_TEXT };

            rt.push(RenderCommand::FillRect {
                x: dx + 4.0,
                y: ay,
                width: DIALOG_WIDTH - 8.0,
                height: DIALOG_APP_ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(4.0),
            });
            rt.push(RenderCommand::Text {
                x: dx + 16.0,
                y: ay + 6.0,
                text: app.name.clone(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            rt.push(RenderCommand::Text {
                x: dx + 16.0,
                y: ay + 20.0,
                text: app.exec_path.clone(),
                color: if is_selected {
                    COLOR_MANTLE
                } else {
                    COLOR_OVERLAY0
                },
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(DIALOG_WIDTH - 32.0),
            });

            ay += DIALOG_APP_ROW_HEIGHT;
        }

        rt.push(RenderCommand::PopClip);

        // "Always use this app" checkbox area
        let checkbox_y = dy + DIALOG_HEIGHT - 56.0;
        let box_size = 16.0;
        rt.push(RenderCommand::StrokeRect {
            x: dx + PADDING,
            y: checkbox_y,
            width: box_size,
            height: box_size,
            color: COLOR_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });
        if self.dialog_always_use {
            rt.push(RenderCommand::FillRect {
                x: dx + PADDING + 3.0,
                y: checkbox_y + 3.0,
                width: box_size - 6.0,
                height: box_size - 6.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }
        rt.push(RenderCommand::Text {
            x: dx + PADDING + box_size + 8.0,
            y: checkbox_y + 2.0,
            text: String::from("Always use this app"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom button bar
        let btn_y = dy + DIALOG_HEIGHT - 30.0;

        // Cancel button
        rt.push(RenderCommand::FillRect {
            x: dx + DIALOG_WIDTH - 2.0 * (BUTTON_WIDTH + PADDING),
            y: btn_y - 4.0,
            width: BUTTON_WIDTH - 10.0,
            height: BUTTON_HEIGHT - 4.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + DIALOG_WIDTH - 2.0 * (BUTTON_WIDTH + PADDING) + 20.0,
            y: btn_y,
            text: String::from("Cancel"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // OK button
        rt.push(RenderCommand::FillRect {
            x: dx + DIALOG_WIDTH - BUTTON_WIDTH - PADDING,
            y: btn_y - 4.0,
            width: BUTTON_WIDTH - 10.0,
            height: BUTTON_HEIGHT - 4.0,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + DIALOG_WIDTH - BUTTON_WIDTH - PADDING + 28.0,
            y: btn_y,
            text: String::from("OK"),
            color: COLOR_BASE,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- FileCategory tests --------------------------------------------------

    #[test]
    fn test_category_label() {
        assert_eq!(FileCategory::Documents.label(), "Documents");
        assert_eq!(FileCategory::Images.label(), "Images");
        assert_eq!(FileCategory::Audio.label(), "Audio");
        assert_eq!(FileCategory::Video.label(), "Video");
        assert_eq!(FileCategory::Archives.label(), "Archives");
        assert_eq!(FileCategory::Code.label(), "Code");
        assert_eq!(FileCategory::Other.label(), "Other");
    }

    #[test]
    fn test_category_from_label() {
        assert_eq!(
            FileCategory::from_label("documents"),
            Some(FileCategory::Documents)
        );
        assert_eq!(
            FileCategory::from_label("IMAGES"),
            Some(FileCategory::Images)
        );
        assert_eq!(FileCategory::from_label("Audio"), Some(FileCategory::Audio));
        assert_eq!(FileCategory::from_label("unknown"), None);
    }

    #[test]
    fn test_category_from_label_empty() {
        assert_eq!(FileCategory::from_label(""), None);
    }

    #[test]
    fn test_category_all_count() {
        assert_eq!(FileCategory::ALL.len(), 7);
    }

    #[test]
    fn test_category_icon_non_empty() {
        for cat in FileCategory::ALL {
            assert!(!cat.icon().is_empty());
        }
    }

    #[test]
    fn test_category_color_is_opaque() {
        for cat in FileCategory::ALL {
            assert_eq!(cat.color().a, 255);
        }
    }

    // -- FileType tests ------------------------------------------------------

    #[test]
    fn test_file_type_new() {
        let ft = FileType::new("txt", "text/plain", "Plain Text");
        assert_eq!(ft.extension, "txt");
        assert_eq!(ft.mime_type, "text/plain");
        assert_eq!(ft.description, "Plain Text");
        assert_eq!(ft.default_app_id, None);
    }

    #[test]
    fn test_file_type_with_default_app() {
        let ft = FileType::with_default_app("txt", "text/plain", "Plain Text", "textedit");
        assert_eq!(ft.default_app_id, Some(String::from("textedit")));
    }

    // -- AppInfo tests -------------------------------------------------------

    #[test]
    fn test_app_info_new() {
        let app = AppInfo::new("test", "Test App", "/usr/bin/test", &["txt", "pdf"], 42);
        assert_eq!(app.id, "test");
        assert_eq!(app.name, "Test App");
        assert_eq!(app.exec_path, "/usr/bin/test");
        assert_eq!(app.supported_extensions.len(), 2);
        assert_eq!(app.icon_id, 42);
    }

    #[test]
    fn test_app_supports_extension_case_insensitive() {
        let app = AppInfo::new("test", "Test", "/bin/test", &["TXT", "pdf"], 1);
        assert!(app.supports_extension("txt"));
        assert!(app.supports_extension("TXT"));
        assert!(app.supports_extension("PDF"));
        assert!(!app.supports_extension("doc"));
    }

    #[test]
    fn test_app_supports_no_extensions() {
        let app = AppInfo::new("empty", "Empty", "/bin/empty", &[], 0);
        assert!(!app.supports_extension("txt"));
    }

    // -- Association tests ---------------------------------------------------

    #[test]
    fn test_association_new() {
        let a = Association::new("txt", "textedit");
        assert_eq!(a.extension, "txt");
        assert_eq!(a.app_id, "textedit");
    }

    #[test]
    fn test_association_to_config_line() {
        let a = Association::new("txt", "textedit");
        assert_eq!(a.to_config_line(), "txt=textedit");
    }

    #[test]
    fn test_association_from_config_line_valid() {
        let a = Association::from_config_line("txt=textedit");
        assert!(a.is_some());
        let a = a.expect("tested above");
        assert_eq!(a.extension, "txt");
        assert_eq!(a.app_id, "textedit");
    }

    #[test]
    fn test_association_from_config_line_with_spaces() {
        let a = Association::from_config_line("  pdf = pdfviewer  ");
        assert!(a.is_some());
        let a = a.expect("tested above");
        assert_eq!(a.extension, "pdf");
        assert_eq!(a.app_id, "pdfviewer");
    }

    #[test]
    fn test_association_from_config_line_empty() {
        assert!(Association::from_config_line("").is_none());
    }

    #[test]
    fn test_association_from_config_line_comment() {
        assert!(Association::from_config_line("# comment").is_none());
    }

    #[test]
    fn test_association_from_config_line_no_equals() {
        assert!(Association::from_config_line("txtonly").is_none());
    }

    #[test]
    fn test_association_from_config_line_empty_value() {
        assert!(Association::from_config_line("txt=").is_none());
    }

    #[test]
    fn test_association_from_config_line_empty_key() {
        assert!(Association::from_config_line("=textedit").is_none());
    }

    // -- AssocError Display tests -------------------------------------------

    #[test]
    fn test_error_display_file_type_not_found() {
        let e = AssocError::FileTypeNotFound(String::from("xyz"));
        let s = format!("{e}");
        assert!(s.contains("xyz"));
    }

    #[test]
    fn test_error_display_app_not_found() {
        let e = AssocError::AppNotFound(String::from("noapp"));
        let s = format!("{e}");
        assert!(s.contains("noapp"));
    }

    #[test]
    fn test_error_display_unsupported() {
        let e = AssocError::UnsupportedExtension {
            app_id: String::from("textedit"),
            extension: String::from("mp3"),
        };
        let s = format!("{e}");
        assert!(s.contains("textedit"));
        assert!(s.contains("mp3"));
    }

    #[test]
    fn test_error_display_parse_error() {
        let e = AssocError::ParseError {
            line_number: 5,
            detail: String::from("bad format"),
        };
        let s = format!("{e}");
        assert!(s.contains("5"));
        assert!(s.contains("bad format"));
    }

    // -- AssociationRegistry tests -------------------------------------------

    #[test]
    fn test_registry_new_is_empty() {
        let reg = AssociationRegistry::new();
        assert_eq!(reg.file_type_count(), 0);
        assert_eq!(reg.app_count(), 0);
        assert_eq!(reg.association_count(), 0);
    }

    #[test]
    fn test_registry_with_defaults_has_file_types() {
        let reg = AssociationRegistry::with_defaults();
        assert!(reg.file_type_count() >= 30);
    }

    #[test]
    fn test_registry_with_defaults_has_apps() {
        let reg = AssociationRegistry::with_defaults();
        assert!(reg.app_count() >= 10);
    }

    #[test]
    fn test_registry_with_defaults_has_associations() {
        let reg = AssociationRegistry::with_defaults();
        assert!(reg.association_count() > 0);
    }

    #[test]
    fn test_register_file_type() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("test", "text/test", "Test File"),
            FileCategory::Other,
        );
        assert_eq!(reg.file_type_count(), 1);
        assert!(reg.get_file_type("test").is_some());
    }

    #[test]
    fn test_register_file_type_case_insensitive() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("TXT", "text/plain", "Plain Text"),
            FileCategory::Documents,
        );
        assert!(reg.get_file_type("txt").is_some());
    }

    #[test]
    fn test_remove_file_type() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("test", "text/test", "Test"),
            FileCategory::Other,
        );
        let result = reg.remove_file_type("test");
        assert!(result.is_ok());
        assert_eq!(reg.file_type_count(), 0);
    }

    #[test]
    fn test_remove_file_type_not_found() {
        let mut reg = AssociationRegistry::new();
        let result = reg.remove_file_type("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_category_default() {
        let reg = AssociationRegistry::new();
        assert_eq!(reg.get_category("xyz"), FileCategory::Other);
    }

    #[test]
    fn test_file_types_by_category() {
        let reg = AssociationRegistry::with_defaults();
        let images = reg.file_types_by_category(FileCategory::Images);
        assert!(images.len() >= 5);
        for ft in &images {
            assert_eq!(reg.get_category(&ft.extension), FileCategory::Images);
        }
    }

    #[test]
    fn test_all_extensions_sorted() {
        let reg = AssociationRegistry::with_defaults();
        let exts = reg.all_extensions();
        let mut sorted = exts.clone();
        sorted.sort();
        assert_eq!(exts, sorted);
    }

    #[test]
    fn test_register_app() {
        let mut reg = AssociationRegistry::new();
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        assert_eq!(reg.app_count(), 1);
        assert!(reg.get_app("myapp").is_some());
    }

    #[test]
    fn test_remove_app() {
        let mut reg = AssociationRegistry::new();
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let result = reg.remove_app("myapp");
        assert!(result.is_ok());
        assert_eq!(reg.app_count(), 0);
    }

    #[test]
    fn test_remove_app_not_found() {
        let mut reg = AssociationRegistry::new();
        let result = reg.remove_app("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_app_clears_associations() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let _ = reg.set_default_app("txt", "myapp");
        assert_eq!(reg.association_count(), 1);
        let _ = reg.remove_app("myapp");
        assert_eq!(reg.association_count(), 0);
    }

    #[test]
    fn test_apps_for_extension() {
        let reg = AssociationRegistry::with_defaults();
        let apps = reg.apps_for_extension("html");
        assert!(apps.len() >= 2); // textedit, codeeditor, browser
    }

    #[test]
    fn test_set_default_app() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let result = reg.set_default_app("txt", "myapp");
        assert!(result.is_ok());
        let default = reg.get_default_app("txt");
        assert!(default.is_some());
        assert_eq!(default.map(|a| a.id.as_str()), Some("myapp"));
    }

    #[test]
    fn test_set_default_app_file_type_not_found() {
        let mut reg = AssociationRegistry::new();
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let result = reg.set_default_app("txt", "myapp");
        assert!(matches!(result, Err(AssocError::FileTypeNotFound(_))));
    }

    #[test]
    fn test_set_default_app_app_not_found() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        let result = reg.set_default_app("txt", "noapp");
        assert!(matches!(result, Err(AssocError::AppNotFound(_))));
    }

    #[test]
    fn test_set_default_app_unsupported_extension() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        reg.register_app(AppInfo::new("imgapp", "Img", "/bin/img", &["png"], 1));
        let result = reg.set_default_app("txt", "imgapp");
        assert!(matches!(
            result,
            Err(AssocError::UnsupportedExtension { .. })
        ));
    }

    #[test]
    fn test_clear_association() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let _ = reg.set_default_app("txt", "myapp");
        let result = reg.clear_association("txt");
        assert!(result.is_ok());
        assert!(reg.get_default_app("txt").is_none());
    }

    #[test]
    fn test_clear_association_not_found() {
        let mut reg = AssociationRegistry::new();
        let result = reg.clear_association("nonexistent");
        assert!(matches!(result, Err(AssocError::FileTypeNotFound(_))));
    }

    #[test]
    fn test_reset_to_defaults() {
        let mut reg = AssociationRegistry::with_defaults();
        let _ = reg.clear_association("txt");
        reg.reset_to_defaults();
        // After reset, txt should have a default again.
        assert!(reg.get_default_app("txt").is_some());
    }

    // -- Search tests --------------------------------------------------------

    #[test]
    fn test_search_by_extension() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search("png");
        assert!(!results.is_empty());
        assert!(results.iter().any(|ft| ft.extension == "png"));
    }

    #[test]
    fn test_search_by_description() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search("Video");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_by_mime() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search("image/");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_empty_query_returns_all() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search("");
        assert_eq!(results.len(), reg.file_type_count());
    }

    #[test]
    fn test_search_no_results() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search("zzzznonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_in_category() {
        let reg = AssociationRegistry::with_defaults();
        let results = reg.search_in_category("mp", FileCategory::Audio);
        assert!(!results.is_empty());
        for ft in &results {
            assert_eq!(reg.get_category(&ft.extension), FileCategory::Audio);
        }
    }

    // -- Import/Export tests -------------------------------------------------

    #[test]
    fn test_export_config() {
        let mut reg = AssociationRegistry::new();
        reg.register_file_type(
            FileType::new("txt", "text/plain", "Text"),
            FileCategory::Documents,
        );
        reg.register_app(AppInfo::new("myapp", "My App", "/bin/myapp", &["txt"], 1));
        let _ = reg.set_default_app("txt", "myapp");
        let config = reg.export_config();
        assert!(config.contains("txt=myapp"));
        assert!(config.starts_with("# Slate OS File Associations"));
    }

    #[test]
    fn test_import_config_valid() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "txt=codeeditor\npng=imageeditor\n";
        let errors = reg.import_config(config);
        assert!(errors.is_empty());
        let app = reg.get_default_app("txt");
        assert!(app.is_some());
        assert_eq!(app.map(|a| a.id.as_str()), Some("codeeditor"));
    }

    #[test]
    fn test_import_config_with_comments() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "# comment\n\ntxt=codeeditor\n";
        let errors = reg.import_config(config);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_import_config_invalid_lines() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "badline\ntxt=codeeditor\n";
        let errors = reg.import_config(config);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_import_config_nonexistent_app() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "txt=doesnotexist\n";
        let errors = reg.import_config(config);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_export_import_roundtrip() {
        let reg = AssociationRegistry::with_defaults();
        let config = reg.export_config();

        let mut reg2 = AssociationRegistry::with_defaults();
        // Clear all associations first.
        let exts: Vec<String> = reg2.all_extensions();
        for ext in &exts {
            let _ = reg2.clear_association(ext);
        }
        let errors = reg2.import_config(&config);
        assert!(errors.is_empty());

        // All original associations should be restored.
        for (ext, assoc) in &reg.associations {
            let restored = reg2.get_default_app(ext);
            assert!(
                restored.is_some(),
                "association for .{ext} was not restored"
            );
            assert_eq!(restored.map(|a| a.id.as_str()), Some(assoc.app_id.as_str()),);
        }
    }

    // -- UI state tests ------------------------------------------------------

    #[test]
    fn test_ui_default() {
        let ui = FileAssocUI::new();
        assert!(ui.selected_category.is_none());
        assert!(ui.search_query.is_empty());
        assert!(ui.selected_index.is_none());
        assert_eq!(ui.active_dialog, ActiveDialog::None);
    }

    #[test]
    fn test_ui_filtered_all() {
        let ui = FileAssocUI::new();
        let filtered = ui.filtered_file_types();
        assert_eq!(filtered.len(), ui.registry.file_type_count());
    }

    #[test]
    fn test_ui_filtered_by_category() {
        let mut ui = FileAssocUI::new();
        ui.select_category(Some(FileCategory::Images));
        let filtered = ui.filtered_file_types();
        assert!(!filtered.is_empty());
        for ft in &filtered {
            assert_eq!(
                ui.registry.get_category(&ft.extension),
                FileCategory::Images
            );
        }
    }

    #[test]
    fn test_ui_filtered_by_search() {
        let mut ui = FileAssocUI::new();
        ui.set_search_query("png");
        let filtered = ui.filtered_file_types();
        assert!(!filtered.is_empty());
        assert!(filtered.iter().any(|ft| ft.extension == "png"));
    }

    #[test]
    fn test_ui_filtered_by_category_and_search() {
        let mut ui = FileAssocUI::new();
        ui.select_category(Some(FileCategory::Code));
        ui.set_search_query("rust");
        let filtered = ui.filtered_file_types();
        assert!(!filtered.is_empty());
        for ft in &filtered {
            assert_eq!(ui.registry.get_category(&ft.extension), FileCategory::Code);
        }
    }

    #[test]
    fn test_ui_select_file_type() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        assert_eq!(ui.selected_index, Some(0));
    }

    #[test]
    fn test_ui_select_file_type_out_of_bounds() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(9999);
        assert_eq!(ui.selected_index, None);
    }

    #[test]
    fn test_ui_selected_file_type() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        assert!(ui.selected_file_type().is_some());
    }

    #[test]
    fn test_ui_select_category_resets_selection() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        ui.select_category(Some(FileCategory::Audio));
        assert_eq!(ui.selected_index, None);
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn test_ui_set_search_resets_selection() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        ui.set_search_query("test");
        assert_eq!(ui.selected_index, None);
    }

    #[test]
    fn test_ui_open_with_dialog() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        ui.open_open_with_dialog();
        assert_eq!(ui.active_dialog, ActiveDialog::OpenWith);
        assert!(!ui.dialog_target_ext.is_empty());
    }

    #[test]
    fn test_ui_open_with_dialog_no_selection() {
        let mut ui = FileAssocUI::new();
        ui.open_open_with_dialog();
        assert_eq!(ui.active_dialog, ActiveDialog::None);
    }

    #[test]
    fn test_ui_confirm_open_with() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        ui.open_open_with_dialog();
        ui.dialog_always_use = true;
        ui.dialog_selected_app = Some(0);
        let result = ui.confirm_open_with();
        assert!(result.is_ok());
        assert_eq!(ui.active_dialog, ActiveDialog::None);
    }

    // -- Render tests --------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let ui = FileAssocUI::new();
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_selection() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_open_with_dialog() {
        let mut ui = FileAssocUI::new();
        ui.select_file_type(0);
        ui.open_open_with_dialog();
        let rt = ui.render();
        // Dialog adds overlay + many commands.
        assert!(rt.len() > 50);
    }

    #[test]
    fn test_render_empty_search() {
        let mut ui = FileAssocUI::new();
        ui.set_search_query("zzzznonexistent");
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_category_filter() {
        let mut ui = FileAssocUI::new();
        ui.select_category(Some(FileCategory::Audio));
        let rt = ui.render();
        assert!(!rt.is_empty());
    }
}
