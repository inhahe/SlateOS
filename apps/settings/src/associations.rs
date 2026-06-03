//! File type associations settings page.
//!
//! Manages the mapping from file extensions/MIME types to default applications.
//! Supports per-extension icons, fallback handlers (auto-switching when an app
//! is uninstalled), category-based filtering, and search.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

// ============================================================================
// Theme colors (same palette as main settings)
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);

// ============================================================================
// Layout constants
// ============================================================================

const ROW_HEIGHT: f32 = 52.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
const TAB_HEIGHT: f32 = 36.0;
const TAB_PADDING: f32 = 16.0;
const ICON_SIZE: f32 = 24.0;
const EXPAND_PANEL_HEIGHT: f32 = 180.0;
const RADIO_SIZE: f32 = 18.0;
const SECTION_SPACING: f32 = 12.0;
const MAX_HANDLER_HISTORY: usize = 3;

// ============================================================================
// Data types
// ============================================================================

/// Category of a file type, used for filtering in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileCategory {
    All,
    Documents,
    Images,
    Audio,
    Video,
    Code,
    Archives,
    Other,
}

impl FileCategory {
    pub const ALL: &[Self] = &[
        Self::All,
        Self::Documents,
        Self::Images,
        Self::Audio,
        Self::Video,
        Self::Code,
        Self::Archives,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Documents => "Documents",
            Self::Images => "Images",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Code => "Code",
            Self::Archives => "Archives",
            Self::Other => "Other",
        }
    }
}

/// Information about an application that can handle file types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub exec_path: String,
    pub installed: bool,
}

/// A single file type association entry.
#[derive(Clone, Debug)]
pub struct AssociationEntry {
    pub extension: String,
    pub mime_type: String,
    pub description: String,
    pub default_app: Option<AppInfo>,
    pub available_apps: Vec<AppInfo>,
    pub icon_id: Option<String>,
    pub fallback_app: Option<AppInfo>,
    /// History of previous handlers (most recent first), up to MAX_HANDLER_HISTORY.
    pub handler_history: Vec<AppInfo>,
}

impl AssociationEntry {
    /// Determine the category of this file type based on its extension.
    pub fn category(&self) -> FileCategory {
        categorize_extension(&self.extension)
    }
}

// ============================================================================
// Category detection
// ============================================================================

/// Determine the category of a file extension.
fn categorize_extension(ext: &str) -> FileCategory {
    let lower = ext.to_lowercase();
    let ext_part = lower.strip_prefix('.').unwrap_or(&lower);

    match ext_part {
        // Documents
        "txt" | "doc" | "docx" | "odt" | "rtf" | "pdf" | "epub" | "md" | "tex" | "csv" | "xls"
        | "xlsx" | "ods" | "ppt" | "pptx" | "odp" => FileCategory::Documents,
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff" | "tif"
        | "psd" | "raw" | "heif" | "heic" | "avif" => FileCategory::Images,
        // Audio
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "wma" | "m4a" | "opus" | "mid" | "midi"
        | "aiff" => FileCategory::Audio,
        // Video
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpeg" | "mpg" | "3gp" => {
            FileCategory::Video
        }
        // Code
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "hpp" | "java" | "go" | "rb" | "swift"
        | "kt" | "cs" | "php" | "sh" | "bash" | "zsh" | "lua" | "zig" | "asm" | "s" | "toml"
        | "yaml" | "yml" | "json" | "xml" | "html" | "css" | "sql" | "make" | "cmake" => {
            FileCategory::Code
        }
        // Archives
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz4" | "lzma" | "cab"
        | "iso" | "dmg" => FileCategory::Archives,
        // Other
        _ => FileCategory::Other,
    }
}

// ============================================================================
// Association manager
// ============================================================================

/// Manages all file type associations for the system.
pub struct AssociationManager {
    entries: Vec<AssociationEntry>,
}

impl AssociationManager {
    /// Create a new manager pre-populated with common file type associations.
    pub fn new() -> Self {
        Self {
            entries: Self::default_entries(),
        }
    }

