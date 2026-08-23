//! Default application settings panel for the desktop shell.
//!
//! Manages which applications open by default for various content types
//! including web browsers, email clients, music players, video players,
//! image viewers, document readers, and custom file type associations.
//!
//! # Colour
//!
//! Every colour is read from the [`Palette`] handed to [`DefaultAppsUI::render`]
//! — this module used to carry its own eleven Mocha constants, which is the
//! defect `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`
//! names. Four judgements were made in the course of that conversion, and each
//! is a claim a test in this file has to be able to refute:
//!
//! 1. **The accent marks which one is in force, and nothing else.** Three
//!    sites take `p.accent`: which tab is open, the default app named under
//!    each category card, and — in an expanded card — the fill of the chip
//!    for the app that is currently the default. The last two are one fact
//!    drawn in two places, so the count of accented things is not fixed:
//!    the Categories tab carries a dozen of them at once. This module
//!    therefore cannot borrow the taskbar's "exactly one thing carries the
//!    accent" test, and checks a per-site table plus a per-tab count
//!    instead.
//! 2. **Peach marks a departure from the defaults, which is a state and not
//!    a position.** The "Reset all" button, an app name reached through a
//!    custom association, and the "Custom" badge beside it are all peach.
//!    That is a *category* mark — it says "this is not how it shipped" —
//!    so it must not follow the accent, and a test pins it against an
//!    accent swept through the whole palette.
//! 3. **The content well is `crust`, one rung below the panel it sits in.**
//!    The panel is `base`; the well is the recess drawn inside it. In both
//!    Mocha and Latte `crust` is the deeper of the two, so this survives the
//!    mode flip as a *relationship* rather than as a pair of values, and the
//!    test asserts the relationship.
//! 4. **The ink on the current chip is computed from the chip, never named.**
//!    It is [`readable_on`] of the accent, because the chip is a filled
//!    accent-coloured pill and its label has to stay legible on whatever
//!    the accent happens to be. This site is the module's one real trap:
//!    the stock accent is blue, whose luma is 175, so `readable_on` answers
//!    `0x11111B` — which *is* Mocha `crust`, the very constant that used to
//!    be written here. A leftover literal would therefore be invisible both
//!    to the membership sweep (which allows the endpoints outright) and to
//!    any test run at the stock accent. Only an off-palette accent exposes
//!    it, which is why the fixture uses one.

