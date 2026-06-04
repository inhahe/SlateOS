//! Default application settings panel for the desktop shell.
//!
//! Manages which applications open by default for various content types
//! including web browsers, email clients, music players, video players,
//! image viewers, document readers, and custom file type associations.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Content categories
// ============================================================================

/// A category of content that can have a default handler application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentCategory {
    WebBrowser,
    EmailClient,
    MusicPlayer,
    VideoPlayer,
    ImageViewer,
    DocumentReader,
    TextEditor,
    ArchiveManager,
    Terminal,
    FileManager,
    Calculator,
    Calendar,
}

impl ContentCategory {
    /// All categories.
    pub fn all() -> &'static [Self] {
        &[
            Self::WebBrowser,
            Self::EmailClient,
            Self::MusicPlayer,
            Self::VideoPlayer,
            Self::ImageViewer,
            Self::DocumentReader,
            Self::TextEditor,
            Self::ArchiveManager,
            Self::Terminal,
            Self::FileManager,
            Self::Calculator,
            Self::Calendar,
        ]
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::WebBrowser => "Web browser",
            Self::EmailClient => "Email",
            Self::MusicPlayer => "Music player",
            Self::VideoPlayer => "Video player",
            Self::ImageViewer => "Photo viewer",
            Self::DocumentReader => "Document reader",
            Self::TextEditor => "Text editor",
            Self::ArchiveManager => "Archive manager",
            Self::Terminal => "Terminal",
            Self::FileManager => "File manager",
            Self::Calculator => "Calculator",
            Self::Calendar => "Calendar",
        }
    }

    /// Description of what this category handles.
    pub fn description(self) -> &'static str {
        match self {
            Self::WebBrowser => "Opens web links and HTML files",
            Self::EmailClient => "Handles mailto: links and email composition",
            Self::MusicPlayer => "Plays audio files (MP3, FLAC, OGG, WAV)",
            Self::VideoPlayer => "Plays video files (MP4, AVI, MKV, WebM)",
            Self::ImageViewer => "Opens image files (PNG, JPEG, BMP, GIF)",
            Self::DocumentReader => "Opens documents (PDF, EPUB)",
            Self::TextEditor => "Opens plain text files",
            Self::ArchiveManager => "Opens archives (ZIP, TAR, GZ, 7Z)",
            Self::Terminal => "Opens terminal/command-line sessions",
            Self::FileManager => "Browses the filesystem",
            Self::Calculator => "Performs calculations",
            Self::Calendar => "Manages calendar events and reminders",
        }
    }

    /// Icon character for display.
    pub fn icon(self) -> &'static str {
        match self {
            Self::WebBrowser => "\u{1F310}",    // globe
            Self::EmailClient => "\u{2709}",    // envelope
            Self::MusicPlayer => "\u{1F3B5}",   // music note
            Self::VideoPlayer => "\u{1F3AC}",   // clapper board
            Self::ImageViewer => "\u{1F5BC}",   // frame with picture
            Self::DocumentReader => "\u{1F4C4}", // page facing up
            Self::TextEditor => "\u{1F4DD}",    // memo
            Self::ArchiveManager => "\u{1F4E6}", // package
            Self::Terminal => "\u{1F4BB}",      // laptop
            Self::FileManager => "\u{1F4C1}",   // file folder
            Self::Calculator => "\u{1F522}",    // input numbers
            Self::Calendar => "\u{1F4C5}",      // calendar
        }
    }

    /// File extensions commonly associated with this category.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::WebBrowser => &["html", "htm", "xhtml", "mhtml", "url"],
            Self::EmailClient => &["eml", "msg"],
            Self::MusicPlayer => &["mp3", "flac", "ogg", "wav", "aac", "m4a", "wma", "opus"],
            Self::VideoPlayer => &["mp4", "avi", "mkv", "webm", "mov", "wmv", "flv", "m4v"],
            Self::ImageViewer => &["png", "jpg", "jpeg", "bmp", "gif", "webp", "svg", "ico", "tiff"],
            Self::DocumentReader => &["pdf", "epub", "djvu", "xps"],
            Self::TextEditor => &["txt", "log", "cfg", "ini", "md", "rst", "yaml", "yml", "toml", "json", "xml"],
            Self::ArchiveManager => &["zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst"],
            Self::Terminal => &[],
            Self::FileManager => &[],
            Self::Calculator => &[],
            Self::Calendar => &["ics", "ical"],
        }
    }

    /// MIME types associated with this category.
    pub fn mime_types(self) -> &'static [&'static str] {
        match self {
            Self::WebBrowser => &["text/html", "application/xhtml+xml"],
            Self::EmailClient => &["message/rfc822", "x-scheme-handler/mailto"],
            Self::MusicPlayer => &["audio/mpeg", "audio/flac", "audio/ogg", "audio/wav"],
            Self::VideoPlayer => &["video/mp4", "video/x-matroska", "video/webm", "video/avi"],
            Self::ImageViewer => &["image/png", "image/jpeg", "image/bmp", "image/gif", "image/webp"],
            Self::DocumentReader => &["application/pdf", "application/epub+zip"],
            Self::TextEditor => &["text/plain", "text/markdown", "application/json"],
            Self::ArchiveManager => &["application/zip", "application/x-tar", "application/gzip"],
            Self::Terminal => &[],
            Self::FileManager => &["inode/directory"],
            Self::Calculator => &[],
            Self::Calendar => &["text/calendar"],
        }
    }
}