    /// Set the default handler for a given extension.
    /// Stores the previous handler as fallback and in the history.
    pub fn set_default(&mut self, extension: &str, app_id: &str) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.extension == extension) else {
            return false;
        };

        // Find the app in available apps
        let Some(new_app) = entry
            .available_apps
            .iter()
            .find(|a| a.id == app_id)
            .cloned()
        else {
            return false;
        };

        // Store current as fallback and in history
        if let Some(prev) = entry.default_app.take() {
            entry.fallback_app = Some(prev.clone());
            // Push to history, keep only MAX_HANDLER_HISTORY entries
            entry.handler_history.insert(0, prev);
            entry.handler_history.truncate(MAX_HANDLER_HISTORY);
        }

        entry.default_app = Some(new_app);
        true
    }

    /// Get the current default handler for an extension.
    pub fn get_default(&self, extension: &str) -> Option<&AppInfo> {
        self.entries
            .iter()
            .find(|e| e.extension == extension)
            .and_then(|e| e.default_app.as_ref())
    }

    /// Get all available apps that can handle a given extension.
    pub fn get_available_apps(&self, extension: &str) -> &[AppInfo] {
        self.entries
            .iter()
            .find(|e| e.extension == extension)
            .map(|e| e.available_apps.as_slice())
            .unwrap_or(&[])
    }

    /// Register a new handler application for an extension.
    pub fn add_association(&mut self, extension: &str, app: AppInfo) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.extension == extension) {
            // Don't add duplicates
            if !entry.available_apps.iter().any(|a| a.id == app.id) {
                entry.available_apps.push(app);
            }
        }
    }

    /// Remove a handler application from an extension's available apps.
    /// Does NOT handle the uninstall fallback logic; use `handle_uninstall` for that.
    pub fn remove_association(&mut self, extension: &str, app_id: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.extension == extension) {
            entry.available_apps.retain(|a| a.id != app_id);
        }
    }

    /// Handle app uninstallation: for every extension where `app_id` is the
    /// default, switch to the fallback handler (or clear if no fallback).
    /// Returns the list of extensions that were affected.
    pub fn handle_uninstall(&mut self, app_id: &str) -> Vec<String> {
        let mut affected = Vec::new();

        for entry in &mut self.entries {
            // Mark app as uninstalled in available list
            if let Some(app) = entry.available_apps.iter_mut().find(|a| a.id == app_id) {
                app.installed = false;
            }

            // If this app is currently the default, fall back
            let is_default = entry
                .default_app
                .as_ref()
                .map(|a| a.id == app_id)
                .unwrap_or(false);

            if is_default {
                affected.push(entry.extension.clone());

                // Try fallback first
                if let Some(fallback) = entry.fallback_app.take() {
                    if fallback.installed {
                        entry.default_app = Some(fallback);
                    } else {
                        // Fallback is also uninstalled; walk history
                        entry.default_app = None;
                        for hist_app in &entry.handler_history {
                            if hist_app.id != app_id && hist_app.installed {
                                entry.default_app = Some(hist_app.clone());
                                break;
                            }
                        }
                    }
                } else {
                    // No fallback; try history
                    entry.default_app = None;
                    for hist_app in &entry.handler_history {
                        if hist_app.id != app_id && hist_app.installed {
                            entry.default_app = Some(hist_app.clone());
                            break;
                        }
                    }
                }
            }

            // Remove uninstalled app from history
            entry.handler_history.retain(|a| a.id != app_id);
            // Remove from fallback if it matches
            if entry
                .fallback_app
                .as_ref()
                .map(|a| a.id == app_id)
                .unwrap_or(false)
            {
                entry.fallback_app = None;
            }
        }

        affected
    }

    /// Search associations by extension or description.
    pub fn search(&self, query: &str) -> Vec<&AssociationEntry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        let lower_query = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.extension.to_lowercase().contains(&lower_query)
                    || e.description.to_lowercase().contains(&lower_query)
                    || e.mime_type.to_lowercase().contains(&lower_query)
            })
            .collect()
    }

    /// Get all entries, optionally filtered by category.
    pub fn entries_by_category(&self, category: FileCategory) -> Vec<&AssociationEntry> {
        if category == FileCategory::All {
            return self.entries.iter().collect();
        }
        self.entries
            .iter()
            .filter(|e| e.category() == category)
            .collect()
    }

    /// Get all entries sorted alphabetically by extension.
    pub fn entries_sorted(&self) -> Vec<&AssociationEntry> {
        let mut sorted: Vec<&AssociationEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| a.extension.cmp(&b.extension));
        sorted
    }

    /// Get entries filtered by category and search query, sorted alphabetically.
    pub fn filtered_entries(&self, category: FileCategory, query: &str) -> Vec<&AssociationEntry> {
        let lower_query = query.to_lowercase();
        let mut results: Vec<&AssociationEntry> = self
            .entries
            .iter()
            .filter(|e| {
                // Category filter
                if category != FileCategory::All && e.category() != category {
                    return false;
                }
                // Search filter
                if !query.is_empty() {
                    let matches_ext = e.extension.to_lowercase().contains(&lower_query);
                    let matches_desc = e.description.to_lowercase().contains(&lower_query);
                    let matches_mime = e.mime_type.to_lowercase().contains(&lower_query);
                    if !(matches_ext || matches_desc || matches_mime) {
                        return false;
                    }
                }
                true
            })
            .collect();
        results.sort_by(|a, b| a.extension.cmp(&b.extension));
        results
    }

    /// Get the total number of registered file types.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Create default stub entries pre-populated with realistic data.
    fn default_entries() -> Vec<AssociationEntry> {
        let text_editor = AppInfo {
            id: "com.ouros.texteditor".into(),
            name: "Text Editor".into(),
            icon: Some("text-editor".into()),
            exec_path: "/usr/bin/texteditor".into(),
            installed: true,
        };
        let image_viewer = AppInfo {
            id: "com.ouros.imageviewer".into(),
            name: "Image Viewer".into(),
            icon: Some("image-viewer".into()),
            exec_path: "/usr/bin/imageviewer".into(),
            installed: true,
        };
        let music_player = AppInfo {
            id: "com.ouros.musicplayer".into(),
            name: "Music Player".into(),
            icon: Some("music-player".into()),
            exec_path: "/usr/bin/musicplayer".into(),
            installed: true,
        };
        let video_player = AppInfo {
            id: "com.ouros.videoplayer".into(),
            name: "Video Player".into(),
            icon: Some("video-player".into()),
            exec_path: "/usr/bin/videoplayer".into(),
            installed: true,
        };
        let file_manager = AppInfo {
            id: "com.ouros.filemanager".into(),
            name: "File Manager".into(),
            icon: Some("file-manager".into()),
            exec_path: "/usr/bin/filemanager".into(),
            installed: true,
        };
        let archive_tool = AppInfo {
            id: "com.ouros.archiver".into(),
            name: "Archive Manager".into(),
            icon: Some("archive-manager".into()),
            exec_path: "/usr/bin/archiver".into(),
            installed: true,
        };
        let code_editor = AppInfo {
            id: "com.ouros.codeeditor".into(),
            name: "Code Editor".into(),
            icon: Some("code-editor".into()),
            exec_path: "/usr/bin/codeeditor".into(),
            installed: true,
        };

        vec![
            // Documents
            Self::entry(
                ".txt",
                "text/plain",
                "Text Document",
                Some(text_editor.clone()),
                vec![text_editor.clone(), code_editor.clone()],
                Some("file-text"),
            ),
            Self::entry(
                ".md",
                "text/markdown",
                "Markdown Document",
                Some(text_editor.clone()),
                vec![text_editor.clone(), code_editor.clone()],
                Some("file-markdown"),
            ),
            Self::entry(
                ".pdf",
                "application/pdf",
                "PDF Document",
                None,
                vec![],
                Some("file-pdf"),
            ),
            Self::entry(
                ".doc",
                "application/msword",
                "Word Document",
                Some(text_editor.clone()),
                vec![text_editor.clone()],
                Some("file-doc"),
            ),
            Self::entry(
                ".docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "Word Document (OOXML)",
                Some(text_editor.clone()),
                vec![text_editor.clone()],
                Some("file-doc"),
            ),
            Self::entry(
                ".odt",
                "application/vnd.oasis.opendocument.text",
                "OpenDocument Text",
                Some(text_editor.clone()),
                vec![text_editor.clone()],
                Some("file-doc"),
            ),
            Self::entry(
                ".rtf",
                "application/rtf",
                "Rich Text Format",
                Some(text_editor.clone()),
                vec![text_editor.clone()],
                Some("file-text"),
            ),
            Self::entry(
                ".csv",
                "text/csv",
                "Comma-Separated Values",
                Some(text_editor.clone()),
                vec![text_editor.clone(), code_editor.clone()],
                Some("file-spreadsheet"),
            ),
            Self::entry(
                ".epub",
                "application/epub+zip",
                "E-Book (EPUB)",
                None,
                vec![],
                Some("file-book"),
            ),
            Self::entry(
                ".tex",
                "application/x-tex",
                "LaTeX Document",
                Some(code_editor.clone()),
                vec![text_editor.clone(), code_editor.clone()],
                Some("file-code"),
            ),
            // Images
            Self::entry(
                ".png",
                "image/png",
                "PNG Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".jpg",
                "image/jpeg",
                "JPEG Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".jpeg",
                "image/jpeg",
                "JPEG Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".gif",
                "image/gif",
                "GIF Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".bmp",
                "image/bmp",
                "Bitmap Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".svg",
                "image/svg+xml",
                "SVG Vector Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone(), code_editor.clone()],
                Some("file-svg"),
            ),
            Self::entry(
                ".webp",
                "image/webp",
                "WebP Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".ico",
                "image/x-icon",
                "Icon File",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".tiff",
                "image/tiff",
                "TIFF Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".heic",
                "image/heic",
                "HEIC Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            Self::entry(
                ".avif",
                "image/avif",
                "AVIF Image",
                Some(image_viewer.clone()),
                vec![image_viewer.clone()],
                Some("file-image"),
            ),
            // Audio
            Self::entry(
                ".mp3",
                "audio/mpeg",
                "MP3 Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".wav",
                "audio/wav",
                "WAV Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".flac",
                "audio/flac",
                "FLAC Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".ogg",
                "audio/ogg",
                "Ogg Vorbis Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".aac",
                "audio/aac",
                "AAC Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".m4a",
                "audio/mp4",
                "M4A Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".opus",
                "audio/opus",
                "Opus Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-audio"),
            ),
            Self::entry(
                ".mid",
                "audio/midi",
                "MIDI Audio",
                Some(music_player.clone()),
                vec![music_player.clone()],
                Some("file-midi"),
            ),
            // Video
            Self::entry(
                ".mp4",
                "video/mp4",
                "MP4 Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            Self::entry(
                ".mkv",
                "video/x-matroska",
                "Matroska Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            Self::entry(
                ".avi",
                "video/x-msvideo",
                "AVI Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            Self::entry(
                ".mov",
                "video/quicktime",
                "QuickTime Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            Self::entry(
                ".webm",
                "video/webm",
                "WebM Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            Self::entry(
                ".flv",
                "video/x-flv",
                "Flash Video",
                Some(video_player.clone()),
                vec![video_player.clone()],
                Some("file-video"),
            ),
            // Code
            Self::entry(
                ".rs",
                "text/x-rust",
                "Rust Source File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-rust"),
            ),
            Self::entry(
                ".py",
                "text/x-python",
                "Python Script",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-python"),
            ),
            Self::entry(
                ".js",
                "text/javascript",
                "JavaScript File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-javascript"),
            ),
            Self::entry(
                ".ts",
                "text/typescript",
                "TypeScript File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-typescript"),
            ),
            Self::entry(
                ".c",
                "text/x-c",
                "C Source File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-c"),
            ),
            Self::entry(
                ".cpp",
                "text/x-c++",
                "C++ Source File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-cpp"),
            ),
            Self::entry(
                ".h",
                "text/x-c-header",
                "C/C++ Header File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-header"),
            ),
            Self::entry(
                ".java",
                "text/x-java",
                "Java Source File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-java"),
            ),
            Self::entry(
                ".go",
                "text/x-go",
                "Go Source File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-go"),
            ),
            Self::entry(
                ".html",
                "text/html",
                "HTML Document",
                None,
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-html"),
            ),
            Self::entry(
                ".css",
                "text/css",
                "CSS Stylesheet",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-css"),
            ),
            Self::entry(
                ".json",
                "application/json",
                "JSON File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-json"),
            ),
            Self::entry(
                ".toml",
                "application/toml",
                "TOML Configuration",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-config"),
            ),
            Self::entry(
                ".yaml",
                "application/x-yaml",
                "YAML File",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-config"),
            ),
            Self::entry(
                ".xml",
                "application/xml",
                "XML Document",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-xml"),
            ),
            Self::entry(
                ".sh",
                "application/x-sh",
                "Shell Script",
                Some(code_editor.clone()),
                vec![code_editor.clone(), text_editor.clone()],
                Some("file-script"),
            ),
            // Archives
            Self::entry(
                ".zip",
                "application/zip",
                "ZIP Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone(), file_manager.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".tar",
                "application/x-tar",
                "Tar Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".gz",
                "application/gzip",
                "Gzip Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".7z",
                "application/x-7z-compressed",
                "7-Zip Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".rar",
                "application/vnd.rar",
                "RAR Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".xz",
                "application/x-xz",
                "XZ Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".zst",
                "application/zstd",
                "Zstandard Archive",
                Some(archive_tool.clone()),
                vec![archive_tool.clone()],
                Some("file-archive"),
            ),
            Self::entry(
                ".iso",
                "application/x-iso9660-image",
                "Disc Image (ISO)",
                Some(file_manager.clone()),
                vec![file_manager.clone(), archive_tool.clone()],
                Some("file-disc"),
            ),
        ]
    }

    /// Helper to create an AssociationEntry with default empty history.
    fn entry(
        ext: &str,
        mime: &str,
        desc: &str,
        default: Option<AppInfo>,
        available: Vec<AppInfo>,
        icon: Option<&str>,
    ) -> AssociationEntry {
        AssociationEntry {
            extension: ext.into(),
            mime_type: mime.into(),
            description: desc.into(),
            default_app: default,
            available_apps: available,
            icon_id: icon.map(|s| s.into()),
            fallback_app: None,
            handler_history: Vec::new(),
        }
    }
}