use appearance::{Palette, readable_on};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

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
            Self::WebBrowser => "\u{1F310}",     // globe
            Self::EmailClient => "\u{2709}",     // envelope
            Self::MusicPlayer => "\u{1F3B5}",    // music note
            Self::VideoPlayer => "\u{1F3AC}",    // clapper board
            Self::ImageViewer => "\u{1F5BC}",    // frame with picture
            Self::DocumentReader => "\u{1F4C4}", // page facing up
            Self::TextEditor => "\u{1F4DD}",     // memo
            Self::ArchiveManager => "\u{1F4E6}", // package
            Self::Terminal => "\u{1F4BB}",       // laptop
            Self::FileManager => "\u{1F4C1}",    // file folder
            Self::Calculator => "\u{1F522}",     // input numbers
            Self::Calendar => "\u{1F4C5}",       // calendar
        }
    }

    /// File extensions commonly associated with this category.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::WebBrowser => &["html", "htm", "xhtml", "mhtml", "url"],
            Self::EmailClient => &["eml", "msg"],
            Self::MusicPlayer => &["mp3", "flac", "ogg", "wav", "aac", "m4a", "wma", "opus"],
            Self::VideoPlayer => &["mp4", "avi", "mkv", "webm", "mov", "wmv", "flv", "m4v"],
            Self::ImageViewer => &[
                "png", "jpg", "jpeg", "bmp", "gif", "webp", "svg", "ico", "tiff",
            ],
            Self::DocumentReader => &["pdf", "epub", "djvu", "xps"],
            Self::TextEditor => &[
                "txt", "log", "cfg", "ini", "md", "rst", "yaml", "yml", "toml", "json", "xml",
            ],
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
            Self::ImageViewer => &[
                "image/png",
                "image/jpeg",
                "image/bmp",
                "image/gif",
                "image/webp",
            ],
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
            id: "org.slateos.terminal".to_string(),
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
            id: "org.slateos.explorer".to_string(),
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
            id: "org.slateos.editor".to_string(),
            name: "Text Editor".to_string(),
            description: "Plain text and code editor".to_string(),
            executable: "/usr/bin/editor".to_string(),
            icon_name: "text-editor".to_string(),
            supported_categories: vec![ContentCategory::TextEditor],
            supported_extensions: vec![
                "txt".into(),
                "log".into(),
                "cfg".into(),
                "ini".into(),
                "md".into(),
                "rst".into(),
                "yaml".into(),
                "yml".into(),
                "toml".into(),
                "json".into(),
                "xml".into(),
                "rs".into(),
                "py".into(),
                "c".into(),
                "cpp".into(),
                "h".into(),
            ],
            supported_mime_types: vec![
                "text/plain".to_string(),
                "text/markdown".to_string(),
                "application/json".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.slateos.viewer".to_string(),
            name: "Photo Viewer".to_string(),
            description: "Image and photo viewer".to_string(),
            executable: "/usr/bin/viewer".to_string(),
            icon_name: "image-viewer".to_string(),
            supported_categories: vec![ContentCategory::ImageViewer],
            supported_extensions: vec![
                "png".into(),
                "jpg".into(),
                "jpeg".into(),
                "bmp".into(),
                "gif".into(),
                "webp".into(),
                "svg".into(),
                "ico".into(),
            ],
            supported_mime_types: vec![
                "image/png".to_string(),
                "image/jpeg".to_string(),
                "image/bmp".to_string(),
                "image/gif".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.slateos.musicplayer".to_string(),
            name: "Music Player".to_string(),
            description: "Audio player and library manager".to_string(),
            executable: "/usr/bin/musicplayer".to_string(),
            icon_name: "music".to_string(),
            supported_categories: vec![ContentCategory::MusicPlayer],
            supported_extensions: vec![
                "mp3".into(),
                "flac".into(),
                "ogg".into(),
                "wav".into(),
                "aac".into(),
                "m4a".into(),
                "opus".into(),
            ],
            supported_mime_types: vec![
                "audio/mpeg".to_string(),
                "audio/flac".to_string(),
                "audio/ogg".to_string(),
                "audio/wav".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.slateos.videoplayer".to_string(),
            name: "Video Player".to_string(),
            description: "Video and media player".to_string(),
            executable: "/usr/bin/videoplayer".to_string(),
            icon_name: "video".to_string(),
            supported_categories: vec![ContentCategory::VideoPlayer],
            supported_extensions: vec![
                "mp4".into(),
                "avi".into(),
                "mkv".into(),
                "webm".into(),
                "mov".into(),
                "wmv".into(),
                "flv".into(),
            ],
            supported_mime_types: vec![
                "video/mp4".to_string(),
                "video/x-matroska".to_string(),
                "video/webm".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.slateos.pdfviewer".to_string(),
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
            id: "org.slateos.archivemanager".to_string(),
            name: "Archive Manager".to_string(),
            description: "Open and create archives".to_string(),
            executable: "/usr/bin/archivemanager".to_string(),
            icon_name: "archive".to_string(),
            supported_categories: vec![ContentCategory::ArchiveManager],
            supported_extensions: vec![
                "zip".into(),
                "tar".into(),
                "gz".into(),
                "bz2".into(),
                "7z".into(),
                "rar".into(),
                "xz".into(),
                "zst".into(),
            ],
            supported_mime_types: vec![
                "application/zip".to_string(),
                "application/x-tar".to_string(),
                "application/gzip".to_string(),
            ],
            is_system: true,
        },
        AppInfo {
            id: "org.slateos.calculator".to_string(),
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
            id: "org.slateos.calendar".to_string(),
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
            self.category_defaults.push((category, app_id.to_string()));
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
                && let Some(fallback) = builtin.iter().find(|a| a.handles_category(entry.0))
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
    ///
    /// `p` is the resolved palette; nothing below reads a colour from
    /// anywhere else.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Default Applications".to_string(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in DefaultAppsTab::all() {
            let label = tab.label();
            let tw = text::padded_width_any_weight(label, 12.0, 13.0);
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
            // The well is a recess *inside* the panel, so it takes the rung
            // below `base`. Both Mocha and Latte order crust deeper than
            // base, so this reads as a recess in either mode.
            color: p.crust,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            DefaultAppsTab::Categories => {
                self.render_categories_tab(&mut cmds, p, cx, cy, cw);
            }
            DefaultAppsTab::FileTypes => {
                self.render_filetypes_tab(&mut cmds, p, cx, cy, cw);
            }
            DefaultAppsTab::AppsList => {
                self.render_apps_tab(&mut cmds, p, cx, cy, cw);
            }
        }

        cmds
    }

    /// Render the categories tab.
    fn render_categories_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 24.0;

        // Reset all button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 100.0,
            y: row_y - 20.0,
            width: 100.0,
            height: 24.0,
            color: p.surface1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 88.0,
            y: row_y - 16.0,
            text: "Reset all".to_string(),
            font_size: 11.0,
            // Peach, not the accent: this is about undoing a departure from
            // the shipped defaults, which is a state rather than a position.
            color: p.peach,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });

            // Icon
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 10.0,
                text: category.icon().to_string(),
                font_size: 20.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Category name
            cmds.push(RenderCommand::Text {
                x: x + 44.0,
                y: row_y + 8.0,
                text: category.label().to_string(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Current default app. The accent is reserved for a category that
            // actually has one: "None set" is the absence of the thing the
            // accent exists to mark, so accenting it would mark every card
            // equally and leave the eye nothing to find. Two of the twelve
            // categories ship with no handler at all, so this is the ordinary
            // case rather than a corner.
            let app_name = default_app.map(|a| a.name.as_str()).unwrap_or("None set");
            cmds.push(RenderCommand::Text {
                x: x + 44.0,
                y: row_y + 30.0,
                text: app_name.to_string(),
                font_size: 12.0,
                color: if default_app.is_some() {
                    p.accent
                } else {
                    p.overlay0
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Expand indicator
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 16.0,
                text: if is_expanded { "\u{25B2}" } else { "\u{25BC}" }.to_string(),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // If expanded, show alternative apps
            if is_expanded {
                let alternatives = self.settings.apps_for_category(*category);
                let mut alt_x = x + 16.0;
                let alt_y = row_y + 56.0;

                for app in &alternatives {
                    let is_current = default_app.is_some_and(|d| d.id == app.id);
                    // Any weight: the current default is drawn bold, and
                    // sizing each chip to its own weight would slide the row
                    // sideways whenever the default changed.
                    let chip_w = text::padded_width_any_weight(&app.name, 12.0, 11.0);

                    cmds.push(RenderCommand::FillRect {
                        x: alt_x,
                        y: alt_y,
                        width: chip_w,
                        height: 28.0,
                        color: if is_current { p.accent } else { p.surface1 },
                        corner_radii: CornerRadii::all(14.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: alt_x + 12.0,
                        y: alt_y + 7.0,
                        text: app.name.clone(),
                        font_size: 11.0,
                        // Computed from the pill under it, never named: the
                        // accent is user-settable, so the only correct ink is
                        // whichever endpoint stays legible on it.
                        color: if is_current {
                            readable_on(p.accent)
                        } else {
                            p.text
                        },
                        font_weight: if is_current {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: None,
                        overflow: TextOverflow::Clip,
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
        p: &Palette,
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
            color: p.surface0,
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
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: p.subtext1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Extension
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y + 6.0,
                    width: 48.0,
                    height: 20.0,
                    color: p.surface1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 9.0,
                    text: format!(".{ext}"),
                    font_size: 11.0,
                    color: p.lavender,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // App name
                cmds.push(RenderCommand::Text {
                    x: x + 66.0,
                    y: row_y + 9.0,
                    text: app.map_or("(none)", |a| a.name.as_str()).to_string(),
                    font_size: 12.0,
                    color: if is_custom { p.peach } else { p.text },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Custom badge
                if is_custom {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 10.0,
                        text: "Custom".to_string(),
                        font_size: 10.0,
                        color: p.peach,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                row_y += 38.0;
            }

            row_y += 8.0;
        }
    }

    /// Render the installed apps tab.
    fn render_apps_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        let total = self.settings.installed_apps.len();
        let third_party = self.settings.third_party_app_count();

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("{total} installed apps ({third_party} third-party)"),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 24.0;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: p.surface0,
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
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });

            // App name
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: app.name.clone(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 28.0,
                text: app.description.clone(),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 120.0),
                overflow: TextOverflow::Ellipsis,
            });

            // System badge
            if app.is_system {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 68.0,
                    y: row_y + 8.0,
                    width: 52.0,
                    height: 18.0,
                    color: p.surface1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 62.0,
                    y: row_y + 10.0,
                    text: "System".to_string(),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Categories this app handles
            let categories: Vec<&str> =
                app.supported_categories.iter().map(|c| c.label()).collect();
            if !categories.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 42.0,
                    text: categories.join(", "),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 32.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            row_y += 64.0;
        }
    }
}

impl Default for DefaultAppsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

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
        let editor = apps.iter().find(|a| a.id == "org.slateos.editor").unwrap();
        assert!(editor.handles_category(ContentCategory::TextEditor));
        assert!(!editor.handles_category(ContentCategory::VideoPlayer));
    }

    #[test]
    fn test_app_handles_extension() {
        let apps = builtin_apps();
        let viewer = apps.iter().find(|a| a.id == "org.slateos.viewer").unwrap();
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
        assert!(!settings.set_default(ContentCategory::WebBrowser, "org.slateos.calculator"));
    }

    #[test]
    fn test_reset_default() {
        let mut settings = DefaultAppsSettings::default();
        settings.reset_default(ContentCategory::Terminal);
        let terminal = settings
            .default_for_category(ContentCategory::Terminal)
            .unwrap();
        assert_eq!(terminal.id, "org.slateos.terminal");
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
        settings.set_extension_handler("txt", "org.slateos.viewer");
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
        settings.set_extension_handler("TXT", "org.slateos.viewer");
        assert_eq!(settings.custom_association_count(), 1);

        // Second set with different case replaces
        settings.set_extension_handler("txt", "org.slateos.editor");
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
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 600.0, 800.0);
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
            .find(|a| a.id == "org.slateos.musicplayer")
            .unwrap();
        assert!(music.handles_mime("audio/mpeg"));
        assert!(!music.handles_mime("video/mp4"));
    }

    // ========================================================================
    // Palette conversion
    //
    // The tests below exist to refute the four judgements in the module docs.
    // ========================================================================

    use guitk::color::Color;

    /// The panel size every render below is taken at.
    ///
    /// Named because half the assertions select a command by its size, and a
    /// size that drifted from the render would silently select nothing.
    const W: f32 = 600.0;
    const H: f32 = 800.0;

    /// A palette whose accent is in no palette at all.
    ///
    /// This matters more than it looks. The stock accent *is* `blue`, so a
    /// site that still drew the deleted `BLUE` constant would be
    /// indistinguishable from one correctly reading `p.accent` — every
    /// assertion about the accent would pass while the bug was present.
    /// Magenta is in neither Mocha nor Latte, which separates the two.
    ///
    /// It is also chosen for its *luma*: at 105 it sits below `readable_on`'s
    /// threshold of 140, so ink computed from it is the light endpoint rather
    /// than `0x11111B`. Since `0x11111B` is exactly the Mocha `CRUST` that
    /// used to be written at the chip-ink site, the stock accent would make a
    /// leftover constant there indistinguishable from the correct answer —
    /// see judgement 4.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            assert!(
                name == "accent"
                    || (role.r, role.g, role.b) != (p.accent.r, p.accent.g, p.accent.b),
                "the fixture accent collides with role {name}, so an assertion \
                 about the accent could pass while reading the wrong role"
            );
        }
        p
    }

    /// A UI whose data reaches the branches the shipped defaults never do.
    ///
    /// Three properties of `DefaultAppsSettings::default()` make it unusable
    /// as a fixture, and each one hides a colour site rather than merely
    /// narrowing what is covered:
    ///
    /// - **every category has exactly one handling app**, so the alternatives
    ///   chip is always the current one and the `surface1`/`text` arm of the
    ///   two chip sites never renders at all;
    /// - **nothing is customised**, so both peach sites — the custom app name
    ///   and the "Custom" badge — never render;
    /// - **every builtin is a system app**, so the third-party count is
    ///   always zero and reads the same whether or not it is computed.
    ///
    /// Adding one rival music player and pointing `.mp3` at it turns all of
    /// those on. The category is left expanded because the chips only exist
    /// in an expanded card.
    fn exercised() -> DefaultAppsUI {
        let mut ui = DefaultAppsUI::new();
        ui.settings.register_app(AppInfo {
            id: "org.example.rival".to_string(),
            name: "Rival Player".to_string(),
            description: "A rival music player".to_string(),
            executable: "/usr/bin/rival".to_string(),
            icon_name: "rival".to_string(),
            supported_categories: vec![ContentCategory::MusicPlayer],
            supported_extensions: vec!["mp3".into()],
            supported_mime_types: vec![],
            is_system: false,
        });
        ui.settings
            .set_extension_handler("mp3", "org.example.rival");
        ui.expanded_category = Some(ContentCategory::MusicPlayer);
        ui
    }

    /// Every colour `cmds` puts on the screen.
    fn all_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colours of every `Text` command whose text is exactly `s`.
    fn text_colors(cmds: &[RenderCommand], s: &str) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one `Text` command reading `s`.
    ///
    /// Panics when there is not exactly one, because an assertion that
    /// silently picked the first of several matches would be checking a
    /// different site than the one it names.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let found = text_colors(cmds, s);
        assert_eq!(found.len(), 1, "expected exactly one command reading {s:?}");
        found[0]
    }

    /// The colours of every `FillRect` of exactly `w` x `h`.
    fn fills_sized(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one `Text` command reading `s` at `size` points.
    ///
    /// Needed wherever the same string is drawn twice at different sizes —
    /// an app's name appears both as the default under its category card and
    /// as the label on its own chip, and the two are different sites.
    fn text_color_sized(cmds: &[RenderCommand], s: &str, size: f32) -> Color {
        let found: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size,
                    ..
                } if text == s && *font_size == size => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            found.len(),
            1,
            "expected exactly one command reading {s:?} at {size}pt"
        );
        found[0]
    }

    /// The colours of every `FillRect` of height `h`, whatever its width.
    ///
    /// The alternatives chips are sized to their own labels, so they cannot
    /// be selected by width.
    fn fills_high(cmds: &[RenderCommand], h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. } if *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Each tab, rendered, paired with the tab it is.
    fn every_tab(p: &Palette) -> Vec<(DefaultAppsTab, Vec<RenderCommand>)> {
        DefaultAppsTab::all()
            .iter()
            .map(|t| {
                let mut ui = exercised();
                ui.active_tab = *t;
                (*t, ui.render(p, 0.0, 0.0, W, H))
            })
            .collect()
    }

    /// Nothing this panel draws comes from outside the palette it was given.
    ///
    /// Both modes and all three tabs, because the sweep is only ever as wide
    /// as the render handed to it: a tab that is never rendered is a tab
    /// whose colours are never checked.
    #[test]
    fn every_colour_this_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for (tab, cmds) in every_tab(&p) {
                // No `derived` list: the only computed colour here is
                // `readable_on(p.accent)`, and the sweep allows both of that
                // function's endpoints outright.
                crate::palette_check::assert_drawn_from(&p, &cmds, &[], &format!("{tab:?}"));
            }
        }
    }

    /// None of the eleven deleted constants is still being drawn.
    ///
    /// This is the check the module conversion actually turns on, and it runs
    /// in **light** mode for the reason the sweep's own docs give: every
    /// deleted constant was a Mocha value, Latte contains none of them, so a
    /// leftover names itself. Run in dark it would be worthless — nine of the
    /// eleven would still equal the role that replaced them.
    ///
    /// It also closes the sweep's documented blind spot. `assert_drawn_from`
    /// cannot reject `0x11111B`, because that is one of `readable_on`'s two
    /// answers; but with an accent below the luma threshold, `readable_on`
    /// returns the *other* endpoint, so `0x11111B` should not appear at all.
    #[test]
    fn none_of_the_eleven_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 11] = [
            ("BASE", 0x001E_1E2E),
            ("CRUST", 0x0011_111B),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("SUBTEXT1", 0x00BA_C2DE),
            ("BLUE", 0x0089_B4FA),
            ("PEACH", 0x00FA_B387),
            ("LAVENDER", 0x00B4_BEFE),
            ("OVERLAY0", 0x006C_7086),
        ];

        let p = accented(true);
        for (tab, cmds) in every_tab(&p) {
            for c in all_colors(&cmds) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                for (name, deleted) in DELETED {
                    assert_ne!(
                        rgb, deleted,
                        "the {tab:?} tab still draws the deleted constant \
                         {name} (#{deleted:06X}) in a light render"
                    );
                }
            }
        }
    }

    /// Every site, named one at a time, in the role this module claims for it.
    ///
    /// The sweep above proves only *membership*, and membership cannot see a
    /// swap: a card painted `surface1` instead of `surface0` draws a legal
    /// colour, and so does a well painted `base` instead of `crust`. Neither
    /// changes any count either, so a coverage test that tallied roles would
    /// pass both. n source sites need n assertions, so this is that table.
    ///
    /// Every command is selected by a size, a y, or a literal string the
    /// renderer writes — never by the colour under test, which would make the
    /// expectation a restatement of the code and unable to fail.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let by_tab = every_tab(&p);
            let cats = &by_tab[0].1;

            assert_eq!(fills_sized(cats, W, H), vec![p.base], "panel");
            assert_eq!(text_color(cats, "Default Applications"), p.text, "title");

            // The well is the recess inside the panel: 584 x 684 at this size.
            assert_eq!(fills_sized(cats, 584.0, 684.0), vec![p.crust], "well");

            // Tab strip: one fill behind the active tab, and three labels of
            // which exactly the active one is accented.
            assert_eq!(
                fills_high(cats, 32.0),
                vec![p.surface0],
                "only the active tab is filled"
            );
            assert_eq!(text_color(cats, "Default apps"), p.accent, "active tab");
            assert_eq!(text_color(cats, "File types"), p.subtext0, "idle tab");
            assert_eq!(
                text_color(cats, "Installed apps"),
                p.subtext0,
                "the other idle tab"
            );

            assert_eq!(
                fills_sized(cats, 100.0, 24.0),
                vec![p.surface1],
                "reset button"
            );
            assert_eq!(text_color(cats, "Reset all"), p.peach, "reset label");
            assert_eq!(
                text_color(cats, "Choose default apps for each type of content"),
                p.subtext0,
                "the tab's own subtitle"
            );

            // Twelve category cards, one of them expanded and so taller.
            // Indexed rather than counted: `{surface0 x 12}` is the same
            // multiset however the rungs are permuted.
            let short = fills_sized(cats, 552.0, 56.0);
            let tall = fills_sized(cats, 552.0, 100.0);
            assert_eq!(short.len(), 11, "eleven collapsed category cards");
            assert_eq!(tall.len(), 1, "one expanded category card");
            assert!(
                short.iter().chain(tall.iter()).all(|c| *c == p.surface0),
                "every category card sits on the same rung, expanded or not"
            );

            assert_eq!(
                text_color(cats, "Web browser"),
                p.text,
                "a category's own name"
            );
            assert_eq!(
                text_colors(cats, "\u{25BC}").len(),
                11,
                "eleven collapsed cards each carry a chevron"
            );
            assert!(
                text_colors(cats, "\u{25BC}")
                    .iter()
                    .chain(text_colors(cats, "\u{25B2}").iter())
                    .all(|c| *c == p.overlay0),
                "a chevron is the dimmest thing on the card"
            );

            // The Installed apps tab: the rows, the badge, and the two dim
            // lines under each app's name.
            let apps = &by_tab[2].1;
            assert!(
                fills_sized(apps, 552.0, 56.0)
                    .iter()
                    .all(|c| *c == p.surface0),
                "app rows"
            );
            assert_eq!(
                text_color(apps, "A rival music player"),
                p.subtext0,
                "an app's description"
            );
            assert_eq!(
                text_color(apps, "11 installed apps (1 third-party)"),
                p.subtext0,
                "the count line"
            );
            assert_eq!(
                fills_sized(apps, 52.0, 18.0).len(),
                10,
                "ten of the eleven apps are system apps and get a badge"
            );
            assert!(
                fills_sized(apps, 52.0, 18.0)
                    .iter()
                    .all(|c| *c == p.surface1),
                "the System badge is a raised pill"
            );
            assert!(
                text_colors(apps, "System").iter().all(|c| *c == p.overlay0),
                "the System badge's own label is dimmer than the pill"
            );

            // The File types tab: the extension pill and its lavender token.
            let types = &by_tab[1].1;
            assert!(
                fills_sized(types, 48.0, 20.0)
                    .iter()
                    .all(|c| *c == p.surface1),
                "extension pills"
            );
            assert_eq!(text_color(types, ".flac"), p.lavender, "extension token");
            assert_eq!(
                text_color(types, "1 custom associations"),
                p.subtext0,
                "the custom-association count"
            );
            assert_eq!(
                text_color(types, "Music player"),
                p.subtext1,
                "a file-type group heading is a rung above the body text"
            );
        }
    }

    /// Judgement 1: the accent marks which one is in force, and only that.
    ///
    /// This module cannot borrow the taskbar's "exactly one thing carries the
    /// accent" test, because the marks are on independent axes and a dozen of
    /// them are legitimately on screen at once. So the check is a per-site
    /// table *plus* a per-tab count — the table catches the accent moving to
    /// the wrong site, and the count catches it appearing at a site nobody
    /// named.
    #[test]
    fn the_accent_marks_which_app_is_in_force_and_nothing_else() {
        for light in [false, true] {
            let p = accented(light);
            let by_tab = every_tab(&p);

            // Categories: the open tab, the app named under each of the
            // twelve cards, and the current app's chip in the expanded one.
            let cats = &by_tab[0].1;
            assert_eq!(text_color(cats, "Default apps"), p.accent, "the open tab");
            assert_eq!(
                text_color_sized(cats, "Music Player", 12.0),
                p.accent,
                "the app in force for a category is named in the accent"
            );
            let chips = fills_high(cats, 28.0);
            assert_eq!(chips.len(), 2, "one chip per app that handles the category");
            assert_eq!(
                chips[0], p.accent,
                "the current app's chip is the filled one"
            );
            assert_eq!(
                chips[1], p.surface1,
                "a rival app's chip is merely raised, not accented"
            );
            let on_cats = all_colors(cats).iter().filter(|c| **c == p.accent).count();
            assert_eq!(
                on_cats, 12,
                "the Categories tab should accent its own tab, the app named \
                 under each of the *ten* cards that have one, and the current \
                 chip — found {on_cats}"
            );

            // The other two tabs accent their own tab and nothing else.
            for (i, name) in [(1, "File types"), (2, "Installed apps")] {
                let cmds = &by_tab[i].1;
                assert_eq!(text_color(cmds, name), p.accent, "the open tab");
                let n = all_colors(cmds).iter().filter(|c| **c == p.accent).count();
                assert_eq!(
                    n, 1,
                    "the {name} tab has nothing in force to mark, so its own \
                     tab should be the only accented thing — found {n}"
                );
            }
        }
    }

    /// A category with no handler is not marked as though it had one.
    ///
    /// `builtin_apps()` ships nothing that handles Web browser or Email, so
    /// two of the twelve cards read "None set". Those two are the absence of
    /// the fact the accent exists to mark, and drawing them in the accent —
    /// which is what this module did until the palette conversion made the
    /// rule explicit enough to notice — accents twelve of twelve cards and so
    /// marks nothing at all.
    ///
    /// The two counts are asserted together on purpose: "ten accented" and
    /// "two dim" only mean something as a pair, since either alone is
    /// satisfied by a card that simply vanished.
    #[test]
    fn a_category_with_no_default_app_is_not_accented_as_if_it_had_one() {
        for light in [false, true] {
            let p = accented(light);
            let cats = &every_tab(&p)[0].1;

            let none_set = text_colors(cats, "None set");
            assert_eq!(
                none_set.len(),
                2,
                "Web browser and Email ship with no handler"
            );
            assert!(
                none_set.iter().all(|c| *c == p.overlay0),
                "an unset category is dim, not accented"
            );

            let named = ContentCategory::all()
                .iter()
                .filter_map(|c| {
                    DefaultAppsSettings::default()
                        .default_for_category(*c)
                        .map(|_| ())
                })
                .count();
            assert_eq!(
                named, 10,
                "ten of the twelve categories ship with a default"
            );
        }
    }

    /// Judgement 2: peach marks a departure from the defaults, not a position.
    ///
    /// So it must hold still while the accent moves. Sweeping the accent
    /// through every role — rather than checking one off-palette value — is
    /// what makes this able to fail: a site that read `p.accent` would track
    /// the sweep, and a site frozen to Mocha peach would be caught by the
    /// light half of the loop.
    #[test]
    fn peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent() {
        for light in [false, true] {
            let base = Palette::for_mode(light);
            for (role, accent) in base.roles() {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let by_tab = every_tab(&p);

                let cats = &by_tab[0].1;
                assert_eq!(
                    text_color(cats, "Reset all"),
                    p.peach,
                    "undoing a customisation stayed peach with accent={role}"
                );

                let types = &by_tab[1].1;
                assert_eq!(
                    text_color_sized(types, "Rival Player", 12.0),
                    p.peach,
                    "an app reached through a custom association stayed peach \
                     with accent={role}"
                );
                assert_eq!(
                    text_color(types, "Custom"),
                    p.peach,
                    "the Custom badge stayed peach with accent={role}"
                );
            }
        }
    }

    /// Judgement 3: the well is a recess, which is a relationship not a pair.
    ///
    /// Asserting the two literal values would be the weaker test: it would
    /// pass on a palette whose crust had been made *lighter* than its base,
    /// which is the one thing that would actually break the appearance. So
    /// assert the ordering, and separately that the two sites do not collapse
    /// onto one colour.
    #[test]
    fn the_content_well_is_deeper_than_the_panel_it_sits_in() {
        fn luma(c: Color) -> f32 {
            0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b)
        }
        for light in [false, true] {
            let p = accented(light);
            let cmds = exercised().render(&p, 0.0, 0.0, W, H);
            let panel = fills_sized(&cmds, W, H)[0];
            let well = fills_sized(&cmds, 584.0, 684.0)[0];
            assert!(
                luma(well) < luma(panel),
                "the well is not deeper than the panel in {} mode: well {:?} \
                 vs panel {:?}",
                if light { "light" } else { "dark" },
                well,
                panel
            );
        }
    }

    /// Judgement 4: the current chip's ink is computed, never named.
    ///
    /// This is the module's one real trap, and it is worth stating plainly.
    /// The site used to read the constant `CRUST`, `0x11111B` — which is also
    /// exactly what `readable_on` answers for the *stock* accent, whose luma
    /// is 175. So at the stock accent a leftover constant and the correct
    /// call produce the same pixel, and the membership sweep cannot help
    /// either, because it allows both `readable_on` endpoints outright.
    ///
    /// Sweeping the accent across the palette is what breaks the tie: the
    /// expected ink flips between the two endpoints as the accent's luma
    /// crosses 140, and a frozen value cannot flip. The final assertion
    /// insists the sweep actually saw both endpoints, so that a palette which
    /// happened to be all-dark could not quietly turn this into a constant
    /// check.
    #[test]
    fn the_current_chips_ink_is_computed_from_the_accent_under_it() {
        for light in [false, true] {
            let mut seen = Vec::new();
            let base = Palette::for_mode(light);
            for (role, accent) in base.roles() {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = exercised().render(&p, 0.0, 0.0, W, H);
                let ink = text_color_sized(&cmds, "Music Player", 11.0);
                assert_eq!(
                    ink,
                    readable_on(accent),
                    "the current chip's ink was not computed from the chip \
                     under it when the accent was {role}"
                );
                assert_eq!(
                    text_color_sized(&cmds, "Rival Player", 11.0),
                    p.text,
                    "a chip that is not the current one takes ordinary ink"
                );
                if !seen.contains(&ink) {
                    seen.push(ink);
                }
            }
            assert_eq!(
                seen.len(),
                2,
                "the accent sweep never crossed readable_on's threshold, so \
                 this test degenerated into comparing one frozen value"
            );
        }
    }

    /// An empty search box is dimmer than one with a query in it.
    ///
    /// Both modes, and **both tabs**. The two search boxes share one query
    /// field but are two separate source sites, and n source sites need n
    /// assertions — checking only the one on the File types tab would leave
    /// the Installed apps box free to draw anything it liked.
    ///
    /// The two-mode loop is not decoration either: `p.text` in Mocha is
    /// exactly the `0xCDD6F4` these constants were converted from, so a
    /// frozen query ink compared against `p.text` in dark would be comparing
    /// a constant against itself.
    #[test]
    fn an_empty_search_box_is_dimmer_than_a_typed_query() {
        for light in [false, true] {
            let p = accented(light);
            for (tab, placeholder) in [
                (DefaultAppsTab::FileTypes, "Search file types..."),
                (DefaultAppsTab::AppsList, "Search apps..."),
            ] {
                let mut ui = exercised();
                ui.active_tab = tab;
                ui.search_query = String::new();
                let empty = ui.render(&p, 0.0, 0.0, W, H);
                assert_eq!(
                    text_color(&empty, placeholder),
                    p.overlay0,
                    "{tab:?}'s placeholder"
                );

                let mut ui = exercised();
                ui.active_tab = tab;
                "mp3".clone_into(&mut ui.search_query);
                let typed = ui.render(&p, 0.0, 0.0, W, H);
                assert_eq!(
                    text_color_sized(&typed, "mp3", 12.0),
                    p.text,
                    "{tab:?}'s typed query"
                );
            }
            assert_ne!(
                p.overlay0, p.text,
                "if these were equal a user could not tell a placeholder from \
                 a query they had typed"
            );
        }
    }
}