// ============================================================================
// Application info
// ============================================================================

/// Information about an installed application that can handle content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub executable: String,
    pub icon_name: String,
    pub supported_categories: Vec<ContentCategory>,
    pub supported_extensions: Vec<String>,
    pub supported_mime_types: Vec<String>,
    pub is_system: bool,
}

impl AppInfo {
    /// Check if this app can handle a given category.
    pub fn handles_category(&self, category: ContentCategory) -> bool {
        self.supported_categories.contains(&category)
    }

    /// Check if this app can handle a given file extension.
    pub fn handles_extension(&self, ext: &str) -> bool {
        let ext_lower = ext.to_lowercase();
        self.supported_extensions
            .iter()
            .any(|e| e.to_lowercase() == ext_lower)
    }

    /// Check if this app can handle a given MIME type.
    pub fn handles_mime(&self, mime: &str) -> bool {
        self.supported_mime_types.iter().any(|m| m == mime)
    }
}

/// Default applications bundled with the OS.
pub fn builtin_apps() -> Vec<AppInfo> {
    vec![
        AppInfo {
            id: "org.ouros.terminal".to_string(),
            name: "Terminal".to_string(),
            description: "Terminal emulator".to_string(),
            executable: "/usr/bin/terminal".to_string(),
            icon_name: "terminal".to_string(),
            supported_categories: vec![ContentCategory::Terminal],
            supported_extensions: vec![],
            supported_mime_types: vec![],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.explorer".to_string(),
            name: "File Explorer".to_string(),
            description: "File manager".to_string(),
            executable: "/usr/bin/explorer".to_string(),
            icon_name: "file-manager".to_string(),
            supported_categories: vec![ContentCategory::FileManager],
            supported_extensions: vec![],
            supported_mime_types: vec!["inode/directory".to_string()],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.editor".to_string(),
            name: "Text Editor".to_string(),
            description: "Plain text and code editor".to_string(),
            executable: "/usr/bin/editor".to_string(),
            icon_name: "text-editor".to_string(),
            supported_categories: vec![ContentCategory::TextEditor],
            supported_extensions: vec![
                "txt".into(), "log".into(), "cfg".into(), "ini".into(),
                "md".into(), "rst".into(), "yaml".into(), "yml".into(),
                "toml".into(), "json".into(), "xml".into(), "rs".into(),
                "py".into(), "c".into(), "cpp".into(), "h".into(),
            ],
            supported_mime_types: vec![
                "text/plain".to_string(),
                "text/markdown".to_string(),
                "application/json".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.viewer".to_string(),
            name: "Photo Viewer".to_string(),
            description: "Image and photo viewer".to_string(),
            executable: "/usr/bin/viewer".to_string(),
            icon_name: "image-viewer".to_string(),
            supported_categories: vec![ContentCategory::ImageViewer],
            supported_extensions: vec![
                "png".into(), "jpg".into(), "jpeg".into(), "bmp".into(),
                "gif".into(), "webp".into(), "svg".into(), "ico".into(),
            ],
            supported_mime_types: vec![
                "image/png".to_string(), "image/jpeg".to_string(),
                "image/bmp".to_string(), "image/gif".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.musicplayer".to_string(),
            name: "Music Player".to_string(),
            description: "Audio player and library manager".to_string(),
            executable: "/usr/bin/musicplayer".to_string(),
            icon_name: "music".to_string(),
            supported_categories: vec![ContentCategory::MusicPlayer],
            supported_extensions: vec![
                "mp3".into(), "flac".into(), "ogg".into(), "wav".into(),
                "aac".into(), "m4a".into(), "opus".into(),
            ],
            supported_mime_types: vec![
                "audio/mpeg".to_string(), "audio/flac".to_string(),
                "audio/ogg".to_string(), "audio/wav".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.videoplayer".to_string(),
            name: "Video Player".to_string(),
            description: "Video and media player".to_string(),
            executable: "/usr/bin/videoplayer".to_string(),
            icon_name: "video".to_string(),
            supported_categories: vec![ContentCategory::VideoPlayer],
            supported_extensions: vec![
                "mp4".into(), "avi".into(), "mkv".into(), "webm".into(),
                "mov".into(), "wmv".into(), "flv".into(),
            ],
            supported_mime_types: vec![
                "video/mp4".to_string(), "video/x-matroska".to_string(),
                "video/webm".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.pdfviewer".to_string(),
            name: "PDF Viewer".to_string(),
            description: "Document reader".to_string(),
            executable: "/usr/bin/pdfviewer".to_string(),
            icon_name: "document".to_string(),
            supported_categories: vec![ContentCategory::DocumentReader],
            supported_extensions: vec!["pdf".into(), "epub".into()],
            supported_mime_types: vec![
                "application/pdf".to_string(),
                "application/epub+zip".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.archivemanager".to_string(),
            name: "Archive Manager".to_string(),
            description: "Open and create archives".to_string(),
            executable: "/usr/bin/archivemanager".to_string(),
            icon_name: "archive".to_string(),
            supported_categories: vec![ContentCategory::ArchiveManager],
            supported_extensions: vec![
                "zip".into(), "tar".into(), "gz".into(), "bz2".into(),
                "7z".into(), "rar".into(), "xz".into(), "zst".into(),
            ],
            supported_mime_types: vec![
                "application/zip".to_string(),
                "application/x-tar".to_string(),
                "application/gzip".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.calculator".to_string(),
            name: "Calculator".to_string(),
            description: "Standard and scientific calculator".to_string(),
            executable: "/usr/bin/calculator".to_string(),
            icon_name: "calculator".to_string(),
            supported_categories: vec![ContentCategory::Calculator],
            supported_extensions: vec![],
            supported_mime_types: vec![],
            is_system: true,
        },
        AppInfo {
            id: "org.ouros.calendar".to_string(),
            name: "Calendar".to_string(),
            description: "Calendar and event management".to_string(),
            executable: "/usr/bin/calendar".to_string(),
            icon_name: "calendar".to_string(),
            supported_categories: vec![ContentCategory::Calendar],
            supported_extensions: vec!["ics".into(), "ical".into()],
            supported_mime_types: vec!["text/calendar".to_string()],
            is_system: true,
        },
    ]
}

// ============================================================================
// File type association
// ============================================================================

/// A specific file extension → application mapping.
#[derive(Clone, Debug)]
pub struct FileTypeAssociation {
    pub extension: String,
    pub app_id: String,
    pub is_custom: bool,
}

// ============================================================================
// Default apps settings
// ============================================================================

/// Default application settings.
#[derive(Clone, Debug)]
pub struct DefaultAppsSettings {
    /// Default app for each content category.
    pub category_defaults: Vec<(ContentCategory, String)>,
    /// Custom file type associations (extension → app_id).
    pub custom_associations: Vec<FileTypeAssociation>,
    /// All known/installed apps.
    pub installed_apps: Vec<AppInfo>,
    /// Whether to confirm before changing a default.
    pub confirm_changes: bool,
    /// Whether to reset custom associations when updating an app.
    pub reset_on_update: bool,
}

impl Default for DefaultAppsSettings {
    fn default() -> Self {
        let apps = builtin_apps();
        let mut category_defaults = Vec::new();

        // Set built-in defaults
        for category in ContentCategory::all() {
            if let Some(app) = apps.iter().find(|a| a.handles_category(*category)) {
                category_defaults.push((*category, app.id.clone()));
            }
        }

        Self {
            category_defaults,
            custom_associations: Vec::new(),
            installed_apps: apps,
            confirm_changes: false,
            reset_on_update: false,
        }
    }
}

impl DefaultAppsSettings {
    /// Get the default app for a content category.
    pub fn default_for_category(&self, category: ContentCategory) -> Option<&AppInfo> {
        let app_id = self
            .category_defaults
            .iter()
            .find(|(c, _)| *c == category)
            .map(|(_, id)| id.as_str())?;

        self.installed_apps.iter().find(|a| a.id == app_id)
    }

    /// Set the default app for a category.
    pub fn set_default(&mut self, category: ContentCategory, app_id: &str) -> bool {
        // Verify the app exists and handles this category
        if !self
            .installed_apps
            .iter()
            .any(|a| a.id == app_id && a.handles_category(category))
        {
            return false;
        }

        if let Some(entry) = self
            .category_defaults
            .iter_mut()
            .find(|(c, _)| *c == category)
        {
            entry.1 = app_id.to_string();
        } else {
            self.category_defaults
                .push((category, app_id.to_string()));
        }
        true
    }

    /// Reset a category to its default app.
    pub fn reset_default(&mut self, category: ContentCategory) {
        let builtin = builtin_apps();
        if let Some(app) = builtin.iter().find(|a| a.handles_category(category)) {
            self.set_default(category, &app.id);
        }
    }

    /// Reset all categories to built-in defaults.
    pub fn reset_all(&mut self) {
        let builtin = builtin_apps();
        for category in ContentCategory::all() {
            if let Some(app) = builtin.iter().find(|a| a.handles_category(*category)) {
                self.set_default(*category, &app.id);
            }
        }
        self.custom_associations.clear();
    }

    /// Get the app for a specific file extension.
    pub fn app_for_extension(&self, ext: &str) -> Option<&AppInfo> {
        let ext_lower = ext.to_lowercase();

        // Check custom associations first
        if let Some(assoc) = self
            .custom_associations
            .iter()
            .find(|a| a.extension.to_lowercase() == ext_lower)
        {
            return self.installed_apps.iter().find(|a| a.id == assoc.app_id);
        }

        // Check category defaults
        for category in ContentCategory::all() {
            if category
                .extensions()
                .iter()
                .any(|e| e.to_lowercase() == ext_lower)
            {
                return self.default_for_category(*category);
            }
        }

        None
    }

    /// Set a custom association for a file extension.
    pub fn set_extension_handler(&mut self, extension: &str, app_id: &str) {
        let ext_lower = extension.to_lowercase();

        // Remove existing custom association if any
        self.custom_associations
            .retain(|a| a.extension.to_lowercase() != ext_lower);

        self.custom_associations.push(FileTypeAssociation {
            extension: ext_lower,
            app_id: app_id.to_string(),
            is_custom: true,
        });
    }

    /// Remove a custom association, falling back to category default.
    pub fn remove_extension_handler(&mut self, extension: &str) -> bool {
        let ext_lower = extension.to_lowercase();
        let before = self.custom_associations.len();
        self.custom_associations
            .retain(|a| a.extension.to_lowercase() != ext_lower);
        self.custom_associations.len() < before
    }

    /// Get all apps that can handle a given category.
    pub fn apps_for_category(&self, category: ContentCategory) -> Vec<&AppInfo> {
        self.installed_apps
            .iter()
            .filter(|a| a.handles_category(category))
            .collect()
    }

    /// Get all apps that can handle a given extension.
    pub fn apps_for_extension(&self, ext: &str) -> Vec<&AppInfo> {
        self.installed_apps
            .iter()
            .filter(|a| a.handles_extension(ext))
            .collect()
    }

    /// Register a new application.
    pub fn register_app(&mut self, app: AppInfo) {
        if !self.installed_apps.iter().any(|a| a.id == app.id) {
            self.installed_apps.push(app);
        }
    }

    /// Unregister an application. Resets any defaults pointing to it.
    pub fn unregister_app(&mut self, app_id: &str) {
        self.installed_apps.retain(|a| a.id != app_id);
        self.custom_associations.retain(|a| a.app_id != app_id);

        // Reset category defaults that pointed to this app
        let builtin = builtin_apps();
        for entry in &mut self.category_defaults {
            if entry.1 == app_id
                && let Some(fallback) = builtin
                    .iter()
                    .find(|a| a.handles_category(entry.0))
                {
                    entry.1 = fallback.id.clone();
                }
        }
    }

    /// Count custom associations.
    pub fn custom_association_count(&self) -> usize {
        self.custom_associations.len()
    }

    /// Count installed third-party (non-system) apps.
    pub fn third_party_app_count(&self) -> usize {
        self.installed_apps.iter().filter(|a| !a.is_system).count()
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// Tabs in the default apps settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAppsTab {
    Categories,
    FileTypes,
    AppsList,
}

impl DefaultAppsTab {
    /// All tabs.
    pub fn all() -> &'static [Self] {
        &[Self::Categories, Self::FileTypes, Self::AppsList]
    }

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Categories => "Default apps",
            Self::FileTypes => "File types",
            Self::AppsList => "Installed apps",
        }
    }
}

/// Default apps settings UI state.
pub struct DefaultAppsUI {
    pub settings: DefaultAppsSettings,
    pub active_tab: DefaultAppsTab,
    pub search_query: String,
    pub selected_category: Option<ContentCategory>,
    pub expanded_category: Option<ContentCategory>,
    pub scroll_offset: f32,
    pub dirty: bool,
}

impl DefaultAppsUI {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            settings: DefaultAppsSettings::default(),
            active_tab: DefaultAppsTab::Categories,
            search_query: String::new(),
            selected_category: None,
            expanded_category: None,
            scroll_offset: 0.0,
            dirty: false,
        }
    }

    /// Switch tab.
    pub fn set_tab(&mut self, tab: DefaultAppsTab) {
        self.active_tab = tab;
        self.scroll_offset = 0.0;
    }

    /// Toggle expanded state of a category.
    pub fn toggle_expand(&mut self, category: ContentCategory) {
        if self.expanded_category == Some(category) {
            self.expanded_category = None;
        } else {
            self.expanded_category = Some(category);
        }
    }

    /// Render the settings panel.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Default Applications".to_string(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in DefaultAppsTab::all() {
            let label = tab.label();
            let tw = label.len() as f32 * 8.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            tab_x += tw + 4.0;
        }

        // Content area
        let content_y = tab_y + 44.0;
        let content_h = height - (content_y - y) - 16.0;

        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: content_y,
            width: width - 16.0,
            height: content_h,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            DefaultAppsTab::Categories => {
                self.render_categories_tab(&mut cmds, cx, cy, cw);
            }
            DefaultAppsTab::FileTypes => {
                self.render_filetypes_tab(&mut cmds, cx, cy, cw);
            }
            DefaultAppsTab::AppsList => {
                self.render_apps_tab(&mut cmds, cx, cy, cw);
            }
        }

        cmds
    }

    /// Render the categories tab.
    fn render_categories_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Choose default apps for each type of content".to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        row_y += 24.0;

        // Reset all button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 100.0,
            y: row_y - 20.0,
            width: 100.0,
            height: 24.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 88.0,
            y: row_y - 16.0,
            text: "Reset all".to_string(),
            font_size: 11.0,
            color: PEACH,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        for category in ContentCategory::all() {
            let is_expanded = self.expanded_category == Some(*category);
            let default_app = self.settings.default_for_category(*category);
            let card_h = if is_expanded { 100.0 } else { 56.0 };

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: card_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });

            // Icon
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 10.0,
                text: category.icon().to_string(),
                font_size: 20.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Category name
            cmds.push(RenderCommand::Text {
                x: x + 44.0,
                y: row_y + 8.0,
                text: category.label().to_string(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Current default app
            let app_name = default_app
                .map(|a| a.name.as_str())
                .unwrap_or("None set");
            cmds.push(RenderCommand::Text {
                x: x + 44.0,
                y: row_y + 30.0,
                text: app_name.to_string(),
                font_size: 12.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Expand indicator
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 16.0,
                text: if is_expanded { "\u{25B2}" } else { "\u{25BC}" }.to_string(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // If expanded, show alternative apps
            if is_expanded {
                let alternatives = self.settings.apps_for_category(*category);
                let mut alt_x = x + 16.0;
                let alt_y = row_y + 56.0;

                for app in &alternatives {
                    let is_current = default_app.is_some_and(|d| d.id == app.id);
                    let chip_w = app.name.len() as f32 * 7.0 + 24.0;

                    cmds.push(RenderCommand::FillRect {
                        x: alt_x,
                        y: alt_y,
                        width: chip_w,
                        height: 28.0,
                        color: if is_current { BLUE } else { SURFACE1 },
                        corner_radii: CornerRadii::all(14.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: alt_x + 12.0,
                        y: alt_y + 7.0,
                        text: app.name.clone(),
                        font_size: 11.0,
                        color: if is_current { CRUST } else { TEXT },
                        font_weight: if is_current {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: None,
                    });

                    alt_x += chip_w + 8.0;
                }
            }

            row_y += card_h + 8.0;
        }
    }

    /// Render the file types tab.
    fn render_filetypes_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        let search_text = if self.search_query.is_empty() {
            "Search file types...".to_string()
        } else {
            self.search_query.clone()
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 8.0,
            text: search_text,
            font_size: 12.0,
            color: if self.search_query.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        row_y += 44.0;

        // Custom associations count
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!(
                "{} custom associations",
                self.settings.custom_association_count()
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        row_y += 24.0;

        // File type list grouped by category
        let search = self.search_query.to_lowercase();
        for category in ContentCategory::all() {
            let extensions = category.extensions();
            if extensions.is_empty() {
                continue;
            }

            let filtered: Vec<&&str> = if search.is_empty() {
                extensions.iter().collect()
            } else {
                extensions
                    .iter()
                    .filter(|e| e.contains(search.as_str()))
                    .collect()
            };

            if filtered.is_empty() {
                continue;
            }

            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: category.label().to_string(),
                font_size: 13.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            row_y += 20.0;

            for ext in &filtered {
                let app = self.settings.app_for_extension(ext);
                let is_custom = self
                    .settings
                    .custom_associations
                    .iter()
                    .any(|a| a.extension == **ext);

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Extension
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y + 6.0,
                    width: 48.0,
                    height: 20.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 9.0,
                    text: format!(".{ext}"),
                    font_size: 11.0,
                    color: LAVENDER,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // App name
                cmds.push(RenderCommand::Text {
                    x: x + 66.0,
                    y: row_y + 9.0,
                    text: app.map_or("(none)", |a| a.name.as_str()).to_string(),
                    font_size: 12.0,
                    color: if is_custom { PEACH } else { TEXT },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Custom badge
                if is_custom {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 10.0,
                        text: "Custom".to_string(),
                        font_size: 10.0,
                        color: PEACH,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                row_y += 38.0;
            }

            row_y += 8.0;
        }
    }

    /// Render the installed apps tab.
    fn render_apps_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        let total = self.settings.installed_apps.len();
        let third_party = self.settings.third_party_app_count();

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("{total} installed apps ({third_party} third-party)"),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        row_y += 24.0;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        let search_text = if self.search_query.is_empty() {
            "Search apps...".to_string()
        } else {
            self.search_query.clone()
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 8.0,
            text: search_text,
            font_size: 12.0,
            color: if self.search_query.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        row_y += 44.0;

        // App list
        let search = self.search_query.to_lowercase();
        let apps: Vec<&AppInfo> = self
            .settings
            .installed_apps
            .iter()
            .filter(|a| {
                search.is_empty()
                    || a.name.to_lowercase().contains(&search)
                    || a.description.to_lowercase().contains(&search)
            })
            .collect();

        for app in &apps {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 56.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });

            // App name
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: app.name.clone(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 28.0,
                text: app.description.clone(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 120.0),
            });

            // System badge
            if app.is_system {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 68.0,
                    y: row_y + 8.0,
                    width: 52.0,
                    height: 18.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 62.0,
                    y: row_y + 10.0,
                    text: "System".to_string(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Categories this app handles
            let categories: Vec<&str> = app
                .supported_categories
                .iter()
                .map(|c| c.label())
                .collect();
            if !categories.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 42.0,
                    text: categories.join(", "),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 32.0),
                });
            }

            row_y += 64.0;
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_all() {
        assert_eq!(ContentCategory::all().len(), 12);
    }

    #[test]
    fn test_category_labels() {
        assert_eq!(ContentCategory::WebBrowser.label(), "Web browser");
        assert_eq!(ContentCategory::Terminal.label(), "Terminal");
        assert_eq!(ContentCategory::Calendar.label(), "Calendar");
    }

    #[test]
    fn test_category_extensions() {
        let exts = ContentCategory::MusicPlayer.extensions();
        assert!(exts.contains(&"mp3"));
        assert!(exts.contains(&"flac"));
        assert!(!exts.contains(&"png"));
    }

    #[test]
    fn test_category_mime_types() {
        let mimes = ContentCategory::ImageViewer.mime_types();
        assert!(mimes.contains(&"image/png"));
        assert!(!mimes.contains(&"audio/mpeg"));
    }

    #[test]
    fn test_builtin_apps() {
        let apps = builtin_apps();
        assert!(!apps.is_empty());
        assert!(apps.iter().all(|a| a.is_system));
    }

    #[test]
    fn test_app_handles_category() {
        let apps = builtin_apps();
        let editor = apps.iter().find(|a| a.id == "org.ouros.editor").unwrap();
        assert!(editor.handles_category(ContentCategory::TextEditor));
        assert!(!editor.handles_category(ContentCategory::VideoPlayer));
    }

    #[test]
    fn test_app_handles_extension() {
        let apps = builtin_apps();
        let viewer = apps.iter().find(|a| a.id == "org.ouros.viewer").unwrap();
        assert!(viewer.handles_extension("png"));
        assert!(viewer.handles_extension("PNG"));
        assert!(!viewer.handles_extension("mp3"));
    }

    #[test]
    fn test_default_settings() {
        let settings = DefaultAppsSettings::default();
        assert!(!settings.installed_apps.is_empty());
        assert!(!settings.category_defaults.is_empty());
    }

    #[test]
    fn test_default_for_category() {
        let settings = DefaultAppsSettings::default();
        let terminal = settings
            .default_for_category(ContentCategory::Terminal)
            .unwrap();
        assert_eq!(terminal.name, "Terminal");
    }

    #[test]
    fn test_set_default() {
        let mut settings = DefaultAppsSettings::default();
        // Can't set an app that doesn't handle the category
        assert!(!settings.set_default(
            ContentCategory::WebBrowser,
            "org.ouros.calculator"
        ));
    }

    #[test]
    fn test_reset_default() {
        let mut settings = DefaultAppsSettings::default();
        settings.reset_default(ContentCategory::Terminal);
        let terminal = settings
            .default_for_category(ContentCategory::Terminal)
            .unwrap();
        assert_eq!(terminal.id, "org.ouros.terminal");
    }

    #[test]
    fn test_reset_all() {
        let mut settings = DefaultAppsSettings::default();
        settings.custom_associations.push(FileTypeAssociation {
            extension: "xyz".to_string(),
            app_id: "test".to_string(),
            is_custom: true,
        });
        settings.reset_all();
        assert!(settings.custom_associations.is_empty());
    }

    #[test]
    fn test_app_for_extension() {
        let settings = DefaultAppsSettings::default();
        let app = settings.app_for_extension("png").unwrap();
        assert_eq!(app.name, "Photo Viewer");

        let app2 = settings.app_for_extension("mp3").unwrap();
        assert_eq!(app2.name, "Music Player");
    }

    #[test]
    fn test_custom_association() {
        let mut settings = DefaultAppsSettings::default();
        settings.set_extension_handler("txt", "org.ouros.viewer");
        let app = settings.app_for_extension("txt").unwrap();
        // Custom association overrides category default
        assert_eq!(app.name, "Photo Viewer");

        // Remove custom association
        assert!(settings.remove_extension_handler("txt"));
        let app2 = settings.app_for_extension("txt").unwrap();
        assert_eq!(app2.name, "Text Editor");
    }

    #[test]
    fn test_custom_association_case_insensitive() {
        let mut settings = DefaultAppsSettings::default();
        settings.set_extension_handler("TXT", "org.ouros.viewer");
        assert_eq!(settings.custom_association_count(), 1);

        // Second set with different case replaces
        settings.set_extension_handler("txt", "org.ouros.editor");
        assert_eq!(settings.custom_association_count(), 1);
    }

    #[test]
    fn test_apps_for_category() {
        let settings = DefaultAppsSettings::default();
        let apps = settings.apps_for_category(ContentCategory::TextEditor);
        assert!(!apps.is_empty());
    }

    #[test]
    fn test_register_app() {
        let mut settings = DefaultAppsSettings::default();
        let count_before = settings.installed_apps.len();
        settings.register_app(AppInfo {
            id: "com.example.app".to_string(),
            name: "Example App".to_string(),
            description: "An example".to_string(),
            executable: "/usr/bin/example".to_string(),
            icon_name: "example".to_string(),
            supported_categories: vec![ContentCategory::TextEditor],
            supported_extensions: vec!["txt".to_string()],
            supported_mime_types: vec!["text/plain".to_string()],
            is_system: false,
        });
        assert_eq!(settings.installed_apps.len(), count_before + 1);

        // Duplicate registration ignored
        settings.register_app(AppInfo {
            id: "com.example.app".to_string(),
            name: "Example App".to_string(),
            description: "An example".to_string(),
            executable: "/usr/bin/example".to_string(),
            icon_name: "example".to_string(),
            supported_categories: vec![],
            supported_extensions: vec![],
            supported_mime_types: vec![],
            is_system: false,
        });
        assert_eq!(settings.installed_apps.len(), count_before + 1);
    }

    #[test]
    fn test_unregister_app() {
        let mut settings = DefaultAppsSettings::default();
        let count_before = settings.installed_apps.len();

        settings.register_app(AppInfo {
            id: "com.example.removal".to_string(),
            name: "To Remove".to_string(),
            description: "Will be removed".to_string(),
            executable: "/usr/bin/remove".to_string(),
            icon_name: "remove".to_string(),
            supported_categories: vec![ContentCategory::TextEditor],
            supported_extensions: vec!["xyz".to_string()],
            supported_mime_types: vec![],
            is_system: false,
        });

        settings.set_extension_handler("xyz", "com.example.removal");

        settings.unregister_app("com.example.removal");
        assert_eq!(settings.installed_apps.len(), count_before);
        assert!(settings.custom_associations.is_empty());
    }

    #[test]
    fn test_third_party_count() {
        let settings = DefaultAppsSettings::default();
        assert_eq!(settings.third_party_app_count(), 0);
    }

    #[test]
    fn test_unknown_extension() {
        let settings = DefaultAppsSettings::default();
        assert!(settings.app_for_extension("xyz123").is_none());
    }

    // UI tests
    #[test]
    fn test_ui_new() {
        let ui = DefaultAppsUI::new();
        assert_eq!(ui.active_tab, DefaultAppsTab::Categories);
        assert!(ui.search_query.is_empty());
        assert!(!ui.dirty);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = DefaultAppsUI::new();
        ui.scroll_offset = 50.0;
        ui.set_tab(DefaultAppsTab::FileTypes);
        assert_eq!(ui.active_tab, DefaultAppsTab::FileTypes);
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn test_ui_toggle_expand() {
        let mut ui = DefaultAppsUI::new();
        assert!(ui.expanded_category.is_none());

        ui.toggle_expand(ContentCategory::WebBrowser);
        assert_eq!(ui.expanded_category, Some(ContentCategory::WebBrowser));

        ui.toggle_expand(ContentCategory::WebBrowser);
        assert!(ui.expanded_category.is_none());

        ui.toggle_expand(ContentCategory::EmailClient);
        assert_eq!(ui.expanded_category, Some(ContentCategory::EmailClient));
    }

    #[test]
    fn test_ui_render_produces_commands() {
        let ui = DefaultAppsUI::new();
        let cmds = ui.render(0.0, 0.0, 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_all() {
        assert_eq!(DefaultAppsTab::all().len(), 3);
    }

    #[test]
    fn test_app_handles_mime() {
        let apps = builtin_apps();
        let music = apps
            .iter()
            .find(|a| a.id == "org.ouros.musicplayer")
            .unwrap();
        assert!(music.handles_mime("audio/mpeg"));
        assert!(!music.handles_mime("video/mp4"));
    }
}