// ============================================================================
// UI state
// ============================================================================

/// State for the file type associations settings page.
pub struct AssociationsPageState {
    pub manager: AssociationManager,
    pub search_query: String,
    pub active_category: FileCategory,
    pub expanded_index: Option<usize>,
    pub scroll_offset: f32,
    pub hovered_row: Option<usize>,
}

impl AssociationsPageState {
    pub fn new() -> Self {
        Self {
            manager: AssociationManager::new(),
            search_query: String::new(),
            active_category: FileCategory::All,
            expanded_index: None,
            scroll_offset: 0.0,
            hovered_row: None,
        }
    }

    /// Get the filtered and sorted entries based on current UI state.
    pub fn visible_entries(&self) -> Vec<&AssociationEntry> {
        self.manager
            .filtered_entries(self.active_category, &self.search_query)
    }

    /// Render the full associations page into a RenderTree.
    pub fn render(&self, tree: &mut RenderTree, x: f32, start_y: f32, content_width: f32) {
        let mut y = start_y;

        // Page title
        y = self.render_page_title(tree, x, y);

        // Search bar
        y = self.render_search_bar(tree, x, y, content_width);
        y += SECTION_SPACING;

        // Category tabs
        y = self.render_category_tabs(tree, x, y);
        y += SECTION_SPACING;

        // File type list
        self.render_file_list(tree, x, y, content_width);
    }

