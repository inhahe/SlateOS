//! Slate OS File Association Manager — GUI for File Type Associations
//!
//! A graphical application for managing which applications open which file
//! types. Provides browsing by category, search/filter, default-app assignment,
//! an "Open With" dialog mockup, and import/export of association configs.
//!
//! Uses the guitk library for rendering. Dark theme (Catppuccin Mocha).

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use std::collections::BTreeMap;
use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::dialog::{DialogAction, FileDialog};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use yamldoc::Document;

// ============================================================================
// Colours come from `appearance::Palette` -- see design-decisions 822.
// ============================================================================

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
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const SEARCH_WIDTH: f32 = 260.0;
const SEARCH_HEIGHT: f32 = 30.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 34.0;
const DIALOG_WIDTH: f32 = 400.0;
const DIALOG_HEIGHT: f32 = 360.0;
const DIALOG_APP_ROW_HEIGHT: f32 = 36.0;

// The toolbar's three buttons. They are different widths because their
// captions are: one shared width either truncated "Add File Type" or left
// "Reset" swimming in empty space.
const ADD_WIDTH: f32 = 110.0;
const EXPORT_WIDTH: f32 = 90.0;
const IMPORT_WIDTH: f32 = 90.0;
const RESET_WIDTH: f32 = 80.0;
/// The gap between two buttons sitting side by side.
const BUTTON_GAP: f32 = 8.0;

/// Height of the "CATEGORIES" caption above the sidebar's category list.
const CATEGORY_LABEL_HEIGHT: f32 = 24.0;
/// A row in the details panel's "Opens with" list.
const COMPAT_ROW_HEIGHT: f32 = 28.0;
/// The label column in the details panel, so the values line up.
const DETAIL_LABEL_WIDTH: f32 = 84.0;

/// The dialog's title strip, and the footer that holds its buttons.
const DIALOG_TITLE_HEIGHT: f32 = 44.0;
const DIALOG_FOOTER_HEIGHT: f32 = 52.0;
const DIALOG_BUTTON_WIDTH: f32 = 88.0;
const DIALOG_BUTTON_HEIGHT: f32 = 30.0;
/// The "always use this app" tick box.
const CHECKBOX_SIZE: f32 = 16.0;
/// A text field in the "Add File Type" dialog, and its caption above it.
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_LABEL_HEIGHT: f32 = 18.0;

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
    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Documents => pal.blue,
            Self::Images => pal.green,
            Self::Audio => pal.peach,
            Self::Video => pal.red,
            Self::Archives => pal.yellow,
            Self::Code => pal.mauve,
            Self::Other => pal.subtext0,
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

/// How many previous handlers a file type remembers.
///
/// Three, as `apps/settings/src/associations.rs` had it. This is a fallback
/// chain rather than a history the user browses: what it has to survive is an
/// application being uninstalled, and needing to walk back further than three
/// means three of a type's handlers were uninstalled between one use and the
/// next.
const MAX_HANDLER_HISTORY: usize = 3;

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
    /// Handlers this type had before the current one, most recent first.
    ///
    /// Exists so that uninstalling an application does not orphan every file
    /// type it opened: [`AssociationRegistry::remove_app`] walks this for the
    /// first handler still installed. Bounded by [`MAX_HANDLER_HISTORY`] --
    /// this is a fallback chain, not an audit log, and an unbounded one would
    /// grow with every change the user ever made.
    ///
    /// Ported from `apps/settings/src/associations.rs`, which had the design
    /// and was unreachable; see `known-issues.md`
    /// `TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`.
    pub handler_history: Vec<String>,
}

impl FileType {
    /// Create a new file type.
    pub fn new(extension: &str, mime_type: &str, description: &str) -> Self {
        Self {
            extension: extension.to_string(),
            mime_type: mime_type.to_string(),
            description: description.to_string(),
            default_app_id: None,
            handler_history: Vec::new(),
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
            handler_history: Vec::new(),
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
    /// The extension typed into the "Add File Type" dialog is not usable.
    ///
    /// There used to be a `ParseError` beside this one, carrying the line
    /// number of a bad line in the config file. It went with the hand-rolled
    /// `ext=app` grammar it belonged to: the file is a YAML document now, read
    /// by `yamldoc`, which repairs what it can and reports no line. Inventing a
    /// line number to keep the variant alive is exactly what the old comment
    /// here warned against doing.
    InvalidExtension(String),
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
            Self::InvalidExtension(detail) => write!(f, "Not a usable extension: {detail}"),
        }
    }
}

// ============================================================================
// AssociationRegistry — central registry for all associations
// ============================================================================

/// The central registry managing file types, applications, and their associations.
/// The configuration file associations live in, under the user's config
/// directory. Named without an extension because `settingsfile` adds one.
pub const CONFIG_NAME: &str = "fileassoc";

/// The mapping inside that file: extension -> application id.
const ASSOCIATIONS_KEY: &str = "associations";