    /// Render the page title.
    fn render_page_title(&self, tree: &mut RenderTree, x: f32, y: f32) -> f32 {
        tree.push(RenderCommand::Text {
            x,
            y,
            text: "File Type Associations".into(),
            color: COL_TEXT,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x,
            y: y + 28.0,
            text: "Choose which apps open each file type".into(),
            color: COL_SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y + 56.0
    }

    /// Render the search/filter bar.
    fn render_search_bar(&self, tree: &mut RenderTree, x: f32, y: f32, content_width: f32) -> f32 {
        // Search field background
        let bar_width = content_width.min(500.0);
        tree.fill_rounded_rect(
            x,
            y,
            bar_width,
            SEARCH_BAR_HEIGHT,
            COL_SURFACE0,
            CornerRadii::all(8.0),
        );
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: bar_width,
            height: SEARCH_BAR_HEIGHT,
            color: COL_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Search icon placeholder
        tree.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 12.0,
            text: "\u{1F50D}".into(), // magnifying glass
            color: COL_OVERLAY0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Search text or placeholder
        let display_text = if self.search_query.is_empty() {
            "Search by extension, type, or description..."
        } else {
            &self.search_query
        };
        let text_color = if self.search_query.is_empty() {
            COL_OVERLAY0
        } else {
            COL_TEXT
        };
        tree.push(RenderCommand::Text {
            x: x + 36.0,
            y: y + 12.0,
            text: display_text.into(),
            color: text_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_width - 48.0),
        });

        y + SEARCH_BAR_HEIGHT
    }

    /// Render the category filter tabs.
    fn render_category_tabs(&self, tree: &mut RenderTree, x: f32, y: f32) -> f32 {
        let mut tab_x = x;

        for &cat in FileCategory::ALL {
            let label = cat.label();
            let is_active = cat == self.active_category;

            // Approximate tab width based on label length
            let tab_width = (label.len() as f32) * 8.0 + TAB_PADDING * 2.0;

            // Tab background
            let bg_color = if is_active { COL_ACCENT } else { COL_SURFACE0 };
            tree.fill_rounded_rect(
                tab_x,
                y,
                tab_width,
                TAB_HEIGHT,
                bg_color,
                CornerRadii::all(6.0),
            );

            // Tab text
            let text_color = if is_active { COL_BASE } else { COL_SUBTEXT0 };
            tree.push(RenderCommand::Text {
                x: tab_x + TAB_PADDING,
                y: y + 10.0,
                text: label.into(),
                color: text_color,
                font_size: 12.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            tab_x += tab_width + 8.0;
        }

        y + TAB_HEIGHT
    }

    /// Render the scrollable file type list.
    fn render_file_list(&self, tree: &mut RenderTree, x: f32, start_y: f32, content_width: f32) {
        let entries = self.visible_entries();
        let mut y = start_y - self.scroll_offset;

        for (idx, entry) in entries.iter().enumerate() {
            // Skip rows above viewport (basic culling)
            if y + ROW_HEIGHT < start_y - 100.0 {
                y += ROW_HEIGHT;
                if self.expanded_index == Some(idx) {
                    y += EXPAND_PANEL_HEIGHT;
                }
                continue;
            }

            let is_expanded = self.expanded_index == Some(idx);
            let is_hovered = self.hovered_row == Some(idx);

            // Row background
            let row_bg = if is_expanded {
                COL_SURFACE0
            } else if is_hovered {
                COL_SURFACE0
            } else {
                COL_BASE
            };
            tree.fill_rounded_rect(
                x,
                y,
                content_width,
                ROW_HEIGHT,
                row_bg,
                CornerRadii::all(4.0),
            );

            // File type icon area
            let icon_x = x + 12.0;
            let icon_y = y + (ROW_HEIGHT - ICON_SIZE) / 2.0;
            tree.fill_rounded_rect(
                icon_x,
                icon_y,
                ICON_SIZE,
                ICON_SIZE,
                COL_SURFACE1,
                CornerRadii::all(4.0),
            );

            // Extension text (bold, prominent)
            let ext_x = x + 48.0;
            tree.push(RenderCommand::Text {
                x: ext_x,
                y: y + 10.0,
                text: entry.extension.clone(),
                color: COL_TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Description text (below extension)
            tree.push(RenderCommand::Text {
                x: ext_x,
                y: y + 30.0,
                text: entry.description.clone(),
                color: COL_SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });

            // Default app name (right side)
            let app_label = match &entry.default_app {
                Some(app) => app.name.clone(),
                None => "No app assigned".into(),
            };
            let app_color = if entry.default_app.is_some() {
                COL_SUBTEXT1
            } else {
                COL_PEACH
            };
            tree.push(RenderCommand::Text {
                x: x + content_width - 200.0,
                y: y + 18.0,
                text: app_label,
                color: app_color,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });

            y += ROW_HEIGHT;

            // Expanded detail panel
            if is_expanded {
                y = self.render_expanded_panel(tree, x, y, content_width, entry);
            }

            // Separator line
            tree.push(RenderCommand::Line {
                x1: x + 12.0,
                y1: y,
                x2: x + content_width - 12.0,
                y2: y,
                color: COL_SURFACE0,
                width: 1.0,
            });
        }
    }

    /// Render the expanded details panel for a selected file type.
    fn render_expanded_panel(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        content_width: f32,
        entry: &AssociationEntry,
    ) -> f32 {
        let panel_x = x + 16.0;
        let panel_width = content_width - 32.0;
        let mut py = y + 8.0;

        // Panel background
        tree.fill_rounded_rect(
            x + 8.0,
            y,
            content_width - 16.0,
            EXPAND_PANEL_HEIGHT,
            COL_SURFACE0,
            CornerRadii::all(8.0),
        );

        // "Choose default app:" label
        tree.push(RenderCommand::Text {
            x: panel_x,
            y: py,
            text: "Choose default app:".into(),
            color: COL_TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        py += 24.0;

        // Radio buttons for each available app
        for app in &entry.available_apps {
            let is_selected = entry
                .default_app
                .as_ref()
                .map(|d| d.id == app.id)
                .unwrap_or(false);

            // Radio button circle
            let radio_y = py + 2.0;
            let radio_color = if is_selected {
                COL_ACCENT
            } else {
                COL_SURFACE2
            };
            tree.push(RenderCommand::StrokeRect {
                x: panel_x,
                y: radio_y,
                width: RADIO_SIZE,
                height: RADIO_SIZE,
                color: radio_color,
                line_width: 2.0,
                corner_radii: CornerRadii::all(RADIO_SIZE / 2.0),
            });
            if is_selected {
                let inner_size = RADIO_SIZE - 8.0;
                let inner_offset = 4.0;
                tree.fill_rounded_rect(
                    panel_x + inner_offset,
                    radio_y + inner_offset,
                    inner_size,
                    inner_size,
                    COL_ACCENT,
                    CornerRadii::all(inner_size / 2.0),
                );
            }

            // App name
            let name_color = if app.installed {
                COL_TEXT
            } else {
                COL_OVERLAY0
            };
            tree.push(RenderCommand::Text {
                x: panel_x + RADIO_SIZE + 10.0,
                y: py + 2.0,
                text: app.name.clone(),
                color: name_color,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 100.0),
            });

            // "(uninstalled)" marker
            if !app.installed {
                let marker_x = panel_x + RADIO_SIZE + 10.0 + (app.name.len() as f32 * 7.5) + 8.0;
                tree.push(RenderCommand::Text {
                    x: marker_x,
                    y: py + 2.0,
                    text: "(uninstalled)".into(),
                    color: COL_RED,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            py += 26.0;
        }

        // "Choose another app..." button
        py += 8.0;
        let btn_width = 180.0;
        let btn_height = 28.0;
        tree.fill_rounded_rect(
            panel_x,
            py,
            btn_width,
            btn_height,
            COL_SURFACE1,
            CornerRadii::all(6.0),
        );
        tree.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: py + 7.0,
            text: "Choose another app...".into(),
            color: COL_ACCENT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // "Reset to default" button
        let reset_x = panel_x + btn_width + 16.0;
        tree.fill_rounded_rect(
            reset_x,
            py,
            140.0,
            btn_height,
            COL_SURFACE1,
            CornerRadii::all(6.0),
        );
        tree.push(RenderCommand::Text {
            x: reset_x + 12.0,
            y: py + 7.0,
            text: "Reset to default".into(),
            color: COL_SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        y + EXPAND_PANEL_HEIGHT
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_entries_populated() {
        let mgr = AssociationManager::new();
        // Should have a substantial number of pre-populated file types
        assert!(
            mgr.count() >= 40,
            "Expected at least 40 entries, got {}",
            mgr.count()
        );
    }

    #[test]
    fn test_set_and_get_default() {
        let mut mgr = AssociationManager::new();

        // .txt should start with text editor as default
        let default = mgr.get_default(".txt");
        assert!(default.is_some());
        assert_eq!(default.unwrap().id, "com.ouros.texteditor");

        // Set code editor as default
        let result = mgr.set_default(".txt", "com.ouros.codeeditor");
        assert!(result);

        let new_default = mgr.get_default(".txt");
        assert!(new_default.is_some());
        assert_eq!(new_default.unwrap().id, "com.ouros.codeeditor");
    }

    #[test]
    fn test_set_default_stores_fallback() {
        let mut mgr = AssociationManager::new();

        // Change default for .txt from text editor to code editor
        mgr.set_default(".txt", "com.ouros.codeeditor");

        // The fallback should now be the text editor
        let entry = mgr.entries.iter().find(|e| e.extension == ".txt").unwrap();
        assert!(entry.fallback_app.is_some());
        assert_eq!(
            entry.fallback_app.as_ref().unwrap().id,
            "com.ouros.texteditor"
        );
    }

    #[test]
    fn test_fallback_on_uninstall() {
        let mut mgr = AssociationManager::new();

        // Set code editor as default for .txt (text editor becomes fallback)
        mgr.set_default(".txt", "com.ouros.codeeditor");

        // Uninstall code editor
        let affected = mgr.handle_uninstall("com.ouros.codeeditor");
        assert!(affected.contains(&".txt".to_string()));

        // Default should have fallen back to text editor
        let default = mgr.get_default(".txt");
        assert!(default.is_some());
        assert_eq!(default.unwrap().id, "com.ouros.texteditor");
    }

    #[test]
    fn test_fallback_clears_when_no_alternative() {
        let mut mgr = AssociationManager::new();

        // .pdf has no default and no available apps
        let default = mgr.get_default(".pdf");
        assert!(default.is_none());

        // Uninstalling something shouldn't crash
        let affected = mgr.handle_uninstall("com.nonexistent.app");
        assert!(affected.is_empty());
    }

    #[test]
    fn test_handler_history() {
        let mut mgr = AssociationManager::new();

        // .txt starts with text editor. Change to code editor.
        mgr.set_default(".txt", "com.ouros.codeeditor");

        let entry = mgr.entries.iter().find(|e| e.extension == ".txt").unwrap();
        assert_eq!(entry.handler_history.len(), 1);
        assert_eq!(entry.handler_history[0].id, "com.ouros.texteditor");
    }

    #[test]
    fn test_handler_history_truncates() {
        let mut mgr = AssociationManager::new();

        // Add more apps to .txt so we can cycle through them
        let extra_app1 = AppInfo {
            id: "com.extra.app1".into(),
            name: "Extra App 1".into(),
            icon: None,
            exec_path: "/usr/bin/extra1".into(),
            installed: true,
        };
        let extra_app2 = AppInfo {
            id: "com.extra.app2".into(),
            name: "Extra App 2".into(),
            icon: None,
            exec_path: "/usr/bin/extra2".into(),
            installed: true,
        };
        let extra_app3 = AppInfo {
            id: "com.extra.app3".into(),
            name: "Extra App 3".into(),
            icon: None,
            exec_path: "/usr/bin/extra3".into(),
            installed: true,
        };

        mgr.add_association(".txt", extra_app1);
        mgr.add_association(".txt", extra_app2);
        mgr.add_association(".txt", extra_app3);

        // Cycle through defaults: text -> code -> extra1 -> extra2 -> extra3
        mgr.set_default(".txt", "com.ouros.codeeditor");
        mgr.set_default(".txt", "com.extra.app1");
        mgr.set_default(".txt", "com.extra.app2");
        mgr.set_default(".txt", "com.extra.app3");

        let entry = mgr.entries.iter().find(|e| e.extension == ".txt").unwrap();
        // History should be capped at MAX_HANDLER_HISTORY
        assert_eq!(entry.handler_history.len(), MAX_HANDLER_HISTORY);
    }

    #[test]
    fn test_category_detection() {
        assert_eq!(categorize_extension(".txt"), FileCategory::Documents);
        assert_eq!(categorize_extension(".pdf"), FileCategory::Documents);
        assert_eq!(categorize_extension(".png"), FileCategory::Images);
        assert_eq!(categorize_extension(".jpg"), FileCategory::Images);
        assert_eq!(categorize_extension(".mp3"), FileCategory::Audio);
        assert_eq!(categorize_extension(".flac"), FileCategory::Audio);
        assert_eq!(categorize_extension(".mp4"), FileCategory::Video);
        assert_eq!(categorize_extension(".mkv"), FileCategory::Video);
        assert_eq!(categorize_extension(".rs"), FileCategory::Code);
        assert_eq!(categorize_extension(".py"), FileCategory::Code);
        assert_eq!(categorize_extension(".zip"), FileCategory::Archives);
        assert_eq!(categorize_extension(".tar"), FileCategory::Archives);
        assert_eq!(categorize_extension(".xyz"), FileCategory::Other);
    }

    #[test]
    fn test_category_detection_case_insensitive() {
        assert_eq!(categorize_extension(".TXT"), FileCategory::Documents);
        assert_eq!(categorize_extension(".PNG"), FileCategory::Images);
        assert_eq!(categorize_extension(".Rs"), FileCategory::Code);
    }

    #[test]
    fn test_search_by_extension() {
        let mgr = AssociationManager::new();
        let results = mgr.search(".rs");
        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.extension == ".rs"));
    }

    #[test]
    fn test_search_by_description() {
        let mgr = AssociationManager::new();
        let results = mgr.search("rust");
        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.extension == ".rs"));
    }

    #[test]
    fn test_search_by_mime_type() {
        let mgr = AssociationManager::new();
        let results = mgr.search("image/png");
        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.extension == ".png"));
    }

    #[test]
    fn test_search_empty_returns_all() {
        let mgr = AssociationManager::new();
        let results = mgr.search("");
        assert_eq!(results.len(), mgr.count());
    }

    #[test]
    fn test_entries_by_category() {
        let mgr = AssociationManager::new();
        let images = mgr.entries_by_category(FileCategory::Images);
        assert!(images.len() >= 5);
        for entry in &images {
            assert_eq!(entry.category(), FileCategory::Images);
        }
    }

    #[test]
    fn test_entries_by_category_all_returns_everything() {
        let mgr = AssociationManager::new();
        let all = mgr.entries_by_category(FileCategory::All);
        assert_eq!(all.len(), mgr.count());
    }

    #[test]
    fn test_get_available_apps() {
        let mgr = AssociationManager::new();
        let apps = mgr.get_available_apps(".rs");
        assert!(apps.len() >= 2); // code editor + text editor
        assert!(apps.iter().any(|a| a.id == "com.ouros.codeeditor"));
        assert!(apps.iter().any(|a| a.id == "com.ouros.texteditor"));
    }

    #[test]
    fn test_get_available_apps_unknown_extension() {
        let mgr = AssociationManager::new();
        let apps = mgr.get_available_apps(".nonexistent");
        assert!(apps.is_empty());
    }

    #[test]
    fn test_add_association() {
        let mut mgr = AssociationManager::new();
        let new_app = AppInfo {
            id: "com.new.viewer".into(),
            name: "New Viewer".into(),
            icon: None,
            exec_path: "/usr/bin/newviewer".into(),
            installed: true,
        };

        let before = mgr.get_available_apps(".png").len();
        mgr.add_association(".png", new_app);
        let after = mgr.get_available_apps(".png").len();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_add_association_no_duplicates() {
        let mut mgr = AssociationManager::new();
        let existing_app = AppInfo {
            id: "com.ouros.imageviewer".into(),
            name: "Image Viewer".into(),
            icon: Some("image-viewer".into()),
            exec_path: "/usr/bin/imageviewer".into(),
            installed: true,
        };

        let before = mgr.get_available_apps(".png").len();
        mgr.add_association(".png", existing_app);
        let after = mgr.get_available_apps(".png").len();
        assert_eq!(after, before); // No duplicate added
    }

    #[test]
    fn test_remove_association() {
        let mut mgr = AssociationManager::new();
        let before = mgr.get_available_apps(".rs").len();
        mgr.remove_association(".rs", "com.ouros.texteditor");
        let after = mgr.get_available_apps(".rs").len();
        assert_eq!(after, before - 1);
    }

    #[test]
    fn test_set_default_nonexistent_extension() {
        let mut mgr = AssociationManager::new();
        let result = mgr.set_default(".nonexistent", "com.ouros.texteditor");
        assert!(!result);
    }

    #[test]
    fn test_set_default_nonexistent_app() {
        let mut mgr = AssociationManager::new();
        let result = mgr.set_default(".txt", "com.nonexistent.app");
        assert!(!result);
    }

    #[test]
    fn test_filtered_entries_combined() {
        let mgr = AssociationManager::new();

        // Filter by category "Code" and search for "rust"
        let results = mgr.filtered_entries(FileCategory::Code, "rust");
        assert!(!results.is_empty());
        for entry in &results {
            assert_eq!(entry.category(), FileCategory::Code);
        }
    }

    #[test]
    fn test_filtered_entries_sorted() {
        let mgr = AssociationManager::new();
        let results = mgr.filtered_entries(FileCategory::All, "");
        // Verify sorted by extension
        for i in 1..results.len() {
            assert!(
                results[i - 1].extension <= results[i].extension,
                "Entries not sorted: {} > {}",
                results[i - 1].extension,
                results[i].extension
            );
        }
    }

    #[test]
    fn test_page_state_visible_entries() {
        let state = AssociationsPageState::new();
        let entries = state.visible_entries();
        assert!(!entries.is_empty());
        assert_eq!(entries.len(), state.manager.count());
    }

    #[test]
    fn test_page_state_category_filter() {
        let mut state = AssociationsPageState::new();
        state.active_category = FileCategory::Images;
        let entries = state.visible_entries();
        assert!(!entries.is_empty());
        for entry in &entries {
            assert_eq!(entry.category(), FileCategory::Images);
        }
    }

    #[test]
    fn test_page_state_search_filter() {
        let mut state = AssociationsPageState::new();
        state.search_query = "python".into();
        let entries = state.visible_entries();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.extension == ".py"));
    }

    #[test]
    fn test_render_produces_commands() {
        let state = AssociationsPageState::new();
        let mut tree = RenderTree::new();
        state.render(&mut tree, 0.0, 0.0, 800.0);
        assert!(!tree.is_empty(), "Render must produce commands");
        assert!(tree.len() > 10, "Expected substantial render output");
    }

    #[test]
    fn test_uninstall_multiple_extensions() {
        let mut mgr = AssociationManager::new();

        // Code editor is default for many code extensions
        let affected = mgr.handle_uninstall("com.ouros.codeeditor");
        // Should affect multiple code file types
        assert!(
            affected.len() > 5,
            "Expected multiple affected extensions, got {}",
            affected.len()
        );

        // All affected extensions should now have text editor as default
        // (since text editor is the other available app for code files)
        for ext in &affected {
            let default = mgr.get_default(ext);
            if default.is_some() {
                assert_eq!(default.unwrap().id, "com.ouros.texteditor");
            }
        }
    }
}