/// What a freshly written associations file says about itself.
///
/// Only used when there is no file yet; an existing one keeps whatever the
/// user wrote in it, comments and key order included, because the document is
/// edited rather than rebuilt.
const CONFIG_HEADER: &str = "\
# Slate OS file associations.
#
# Each entry names the application that opens one kind of file. Editing this
# by hand is fine -- the File Associations program reads it back, and keeps
# any comments you add.
#
# The key below is part of this header, empty though it is: a comment with
# no key under it is trailing content with nothing to attach to, and the
# first association written would be inserted above it -- leaving the file
# explaining itself at the bottom.
associations:
";

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

    /// Remove an application, handing each file type it opened back to its
    /// previous handler.
    ///
    /// # What this used to do, and why it was wrong twice
    ///
    /// It deleted every association naming the app and stopped there.
    ///
    /// *First*, that orphans the file types: uninstalling an editor left every
    /// `.rs`, `.toml` and `.log` on the machine with no handler at all, even
    /// though the user had a perfectly good previous one. That is what
    /// [`FileType::handler_history`] now prevents.
    ///
    /// *Second* — and this is the part no test caught — it updated only *one*
    /// of the two records of "what opens this". `associations` lost the entry
    /// while `file_types[ext].default_app_id` went on naming the removed
    /// application, so the registry disagreed with itself and which answer a
    /// caller got depended on which map it happened to ask.
    pub fn remove_app(&mut self, app_id: &str) -> Result<AppInfo, AssocError> {
        let affected: Vec<String> = self
            .file_types
            .iter()
            .filter(|(_, ft)| ft.default_app_id.as_deref() == Some(app_id))
            .map(|(ext, _)| ext.clone())
            .collect();

        // Gone before the fallback search, so an app cannot fall back to
        // itself through a stale history entry.
        let removed = self
            .apps
            .remove(app_id)
            .ok_or_else(|| AssocError::AppNotFound(app_id.to_string()))?;

        for ext in affected {
            let successor = self.file_types.get(&ext).and_then(|ft| {
                ft.handler_history
                    .iter()
                    .find(|h| self.apps.contains_key(*h))
                    .cloned()
            });
            match successor {
                Some(next) => {
                    self.associations
                        .insert(ext.clone(), Association::new(&ext, &next));
                    if let Some(ft) = self.file_types.get_mut(&ext) {
                        ft.default_app_id = Some(next.clone());
                        ft.handler_history.retain(|h| h != &next);
                    }
                }
                None => {
                    self.associations.remove(&ext);
                    if let Some(ft) = self.file_types.get_mut(&ext) {
                        ft.default_app_id = None;
                    }
                }
            }
            // Whatever happened, the departed app is not a future fallback.
            if let Some(ft) = self.file_types.get_mut(&ext) {
                ft.handler_history.retain(|h| h != app_id);
            }
        }

        // Associations naming the app for a type it was not the *default* of.
        let stale: Vec<String> = self
            .associations
            .iter()
            .filter(|(_, a)| a.app_id == app_id)
            .map(|(k, _)| k.clone())
            .collect();
        for ext in stale {
            self.associations.remove(&ext);
            if let Some(ft) = self.file_types.get_mut(&ext) {
                if ft.default_app_id.as_deref() == Some(app_id) {
                    ft.default_app_id = None;
                }
            }
        }

        Ok(removed)
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

        // Also update the file type's default_app_id, remembering the handler
        // being replaced so that uninstalling the new one can fall back to it.
        if let Some(ft) = self.file_types.get_mut(&ext) {
            if let Some(previous) = ft.default_app_id.replace(app_id.to_string()) {
                if previous != app_id {
                    ft.handler_history.retain(|h| h != &previous);
                    ft.handler_history.insert(0, previous);
                    ft.handler_history.truncate(MAX_HANDLER_HISTORY);
                }
            }
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
    /// The writer is [`Association::to_config_line`] rather than a second copy
    /// of it inline. Having two was how the halves drifted apart: the reader
    /// grew a `trim` and an escape-aware split while the writer here stayed a
    /// bare `push_str`, so what came out was not what went back in.
    pub fn export_config(&self) -> String {
        let mut doc = Document::parse(CONFIG_HEADER);
        self.write_into(&mut doc);
        doc.to_text()
    }

    /// Read associations out of a configuration document.
    ///
    /// Answers the entries it could not apply rather than failing on them: an
    /// association naming an application that is no longer installed is the
    /// ordinary consequence of uninstalling something, not a damaged file, and
    /// every other entry is still good.
    pub fn read_from(&mut self, doc: &Document) -> Vec<AssocError> {
        let mut errors = Vec::new();
        for extension in doc.keys(&[ASSOCIATIONS_KEY]) {
            let Some(written) = doc.get_str(&[ASSOCIATIONS_KEY, &extension]) else {
                continue;
            };
            // A path is what `write_into` records. An id is accepted too, so a
            // file written before 2026-09-14 still loads rather than silently
            // losing every association in it -- the reader is where leniency
            // belongs, and the next save rewrites it as a path.
            let app_id = self
                .app_id_for_exec(&written)
                .unwrap_or_else(|| written.clone());
            if let Err(e) = self.set_default_app(&extension, &app_id) {
                errors.push(e);
            }
        }
        errors
    }

    /// The id of the application installed at `exec_path`, if any.
    #[must_use]
    pub fn app_id_for_exec(&self, exec_path: &str) -> Option<String> {
        self.apps
            .values()
            .find(|app| app.exec_path == exec_path)
            .map(|app| app.id.clone())
    }

    /// What opens files with this extension, as a path a caller can run.
    ///
    /// The question every other program actually has -- the file manager wants
    /// to start something, not to learn an id. Answered from the registry so
    /// the caller needs no catalogue of its own.
    #[must_use]
    pub fn opener_path(&self, extension: &str) -> Option<&str> {
        let assoc = self.associations.get(&extension.to_lowercase())?;
        self.apps
            .get(&assoc.app_id)
            .map(|app| app.exec_path.as_str())
    }

    /// Fold the current associations into a configuration document.
    ///
    /// **Entries the registry no longer holds are removed**, which is the half
    /// that is easy to leave out: writing only what is present would leave a
    /// cleared association sitting in the file, and it would come back at the
    /// next start looking like the clear had never happened.
    pub fn write_into(&self, doc: &mut Document) {
        for extension in doc.keys(&[ASSOCIATIONS_KEY]) {
            if !self.associations.contains_key(&extension) {
                doc.remove(&[ASSOCIATIONS_KEY, &extension]);
            }
        }
        for (extension, assoc) in &self.associations {
            // The executable path, not the application id.
            //
            // An id is a name only this program can resolve -- the catalogue
            // that maps `editor` to `/usr/bin/editor` lives in this binary, so
            // a file saying `txt: editor` is unreadable to everyone else. The
            // file manager needs to know what opens a `.txt` in order to open
            // one, and a path it can hand to the kernel is an answer where an
            // id is a question.
            //
            // This is the rule the taskbar's pinned apps already follow, for
            // the same reason and in the same words: a pin is an executable
            // path, and the name shown on it is looked up when it is drawn.
            // Storing the name too would be a second copy of it, stale the
            // first time the program is renamed.
            //
            // An association naming a program that has since been uninstalled
            // reads back as "could not be restored", which is the case
            // `read_from` already reports.
            let Some(app) = self.apps.get(&assoc.app_id) else {
                continue;
            };
            doc.set_str(&[ASSOCIATIONS_KEY, extension], &app.exec_path);
        }
    }

    /// Import associations from the text of a configuration document.
    /// Skips blank lines and comment lines (starting with `#`).
    /// Returns a list of errors for lines that failed to parse or apply.
    pub fn import_config(&mut self, config: &str) -> Vec<AssocError> {
        self.read_from(&Document::parse(config))
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

    /// Populate with the applications this system actually ships.
    ///
    /// **Every id here is a directory under `apps/`, and every path is the
    /// binary that directory builds.** That was not true until 2026-09-14:
    /// the table named `textedit`, `photoviewer`, `archiver`, `fileexplorer`,
    /// `codeeditor`, `imageeditor`, `browser` and `office`, of which *none*
    /// exist. Eight of eleven entries were fictional, so the program let a user
    /// choose which imaginary application should open their photographs, and
    /// as of the same morning it wrote that choice to disk and read it back.
    ///
    /// Two of the eight were also duplicates of a role already in the list --
    /// `codeeditor` beside `textedit`, `imageeditor` beside `photoviewer` --
    /// which is why the count falls: one program per role, named after the
    /// program.
    ///
    /// `browser` and `office` are gone with no replacement because this tree
    /// has neither. An association is a promise that pressing Enter opens
    /// something, and there is nothing to open.
    fn add_default_apps(&mut self) {
        let apps: &[(&str, &str, &str, &[&str], u64)] = &[
            (
                "editor",
                "Text Editor",
                "/usr/bin/editor",
                &[
                    "txt", "rs", "py", "js", "ts", "html", "css", "json", "xml", "toml", "yaml",
                    "c", "cpp", "h", "log", "ini", "csv", "rtf", "odt",
                ],
                1,
            ),
            ("pdfviewer", "PDF Viewer", "/usr/bin/pdfviewer", &["pdf"], 2),
            (
                "imageviewer",
                "Image Viewer",
                "/usr/bin/imageviewer",
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
                "archivemanager",
                "Archive Manager",
                "/usr/bin/archivemanager",
                &["zip", "tar", "gz", "7z", "rar"],
                6,
            ),
            (
                "explorer",
                "File Explorer",
                "/usr/bin/explorer",
                &["iso", "bin", "zip", "tar", "gz", "7z", "rar"],
                10,
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
            (FileCategory::Documents, "editor"),
            (FileCategory::Images, "imageviewer"),
            (FileCategory::Audio, "musicplayer"),
            (FileCategory::Video, "videoplayer"),
            (FileCategory::Archives, "archivemanager"),
            // Code opened `codeeditor`, which did not exist. The text editor
            // does, and it has syntax highlighting for twelve languages, so
            // this is the program that was meant rather than a downgrade.
            (FileCategory::Code, "editor"),
            (FileCategory::Other, "editor"),
        ];

        // Specific overrides that take precedence over the category default.
        //
        // The nine office-document overrides are gone with the `office` entry
        // they pointed at. Those file types keep their category default, which
        // is the text editor: opening a .docx in a text editor is a poor
        // answer, and it is a better one than a association naming a program
        // that was never written. When an office suite exists it gets its
        // overrides back in one line each.
        let specific_overrides: &[(&str, &str)] = &[
            ("pdf", "pdfviewer"),
            ("iso", "explorer"),
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
    /// "Open With" dialog for the extension in `dialog_target_ext`.
    OpenWith,
    /// Add New File Type dialog.
    AddFileType,
    /// The file picker, for an import or an export.
    ChooseFile,
}

/// Which way a configuration file is being moved.
///
/// Both directions put the same file picker on screen, and what the chosen
/// path means depends entirely on this. Keeping it beside the dialog rather
/// than inferring it later is what stops a cancelled export from being read
/// back as an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// Read a configuration document and apply it.
    Import,
    /// Write the current associations out.
    Export,
}

/// Which of the "Add File Type" dialog's three text fields has the caret.
///
/// The dialog is the reason this exists at all: `ActiveDialog::AddFileType`
/// was declared and then never drawn, never opened and never handled, so the
/// variant was a promise the program did not keep. A dialog that types needs
/// to know *what* is being typed into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewField {
    Extension,
    MimeType,
    Description,
}

impl NewField {
    const ALL: [Self; 3] = [Self::Extension, Self::MimeType, Self::Description];

    /// The caption drawn beside the box.
    pub fn label(self) -> &'static str {
        match self {
            Self::Extension => "Extension",
            Self::MimeType => "MIME Type",
            Self::Description => "Description",
        }
    }

    /// The field Tab moves to, wrapping at the end.
    pub fn next(self) -> Self {
        match self {
            Self::Extension => Self::MimeType,
            Self::MimeType => Self::Description,
            Self::Description => Self::Extension,
        }
    }
}

/// Everything the user can point at.
///
/// The renderer records these as it draws, so "where is the Export button?"
/// and "what did the user just click?" are answered by the same walk. There
/// was no answer to the second question at all before: this program drew a
/// toolbar, a sidebar, a table and a modal dialog, and handled no events, so
/// every control in it was a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The search box. Clicking it puts the caret in it.
    Search,
    AddButton,
    ImportButton,
    ExportButton,
    ResetButton,
    /// A sidebar entry. `None` is "All Types".
    Category(Option<FileCategory>),
    /// A row of the file-type table, by index into the *filtered* list.
    Row(usize),
    /// The table under its rows: the wheel scrolls anywhere over it, but a
    /// click below the last row selects nothing because no row is drawn there.
    Table,
    OpenWithButton,
    ClearButton,
    /// An entry of the details panel's "Compatible Apps" list.
    CompatibleApp(usize),
    /// A row of the "Open With" dialog's application list.
    DialogApp(usize),
    DialogAlwaysUse,
    DialogOk,
    DialogCancel,
    /// One of the "Add File Type" dialog's text boxes.
    DialogField(NewField),
    /// The "Add File Type" dialog's category button, which cycles.
    DialogCategory,
    /// Everything an open dialog covers.
    Scrim,
}

type Frame = guitk::frame::Frame<Target>;

/// Where every control is, for one window size.
///
/// Derived from the live size rather than remembered, because a compositor may
/// hand `render` a size it has never sent a `Resize` for. Nothing is clamped up
/// to a minimum: a control laid out past the window's edge would keep its hit
/// box and so stay clickable while invisible, which is worse than a cramped
/// window. Every piece shrinks instead, down to nothing, and a final trim
/// against the window rectangle catches the positions that are fixed offsets
/// from the toolbar.
struct Layout {
    width: f32,
    height: f32,
    search: Rect,
    /// Where the last action's outcome is reported, between the search box and
    /// the buttons. Whatever room is left over, which may be none.
    status: Rect,
    add: Rect,
    import: Rect,
    export: Rect,
    reset: Rect,
    sidebar: Rect,
    all_types: Rect,
    /// The top of the first category row; they run downwards from here.
    categories_top: f32,
    table: Rect,
    table_header: Rect,
    /// The part of the table the rows are visible in.
    rows: Rect,
    /// Absolute x of the Ext / Description / MIME / Default App columns.
    columns: [f32; 4],
    details: Rect,
    dialog: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        // A compositor mid-resize can report a zero-width window, and a NaN
        // would poison every comparison downstream -- once `scroll_offset` is
        // NaN the table never moves again.
        let width = if width.is_finite() {
            width.max(1.0)
        } else {
            WINDOW_WIDTH
        };
        let height = if height.is_finite() {
            height.max(1.0)
        } else {
            WINDOW_HEIGHT
        };

        // --- toolbar: buttons right-aligned, search and status take the rest -
        let btn_y = (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0;
        let reset = Rect::new(
            (width - RESET_WIDTH - PADDING).max(0.0),
            btn_y,
            RESET_WIDTH,
            BUTTON_HEIGHT,
        );
        let export = Rect::new(
            (reset.x - EXPORT_WIDTH - BUTTON_GAP).max(0.0),
            btn_y,
            EXPORT_WIDTH,
            BUTTON_HEIGHT,
        );
        let import = Rect::new(
            (export.x - IMPORT_WIDTH - BUTTON_GAP).max(0.0),
            btn_y,
            IMPORT_WIDTH,
            BUTTON_HEIGHT,
        );
        let add = Rect::new(
            (import.x - ADD_WIDTH - BUTTON_GAP).max(0.0),
            btn_y,
            ADD_WIDTH,
            BUTTON_HEIGHT,
        );

        let sidebar_w = SIDEBAR_WIDTH.min(width);
        let search_x = sidebar_w + PADDING;
        let search = Rect::new(
            search_x,
            (TOOLBAR_HEIGHT - SEARCH_HEIGHT) / 2.0,
            SEARCH_WIDTH.min((add.x - BUTTON_GAP - search_x).max(0.0)),
            SEARCH_HEIGHT,
        );
        let status_x = search.right() + PADDING;
        let status = Rect::new(
            status_x,
            btn_y,
            (add.x - BUTTON_GAP - status_x).max(0.0),
            BUTTON_HEIGHT,
        );

        // --- body: sidebar | table | details --------------------------------
        let body_y = TOOLBAR_HEIGHT.min(height);
        let body_h = (height - body_y).max(0.0);
        let sidebar = Rect::new(0.0, body_y, sidebar_w, body_h);
        let all_types = Rect::new(0.0, body_y, sidebar_w, SIDEBAR_ITEM_HEIGHT.min(body_h));
        // The "CATEGORIES" caption sits in the gap between the two.
        let categories_top = body_y + SIDEBAR_ITEM_HEIGHT + CATEGORY_LABEL_HEIGHT;

        // The details panel gives way first when the window narrows, so the
        // table -- the part of the window that has content in it -- keeps the
        // larger share instead of being squeezed to nothing behind a panel of
        // fixed width.
        let details_w = DETAILS_PANEL_WIDTH.min((width - sidebar_w).max(0.0) / 2.0);
        let details = Rect::new(width - details_w, body_y, details_w, body_h);
        let table = Rect::new(sidebar_w, body_y, (details.x - sidebar_w).max(0.0), body_h);
        let header_h = TABLE_HEADER_HEIGHT.min(table.h);
        let table_header = Rect::new(table.x, table.y, table.w, header_h);
        let rows = Rect::new(
            table.x,
            table.y + header_h,
            table.w,
            (table.h - header_h).max(0.0),
        );

        // Fractions of the table's own width rather than offsets from its left
        // edge, so a narrow table draws four narrow columns instead of putting
        // "Default App" past its right edge.
        let inner = (table.w - 2.0 * PADDING).max(0.0);
        let col = |f: f32| table.x + PADDING + f * inner;
        let columns = [col(0.0), col(0.14), col(0.46), col(0.80)];

        let dw = DIALOG_WIDTH.min(width);
        let dh = DIALOG_HEIGHT.min(height);
        let dialog = Rect::new((width - dw) / 2.0, (height - dh) / 2.0, dw, dh);

        let window = Rect::new(0.0, 0.0, width, height);
        let trim = |r: Rect| r.intersect(window).unwrap_or(Rect::EMPTY);

        Self {
            width,
            height,
            search: trim(search),
            status: trim(status),
            add: trim(add),
            import: trim(import),
            export: trim(export),
            reset: trim(reset),
            sidebar: trim(sidebar),
            all_types: trim(all_types),
            categories_top,
            table: trim(table),
            table_header: trim(table_header),
            rows: trim(rows),
            columns,
            details: trim(details),
            dialog: trim(dialog),
        }
    }

    /// The `i`th sidebar category row, trimmed to the sidebar.
    fn category_row(&self, i: usize) -> Rect {
        let y = self.categories_top + i as f32 * SIDEBAR_ITEM_HEIGHT;
        Rect::new(0.0, y, self.sidebar.w, SIDEBAR_ITEM_HEIGHT)
            .intersect(self.sidebar)
            .unwrap_or(Rect::EMPTY)
    }

    /// The "Open With" dialog's application list viewport.
    fn dialog_list(&self) -> Rect {
        Rect::new(
            self.dialog.x,
            self.dialog.y + DIALOG_TITLE_HEIGHT,
            self.dialog.w,
            (self.dialog.h - DIALOG_TITLE_HEIGHT - DIALOG_FOOTER_HEIGHT).max(0.0),
        )
        .intersect(self.dialog)
        .unwrap_or(Rect::EMPTY)
    }

    /// The dialog's `Cancel` and `OK` buttons, in that order.
    fn dialog_buttons(&self) -> (Rect, Rect) {
        let h = DIALOG_BUTTON_HEIGHT.min(self.dialog.h);
        let y = self.dialog.bottom() - PADDING - h;
        let ok_x = self.dialog.right() - PADDING - DIALOG_BUTTON_WIDTH;
        let cancel_x = ok_x - BUTTON_GAP - DIALOG_BUTTON_WIDTH;
        let clip = |r: Rect| r.intersect(self.dialog).unwrap_or(Rect::EMPTY);
        (
            clip(Rect::new(cancel_x, y, DIALOG_BUTTON_WIDTH, h)),
            clip(Rect::new(ok_x, y, DIALOG_BUTTON_WIDTH, h)),
        )
    }

    /// The "Always use this app" checkbox, and the whole clickable strip that
    /// includes its caption -- a 16-pixel box is a hard target for a mouse and
    /// a harder one for a finger.
    fn dialog_checkbox(&self) -> (Rect, Rect) {
        let y = self.dialog.bottom() - DIALOG_FOOTER_HEIGHT + PADDING;
        let box_rect = Rect::new(self.dialog.x + PADDING, y, CHECKBOX_SIZE, CHECKBOX_SIZE);
        let strip = Rect::new(
            self.dialog.x + PADDING,
            y - 4.0,
            (self.dialog.w - 2.0 * PADDING).max(0.0),
            CHECKBOX_SIZE + 8.0,
        );
        let clip = |r: Rect| r.intersect(self.dialog).unwrap_or(Rect::EMPTY);
        (clip(box_rect), clip(strip))
    }

    /// The `i`th text box of the "Add File Type" dialog.
    fn dialog_field(&self, i: usize) -> Rect {
        let y = self.dialog.y
            + DIALOG_TITLE_HEIGHT
            + PADDING
            + i as f32 * (FIELD_HEIGHT + FIELD_LABEL_HEIGHT + PADDING)
            + FIELD_LABEL_HEIGHT;
        Rect::new(
            self.dialog.x + PADDING,
            y,
            (self.dialog.w - 2.0 * PADDING).max(0.0),
            FIELD_HEIGHT,
        )
        .intersect(self.dialog)
        .unwrap_or(Rect::EMPTY)
    }

    /// The category button below the three text boxes.
    fn dialog_category(&self) -> Rect {
        let last = self.dialog_field(NewField::ALL.len().saturating_sub(1));
        Rect::new(
            self.dialog.x + PADDING,
            last.bottom() + PADDING + FIELD_LABEL_HEIGHT,
            (self.dialog.w - 2.0 * PADDING).max(0.0),
            FIELD_HEIGHT,
        )
        .intersect(self.dialog)
        .unwrap_or(Rect::EMPTY)
    }
}

/// Full UI state for the file association manager.
pub struct FileAssocUI {
    /// The underlying association registry.
    pub registry: AssociationRegistry,
    /// Currently selected category in the sidebar (None = show all).
    pub selected_category: Option<FileCategory>,
    /// Current search query string.
    pub search_query: String,
    /// Whether typed text goes to the search box.
    pub search_focused: bool,
    /// Index of the selected file type in the current filtered list.
    pub selected_index: Option<usize>,
    /// Scroll offset for the file type list, in pixels.
    pub scroll_offset: f32,
    /// Currently active dialog.
    pub active_dialog: ActiveDialog,
    /// In the "Open With" dialog, which app is selected.
    pub dialog_selected_app: Option<usize>,
    /// "Always use this app" checkbox state in the "Open With" dialog.
    pub dialog_always_use: bool,
    /// The extension that the "Open With" dialog is targeting.
    pub dialog_target_ext: String,
    /// The "Add File Type" dialog's three text fields.
    pub new_ext: String,
    pub new_mime: String,
    pub new_desc: String,
    /// The category the new file type will be filed under.
    pub new_category: FileCategory,
    /// Which of the three fields has the caret.
    pub new_field: NewField,
    /// What the last action did, reported in the toolbar.
    ///
    /// Export and Reset used to be buttons that could not be pressed; now that
    /// they can, they have to say something, because "the config was exported"
    /// is otherwise indistinguishable from "the click missed".
    pub status: String,
    /// The file picker, when one is up.
    ///
    /// Replaces a `last_export: String` that held the exported text and was
    /// **rendered nowhere**. Clicking Export set that field and put "Exported
    /// 8 association(s)" in the status bar, and that was the whole of it: no
    /// file, no panel, nothing the user could carry to another machine. The
    /// message was true about a variable and false about the world.
    ///
    /// `import_config` had the matching problem from the other side -- fully
    /// tested, reachable from nothing. Both are this dialog now.
    pub file_dialog: Option<FileDialog>,
    /// Which direction `file_dialog` is serving.
    pub transfer: Transfer,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// The configuration file as it was read, kept rather than rebuilt.
    ///
    /// This is what makes a hand-edited file survive being saved over: the
    /// user's comments, their key order and their spacing are all in here, and
    /// `write_into` edits it in place instead of writing a fresh one.
    doc: Document,
}

impl Default for FileAssocUI {
    fn default() -> Self {
        Self::new()
    }
}

impl FileAssocUI {
    /// Open on the user's saved associations.
    ///
    /// The one `main` calls. A missing or unreadable file yields the defaults,
    /// which is the ordinary state on a fresh install rather than an error to
    /// report to someone who has simply never changed an association.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(settingsfile::load(CONFIG_NAME))
    }

    /// Open on an already-read document.
    ///
    /// Split out from [`load`](Self::load) so the saved format can be
    /// exercised without a filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        let mut ui = Self::new();
        // **Once a file exists it is the whole truth about associations**, so
        // the built-in defaults are cleared before it is applied. Otherwise an
        // association the user deliberately cleared comes straight back: a
        // cleared entry is an *absence* from the file, and an absence is
        // indistinguishable from one that was never set, so the default under
        // it would win every time. The file types and the applications are
        // untouched -- those are the catalogue, not the choices.
        //
        // A missing file has no `associations` key at all, which is the
        // first-run case and the one where the defaults are wanted.
        if doc.contains(&[ASSOCIATIONS_KEY]) {
            ui.registry.associations.clear();
        }
        let dropped = ui.registry.read_from(&doc);
        if !dropped.is_empty() {
            // Said out loud rather than swallowed. An entry that will not
            // restore is almost always an application that has been
            // uninstalled since, and the association silently reverting to a
            // default is the kind of change a user notices later and cannot
            // explain.
            ui.status = format!(
                "{} saved association(s) could not be restored",
                dropped.len()
            );
        }
        ui.doc = doc;
        ui
    }

    /// Write the associations back to the user's configuration file.
    ///
    /// Called after every change, because there is no Save button and there
    /// should not be one: this program is a list of choices, and a choice that
    /// has to be confirmed somewhere else is a choice the user can lose. Until
    /// this existed, every one of them *was* lost -- the registry was built
    /// from defaults at startup and never read or written to a disk at all,
    /// while `export_config` and `import_config` sat beside it fully tested,
    /// with the export reachable only as text in a panel and the import
    /// reachable from nothing.
    ///
    /// Only failure is reported. A save that worked has nothing to say that
    /// the change itself did not already say.
    fn persist(&mut self) {
        self.registry.write_into(&mut self.doc);
        if let Err(e) = settingsfile::store(CONFIG_NAME, &self.doc) {
            self.status = format!("Could not save associations: {e}");
        }
    }

    /// Create a new UI state with the default registry and no saved file.
    ///
    /// **Touches no filesystem**, which is why it is not the one `main` calls:
    /// 48 tests build a UI, and a constructor that read the configuration
    /// directory would have all of them reading -- and the ones that go on to
    /// change an association, writing -- the developer's own settings. See
    /// [`load`](Self::load).
    pub fn new() -> Self {
        Self {
            registry: AssociationRegistry::with_defaults(),
            doc: Document::parse(CONFIG_HEADER),
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            selected_category: None,
            search_query: String::new(),
            search_focused: false,
            selected_index: None,
            scroll_offset: 0.0,
            active_dialog: ActiveDialog::None,
            dialog_selected_app: None,
            dialog_always_use: false,
            dialog_target_ext: String::new(),
            new_ext: String::new(),
            new_mime: String::new(),
            new_desc: String::new(),
            new_category: FileCategory::Other,
            new_field: NewField::Extension,
            status: String::new(),
            file_dialog: None,
            transfer: Transfer::Export,
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
        // `filtered_file_types` hands back borrows *into* the registry, so
        // everything wanted from the row is taken as owned values before a
        // single field of `self` is written. Nothing here is expensive enough
        // for that to matter, and the alternative — holding the borrow across
        // the assignment — is what the borrow checker is objecting to.
        let Some(idx) = self.selected_index else {
            return;
        };
        let Some((ext, current)) = self
            .filtered_file_types()
            .get(idx)
            .map(|ft| (ft.extension.clone(), ft.default_app_id.clone()))
        else {
            return;
        };

        // Start on the app that is already the default, so pressing OK with
        // "always use" ticked confirms what is already true rather than
        // silently changing it to whichever app happens to sort first.
        let compatible = self.registry.apps_for_extension(&ext);
        self.dialog_selected_app = current
            .as_deref()
            .and_then(|id| compatible.iter().position(|a| a.id == id))
            .or(if compatible.is_empty() { None } else { Some(0) });
        self.dialog_target_ext = ext;
        self.active_dialog = ActiveDialog::OpenWith;
        self.dialog_always_use = false;
    }

    /// Open the "Add File Type" dialog with empty fields.
    pub fn open_add_file_type_dialog(&mut self) {
        self.new_ext.clear();
        self.new_mime.clear();
        self.new_desc.clear();
        self.new_category = self.selected_category.unwrap_or(FileCategory::Other);
        self.new_field = NewField::Extension;
        self.active_dialog = ActiveDialog::AddFileType;
    }

    /// Confirm the "Open With" dialog selection.
    pub fn confirm_open_with(&mut self) -> Result<(), AssocError> {
        if self.active_dialog != ActiveDialog::OpenWith {
            return Ok(());
        }

        let ext = self.dialog_target_ext.clone();
        let chosen = self.dialog_selected_app.and_then(|i| {
            self.registry
                .apps_for_extension(&ext)
                .get(i)
                .map(|a| a.id.clone())
        });

        let mut outcome = Ok(());
        if let Some(app_id) = chosen {
            if self.dialog_always_use {
                outcome = self.registry.set_default_app(&ext, &app_id);
                self.status = match &outcome {
                    Ok(()) => format!(".{ext} now opens with {app_id}"),
                    Err(e) => format!("Could not set the default for .{ext}: {e}"),
                };
                self.persist();
            } else {
                // Without "always", the choice is a one-off launch. There is no
                // launcher to hand it to yet, so say so rather than pretending
                // the association changed.
                self.status = format!("Opened .{ext} with {app_id} once (not made default)");
            }
        }

        self.active_dialog = ActiveDialog::None;
        outcome
    }

    /// Confirm the "Add File Type" dialog, registering the new type.
    ///
    /// The extension is the only required field: a MIME type nobody knows and
    /// a description nobody wrote are better left blank than guessed at, and
    /// both are visible in the table where they can be seen to be missing.
    pub fn confirm_add_file_type(&mut self) -> Result<(), AssocError> {
        if self.active_dialog != ActiveDialog::AddFileType {
            return Ok(());
        }

        let ext = self.new_ext.trim().trim_start_matches('.').to_string();
        if ext.is_empty() {
            self.status = String::from("A file type needs an extension");
            return Err(AssocError::InvalidExtension(String::from(
                "the extension box is empty",
            )));
        }
        if self.registry.get_file_type(&ext).is_some() {
            self.status = format!(".{ext} is already registered");
            return Err(AssocError::AlreadyExists(ext));
        }

        let mime = if self.new_mime.trim().is_empty() {
            String::from("application/octet-stream")
        } else {
            self.new_mime.trim().to_string()
        };
        let desc = if self.new_desc.trim().is_empty() {
            format!("{} file", ext.to_uppercase())
        } else {
            self.new_desc.trim().to_string()
        };

        self.registry
            .register_file_type(FileType::new(&ext, &mime, &desc), self.new_category);
        self.status = format!(".{ext} added to {}", self.new_category.label());
        self.active_dialog = ActiveDialog::None;
        // The new row may not be in the current filter at all, so the selection
        // is dropped rather than left pointing at whatever moved into its slot.
        self.selected_index = None;
        self.clamp_scroll();
        Ok(())
    }

    /// Shut whichever dialog is open, changing nothing.
    pub fn cancel_dialog(&mut self) {
        self.active_dialog = ActiveDialog::None;
        self.file_dialog = None;
    }

    /// Put up the file picker for an import or an export.
    ///
    /// The toolkit's dialog is pure: it draws whatever listing it is given and
    /// reads no directory itself, so the host lists and hands it over. That is
    /// the same contract `pathbar::set_completions` follows, and the reason a
    /// dialog opened without this step draws its chrome and lists nothing for
    /// ever.
    fn open_transfer_dialog(&mut self, transfer: Transfer) {
        let start = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let mut dialog = match transfer {
            Transfer::Import => FileDialog::open().with_initial_path(start),
            Transfer::Export => FileDialog::save()
                .with_initial_path(start)
                .with_filename("associations.conf"),
        };
        dialog.set_entries(guitk::dialog::list_directory(dialog.current_path()));
        self.transfer = transfer;
        self.file_dialog = Some(dialog);
        self.active_dialog = ActiveDialog::ChooseFile;
    }

    /// Carry out the transfer the user just chose a path for.
    ///
    /// An import saves afterwards for the same reason every other change here
    /// does: this program has no Save button, and a choice that has to be
    /// confirmed somewhere else is a choice the user can lose.
    fn transfer_with(&mut self, path: &std::path::Path) {
        match self.transfer {
            Transfer::Export => {
                let text = self.registry.export_config();
                self.status = match safeio::write_str_atomically(path, &text) {
                    Ok(()) => format!(
                        "Exported {} association(s) to {}",
                        self.registry.association_count(),
                        path.display()
                    ),
                    Err(e) => format!("Could not write {}: {e}", path.display()),
                };
            }
            Transfer::Import => match std::fs::read_to_string(path) {
                Ok(text) => {
                    let errors = self.registry.import_config(&text);
                    self.status = match errors.first() {
                        // Every line that parsed has been applied; the count is
                        // what could not be. Naming the first is what makes the
                        // message actionable -- "3 problems" sends the user
                        // looking through the file themselves.
                        Some(first) => format!(
                            "Imported with {} problem(s), first: {first}",
                            errors.len()
                        ),
                        None => format!(
                            "Imported {} association(s) from {}",
                            self.registry.association_count(),
                            path.display()
                        ),
                    };
                    self.persist();
                }
                Err(e) => self.status = format!("Could not read {}: {e}", path.display()),
            },
        }
    }

    /// Give the picker an event; `None` when there is no picker up.
    fn file_dialog_event(&mut self, event: &Event) -> Option<EventResult> {
        let (width, height) = (self.window_width, self.window_height);
        let action = {
            let dialog = self.file_dialog.as_mut()?;
            match event {
                Event::Key(key) if key.pressed => dialog.handle_event(key, height),
                Event::Mouse(mouse) => dialog.handle_mouse(mouse, width, height),
                _ => DialogAction::None,
            }
        };
        match action {
            DialogAction::Selected(path) => {
                self.cancel_dialog();
                self.transfer_with(&path);
            }
            DialogAction::Cancelled => self.cancel_dialog(),
            // It has moved and is still showing the old directory's contents
            // until it is told what is in the new one.
            DialogAction::NavigatedTo(path) => {
                if let Some(dialog) = self.file_dialog.as_mut() {
                    dialog.set_entries(guitk::dialog::list_directory(&path));
                }
            }
            DialogAction::None => {}
        }
        Some(EventResult::Consumed)
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

    // ========================================================================
    // Layout and scrolling
    // ========================================================================

    fn layout(&self) -> Layout {
        Layout::new(self.window_width, self.window_height)
    }

    /// How far the table can scroll before its last row sits on the bottom of
    /// the viewport.
    pub fn max_scroll(&self) -> f32 {
        self.max_scroll_in(&self.layout())
    }

    fn max_scroll_in(&self, l: &Layout) -> f32 {
        let content = self.filtered_file_types().len() as f32 * ROW_HEIGHT;
        (content - l.rows.h).max(0.0)
    }

    /// Pull the offset back inside its bounds after the list or the viewport
    /// changed shape under it.
    pub fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());
    }

    fn scroll_by(&mut self, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        self.scroll_offset += delta;
        self.clamp_scroll();
    }

    /// Bring the selected row inside the viewport, moving as little as
    /// possible. Keyboard navigation is useless without it: Down would walk
    /// the selection off the bottom of a list that never followed.
    fn scroll_selection_into_view(&mut self) {
        let Some(i) = self.selected_index else {
            return;
        };
        let l = self.layout();
        let top = i as f32 * ROW_HEIGHT;
        let bottom = top + ROW_HEIGHT;
        if top < self.scroll_offset {
            self.scroll_offset = top;
        } else if bottom > self.scroll_offset + l.rows.h {
            self.scroll_offset = bottom - l.rows.h;
        }
        self.clamp_scroll();
    }

    /// Every visible table row, as `(screen y of its top, index into the
    /// filtered list)`.
    ///
    /// The renderer and the hit test read the same numbers rather than merely
    /// agreeing on a formula.
    fn table_rows(&self, l: &Layout) -> Vec<(f32, usize)> {
        let top = l.rows.y - self.scroll_offset;
        (0..self.filtered_file_types().len())
            .map(|i| (top + i as f32 * ROW_HEIGHT, i))
            .filter(|&(y, _)| y + ROW_HEIGHT > l.rows.y && y < l.rows.bottom())
            .collect()
    }

    /// Adopt a new window size and pull anything that hung off the old one
    /// back inside.
    fn resize(&mut self, width: f32, height: f32) {
        self.window_width = width;
        self.window_height = height;
        self.clamp_scroll();
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// The control under `(x, y)`, or `None` for bare background.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.window_width, self.window_height)
            .hit_test(x, y)
    }

    /// Handle a UI event (keyboard or mouse).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker is modal and answers everything but a resize, which
        // belongs to the surface rather than to what is drawn on it.
        if !matches!(event, Event::Resize { .. })
            && let Some(result) = self.file_dialog_event(event)
        {
            return result;
        }
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Resize { width, height } => {
                self.resize(*width as f32, *height as f32);
                EventResult::Consumed
            }
            Event::Mouse(mouse) => {
                let (x, y) = (mouse.x, mouse.y);
                match mouse.kind {
                    MouseEventKind::Press(MouseButton::Left) => self.handle_click(x, y),
                    // `dy` is a notch count, not a distance (see `guitk::wheel`),
                    // and three rows a notch is the toolkit's convention.
                    MouseEventKind::Scroll { dy, .. } => self.handle_scroll(x, y, dy),
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_scroll(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        match self.target_at(x, y) {
            Some(Target::Table | Target::Row(_)) => {
                self.scroll_by(wheel::pixels(dy, ROW_HEIGHT));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        let target = self.target_at(x, y);
        // Any click that is not in the search box takes the caret out of it,
        // so the next keystroke does not silently filter the table.
        if target != Some(Target::Search) {
            self.search_focused = false;
        }

        match target {
            Some(Target::Search) => {
                self.search_focused = true;
                EventResult::Consumed
            }
            Some(Target::AddButton) => {
                self.open_add_file_type_dialog();
                EventResult::Consumed
            }
            Some(Target::ImportButton) => {
                self.open_transfer_dialog(Transfer::Import);
                EventResult::Consumed
            }
            Some(Target::ExportButton) => {
                self.open_transfer_dialog(Transfer::Export);
                EventResult::Consumed
            }
            Some(Target::ResetButton) => {
                self.registry.reset_to_defaults();
                self.selected_index = None;
                self.clamp_scroll();
                self.status = String::from("Associations reset to defaults");
                self.persist();
                EventResult::Consumed
            }
            Some(Target::Category(cat)) => {
                self.select_category(cat);
                EventResult::Consumed
            }
            Some(Target::Row(i)) => {
                self.select_file_type(i);
                EventResult::Consumed
            }
            Some(Target::OpenWithButton) => {
                self.open_open_with_dialog();
                EventResult::Consumed
            }
            Some(Target::ClearButton) => {
                let ext = self.selected_file_type().map(|ft| ft.extension.clone());
                if let Some(ext) = ext {
                    self.status = match self.registry.clear_association(&ext) {
                        Ok(()) => format!(".{ext} has no default app"),
                        Err(e) => format!("Could not clear .{ext}: {e}"),
                    };
                    self.persist();
                }
                EventResult::Consumed
            }
            Some(Target::CompatibleApp(i)) => {
                // Clicking an app in "Compatible Apps" makes it the default.
                // The list is otherwise a read-out, and the shortest path from
                // "I can see the app I want" to "that app opens it" should not
                // be a trip through a modal dialog.
                let pair = self
                    .selected_file_type()
                    .map(|ft| ft.extension.clone())
                    .and_then(|ext| {
                        self.registry
                            .apps_for_extension(&ext)
                            .get(i)
                            .map(|a| (ext.clone(), a.id.clone()))
                    });
                if let Some((ext, app_id)) = pair {
                    self.status = match self.registry.set_default_app(&ext, &app_id) {
                        Ok(()) => format!(".{ext} now opens with {app_id}"),
                        Err(e) => format!("Could not set the default for .{ext}: {e}"),
                    };
                    self.persist();
                }
                EventResult::Consumed
            }
            Some(Target::DialogApp(i)) => {
                self.dialog_selected_app = Some(i);
                EventResult::Consumed
            }
            Some(Target::DialogAlwaysUse) => {
                self.dialog_always_use = !self.dialog_always_use;
                EventResult::Consumed
            }
            Some(Target::DialogOk) => {
                self.confirm_dialog();
                EventResult::Consumed
            }
            // The scrim is recorded as a target rather than left to a
            // fall-through, so a control added later cannot be clicked through
            // an open dialog.
            Some(Target::DialogCancel | Target::Scrim) => {
                self.cancel_dialog();
                EventResult::Consumed
            }
            Some(Target::DialogField(field)) => {
                self.new_field = field;
                EventResult::Consumed
            }
            Some(Target::DialogCategory) => {
                self.cycle_new_category();
                EventResult::Consumed
            }
            Some(Target::Table) | None => EventResult::Ignored,
        }
    }

    /// Whichever dialog is open, do the thing its OK button says.
    fn confirm_dialog(&mut self) {
        match self.active_dialog {
            ActiveDialog::OpenWith => {
                // The status line already carries the outcome, including the
                // failure, so the `Result` has nothing left to tell anyone.
                let _confirmed = self.confirm_open_with();
            }
            ActiveDialog::AddFileType => {
                // Same: a rejected extension leaves the dialog open with the
                // reason in the status line, which is the point of returning
                // the error at all.
                let _added = self.confirm_add_file_type();
            }
            // The picker decides for itself what Enter means -- a directory
            // row opens it, a file row chooses it -- and it sees the key
            // first, in `file_dialog_event`. Nothing here can improve on
            // that, and guessing would mean confirming a selection the
            // dialog had not made.
            ActiveDialog::ChooseFile => {}
            ActiveDialog::None => {}
        }
    }

    fn cycle_new_category(&mut self) {
        let pos = FileCategory::ALL
            .iter()
            .position(|c| *c == self.new_category)
            .unwrap_or(0);
        // `checked_rem` rather than `%`: the list is a `const` with seven
        // entries today, but a wrap that divides by its length should not be
        // the thing that panics if someone ever empties it.
        let next = pos
            .checked_add(1)
            .and_then(|n| n.checked_rem(FileCategory::ALL.len()))
            .unwrap_or(0);
        if let Some(cat) = FileCategory::ALL.get(next) {
            self.new_category = *cat;
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Typed text goes wherever the caret is, and is checked before the
        // named keys so a key that produces text is not also read as a command.
        if !key.text.is_empty() && !key.modifiers.ctrl && !key.modifiers.alt {
            return self.type_text(&key.text);
        }

        match key.key {
            Key::Escape => self.handle_escape(),
            Key::Enter => {
                if self.active_dialog == ActiveDialog::None {
                    if self.selected_index.is_some() {
                        self.open_open_with_dialog();
                        return EventResult::Consumed;
                    }
                    return EventResult::Ignored;
                }
                self.confirm_dialog();
                EventResult::Consumed
            }
            Key::Tab if self.active_dialog == ActiveDialog::AddFileType => {
                self.new_field = self.new_field.next();
                EventResult::Consumed
            }
            Key::Backspace => self.backspace(),
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            Key::E if key.modifiers.ctrl => {
                self.open_transfer_dialog(Transfer::Export);
                EventResult::Consumed
            }
            Key::I if key.modifiers.ctrl => {
                self.open_transfer_dialog(Transfer::Import);
                EventResult::Consumed
            }
            Key::F if key.modifiers.ctrl => {
                self.search_focused = true;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Escape backs out of exactly one thing, innermost first.
    fn handle_escape(&mut self) -> EventResult {
        if self.active_dialog != ActiveDialog::None {
            self.cancel_dialog();
            return EventResult::Consumed;
        }
        if self.search_focused || !self.search_query.is_empty() {
            self.search_focused = false;
            if !self.search_query.is_empty() {
                self.set_search_query("");
            }
            return EventResult::Consumed;
        }
        if self.selected_index.is_some() {
            self.selected_index = None;
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn type_text(&mut self, text: &str) -> EventResult {
        // A control character is not text a field wants: a Backspace that also
        // carried "\u{8}" would delete a character and then insert one.
        if text.chars().any(char::is_control) {
            return EventResult::Ignored;
        }
        match self.active_dialog {
            ActiveDialog::AddFileType => {
                self.new_field_mut().push_str(text);
                EventResult::Consumed
            }
            // The "Open With" dialog has nothing to type into.
            ActiveDialog::OpenWith => EventResult::Ignored,
            // Unreachable: the picker consumes every key before `handle_key`
            // is called, including the ones that fill in its filename field.
            ActiveDialog::ChooseFile => EventResult::Ignored,
            ActiveDialog::None if self.search_focused => {
                let mut q = self.search_query.clone();
                q.push_str(text);
                self.set_search_query(&q);
                EventResult::Consumed
            }
            ActiveDialog::None => EventResult::Ignored,
        }
    }

    fn backspace(&mut self) -> EventResult {
        match self.active_dialog {
            // `String::pop` removes a whole `char`, so a multi-byte character
            // goes in one press instead of leaving a broken tail behind.
            ActiveDialog::AddFileType => {
                self.new_field_mut().pop();
                EventResult::Consumed
            }
            ActiveDialog::OpenWith | ActiveDialog::ChooseFile => EventResult::Ignored,
            ActiveDialog::None if self.search_focused => {
                let mut q = self.search_query.clone();
                if q.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.set_search_query(&q);
                EventResult::Consumed
            }
            ActiveDialog::None => EventResult::Ignored,
        }
    }

    fn new_field_mut(&mut self) -> &mut String {
        match self.new_field {
            NewField::Extension => &mut self.new_ext,
            NewField::MimeType => &mut self.new_mime,
            NewField::Description => &mut self.new_desc,
        }
    }

    /// The text currently in a given "Add File Type" field.
    pub fn new_field_text(&self, field: NewField) -> &str {
        match field {
            NewField::Extension => &self.new_ext,
            NewField::MimeType => &self.new_mime,
            NewField::Description => &self.new_desc,
        }
    }

    /// Step a selection by `delta` within `0..=last`.
    ///
    /// Shared by the table and the dialog list, which want exactly the same
    /// behaviour: Down from nothing enters at the top, Up from nothing enters
    /// at the bottom, and both ends stop rather than wrap.
    ///
    /// Every conversion is checked and the addition saturates. A list longer
    /// than `isize::MAX` cannot exist, but an `as` cast that wrapped would put
    /// the selection on a negative row, and a `delta` large enough to overflow
    /// would wrap *past* the clamp that is there to catch it.
    fn stepped(current: Option<usize>, delta: isize, last: usize) -> usize {
        let last_i = isize::try_from(last).unwrap_or(isize::MAX);
        match current {
            None if delta > 0 => 0,
            None => last,
            Some(i) => {
                let moved = isize::try_from(i).unwrap_or(last_i).saturating_add(delta);
                usize::try_from(moved.clamp(0, last_i)).unwrap_or(0)
            }
        }
    }

    /// Move the table selection by `delta` rows, clamped at both ends.
    fn move_selection(&mut self, delta: isize) -> EventResult {
        if self.active_dialog != ActiveDialog::None {
            return self.move_dialog_selection(delta);
        }
        let count = self.filtered_file_types().len();
        if count == 0 {
            return EventResult::Ignored;
        }
        let next = Self::stepped(self.selected_index, delta, count.saturating_sub(1));
        if self.selected_index == Some(next) {
            return EventResult::Ignored;
        }
        self.selected_index = Some(next);
        self.scroll_selection_into_view();
        EventResult::Consumed
    }

    fn move_dialog_selection(&mut self, delta: isize) -> EventResult {
        if self.active_dialog != ActiveDialog::OpenWith {
            return EventResult::Ignored;
        }
        let count = self
            .registry
            .apps_for_extension(&self.dialog_target_ext)
            .len();
        if count == 0 {
            return EventResult::Ignored;
        }
        let next = Self::stepped(self.dialog_selected_app, delta, count.saturating_sub(1));
        if self.dialog_selected_app == Some(next) {
            return EventResult::Ignored;
        }
        self.dialog_selected_app = Some(next);
        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        self.frame(self.window_width, self.window_height)
            .into_tree()
    }

    /// Draw the whole window, recording a hit box for every control as it goes.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        // From here on the layout's own `width`/`height` are used rather than
        // the arguments: `Layout::new` is where a zero or a NaN gets turned
        // into something drawable, and a background painted from the raw
        // arguments would disagree with every rectangle laid on top of it.
        let mut frame = Frame::new(l.width, l.height);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.width,
            height: l.height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.draw_toolbar(&mut frame, &l);
        self.draw_sidebar(&mut frame, &l);
        self.draw_table(&mut frame, &l);
        self.draw_details_panel(&mut frame, &l);

        if self.active_dialog != ActiveDialog::None {
            // A dialog is modal: what it covers keeps its pixels and loses its
            // clicks. Without this the toolbar behind it still worked, so the
            // dialog only *looked* in front.
            frame.discard_hits();
            frame.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: l.width,
                height: l.height,
                color: Color::rgba(0, 0, 0, 128),
                corner_radii: CornerRadii::ZERO,
            });
            frame.hit(Target::Scrim, Rect::new(0.0, 0.0, l.width, l.height));
            match self.active_dialog {
                ActiveDialog::OpenWith => self.draw_open_with_dialog(&mut frame, &l),
                ActiveDialog::AddFileType => self.draw_add_file_type_dialog(&mut frame, &l),
                ActiveDialog::ChooseFile => {
                    if let Some(dialog) = &self.file_dialog {
                        for cmd in dialog.render(&self.palette, l.width, l.height) {
                            frame.push(cmd);
                        }
                    }
                }
                ActiveDialog::None => {}
            }
        }

        frame
    }

    /// Draw a rounded button with its label centred by measurement.
    fn draw_button(
        frame: &mut Frame,
        target: Target,
        r: Rect,
        label: &str,
        bg: Color,
        fg: Color,
        size: f32,
    ) {
        if r.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        let w = text::measure(label, size, FontWeightHint::Regular).min(r.w);
        frame.push(RenderCommand::Text {
            x: r.x + (r.w - w).max(0.0) / 2.0,
            y: r.y + (r.h - size).max(0.0) / 2.0,
            text: String::from(label),
            color: fg,
            font_size: size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(r.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(target, r);
    }

    /// Render the top toolbar with search bar and action buttons.
    fn draw_toolbar(&self, frame: &mut Frame, l: &Layout) {
        self.palette.push_surface(
            frame,
            0.0,
            0.0,
            l.width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        frame.push(RenderCommand::Text {
            x: PADDING,
            y: (TOOLBAR_HEIGHT - FONT_SIZE_HEADING) / 2.0,
            text: String::from("File Associations"),
            color: self.palette.text,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some((l.search.x - PADDING).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Search box. The caret is a border rather than a blinking bar: this
        // window has no clock, and a caret that cannot blink is better drawn as
        // something that does not look like it should.
        if !l.search.is_empty() {
            let r = l.search;
            // Focus recolours the field's outline; it does not add a second
            // one inside the first, which is what a separate stroke was under
            // the bordered theme.
            let mut paint = self.palette.surface_paint(Surface::Card);
            if self.search_focused {
                paint.border = Some(self.palette.blue);
            }
            self.palette
                .push_paint_radii(frame, r.x, r.y, r.w, r.h, CornerRadii::all(4.0), paint);
            let empty = self.search_query.is_empty();
            let shown = if empty {
                String::from("Search by extension or description...")
            } else {
                self.search_query.clone()
            };
            frame.push(RenderCommand::Text {
                x: r.x + 8.0,
                y: r.y + (r.h - FONT_SIZE).max(0.0) / 2.0,
                text: shown,
                color: if empty {
                    self.palette.subtext0
                } else {
                    self.palette.text
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((r.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::Search, r);
        }

        if !self.status.is_empty() && !l.status.is_empty() {
            frame.push(RenderCommand::Text {
                x: l.status.x,
                y: l.status.y + (l.status.h - FONT_SIZE_SMALL).max(0.0) / 2.0,
                text: self.status.clone(),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(l.status.w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        Self::draw_button(
            frame,
            Target::AddButton,
            l.add,
            "Add Type",
            self.palette.surface1,
            self.palette.text,
            FONT_SIZE_SMALL,
        );
        Self::draw_button(
            frame,
            Target::ImportButton,
            l.import,
            "Import",
            self.palette.surface1,
            self.palette.text,
            FONT_SIZE_SMALL,
        );
        Self::draw_button(
            frame,
            Target::ExportButton,
            l.export,
            "Export",
            self.palette.surface1,
            self.palette.text,
            FONT_SIZE_SMALL,
        );
        Self::draw_button(
            frame,
            Target::ResetButton,
            l.reset,
            "Reset Defaults",
            self.palette.surface1,
            self.palette.text,
            FONT_SIZE_SMALL,
        );
    }

    /// Render the category sidebar.
    fn draw_sidebar(&self, frame: &mut Frame, l: &Layout) {
        self.palette.push_surface(
            frame,
            l.sidebar.x,
            l.sidebar.y,
            l.sidebar.w,
            l.sidebar.h,
            0.0,
            Surface::Sidebar,
        );

        // Everything in the sidebar is cut to the sidebar, so a category row
        // that falls off the bottom of a short window loses its hit box with
        // its pixels rather than staying clickable in the dark.
        frame.clip(l.sidebar);

        let all_selected = self.selected_category.is_none();
        self.draw_sidebar_item(frame, l.all_types, None, "All Types", all_selected, None);

        frame.push(RenderCommand::Text {
            x: PADDING,
            y: l.sidebar.y + SIDEBAR_ITEM_HEIGHT + 6.0,
            text: String::from("CATEGORIES"),
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some((l.sidebar.w - 2.0 * PADDING).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        for (i, cat) in FileCategory::ALL.iter().enumerate() {
            let r = l.category_row(i);
            let selected = self.selected_category == Some(*cat);
            let count = self.registry.file_types_by_category(*cat).len();
            self.draw_sidebar_item(frame, r, Some(*cat), cat.label(), selected, Some(count));
        }

        frame.unclip();
    }

    fn draw_sidebar_item(
        &self,
        frame: &mut Frame,
        r: Rect,
        cat: Option<FileCategory>,
        label: &str,
        selected: bool,
        count: Option<usize>,
    ) {
        if r.is_empty() {
            return;
        }
        self.palette.push_surface(
            frame,
            r.x,
            r.y,
            r.w,
            r.h,
            0.0,
            if selected {
                Surface::Selected
            } else {
                Surface::Card
            },
        );

        if let Some(cat) = cat {
            frame.push(RenderCommand::FillRect {
                x: PADDING,
                y: r.y + 7.0,
                width: 20.0,
                height: 20.0,
                color: cat.color(&self.palette),
                corner_radii: CornerRadii::all(10.0),
            });
            frame.push(RenderCommand::Text {
                x: PADDING + 5.0,
                y: r.y + 10.0,
                text: String::from(cat.icon()),
                color: self.palette.base,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        frame.push(RenderCommand::Text {
            x: PADDING + 28.0,
            y: r.y + 10.0,
            text: String::from(label),
            color: if selected {
                self.palette.ink(self.palette.blue)
            } else {
                self.palette.text
            },
            font_size: FONT_SIZE,
            font_weight: if selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some((r.w - PADDING - 28.0 - 26.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        if let Some(count) = count {
            frame.push(RenderCommand::Text {
                x: (r.right() - 30.0).max(0.0),
                y: r.y + 10.0,
                text: format!("{count}"),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        frame.hit(Target::Category(cat), r);
    }

    /// Render the main file type table.
    fn draw_table(&self, frame: &mut Frame, l: &Layout) {
        frame.push(RenderCommand::FillRect {
            x: l.table.x,
            y: l.table.y,
            width: l.table.w,
            height: l.table.h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        // Recorded before the rows, so a row painted on top of it wins the hit
        // test and the bare table only answers where no row is drawn. That is
        // what lets the wheel work over the header and the empty space below
        // the last row while a click there still selects nothing.
        frame.hit(Target::Table, l.table);

        self.palette.push_surface(
            frame,
            l.table_header.x,
            l.table_header.y,
            l.table_header.w,
            l.table_header.h,
            0.0,
            Surface::Card,
        );

        let header_y = l.table_header.y + (l.table_header.h - FONT_SIZE_SMALL).max(0.0) / 2.0;
        for (x, title) in l
            .columns
            .iter()
            .zip(["Ext", "Description", "MIME Type", "Default App"])
        {
            frame.push(RenderCommand::Text {
                x: *x,
                y: header_y,
                text: String::from(title),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some((l.table.right() - x).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        frame.push(RenderCommand::Line {
            x1: l.table.x,
            y1: l.rows.y,
            x2: l.table.right(),
            y2: l.rows.y,
            color: self.palette.surface1,
            width: 1.0,
        });

        // The frame trims every hit box recorded inside this clip and drops the
        // ones it cuts to nothing, so a row scrolled out of the viewport stops
        // being clickable by construction rather than by a bound the hit test
        // has to remember to apply.
        frame.clip(l.rows);

        let filtered = self.filtered_file_types();
        for (y, i) in self.table_rows(l) {
            let Some(ft) = filtered.get(i) else {
                continue;
            };
            let selected = self.selected_index == Some(i);
            let row_bg = if selected {
                self.palette.surface1
            } else if i % 2 == 0 {
                self.palette.base
            } else {
                self.palette.surface0
            };

            frame.push(RenderCommand::FillRect {
                x: l.table.x,
                y,
                width: l.table.w,
                height: ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let text_y = y + (ROW_HEIGHT - FONT_SIZE).max(0.0) / 2.0;
            let cat = self.registry.get_category(&ft.extension);
            frame.push(RenderCommand::FillRect {
                x: l.columns[0],
                y: y + (ROW_HEIGHT - 8.0) / 2.0,
                width: 8.0,
                height: 8.0,
                color: cat.color(&self.palette),
                corner_radii: CornerRadii::all(4.0),
            });
            frame.push(RenderCommand::Text {
                x: l.columns[0] + 14.0,
                y: text_y,
                text: format!(".{}", ft.extension),
                color: if selected {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.text
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some((l.columns[1] - l.columns[0] - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Text {
                x: l.columns[1],
                y: text_y,
                text: ft.description.clone(),
                color: self.palette.text,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((l.columns[2] - l.columns[1] - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Text {
                x: l.columns[2],
                y: text_y,
                text: ft.mime_type.clone(),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some((l.columns[3] - l.columns[2] - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let app_name = ft
                .default_app_id
                .as_ref()
                .and_then(|id| self.registry.get_app(id))
                .map_or_else(|| String::from("(none)"), |a| a.name.clone());
            frame.push(RenderCommand::Text {
                x: l.columns[3],
                y: text_y,
                text: app_name,
                color: if ft.default_app_id.is_some() {
                    self.palette.ink(self.palette.green)
                } else {
                    self.palette.subtext0
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((l.table.right() - PADDING - l.columns[3]).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });

            frame.hit(
                Target::Row(i),
                Rect::new(l.table.x, y, l.table.w, ROW_HEIGHT),
            );
        }

        frame.unclip();

        if filtered.is_empty() {
            let msg = "No matching file types";
            let w = text::measure(msg, FONT_SIZE, FontWeightHint::Regular).min(l.rows.w);
            frame.push(RenderCommand::Text {
                x: l.rows.x + (l.rows.w - w).max(0.0) / 2.0,
                y: l.rows.y + 40.0,
                text: String::from(msg),
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(l.rows.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render the right-side details panel for the selected file type.
    fn draw_details_panel(&self, frame: &mut Frame, l: &Layout) {
        let panel = l.details;
        self.palette.push_surface(
            frame,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            0.0,
            Surface::Card,
        );
        frame.push(RenderCommand::Line {
            x1: panel.x,
            y1: panel.y,
            x2: panel.x,
            y2: panel.bottom(),
            color: self.palette.surface1,
            width: 1.0,
        });

        // Same reason as the sidebar: a button laid out below a short window's
        // bottom edge must lose its hit box, not merely its pixels.
        frame.clip(panel);

        let x = panel.x + PADDING;
        let content_w = (panel.w - 2.0 * PADDING).max(0.0);
        let mut y = panel.y + PADDING;

        frame.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Details"),
            color: self.palette.text,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        y += 28.0;

        let filtered = self.filtered_file_types();
        let Some(ft) = self.selected_index.and_then(|i| filtered.get(i).copied()) else {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: String::from("Select a file type to see details"),
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            frame.unclip();
            return;
        };

        let cat = self.registry.get_category(&ft.extension);
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: 60.0_f32.min(content_w),
            height: 28.0,
            color: cat.color(&self.palette),
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 7.0,
            text: format!(".{}", ft.extension),
            color: self.palette.base,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some((content_w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        y += 40.0;

        let app_name = ft
            .default_app_id
            .as_ref()
            .and_then(|id| self.registry.get_app(id))
            .map_or_else(|| String::from("(none)"), |a| a.name.clone());
        for (label, value) in [
            ("Description", ft.description.as_str()),
            ("MIME Type", ft.mime_type.as_str()),
            ("Category", cat.label()),
            ("Default App", app_name.as_str()),
        ] {
            Self::draw_detail_row(frame, &self.palette, x, y, content_w, label, value);
            y += 24.0;
        }
        y += 12.0;

        Self::draw_button(
            frame,
            Target::OpenWithButton,
            Rect::new(x, y, content_w, BUTTON_HEIGHT),
            "Open With...",
            self.palette.blue,
            self.palette.base,
            FONT_SIZE,
        );
        y += BUTTON_HEIGHT + 8.0;

        Self::draw_button(
            frame,
            Target::ClearButton,
            Rect::new(x, y, content_w, BUTTON_HEIGHT),
            "Clear Association",
            self.palette.red,
            self.palette.base,
            FONT_SIZE,
        );
        y += BUTTON_HEIGHT + 16.0;

        frame.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Compatible Apps"),
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        y += 20.0;

        let compatible = self.registry.apps_for_extension(&ft.extension);
        if compatible.is_empty() {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: String::from("No compatible apps"),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            for (i, app) in compatible.iter().enumerate() {
                let is_default = ft.default_app_id.as_deref() == Some(app.id.as_str());
                let r = Rect::new(x, y, content_w, COMPAT_ROW_HEIGHT);
                self.palette.push_surface(
                    frame,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    3.0,
                    if is_default {
                        Surface::Selected
                    } else {
                        Surface::Card
                    },
                );
                frame.push(RenderCommand::Text {
                    x: r.x + 8.0,
                    y: r.y + (r.h - FONT_SIZE_SMALL).max(0.0) / 2.0,
                    text: app.name.clone(),
                    color: if is_default {
                        self.palette.ink(self.palette.green)
                    } else {
                        self.palette.text
                    },
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((r.w - 16.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
                frame.hit(Target::CompatibleApp(i), r);
                y += COMPAT_ROW_HEIGHT + 2.0;
            }
        }

        frame.unclip();
    }

    /// Helper: render a label+value detail row.
    fn draw_detail_row(
        frame: &mut Frame,
        pal: &Palette,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        value: &str,
    ) {
        let value_x = x + DETAIL_LABEL_WIDTH;
        frame.push(RenderCommand::Text {
            x,
            y,
            text: String::from(label),
            color: pal.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(DETAIL_LABEL_WIDTH.min(w)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.push(RenderCommand::Text {
            x: value_x,
            y,
            text: String::from(value),
            color: pal.text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((w - DETAIL_LABEL_WIDTH).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The panel, shadow and title bar every dialog shares.
    fn draw_dialog_chrome(frame: &mut Frame, pal: &Palette, l: &Layout, title: &str) {
        let d = l.dialog;
        frame.push(RenderCommand::BoxShadow {
            x: d.x,
            y: d.y,
            width: d.w,
            height: d.h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        pal.push_surface(frame, d.x, d.y, d.w, d.h, CORNER_RADIUS, Surface::Card);
        pal.push_surface_radii(
            frame,
            d.x,
            d.y,
            d.w,
            DIALOG_TITLE_HEIGHT.min(d.h),
            CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
            Surface::Panel,
        );
        frame.push(RenderCommand::Text {
            x: d.x + PADDING,
            y: d.y + (DIALOG_TITLE_HEIGHT - FONT_SIZE).max(0.0) / 2.0,
            text: String::from(title),
            color: pal.text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some((d.w - 2.0 * PADDING).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The `Cancel` / `OK` pair every dialog shares.
    fn draw_dialog_buttons(frame: &mut Frame, pal: &Palette, l: &Layout, ok_label: &str) {
        let (cancel, ok) = l.dialog_buttons();
        Self::draw_button(
            frame,
            Target::DialogCancel,
            cancel,
            "Cancel",
            pal.surface1,
            pal.text,
            FONT_SIZE_SMALL,
        );
        Self::draw_button(
            frame,
            Target::DialogOk,
            ok,
            ok_label,
            pal.blue,
            pal.base,
            FONT_SIZE_SMALL,
        );
    }

    /// Render the "Open With" modal dialog.
    fn draw_open_with_dialog(&self, frame: &mut Frame, l: &Layout) {
        Self::draw_dialog_chrome(
            frame,
            &self.palette,
            l,
            &format!("Open With \u{2014} .{}", self.dialog_target_ext),
        );

        let list = l.dialog_list();
        frame.clip(list);
        let compatible = self.registry.apps_for_extension(&self.dialog_target_ext);
        for (i, app) in compatible.iter().enumerate() {
            let r = Rect::new(
                l.dialog.x + 4.0,
                list.y + i as f32 * DIALOG_APP_ROW_HEIGHT,
                (l.dialog.w - 8.0).max(0.0),
                DIALOG_APP_ROW_HEIGHT,
            );
            let selected = self.dialog_selected_app == Some(i);
            frame.push(RenderCommand::FillRect {
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                color: if selected {
                    self.palette.blue
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(4.0),
            });
            frame.push(RenderCommand::Text {
                x: r.x + 12.0,
                y: r.y + 6.0,
                text: app.name.clone(),
                color: if selected {
                    self.palette.base
                } else {
                    self.palette.text
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some((r.w - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Text {
                x: r.x + 12.0,
                y: r.y + 20.0,
                text: app.exec_path.clone(),
                color: if selected {
                    self.palette.mantle
                } else {
                    self.palette.subtext0
                },
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some((r.w - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::DialogApp(i), r);
        }
        if compatible.is_empty() {
            frame.push(RenderCommand::Text {
                x: l.dialog.x + PADDING,
                y: list.y + PADDING,
                text: format!("No app handles .{}", self.dialog_target_ext),
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((l.dialog.w - 2.0 * PADDING).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        frame.unclip();

        let (box_rect, strip) = l.dialog_checkbox();
        frame.push(RenderCommand::StrokeRect {
            x: box_rect.x,
            y: box_rect.y,
            width: box_rect.w,
            height: box_rect.h,
            color: self.palette.overlay0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });
        if self.dialog_always_use {
            frame.push(RenderCommand::FillRect {
                x: box_rect.x + 3.0,
                y: box_rect.y + 3.0,
                width: (box_rect.w - 6.0).max(0.0),
                height: (box_rect.h - 6.0).max(0.0),
                color: self.palette.blue,
                corner_radii: CornerRadii::all(2.0),
            });
        }
        frame.push(RenderCommand::Text {
            x: box_rect.right() + 8.0,
            y: box_rect.y + 1.0,
            text: String::from("Always use this app"),
            color: self.palette.text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((strip.right() - box_rect.right() - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::DialogAlwaysUse, strip);

        Self::draw_dialog_buttons(frame, &self.palette, l, "OK");
    }

    /// Render the "Add File Type" modal dialog.
    fn draw_add_file_type_dialog(&self, frame: &mut Frame, l: &Layout) {
        Self::draw_dialog_chrome(frame, &self.palette, l, "Add File Type");

        for (i, field) in NewField::ALL.iter().enumerate() {
            let r = l.dialog_field(i);
            if r.is_empty() {
                continue;
            }
            frame.push(RenderCommand::Text {
                x: r.x,
                y: (r.y - FIELD_LABEL_HEIGHT).max(l.dialog.y),
                text: String::from(field.label()),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(r.w),
                overflow: TextOverflow::Ellipsis,
            });
            let focused = self.new_field == *field;
            let mut paint = self.palette.surface_paint(Surface::Card);
            if focused {
                paint.border = Some(self.palette.blue);
            }
            self.palette
                .push_paint_radii(frame, r.x, r.y, r.w, r.h, CornerRadii::all(4.0), paint);
            let value = self.new_field_text(*field);
            let empty = value.is_empty();
            frame.push(RenderCommand::Text {
                x: r.x + 8.0,
                y: r.y + (r.h - FONT_SIZE).max(0.0) / 2.0,
                text: if empty {
                    match field {
                        NewField::Extension => String::from("txt"),
                        NewField::MimeType => String::from("application/octet-stream"),
                        NewField::Description => String::from("(optional)"),
                    }
                } else {
                    value.to_string()
                },
                color: if empty {
                    self.palette.subtext0
                } else {
                    self.palette.text
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((r.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::DialogField(*field), r);
        }

        let cat = l.dialog_category();
        if !cat.is_empty() {
            frame.push(RenderCommand::Text {
                x: cat.x,
                y: (cat.y - FIELD_LABEL_HEIGHT).max(l.dialog.y),
                text: String::from("Category"),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(cat.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
        Self::draw_button(
            frame,
            Target::DialogCategory,
            cat,
            self.new_category.label(),
            self.new_category.color(&self.palette),
            self.palette.base,
            FONT_SIZE,
        );

        Self::draw_dialog_buttons(frame, &self.palette, l, "Add");
    }
}

// ============================================================================
// The window
// ============================================================================

impl App for FileAssocUI {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        String::from("File Associations")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Ctrl+Q closes the window. Escape does not: it backs out of a dialog,
        // a search or a selection, which is what the key is for here.
        if let Event::Key(key) = event
            && key.pressed
            && key.key == Key::Q
            && key.modifiers.ctrl
        {
            return Response::Exit;
        }
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The remembered size is only ever a starting guess; this is the real
        // one, and the hit test reads it back through `handle_event`.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for FileAssocUI {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    app::launch("fileassoc", &mut FileAssocUI::load())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that cannot index a slice or unwrap an `Option` it has just built
    // is a test that spends more lines apologising than asserting. Panicking on
    // bad data is the point here -- it is how the test fails.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    // Not in the production imports: nothing outside the tests names a
    // modifier set, because the app reads `key.modifiers.ctrl` off the event it
    // was handed and never constructs one.
    use guitk::event::Modifiers;
    // The free helpers -- `click`, `rect_of`, `press`. The production code
    // imports only the `Probe` trait it implements.
    use guitk::probe;

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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        for cat in FileCategory::ALL {
            assert_eq!(cat.color(&pal).a, 255);
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
    fn test_error_display_invalid_extension() {
        let e = AssocError::InvalidExtension(String::from("the extension box is empty"));
        assert!(format!("{e}").contains("the extension box is empty"));
    }

    #[test]
    fn test_error_display_already_exists() {
        let e = AssocError::AlreadyExists(String::from("mp3"));
        assert!(format!("{e}").contains("mp3"));
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
        // Eight, and the floor moved down from ten on 2026-09-14 when four
        // entries naming programs that do not exist were removed. Lowering a
        // threshold to match is usually how a test stops testing -- here the
        // number it asserted was made of `browser`, `office`, `codeeditor` and
        // `imageeditor`, none of which this tree builds, so the old ten was the
        // fiction and the eight is the measurement.
        assert!(reg.app_count() >= 8, "{} apps", reg.app_count());
        for app in reg.apps.values() {
            assert!(
                app.exec_path.starts_with("/usr/bin/"),
                "{} has no binary path",
                app.id
            );
        }
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
        // `zip`, because it is a file type this system really does have two
        // programs for. The extension used to be `html` on the strength of
        // "textedit, codeeditor, browser" -- the test's own comment named three
        // programs, none of which exist, so the assertion rested entirely on
        // fiction. `html` now has exactly one handler, which is the truth about
        // this system rather than a regression.
        let apps = reg.apps_for_extension("zip");
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert!(
            ids.contains(&"archivemanager") && ids.contains(&"explorer"),
            "zip should be openable by both, got {ids:?}"
        );
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

    /// **What opens a `.mp3`, answered as something a caller can run.**
    ///
    /// The question every other program has. `apps/explorer` reads the same
    /// answer out of the saved file without linking to this program, which is
    /// the whole reason the file records a path.
    #[test]
    fn the_registry_answers_with_a_runnable_path() {
        let mut reg = registry_over(&["mp3"]);
        assert_eq!(reg.opener_path("mp3"), None, "nothing is associated yet");

        reg.set_default_app("mp3", "app0")
            .expect("a registered pair");

        assert_eq!(reg.opener_path("mp3"), Some("/bin/app0"));
        assert_eq!(reg.opener_path("MP3"), Some("/bin/app0"), "case");
        assert_eq!(reg.opener_path("wav"), None, "nothing claims wav");
    }

    /// A file written before the format recorded paths still loads.
    ///
    /// The reader is where leniency belongs: refusing an older file would lose
    /// every association in it, and the next save rewrites it as a path.
    #[test]
    fn a_file_written_with_ids_still_loads() {
        let mut reg = registry_over(&["mp3"]);
        let errors = reg.import_config(
            "associations:
  mp3: app0
",
        );

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(reg.opener_path("mp3"), Some("/bin/app0"));

        // And it is written back as a path, so the leniency is needed once.
        assert!(reg.export_config().contains("mp3: /bin/app0"));
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
        assert!(
            // The path, not the id: the file is written for programs that do
            // not have this registry to look an id up in.
            config.contains("txt: /bin/myapp"),
            "the association is not in the exported document: {config}"
        );
        assert!(
            config.contains("associations:"),
            "the exported document has no associations mapping: {config}"
        );
        // The header a fresh file is written with survives the fold.
        assert!(
            config.starts_with("# Slate OS file associations."),
            "header lost: {config:?}"
        );
    }

    #[test]
    fn test_import_config_valid() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "associations:\n  txt: editor\n  png: imageviewer\n";
        let errors = reg.import_config(config);
        assert!(errors.is_empty());
        let app = reg.get_default_app("txt");
        assert!(app.is_some());
        assert_eq!(app.map(|a| a.id.as_str()), Some("editor"));
    }

    #[test]
    fn test_import_config_with_comments() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "# comment\n\nassociations:\n  # which editor\n  txt: editor\n";
        let errors = reg.import_config(config);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_import_config_nonexistent_app() {
        let mut reg = AssociationRegistry::with_defaults();
        let config = "associations:\n  txt: doesnotexist\n";
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

    /// Extensions that are legal here -- a filename may contain any byte but
    /// `/` and NUL -- and that the unescaped config format mangled. None of
    /// these is a crafted payload; each is just a name someone could type.
    const HOSTILE_EXTENSIONS: &[&str] = &[
        "txt ",
        " txt",
        "a=b",
        "#txt",
        "back\\slash",
        r"\n",
        "two words",
    ];

    /// Build a registry in which every name in `exts` is a registered file
    /// type with its own dedicated application.
    fn registry_over(exts: &[&str]) -> AssociationRegistry {
        let mut reg = AssociationRegistry::new();
        for (i, ext) in exts.iter().enumerate() {
            reg.register_file_type(
                FileType::new(ext, "application/octet-stream", "Thing"),
                FileCategory::Documents,
            );
            // A distinct binary per app. They all shared `/bin/app` until
            // 2026-09-14, which no real system does and which made two
            // applications indistinguishable the moment the association file
            // started recording paths rather than ids.
            reg.register_app(AppInfo::new(
                &format!("app{i}"),
                "App",
                &format!("/bin/app{i}"),
                &[ext],
                1,
            ));
        }
        reg
    }

    #[test]
    fn an_extension_with_an_edge_space_keeps_its_own_association() {
        // The specific silent-wrong-result the escaping was added for. Both
        // names are registerable: `register_file_type` does not validate the
        // extension string, and `set_default_app` only lowercases it, so
        // `"txt"` and `"txt "` are two distinct entries in the registry.
        // Unescaped, the second exported as `txt =app1`, and the reader's
        // `trim` turned it back into `txt` -- reassigning the *first*
        // extension's default application, with no error reported anywhere.
        let mut reg = registry_over(&["txt", "txt "]);
        reg.set_default_app("txt", "app0").expect("plain txt");
        reg.set_default_app("txt ", "app1").expect("padded txt");

        let mut reg2 = registry_over(&["txt", "txt "]);
        let errors = reg2.import_config(&reg.export_config());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            reg2.get_default_app("txt").map(|a| a.id.as_str()),
            Some("app0"),
            "the padded extension overwrote the plain one"
        );
        assert_eq!(
            reg2.get_default_app("txt ").map(|a| a.id.as_str()),
            Some("app1")
        );
    }

    #[test]
    fn every_hostile_extension_survives_the_config_round_trip() {
        let mut reg = registry_over(HOSTILE_EXTENSIONS);
        for (i, ext) in HOSTILE_EXTENSIONS.iter().enumerate() {
            reg.set_default_app(ext, &format!("app{i}"))
                .unwrap_or_else(|e| panic!("could not associate {ext:?}: {e:?}"));
        }

        let mut reg2 = registry_over(HOSTILE_EXTENSIONS);
        let errors = reg2.import_config(&reg.export_config());
        assert!(errors.is_empty(), "{errors:?}");
        for (i, ext) in HOSTILE_EXTENSIONS.iter().enumerate() {
            assert_eq!(
                reg2.get_default_app(ext).map(|a| a.id.as_str()),
                Some(format!("app{i}").as_str()),
                "association for {ext:?} was not restored"
            );
        }
    }

    /// Carry one extension out to a document and back again.
    ///
    /// These three used to call the hand-rolled `to_config_line` /
    /// `from_config_line` pair directly. Going through `export_config` and
    /// `import_config` instead tests the path the program takes, not a codec
    /// beside it -- and it is the path that changed, from an escaped
    /// `ext=app` grammar to a YAML document, so the question these tests ask
    /// is exactly the question the change raises.
    fn survives_a_round_trip(extension: &str) -> Option<String> {
        let mut reg = registry_over(&[extension]);
        reg.set_default_app(extension, "app0")
            .expect("a registered extension and an app that opens it");

        let text = reg.export_config();
        let mut back = registry_over(&[extension]);
        let errors = back.import_config(&text);
        assert!(errors.is_empty(), "{extension:?} -> {text}: {errors:?}");

        back.get_default_app(extension).map(|a| a.id.clone())
    }

    #[test]
    fn an_extension_starting_with_a_hash_is_not_written_as_a_comment() {
        // `#` opens a comment in YAML exactly as it did in the old grammar, so
        // an unquoted `#txt:` key would be skipped in full -- losing the
        // association silently rather than misreporting it.
        assert_eq!(survives_a_round_trip("#txt").as_deref(), Some("app0"));
    }

    #[test]
    fn an_extension_containing_the_separator_stays_one_extension() {
        // The separator moved with the format: `=` was the old one, `:` is
        // YAML's, and an extension may legally contain either.
        assert_eq!(survives_a_round_trip("a=b").as_deref(), Some("app0"));
        assert_eq!(survives_a_round_trip("a: b").as_deref(), Some("app0"));
    }

    #[test]
    fn a_hand_written_file_means_what_it_looks_like() {
        // These files are meant to be edited by hand, so the plainest possible
        // spelling has to work -- no quoting, ordinary indentation.
        let mut reg = registry_over(&["pdf"]);
        let errors = reg.import_config("associations:\n  pdf: app0\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            reg.get_default_app("pdf").map(|a| a.id.as_str()),
            Some("app0")
        );
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

    /// Run a body that changes an association.
    ///
    /// Changing one writes the configuration file, so these have to run against
    /// a throwaway directory rather than the developer's own. Before
    /// `FileAssocUI::persist` existed this program wrote nothing at all, which
    /// is why no test here needed it until now.
    fn writing<T>(tag: &str, body: impl FnOnce() -> T) -> T {
        settingsfile::testing::with_scratch_config(tag, |_| body())
    }

    #[test]
    fn test_ui_confirm_open_with() {
        writing("confirm_open_with", || {
            let mut ui = FileAssocUI::new();
            ui.select_file_type(0);
            ui.open_open_with_dialog();
            ui.dialog_always_use = true;
            ui.dialog_selected_app = Some(0);
            let result = ui.confirm_open_with();
            assert!(result.is_ok());
            assert_eq!(ui.active_dialog, ActiveDialog::None);
        });
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

    // ========================================================================
    // Interaction — driven through the probe, as a user would
    //
    // The tests above this line all call methods. That was the only thing they
    // *could* do: before this app was wired, no click reached any of those
    // methods, so `test_ui_open_with_dialog` proved the dialog opened when
    // asked and proved nothing about whether anything could ask. These press
    // the pixels instead.
    // ========================================================================

    /// The layout at the default window size, which is the size the probe
    /// helpers use unless a test says otherwise.
    fn layout() -> Layout {
        Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    /// The controls this window always draws, whatever is selected.
    ///
    /// `Target::Table` is not among them, and deliberately: it is recorded
    /// *under* the rows so that the wheel works anywhere over the table while a
    /// click only selects where a row is actually drawn. Its centre therefore
    /// answers `Row`, which is the intended behaviour rather than a fault —
    /// `the_empty_table_below_the_last_row_selects_nothing` is where it is
    /// checked.
    const ALWAYS_DRAWN: [Target; 6] = [
        Target::Search,
        Target::AddButton,
        Target::ImportButton,
        Target::ExportButton,
        Target::ResetButton,
        Target::Category(None),
    ];

    /// Filter the table down to `ext` and click its row, as a user hunting for
    /// one file type would.
    ///
    /// Searching by extension can match more than one row (`html` also matches
    /// anything whose MIME type mentions it), so the row is found by its own
    /// extension rather than assumed to be the first.
    fn select_ext(ui: &mut FileAssocUI, ext: &str) {
        ui.set_search_query(ext);
        let i = ui
            .filtered_file_types()
            .iter()
            .position(|ft| ft.extension == ext)
            .unwrap_or_else(|| panic!("no row for .{ext} after filtering for it"));
        assert_eq!(probe::click(ui, Target::Row(i)), EventResult::Consumed);
        assert_eq!(ui.selected_index, Some(i));
    }

    /// An extension more than one installed app can open, which is what the
    /// "Open With" dialog and the compatible-apps list are for.
    /// A file type more than one installed program can open, which is what
    /// the "Open with" tests need to have anything to choose between.
    ///
    /// Was `html` while three fictional programs claimed it. `zip` is claimed
    /// by `archivemanager` and `explorer`, both of which are directories under
    /// `apps/` that build the binaries named here.
    const SHARED_EXT: &str = "zip";

    #[test]
    fn every_control_answers_where_the_frame_draws_it() {
        let ui = FileAssocUI::new();
        for target in ALWAYS_DRAWN {
            let r = probe::rect_of(&ui, target)
                .unwrap_or_else(|| panic!("nothing drawn for {target:?}"));
            let (cx, cy) = r.centre();
            assert_eq!(
                ui.target_at(cx, cy),
                Some(target),
                "{target:?} is drawn at {r:?} but the hit test does not answer there",
            );
        }
    }

    #[test]
    fn no_size_puts_a_hit_box_outside_the_window() {
        // Down to one pixel: `Frame` does not clip to the window, so a control
        // whose position is a fixed offset from the toolbar would keep a hit
        // box below the bottom edge and stay clickable while invisible.
        for (w, h) in [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (640.0, 480.0),
            (320.0, 240.0),
            (160.0, 120.0),
            (40.0, 40.0),
            (1.0, 1.0),
        ] {
            let mut ui = FileAssocUI::new();
            ui.select_file_type(0);
            ui.open_open_with_dialog();
            let window = Rect::new(0.0, 0.0, w, h);
            for (target, r) in ui.frame(w, h).hits() {
                assert_eq!(
                    r.intersect(window),
                    Some(*r),
                    "at ({w}, {h}) the hit box for {target:?} is {r:?}, outside the window",
                );
            }
        }
    }

    #[test]
    fn a_click_selects_the_row_it_lands_on() {
        let mut ui = FileAssocUI::new();
        assert_eq!(ui.selected_index, None);

        let row = probe::target_matching(&ui, |t| matches!(t, Target::Row(_)))
            .expect("the table drew no rows");
        assert_eq!(probe::click(&mut ui, row), EventResult::Consumed);

        let Target::Row(i) = row else {
            panic!("target_matching returned {row:?}");
        };
        assert_eq!(ui.selected_index, Some(i));
        // And the details panel now has something to say about it.
        assert!(probe::is_visible(&ui, Target::OpenWithButton));
        assert!(probe::is_visible(&ui, Target::ClearButton));
    }

    #[test]
    fn the_empty_table_below_the_last_row_selects_nothing() {
        let mut ui = FileAssocUI::new();
        // A filter that matches one row, so the space under it is bare table.
        ui.set_search_query("mp3");
        assert_eq!(ui.filtered_file_types().len(), 1);

        let l = layout();
        let y = l.rows.bottom() - 4.0;
        assert_eq!(ui.target_at(l.rows.centre().0, y), Some(Target::Table));
        assert_eq!(
            ui.handle_event(&Event::Mouse(MouseEvent {
                x: l.rows.centre().0,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            EventResult::Ignored,
        );
        assert_eq!(ui.selected_index, None);
    }

    #[test]
    fn a_sidebar_category_filters_the_table_and_the_table_says_so() {
        let mut ui = FileAssocUI::new();
        let all = ui.filtered_file_types().len();

        assert_eq!(
            probe::click(&mut ui, Target::Category(Some(FileCategory::Audio))),
            EventResult::Consumed,
        );
        assert_eq!(ui.selected_category, Some(FileCategory::Audio));
        let audio = ui.filtered_file_types();
        assert!(!audio.is_empty() && audio.len() < all);
        for ft in &audio {
            assert_eq!(ui.registry.get_category(&ft.extension), FileCategory::Audio,);
        }

        // And "All Types" puts them all back.
        probe::click(&mut ui, Target::Category(None));
        assert_eq!(ui.filtered_file_types().len(), all);
    }

    #[test]
    fn typing_only_reaches_the_search_box_once_it_has_been_clicked() {
        let mut ui = FileAssocUI::new();

        // Unfocused, a letter is not a filter. This is the whole reason
        // `search_focused` exists: a table you can arrow around would otherwise
        // empty itself the moment you pressed a letter.
        assert_eq!(
            probe::key(&mut ui, &probe::typing("m")),
            EventResult::Ignored
        );
        assert_eq!(ui.search_query, "");

        probe::click(&mut ui, Target::Search);
        assert!(ui.search_focused);
        probe::type_str(&mut ui, "mp3");
        assert_eq!(ui.search_query, "mp3");
        assert_eq!(ui.filtered_file_types().len(), 1);

        probe::key(&mut ui, &probe::press(Key::Backspace));
        assert_eq!(ui.search_query, "mp");
    }

    #[test]
    fn a_click_anywhere_else_takes_the_caret_out_of_the_search_box() {
        let mut ui = FileAssocUI::new();
        probe::click(&mut ui, Target::Search);
        assert!(ui.search_focused);

        probe::click(&mut ui, Target::Category(Some(FileCategory::Images)));
        assert!(!ui.search_focused);
        // So the next keystroke does not silently filter the table.
        assert_eq!(
            probe::key(&mut ui, &probe::typing("x")),
            EventResult::Ignored
        );
        assert_eq!(ui.search_query, "");
    }

    #[test]
    fn escape_backs_out_of_one_thing_at_a_time() {
        let mut ui = FileAssocUI::new();
        probe::click(&mut ui, Target::Search);
        probe::type_str(&mut ui, "mp3");
        let row = probe::target_matching(&ui, |t| matches!(t, Target::Row(_))).unwrap();
        probe::click(&mut ui, row);
        probe::click(&mut ui, Target::OpenWithButton);
        assert_eq!(ui.active_dialog, ActiveDialog::OpenWith);

        // Dialog first...
        probe::key(&mut ui, &probe::press(Key::Escape));
        assert_eq!(ui.active_dialog, ActiveDialog::None);
        assert_eq!(ui.search_query, "mp3");

        // ...then the search...
        probe::key(&mut ui, &probe::press(Key::Escape));
        assert_eq!(ui.search_query, "");

        // ...then the selection, which the cleared search kept alive only if it
        // still points at a row. It does not, so the third press finds nothing.
        assert_eq!(ui.selected_index, None);
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Escape)),
            EventResult::Ignored,
        );
    }

    #[test]
    fn an_open_dialog_takes_the_clicks_of_everything_it_covers() {
        let mut ui = FileAssocUI::new();
        probe::click(&mut ui, Target::Category(Some(FileCategory::Audio)));
        let before = ui.selected_category;

        probe::click(&mut ui, Target::AddButton);
        assert_eq!(ui.active_dialog, ActiveDialog::AddFileType);

        // The sidebar is still painted, and is no longer a control.
        assert!(!probe::is_visible(&ui, Target::Category(None)));
        assert!(!probe::is_visible(&ui, Target::ResetButton));
        let l = layout();
        assert_eq!(
            ui.target_at(l.reset.centre().0, l.reset.centre().1),
            Some(Target::Scrim)
        );

        // Clicking the scrim shuts the dialog and changes nothing else.
        probe::click(&mut ui, Target::Scrim);
        assert_eq!(ui.active_dialog, ActiveDialog::None);
        assert_eq!(ui.selected_category, before);
        assert!(probe::is_visible(&ui, Target::ResetButton));
    }

    #[test]
    fn the_add_dialog_registers_a_file_type_that_was_typed_into_it() {
        let mut ui = FileAssocUI::new();
        let before = ui.registry.file_type_count();

        probe::click(&mut ui, Target::AddButton);
        assert_eq!(ui.new_field, NewField::Extension);
        probe::type_str(&mut ui, "opus");
        probe::key(&mut ui, &probe::press(Key::Tab));
        assert_eq!(ui.new_field, NewField::MimeType);
        probe::type_str(&mut ui, "audio/opus");
        probe::key(&mut ui, &probe::press(Key::Tab));
        probe::type_str(&mut ui, "Opus Audio");

        // Clicking the category button cycles it, which is the only way to set
        // it -- there is no room in a 400-pixel dialog for a seven-item list.
        let first = ui.new_category;
        probe::click(&mut ui, Target::DialogCategory);
        assert_ne!(ui.new_category, first);

        probe::click(&mut ui, Target::DialogOk);
        assert_eq!(ui.active_dialog, ActiveDialog::None);
        assert_eq!(ui.registry.file_type_count(), before + 1);
        let added = ui
            .registry
            .get_file_type("opus")
            .expect(".opus was not registered");
        assert_eq!(added.mime_type, "audio/opus");
        assert_eq!(added.description, "Opus Audio");
    }

    #[test]
    fn the_add_dialog_stays_open_and_says_why_when_it_cannot_add() {
        let mut ui = FileAssocUI::new();
        probe::click(&mut ui, Target::AddButton);

        // Nothing typed: OK must not close over an empty extension.
        probe::click(&mut ui, Target::DialogOk);
        assert_eq!(ui.active_dialog, ActiveDialog::AddFileType);
        assert!(
            ui.status.contains("extension"),
            "status was {:?}",
            ui.status
        );

        // A duplicate is refused too, and says which one.
        probe::type_str(&mut ui, "mp3");
        probe::click(&mut ui, Target::DialogOk);
        assert_eq!(ui.active_dialog, ActiveDialog::AddFileType);
        assert!(ui.status.contains("mp3"), "status was {:?}", ui.status);

        // Cancel leaves the registry exactly as it found it.
        let before = ui.registry.file_type_count();
        probe::click(&mut ui, Target::DialogCancel);
        assert_eq!(ui.active_dialog, ActiveDialog::None);
        assert_eq!(ui.registry.file_type_count(), before);
    }

    #[test]
    fn the_open_with_dialog_only_changes_the_default_when_always_is_ticked() {
        writing("open_with_dialog", || {
            let mut ui = FileAssocUI::new();
            select_ext(&mut ui, SHARED_EXT);
            let before = ui
                .registry
                .get_default_app(SHARED_EXT)
                .map(|a| a.id.clone())
                .expect("the shared extension has no default to change");

            // Pick a different app, leave "always" alone, confirm.
            probe::click(&mut ui, Target::OpenWithButton);
            let other = ui
                .registry
                .apps_for_extension(SHARED_EXT)
                .iter()
                .position(|a| a.id != before)
                .expect("only one app opens it, so this test cannot say anything");
            probe::click(&mut ui, Target::DialogApp(other));
            probe::click(&mut ui, Target::DialogOk);
            assert_eq!(
                ui.registry
                    .get_default_app(SHARED_EXT)
                    .map(|a| a.id.clone()),
                Some(before.clone()),
                "a one-off open changed the default",
            );

            // Again, with the box ticked this time.
            probe::click(&mut ui, Target::OpenWithButton);
            probe::click(&mut ui, Target::DialogApp(other));
            probe::click(&mut ui, Target::DialogAlwaysUse);
            assert!(ui.dialog_always_use);
            probe::click(&mut ui, Target::DialogOk);
            assert_ne!(
                ui.registry
                    .get_default_app(SHARED_EXT)
                    .map(|a| a.id.clone()),
                Some(before),
            );
        });
    }

    #[test]
    fn the_open_with_dialog_starts_on_the_app_that_is_already_the_default() {
        let mut ui = FileAssocUI::new();
        select_ext(&mut ui, SHARED_EXT);
        let current = ui
            .registry
            .get_default_app(SHARED_EXT)
            .map(|a| a.id.clone())
            .unwrap();

        probe::click(&mut ui, Target::OpenWithButton);
        let expected = ui
            .registry
            .apps_for_extension(SHARED_EXT)
            .iter()
            .position(|a| a.id == current);
        assert_eq!(ui.dialog_selected_app, expected);
        assert!(
            expected.is_some(),
            "the default is not among the apps offered"
        );
    }

    #[test]
    fn clicking_a_compatible_app_makes_it_the_default_without_a_dialog() {
        writing("compatible_app", || {
            let mut ui = FileAssocUI::new();
            select_ext(&mut ui, SHARED_EXT);

            let apps = ui.registry.apps_for_extension(SHARED_EXT);
            let before = ui
                .registry
                .get_default_app(SHARED_EXT)
                .map(|a| a.id.clone());
            let other = apps
                .iter()
                .position(|a| Some(&a.id) != before.as_ref())
                .expect("only one app opens the shared extension");
            let wanted = apps[other].id.clone();

            probe::click(&mut ui, Target::CompatibleApp(other));
            assert_eq!(
                ui.registry
                    .get_default_app(SHARED_EXT)
                    .map(|a| a.id.clone()),
                Some(wanted),
            );
            assert_eq!(ui.active_dialog, ActiveDialog::None, "a dialog opened");
        });
    }

    #[test]
    fn clear_association_empties_the_default_for_the_selected_row() {
        writing("clear_association", || {
            let mut ui = FileAssocUI::new();
            select_ext(&mut ui, "mp3");
            assert!(ui.registry.get_default_app("mp3").is_some());

            probe::click(&mut ui, Target::ClearButton);
            assert!(ui.registry.get_default_app("mp3").is_none());
            assert!(ui.status.contains("mp3"), "status was {:?}", ui.status);
        });
    }

    #[test]
    fn reset_puts_back_an_association_that_was_cleared() {
        writing("reset_association", || {
            let mut ui = FileAssocUI::new();
            select_ext(&mut ui, "mp3");
            probe::click(&mut ui, Target::ClearButton);
            assert!(ui.registry.get_default_app("mp3").is_none());

            // Reset is in the toolbar, which the search filter does not cover.
            probe::click(&mut ui, Target::ResetButton);
            assert!(ui.registry.get_default_app("mp3").is_some());
            assert_eq!(ui.selected_index, None);
        });
    }

    // -- Persistence ---------------------------------------------------------

    /// **A change survives the program closing.**
    ///
    /// The whole point. Until 2026-09-14 this program built its registry from
    /// built-in defaults at startup and never read or wrote a file: every
    /// choice the user made was gone the moment the window closed. The test
    /// goes through the real door -- a click, then a fresh `load` -- because
    /// the parts either side of the door were already tested and it was the
    /// door that did not exist.
    #[test]
    fn an_association_survives_a_restart() {
        writing("survives_restart", || {
            let mut ui = FileAssocUI::new();
            select_ext(&mut ui, "mp3");
            probe::click(&mut ui, Target::ClearButton);
            assert!(
                ui.registry.get_default_app("mp3").is_none(),
                "the clear did not take: {:?}",
                ui.status
            );
            assert!(
                !ui.status.starts_with("Could not save"),
                "the save failed: {:?}",
                ui.status
            );

            // A second program, reading what the first one wrote.
            let restarted = FileAssocUI::load();
            assert!(
                restarted.registry.get_default_app("mp3").is_none(),
                "the cleared association came back after a restart"
            );
            // And the ones that were never touched are still there, so
            // clearing the defaults to honour the file does not throw the
            // rest of them away.
            assert!(
                restarted.registry.association_count() > 0,
                "a restart lost every association, not just the cleared one"
            );
        });
    }

    /// And a cleared entry is *removed* from the file rather than left in it.
    ///
    /// The half that is easy to leave out of a writer: writing only what is
    /// present leaves the old key sitting there, and it reads back as though
    /// the clear never happened. Asked of the document directly, because the
    /// test above would pass either way if the registry happened to re-derive
    /// the same answer.
    #[test]
    fn clearing_an_association_takes_it_out_of_the_file() {
        let mut reg = registry_over(&["mp3"]);
        reg.set_default_app("mp3", "app0")
            .expect("a registered pair");

        let mut doc = Document::parse(CONFIG_HEADER);
        reg.write_into(&mut doc);
        assert_eq!(
            doc.get_str(&["associations", "mp3"]).as_deref(),
            // The path, because that is what another program can run. The id
            // `app0` means nothing outside this registry.
            Some("/bin/app0")
        );

        reg.clear_association("mp3")
            .expect("an association to clear");
        reg.write_into(&mut doc);
        assert!(
            doc.get_str(&["associations", "mp3"]).is_none(),
            "the cleared association is still in the file: {}",
            doc.to_text()
        );
    }

    /// An entry naming an application that is gone is reported, not hidden.
    #[test]
    fn an_association_for_a_missing_app_is_reported_on_load() {
        let doc = Document::parse("associations:\n  mp3: nosuchapp\n");
        let ui = FileAssocUI::from_document(doc);
        assert!(
            ui.status.contains("could not be restored"),
            "status was {:?}",
            ui.status
        );
    }

    /// Point the picker at `dir` and list it, as the program does on opening.
    fn showing<'a>(ui: &'a mut FileAssocUI, dir: &std::path::Path) -> &'a mut FileDialog {
        let dialog = ui.file_dialog.as_mut().expect("no picker is up");
        dialog.navigate_to(dir);
        dialog.set_entries(guitk::dialog::list_directory(dir));
        dialog
    }

    /// Type a name and confirm it. **A save picker only.**
    fn save_as(ui: &mut FileAssocUI, dir: &std::path::Path, name: &str) {
        showing(ui, dir).set_filename(name);
        probe::key(ui, &probe::press(Key::Enter));
    }

    /// Select the row for an existing file and confirm it. **An open picker.**
    ///
    /// Not `set_filename`: an open picker has no filename field, so typing at
    /// one is typing at nothing. Both import tests were written that way first
    /// and both failed at the confirm -- the picker simply stayed up, which is
    /// exactly what a user would have seen.
    fn pick(ui: &mut FileAssocUI, dir: &std::path::Path, name: &str) {
        let dialog = showing(ui, dir);
        let index = dialog
            .entries()
            .iter()
            .position(|e| e.name == *std::ffi::OsStr::new(name))
            .unwrap_or_else(|| panic!("{name} is not in the listing"));
        dialog.select_entry(index);
        probe::key(ui, &probe::press(Key::Enter));
    }

    /// **Export puts the associations in a file the user can carry away.**
    ///
    /// It used to put them in `last_export`, a `String` field that nothing
    /// rendered, and then say "Exported 8 association(s)" -- a true statement
    /// about a variable and a false one about the world. This test would not
    /// have compiled against that version, which is the point: it reads the
    /// file back off the disk.
    #[test]
    fn export_writes_the_associations_to_the_chosen_file() {
        let dir = scratchdir::ScratchDir::new("fileassoc_export");
        let mut ui = FileAssocUI::new();

        probe::click(&mut ui, Target::ExportButton);
        assert_eq!(ui.active_dialog, ActiveDialog::ChooseFile);
        save_as(&mut ui, dir.dir(), "assoc.conf");

        assert!(ui.file_dialog.is_none(), "the picker stayed up");
        let written = std::fs::read_to_string(dir.path("assoc.conf"))
            .unwrap_or_else(|e| panic!("nothing was written: {e}; status {:?}", ui.status));
        assert!(written.contains("mp3"), "exported {written:?}");
        assert!(ui.status.contains("Exported"), "status was {:?}", ui.status);
    }

    /// Ctrl+E reaches the same picker as the button.
    #[test]
    fn ctrl_e_opens_the_export_picker() {
        let mut ui = FileAssocUI::new();
        probe::key(&mut ui, &probe::ctrl(Key::E));
        assert_eq!(ui.active_dialog, ActiveDialog::ChooseFile);
        assert_eq!(ui.transfer, Transfer::Export);
    }

    /// Ctrl+I reaches the import picker, which had no door of any kind.
    #[test]
    fn ctrl_i_opens_the_import_picker() {
        let mut ui = FileAssocUI::new();
        probe::key(&mut ui, &probe::ctrl(Key::I));
        assert_eq!(ui.active_dialog, ActiveDialog::ChooseFile);
        assert_eq!(ui.transfer, Transfer::Import);
    }

    /// **A choice exported from one copy of the program arrives in another.**
    ///
    /// The whole feature, end to end, through the two buttons a user has.
    /// `import_config` was complete and tested for weeks with nothing calling
    /// it outside its own tests -- `scripts/check-tested-but-uncalled.py` is
    /// what eventually said so, and only after it was itself repaired.
    #[test]
    fn an_association_survives_export_and_import_into_another_copy() {
        writing("fileassoc_transfer", || {
            let dir = scratchdir::ScratchDir::new("fileassoc_transfer");

            let mut from = FileAssocUI::new();
            from.registry
                .set_default_app("zip", "explorer")
                .expect("explorer handles zip");
            probe::click(&mut from, Target::ExportButton);
            save_as(&mut from, dir.dir(), "carried.conf");

            let mut to = FileAssocUI::new();
            assert_ne!(
                to.registry.get_default_app("zip").map(|a| a.id.as_str()),
                Some("explorer"),
                "the fixture proves nothing if it is already the default"
            );

            probe::click(&mut to, Target::ImportButton);
            pick(&mut to, dir.dir(), "carried.conf");

            assert!(to.file_dialog.is_none(), "the picker stayed up");
            assert_eq!(
                to.registry.get_default_app("zip").map(|a| a.id.as_str()),
                Some("explorer"),
                "status was {:?}",
                to.status
            );
        });
    }

    /// Escape puts the picker away and imports nothing.
    #[test]
    fn cancelling_the_picker_changes_no_association() {
        let mut ui = FileAssocUI::new();
        let before = ui.registry.export_config();

        probe::click(&mut ui, Target::ImportButton);
        // Without this the test passes when the button does nothing at all:
        // "no picker is up" is what it asserts afterwards, and that is also
        // true of a button that never opened one. Caught by removing the
        // door and finding only one of the two tests went red.
        assert_eq!(ui.active_dialog, ActiveDialog::ChooseFile);

        probe::key(&mut ui, &probe::press(Key::Escape));

        assert!(ui.file_dialog.is_none(), "Escape did not dismiss it");
        assert_eq!(ui.active_dialog, ActiveDialog::None);
        assert_eq!(ui.registry.export_config(), before);
    }

    /// A file that cannot be read is reported, not swallowed.
    ///
    /// Called directly rather than through the picker, because a picker cannot
    /// offer a file that is not there -- the first version of this test tried,
    /// and proved only that an open dialog ignores a typed filename. The path
    /// this covers is real (a file removed between the listing and the read,
    /// or one whose permissions changed), and the door it sits behind is
    /// proven by `an_association_survives_export_and_import_into_another_copy`.
    #[test]
    fn an_unreadable_import_says_so_instead_of_failing_silently() {
        let dir = scratchdir::ScratchDir::new("fileassoc_missing");
        let mut ui = FileAssocUI::new();
        ui.transfer = Transfer::Import;

        ui.transfer_with(&dir.path("no-such-file.conf"));

        assert!(
            ui.status.contains("Could not read"),
            "status was {:?}",
            ui.status
        );
    }

    #[test]
    fn the_wheel_scrolls_the_table_and_stops_at_both_ends() {
        let mut ui = FileAssocUI::new();
        let l = layout();
        let (cx, cy) = l.rows.centre();

        // The list is longer than the viewport, or this test proves nothing.
        assert!(
            ui.max_scroll() > 0.0,
            "the default table fits, so it cannot scroll"
        );

        let scroll = |ui: &mut FileAssocUI, dy: f32| {
            ui.handle_event(&Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }))
        };

        assert_eq!(scroll(&mut ui, -1.0), EventResult::Consumed);
        assert_eq!(ui.scroll_offset, wheel::pixels(-1.0, ROW_HEIGHT).abs());

        // Up past the top clamps at zero rather than going negative.
        scroll(&mut ui, 50.0);
        assert_eq!(ui.scroll_offset, 0.0);

        // Down past the end clamps at the last screenful.
        scroll(&mut ui, -500.0);
        assert_eq!(ui.scroll_offset, ui.max_scroll());
    }

    #[test]
    fn the_wheel_over_the_toolbar_leaves_the_table_alone() {
        let mut ui = FileAssocUI::new();
        let l = layout();
        let (cx, cy) = l.reset.centre();
        assert_eq!(
            ui.handle_event(&Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
            })),
            EventResult::Ignored,
        );
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn the_arrows_walk_the_selection_and_drag_the_viewport_with_them() {
        let mut ui = FileAssocUI::new();
        probe::key(&mut ui, &probe::press(Key::Down));
        assert_eq!(ui.selected_index, Some(0));

        // Walk to the end. The selection must stay on screen the whole way --
        // without `scroll_selection_into_view` the arrow keys march off the
        // bottom of a list that never follows.
        let last = ui.filtered_file_types().len() - 1;
        for _ in 0..last {
            probe::key(&mut ui, &probe::press(Key::Down));
        }
        assert_eq!(ui.selected_index, Some(last));
        assert!(
            probe::is_visible(&ui, Target::Row(last)),
            "row {last} is selected but scrolled out of sight",
        );

        // And it stops there rather than wrapping.
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Down)),
            EventResult::Ignored,
        );
        assert_eq!(ui.selected_index, Some(last));
    }

    #[test]
    fn a_narrower_window_shrinks_the_details_panel_before_the_table() {
        let wide = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let narrow = Layout::new(420.0, WINDOW_HEIGHT);
        assert!(narrow.details.w < wide.details.w);
        assert!(narrow.table.w > 0.0, "the table was squeezed out entirely");
        // The three body panels tile the window without overlapping.
        assert_eq!(narrow.sidebar.right(), narrow.table.x);
        assert_eq!(narrow.table.right(), narrow.details.x);
    }

    #[test]
    fn a_resized_window_lays_out_again_and_the_hit_test_follows() {
        let mut ui = FileAssocUI::new();
        let old = probe::rect_of(&ui, Target::ResetButton).unwrap();

        ui.handle_event(&Event::Resize {
            width: 640,
            height: 480,
        });
        let new = probe::rect_of_sized(&ui, Target::ResetButton, (640.0, 480.0)).unwrap();
        assert_ne!(old.x, new.x);
        let (cx, cy) = new.centre();
        assert_eq!(ui.target_at(cx, cy), Some(Target::ResetButton));
    }

    #[test]
    fn render_lays_out_at_the_size_it_is_handed() {
        let mut ui = FileAssocUI::new();
        // A compositor may call `render` at a size it never sent a `Resize`
        // for, and a window that drew its old layout there would put every
        // control in the wrong place.
        let rt = App::render(&mut ui, 500.0, 400.0);
        assert!(!rt.is_empty());
        assert_eq!(ui.window_width, 500.0);
        assert_eq!(ui.window_height, 400.0);
        assert_eq!(
            probe::rect_of_sized(&ui, Target::ResetButton, (500.0, 400.0)),
            Some(Layout::new(500.0, 400.0).reset),
        );
    }

    #[test]
    fn a_degenerate_size_does_not_poison_the_layout() {
        // A compositor mid-resize can report zero, and a NaN width would make
        // every comparison downstream false -- including the scroll clamp, so
        // the table would never move again.
        for (w, h) in [(0.0, 0.0), (f32::NAN, 300.0), (300.0, f32::INFINITY)] {
            let l = Layout::new(w, h);
            assert!(l.width.is_finite() && l.width > 0.0);
            assert!(l.height.is_finite() && l.height > 0.0);
        }
        let mut ui = FileAssocUI::new();
        ui.resize(f32::NAN, f32::NAN);
        ui.scroll_by(f32::NAN);
        assert!(ui.scroll_offset.is_finite());
    }

    #[test]
    fn ctrl_q_closes_the_window_and_the_close_button_does_too() {
        let mut ui = FileAssocUI::new();
        assert_eq!(
            ui.on_event(&Event::Key(probe::ctrl(Key::Q))),
            Response::Exit,
        );
        assert_eq!(ui.on_event(&Event::CloseRequested), Response::Exit);
        // A bare Q is text, not a command: it goes to whatever has the caret.
        assert_eq!(
            ui.on_event(&Event::Key(probe::press_with(Key::Q, Modifiers::default()))),
            Response::Idle,
        );
    }

    #[test]
    fn only_an_event_that_changes_something_asks_for_a_frame() {
        writing("event_asks_for_a_frame", || {
            let mut ui = FileAssocUI::new();
            // Nothing under the pointer, nothing to redraw.
            let (bx, by) = probe::bare_point(&ui, (WINDOW_WIDTH, WINDOW_HEIGHT))
                .expect("every point in the window is covered");
            assert_eq!(
                ui.on_event(&Event::Mouse(MouseEvent {
                    x: bx,
                    y: by,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })),
                Response::Idle,
            );
            assert_eq!(ui.on_event(&Event::FocusOut), Response::Idle);

            let l = layout();
            assert_eq!(
                ui.on_event(&Event::Mouse(MouseEvent {
                    x: l.reset.centre().0,
                    y: l.reset.centre().1,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })),
                Response::Redraw,
            );
        });
    }

    #[test]
    fn every_target_the_frame_records_is_one_the_app_handles() {
        writing("every_target_handled", || {
            // A control drawn but not routed is the fault this whole app had: a
            // toolbar, a sidebar, a table and a modal dialog, all pictures. This
            // walks every state that draws anything and clicks all of it.
            let mut seen: Vec<String> = Vec::new();
            for state in 0..3 {
                let mut ui = FileAssocUI::new();
                ui.select_file_type(0);
                match state {
                    1 => ui.open_open_with_dialog(),
                    2 => ui.open_add_file_type_dialog(),
                    _ => {}
                }
                for name in probe::control_names(&ui) {
                    if !seen.contains(&name) {
                        seen.push(name);
                    }
                }
                let targets: Vec<Target> = ui
                    .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                    .hits()
                    .iter()
                    .map(|(t, _)| *t)
                    .collect();
                for target in targets {
                    let mut fresh = FileAssocUI::new();
                    fresh.select_file_type(0);
                    match state {
                        1 => fresh.open_open_with_dialog(),
                        2 => fresh.open_add_file_type_dialog(),
                        _ => {}
                    }
                    // `Table` is the one deliberate exception: it exists so the
                    // wheel works over the header and the space below the last
                    // row, and a click there is meant to do nothing.
                    let outcome = probe::click(&mut fresh, target);
                    if target != Target::Table {
                        assert_eq!(
                            outcome,
                            EventResult::Consumed,
                            "{target:?} is drawn but a click on it does nothing",
                        );
                    }
                }
            }

            // And every variant of `Target` is reachable in at least one of them.
            for name in [
                "Search",
                "AddButton",
                "ExportButton",
                "ResetButton",
                "Category",
                "Row",
                "Table",
                "OpenWithButton",
                "ClearButton",
                "CompatibleApp",
                "DialogApp",
                "DialogAlwaysUse",
                "DialogOk",
                "DialogCancel",
                "DialogField",
                "DialogCategory",
                "Scrim",
            ] {
                assert!(
                    seen.iter().any(|s| s == name),
                    "no state draws {name}; drawn were {seen:?}",
                );
            }
        });
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut FileAssocUI) -> Vec<Color> {
            app.render(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = FileAssocUI::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ---- uninstalling a handler ------------------------------------------

    /// A type whose handler is uninstalled goes back to the handler it had
    /// before, rather than being left with none.
    #[test]
    fn uninstalling_a_handler_falls_back_to_the_previous_one() {
        let mut reg = AssociationRegistry::with_defaults();
        let openers: Vec<String> = reg
            .apps_for_extension(SHARED_EXT)
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert!(
            openers.len() >= 2,
            "this test needs two apps that open .{SHARED_EXT}; found {openers:?}"
        );

        reg.set_default_app(SHARED_EXT, &openers[0])
            .expect("assignable");
        reg.set_default_app(SHARED_EXT, &openers[1])
            .expect("assignable");
        reg.remove_app(&openers[1]).expect("registered");

        assert_eq!(
            reg.file_types[SHARED_EXT].default_app_id.as_ref(),
            Some(&openers[0]),
            "uninstalling the handler orphaned the file type instead of falling back to the one before it"
        );
    }

    /// And the two records of "what opens this" agree afterwards.
    ///
    /// They did not: `remove_app` deleted the `associations` entry and left
    /// `file_types[ext].default_app_id` naming the removed application, so
    /// which answer a caller got depended on which map it asked. No test
    /// covered it because each map was only ever checked on its own.
    #[test]
    fn both_records_of_the_default_agree_after_a_removal() {
        let mut reg = AssociationRegistry::with_defaults();
        let openers: Vec<String> = reg
            .apps_for_extension(SHARED_EXT)
            .iter()
            .map(|a| a.id.clone())
            .collect();
        reg.set_default_app(SHARED_EXT, &openers[0])
            .expect("assignable");
        reg.set_default_app(SHARED_EXT, &openers[1])
            .expect("assignable");
        reg.remove_app(&openers[1]).expect("registered");

        assert_eq!(
            reg.file_types[SHARED_EXT].default_app_id.as_deref(),
            reg.associations.get(SHARED_EXT).map(|a| a.app_id.as_str()),
            "the registry disagrees with itself about what opens .{SHARED_EXT}"
        );
    }

    /// With nothing to fall back to, the type is cleared — in *both* records.
    #[test]
    fn uninstalling_the_only_handler_clears_the_type_in_both_records() {
        let mut reg = AssociationRegistry::with_defaults();
        let only = reg.apps_for_extension("pdf").first().map(|a| a.id.clone());
        let Some(only) = only else { return };
        if reg.apps_for_extension("pdf").len() != 1 {
            return; // another opener exists; that is the test above
        }
        reg.set_default_app("pdf", &only).expect("assignable");
        reg.remove_app(&only).expect("registered");

        assert_eq!(reg.file_types["pdf"].default_app_id, None);
        assert!(!reg.associations.contains_key("pdf"));
    }

    /// The history is a fallback chain and not an audit log, so it is bounded.
    #[test]
    fn the_handler_history_is_bounded() {
        let mut reg = AssociationRegistry::with_defaults();
        let openers: Vec<String> = reg
            .apps_for_extension(SHARED_EXT)
            .iter()
            .map(|a| a.id.clone())
            .collect();
        // Cycle through the openers enough times to overflow the bound.
        for round in 0..(MAX_HANDLER_HISTORY + 3) {
            let id = &openers[round % openers.len()];
            reg.set_default_app(SHARED_EXT, id).expect("assignable");
        }
        assert!(
            reg.file_types[SHARED_EXT].handler_history.len() <= MAX_HANDLER_HISTORY,
            "the history grew past its bound: {:?}",
            reg.file_types[SHARED_EXT].handler_history
        );
    }

    /// Re-selecting the current handler does not push it onto its own history,
    /// which would let an app fall back to itself after being uninstalled.
    #[test]
    fn an_app_never_becomes_its_own_fallback() {
        let mut reg = AssociationRegistry::with_defaults();
        let one = reg.apps_for_extension(SHARED_EXT)[0].id.clone();
        reg.set_default_app(SHARED_EXT, &one).expect("assignable");
        reg.set_default_app(SHARED_EXT, &one).expect("assignable");
        assert!(
            !reg.file_types[SHARED_EXT].handler_history.contains(&one),
            "the current handler is in its own fallback history"
        );

        reg.remove_app(&one).expect("registered");
        assert_ne!(
            reg.file_types[SHARED_EXT].default_app_id.as_ref(),
            Some(&one),
            "an uninstalled app came back as its own fallback"
        );
    }
}
