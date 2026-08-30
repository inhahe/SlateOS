//! Context menu extension system.
//!
//! Allows applications to register context menu items with the desktop shell.
//! Extensions are loaded lazily — the app's code is not loaded until the user
//! actually clicks the menu item. Each extension requires the
//! `context_menu_extension` capability. The user can see and disable extensions
//! in a settings panel.
//!
//! Design constraints (from design.txt):
//! - Programs must request a capability to add context menu items.
//! - Items load lazily (don't load the program just to show the menu).
//! - Settings page to see and disable individual extensions.
//! - Rate limit: if a handler takes >200ms, skip it with "loading..." entry.

use appearance::Palette;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour drawn here comes from the `&Palette` passed to
// `render_context_menu` and to `ExtensionSettingsUI::render`, so both follow the
// desktop's mode and accent.  Four judgements had to be made when the hardcoded
// hexes came out, because a literal carries no role until someone assigns one:
//
// *Pointing at a row is said by the row, not by its contents.*  The hovered
// row lifts one surface step — `base` menu, `surface0` highlight — which is the
// same thing a hovered list row does in `notif_pane`, and a selected row in the
// settings panel does the same over `mantle`.  A surface step is how this crate
// says "here", and it costs the accent nothing.
//
// *One thing takes the accent: the icon of a hovered extension item.*  It was
// a hardcoded blue that appears in no other state, and a colour that appears
// only in one state marks that **state** — hover, which is a position, which is
// what the accent is for.  A built-in item's icon merely brightens to `text`
// instead.  That is a hierarchy rather than an inconsistency: an extension item
// invokes code the shell did not write, so it gets the stronger mark of the
// two, and the row highlight already tells you where the pointer is either way.
//
// *Three things are frozen, because they report facts rather than offering
// choices*: the green/red dot that says whether an extension is enabled, and
// the yellow "Slow" badge.  Each is the answer to a question about the
// extension — an answer that must not change because the user picked a
// different accent, and must not be mistaken for something clickable.
//
// *The shadow is the shared popup shadow.*  It was `rgba(0, 0, 0, 80)`, chosen
// alone; `Palette::shadow()` is `rgba(0, 0, 0, 120)` and is documented as the
// one shadow for every popup that sits on top of a window, which a context menu
// is.  This menu is the fourth to join it, and the slight deepening is the
// point: four popups that used to be four different depths are now one.  Note
// that the membership sweep cannot check this, since it allows black at any
// alpha; the shadow therefore carries its own assertion.

// ============================================================================
// Types
// ============================================================================

/// Unique identifier for a context menu extension.
pub type ExtensionId = u64;

/// What kind of target the extension applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// Any file.
    AnyFile,
    /// Files matching specific extensions (e.g., `png`, `jpg`).
    FileExtensions(Vec<String>),
    /// Directories only.
    Directory,
    /// Desktop background (right-click on desktop).
    DesktopBackground,
    /// Multiple selected files/folders.
    MultipleSelection,
    /// Any target (universal).
    Any,
}

impl TargetKind {
    /// Check if this target kind matches a given context.
    pub fn matches(&self, ctx: &ContextTarget) -> bool {
        match self {
            Self::Any => true,
            Self::AnyFile => ctx.is_file,
            Self::Directory => ctx.is_directory,
            Self::DesktopBackground => ctx.is_desktop,
            Self::MultipleSelection => ctx.selection_count > 1,
            Self::FileExtensions(exts) => {
                if let Some(ext) = &ctx.file_extension {
                    let lower = ext.to_lowercase();
                    exts.iter().any(|e| e.to_lowercase() == lower)
                } else {
                    false
                }
            }
        }
    }
}

/// Describes the context (right-click target) at menu invocation time.
#[derive(Clone, Debug)]
pub struct ContextTarget {
    /// Whether the target is a file.
    pub is_file: bool,
    /// Whether the target is a directory.
    pub is_directory: bool,
    /// Whether it's the desktop background.
    pub is_desktop: bool,
    /// File extension (lowercase, without dot), if applicable.
    pub file_extension: Option<String>,
    /// Full path of the target, if applicable.
    pub path: Option<String>,
    /// How many items are selected.
    pub selection_count: usize,
    /// MIME type, if known.
    pub mime_type: Option<String>,
}

impl ContextTarget {
    /// Create a target for a single file.
    pub fn file(path: &str, extension: Option<&str>) -> Self {
        Self {
            is_file: true,
            is_directory: false,
            is_desktop: false,
            file_extension: extension.map(|s| s.to_string()),
            path: Some(path.to_string()),
            selection_count: 1,
            mime_type: None,
        }
    }

    /// Create a target for a directory.
    pub fn directory(path: &str) -> Self {
        Self {
            is_file: false,
            is_directory: true,
            is_desktop: false,
            file_extension: None,
            path: Some(path.to_string()),
            selection_count: 1,
            mime_type: None,
        }
    }

    /// Create a target for the desktop background.
    pub fn desktop() -> Self {
        Self {
            is_file: false,
            is_directory: false,
            is_desktop: true,
            file_extension: None,
            path: None,
            selection_count: 0,
            mime_type: None,
        }
    }
}

/// Where the extension item appears in the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MenuPosition {
    /// Near the top (open/run actions).
    Top,
    /// Middle section (editing actions).
    Middle,
    /// Near the bottom (utilities, "Send to", etc.).
    Bottom,
}

/// A registered context menu extension.
#[derive(Clone, Debug)]
pub struct ContextMenuExtension {
    /// Unique ID.
    pub id: ExtensionId,
    /// Display label for the menu item.
    pub label: String,
    /// Optional icon text (emoji or symbol).
    pub icon: Option<String>,
    /// Which targets this extension applies to.
    pub target_kind: TargetKind,
    /// Where in the menu to place this item.
    pub position: MenuPosition,
    /// Application that registered this extension.
    pub app_name: String,
    /// Process name / executable used to invoke the handler.
    pub handler_command: String,
    /// Whether the extension is enabled by the user.
    pub enabled: bool,
    /// Capability token ID (must have `context_menu_extension` cap).
    pub capability_token: u64,
    /// Keyboard shortcut hint (if any).
    pub shortcut_hint: Option<String>,
    /// Whether this extension supports multiple selected items.
    pub supports_multi_select: bool,
    /// Submenu items (if this is a submenu parent).
    pub submenu: Vec<SubMenuItem>,
    /// Average response time in ms (tracked for rate limiting).
    response_time_avg_ms: f64,
    /// Number of invocations (for tracking).
    invocation_count: u64,
    /// Whether currently loading (took too long last time).
    pub loading_timeout: bool,
}

impl ContextMenuExtension {
    /// Create a new extension.
    pub fn new(
        id: ExtensionId,
        label: &str,
        target_kind: TargetKind,
        app_name: &str,
        handler_command: &str,
        capability_token: u64,
    ) -> Self {
        Self {
            id,
            label: label.to_string(),
            icon: None,
            target_kind,
            position: MenuPosition::Bottom,
            app_name: app_name.to_string(),
            handler_command: handler_command.to_string(),
            enabled: true,
            capability_token,
            shortcut_hint: None,
            supports_multi_select: false,
            submenu: Vec::new(),
            response_time_avg_ms: 0.0,
            invocation_count: 0,
            loading_timeout: false,
        }
    }

    /// Set the icon.
    pub fn with_icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_string());
        self
    }

    /// Set menu position.
    pub fn with_position(mut self, pos: MenuPosition) -> Self {
        self.position = pos;
        self
    }

    /// Set shortcut hint text.
    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut_hint = Some(shortcut.to_string());
        self
    }

    /// Set multi-select support.
    pub fn with_multi_select(mut self) -> Self {
        self.supports_multi_select = true;
        self
    }

    /// Add a submenu item.
    pub fn add_submenu_item(&mut self, item: SubMenuItem) {
        self.submenu.push(item);
    }

    /// Record an invocation time for rate-limiting tracking.
    pub fn record_invocation(&mut self, duration_ms: f64) {
        self.invocation_count = self.invocation_count.saturating_add(1);
        // Exponential moving average.
        let alpha = 0.3;
        self.response_time_avg_ms = alpha * duration_ms + (1.0 - alpha) * self.response_time_avg_ms;
        self.loading_timeout = self.response_time_avg_ms > 200.0;
    }

    /// Whether this extension is slow (avg > 200ms).
    pub fn is_slow(&self) -> bool {
        self.response_time_avg_ms > 200.0
    }

    /// Whether this extension matches the given context.
    pub fn matches_context(&self, ctx: &ContextTarget) -> bool {
        if !self.enabled {
            return false;
        }
        if ctx.selection_count > 1 && !self.supports_multi_select {
            return false;
        }
        self.target_kind.matches(ctx)
    }
}

/// A submenu item within an extension.
#[derive(Clone, Debug)]
pub struct SubMenuItem {
    /// Label text.
    pub label: String,
    /// Optional icon.
    pub icon: Option<String>,
    /// Command argument appended to the handler command.
    pub action_arg: String,
    /// Enabled state.
    pub enabled: bool,
}

impl SubMenuItem {
    pub fn new(label: &str, action_arg: &str) -> Self {
        Self {
            label: label.to_string(),
            icon: None,
            action_arg: action_arg.to_string(),
            enabled: true,
        }
    }

    pub fn with_icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_string());
        self
    }
}

// ============================================================================
// Extension manager
// ============================================================================

/// Manages all registered context menu extensions.
pub struct ContextMenuExtensionManager {
    /// All registered extensions.
    extensions: Vec<ContextMenuExtension>,
    /// Source of extension IDs.
    ids: IdSeq<ExtensionId>,
    /// Maximum extensions per app (prevent abuse).
    pub max_per_app: usize,
    /// Global timeout threshold in ms.
    pub timeout_threshold_ms: f64,
    /// Whether extensions are globally enabled.
    pub extensions_enabled: bool,
}

impl ContextMenuExtensionManager {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
            ids: IdSeq::new(),
            max_per_app: 10,
            timeout_threshold_ms: 200.0,
            extensions_enabled: true,
        }
    }

    /// Register a new extension. Returns the assigned ID, or None if rejected.
    pub fn register(
        &mut self,
        label: &str,
        target_kind: TargetKind,
        app_name: &str,
        handler_command: &str,
        capability_token: u64,
    ) -> Option<ExtensionId> {
        // Check per-app limit.
        let app_count = self
            .extensions
            .iter()
            .filter(|e| e.app_name == app_name)
            .count();
        if app_count >= self.max_per_app {
            return None;
        }

        // Check for duplicate label from same app.
        if self
            .extensions
            .iter()
            .any(|e| e.app_name == app_name && e.label == label)
        {
            return None;
        }

        let id = self.ids.issue()?;

        let ext = ContextMenuExtension::new(
            id,
            label,
            target_kind,
            app_name,
            handler_command,
            capability_token,
        );
        self.extensions.push(ext);
        Some(id)
    }

    /// Unregister an extension by ID.
    pub fn unregister(&mut self, id: ExtensionId) -> bool {
        let len_before = self.extensions.len();
        self.extensions.retain(|e| e.id != id);
        self.extensions.len() < len_before
    }

    /// Unregister all extensions from a specific app.
    pub fn unregister_app(&mut self, app_name: &str) -> usize {
        let len_before = self.extensions.len();
        self.extensions.retain(|e| e.app_name != app_name);
        // `retain` only ever shrinks, so this cannot go negative — written
        // saturating so that is a property of the expression rather than of
        // the reader's memory of what `retain` does.
        len_before.saturating_sub(self.extensions.len())
    }

    /// Enable or disable a specific extension.
    pub fn set_enabled(&mut self, id: ExtensionId, enabled: bool) -> bool {
        if let Some(ext) = self.extensions.iter_mut().find(|e| e.id == id) {
            ext.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get a mutable reference to an extension.
    pub fn get_mut(&mut self, id: ExtensionId) -> Option<&mut ContextMenuExtension> {
        self.extensions.iter_mut().find(|e| e.id == id)
    }

    /// Get a reference to an extension.
    pub fn get(&self, id: ExtensionId) -> Option<&ContextMenuExtension> {
        self.extensions.iter().find(|e| e.id == id)
    }

    /// Query which extensions match a given context, sorted by position.
    pub fn query(&self, ctx: &ContextTarget) -> Vec<&ContextMenuExtension> {
        if !self.extensions_enabled {
            return Vec::new();
        }
        let mut matches: Vec<&ContextMenuExtension> = self
            .extensions
            .iter()
            .filter(|e| e.matches_context(ctx))
            .collect();
        matches.sort_by_key(|e| e.position);
        matches
    }

    /// List all extensions (for the settings panel).
    pub fn all_extensions(&self) -> &[ContextMenuExtension] {
        &self.extensions
    }

    /// Count extensions per app.
    pub fn extensions_by_app(&self) -> Vec<(String, usize)> {
        let mut app_counts: Vec<(String, usize)> = Vec::new();
        for ext in &self.extensions {
            if let Some(entry) = app_counts
                .iter_mut()
                .find(|(name, _)| name == &ext.app_name)
            {
                entry.1 = entry.1.saturating_add(1);
            } else {
                app_counts.push((ext.app_name.clone(), 1));
            }
        }
        app_counts.sort_by(|a, b| a.0.cmp(&b.0));
        app_counts
    }

    /// Get total count.
    pub fn count(&self) -> usize {
        self.extensions.len()
    }

    /// Get count of enabled extensions.
    pub fn enabled_count(&self) -> usize {
        self.extensions.iter().filter(|e| e.enabled).count()
    }

    /// Get count of slow extensions (avg > timeout threshold).
    pub fn slow_count(&self) -> usize {
        self.extensions.iter().filter(|e| e.is_slow()).count()
    }
}

impl Default for ContextMenuExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in context menu entries
// ============================================================================

/// Standard context menu items that the shell itself provides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltinMenuItem {
    Open,
    OpenWith,
    Cut,
    Copy,
    Paste,
    Delete,
    Rename,
    Properties,
    NewFolder,
    NewFile,
    Refresh,
    SortBy,
    ViewMode,
    CopyPath,
    OpenTerminalHere,
    CompressToArchive,
    ExtractHere,
}

impl BuiltinMenuItem {
    /// Label text.
    pub fn label(&self) -> &str {
        match self {
            Self::Open => "Open",
            Self::OpenWith => "Open With...",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Delete => "Delete",
            Self::Rename => "Rename",
            Self::Properties => "Properties",
            Self::NewFolder => "New Folder",
            Self::NewFile => "New File",
            Self::Refresh => "Refresh",
            Self::SortBy => "Sort By",
            Self::ViewMode => "View",
            Self::CopyPath => "Copy Path",
            Self::OpenTerminalHere => "Open Terminal Here",
            Self::CompressToArchive => "Compress to Archive",
            Self::ExtractHere => "Extract Here",
        }
    }

    /// Shortcut hint.
    pub fn shortcut(&self) -> Option<&str> {
        match self {
            Self::Cut => Some("Ctrl+X"),
            Self::Copy => Some("Ctrl+C"),
            Self::Paste => Some("Ctrl+V"),
            Self::Delete => Some("Del"),
            Self::Rename => Some("F2"),
            Self::Refresh => Some("F5"),
            Self::Properties => Some("Alt+Enter"),
            Self::CopyPath => Some("Ctrl+Shift+C"),
            _ => None,
        }
    }

    /// Icon text.
    pub fn icon(&self) -> &str {
        match self {
            Self::Open => "\u{1F4C2}",
            Self::OpenWith => "\u{1F4E6}",
            Self::Cut => "\u{2702}",
            Self::Copy => "\u{1F4CB}",
            Self::Paste => "\u{1F4CB}",
            Self::Delete => "\u{1F5D1}",
            Self::Rename => "\u{270F}",
            Self::Properties => "\u{2139}",
            Self::NewFolder => "\u{1F4C1}",
            Self::NewFile => "\u{1F4C4}",
            Self::Refresh => "\u{1F504}",
            Self::SortBy => "\u{2195}",
            Self::ViewMode => "\u{1F441}",
            Self::CopyPath => "\u{1F517}",
            Self::OpenTerminalHere => "\u{1F4BB}",
            Self::CompressToArchive => "\u{1F4E6}",
            Self::ExtractHere => "\u{1F4E5}",
        }
    }

    /// Get the file context builtins.
    pub fn file_items() -> Vec<Self> {
        vec![
            Self::Open,
            Self::OpenWith,
            Self::Cut,
            Self::Copy,
            Self::Delete,
            Self::Rename,
            Self::CopyPath,
            Self::Properties,
        ]
    }

    /// Get the directory context builtins.
    pub fn directory_items() -> Vec<Self> {
        vec![
            Self::Open,
            Self::OpenTerminalHere,
            Self::Cut,
            Self::Copy,
            Self::Paste,
            Self::Delete,
            Self::Rename,
            Self::CopyPath,
            Self::Properties,
        ]
    }

    /// Get the desktop background builtins.
    pub fn desktop_items() -> Vec<Self> {
        vec![
            Self::Paste,
            Self::NewFolder,
            Self::NewFile,
            Self::Refresh,
            Self::SortBy,
            Self::ViewMode,
            Self::Properties,
        ]
    }
}

// ============================================================================
// Context menu builder
// ============================================================================

/// An entry in the assembled context menu.
#[derive(Clone, Debug)]
pub enum ContextMenuEntry {
    /// A built-in shell item.
    Builtin(BuiltinMenuItem),
    /// An extension item.
    Extension {
        id: ExtensionId,
        label: String,
        icon: Option<String>,
        shortcut: Option<String>,
        slow: bool,
        submenu: Vec<SubMenuItem>,
    },
    /// A visual separator.
    Separator,
}

/// Build a complete context menu for a given target, merging builtins and
/// matching extensions.
pub fn build_context_menu(
    ctx: &ContextTarget,
    ext_mgr: &ContextMenuExtensionManager,
) -> Vec<ContextMenuEntry> {
    let mut menu = Vec::new();

    // Built-in items first.
    let builtins = if ctx.is_desktop {
        BuiltinMenuItem::desktop_items()
    } else if ctx.is_directory {
        BuiltinMenuItem::directory_items()
    } else {
        BuiltinMenuItem::file_items()
    };

    for item in builtins {
        menu.push(ContextMenuEntry::Builtin(item));
    }

    // Extension items, grouped by position.
    let matches = ext_mgr.query(ctx);
    if !matches.is_empty() {
        // Insert extensions sorted by position, with separators between groups.
        let mut prev_position: Option<MenuPosition> = None;
        let mut ext_entries: Vec<ContextMenuEntry> = Vec::new();

        for ext in &matches {
            if let Some(prev) = prev_position
                && ext.position != prev
            {
                ext_entries.push(ContextMenuEntry::Separator);
            }
            ext_entries.push(ContextMenuEntry::Extension {
                id: ext.id,
                label: ext.label.clone(),
                icon: ext.icon.clone(),
                shortcut: ext.shortcut_hint.clone(),
                slow: ext.is_slow(),
                submenu: ext.submenu.clone(),
            });
            prev_position = Some(ext.position);
        }

        // Insert a separator before extensions.
        if !ext_entries.is_empty() {
            menu.push(ContextMenuEntry::Separator);
            menu.extend(ext_entries);
        }
    }

    menu
}

// ============================================================================
// Rendering
// ============================================================================

/// Render a context menu into render commands.
pub fn render_context_menu(
    menu: &[ContextMenuEntry],
    p: &Palette,
    x: f32,
    y: f32,
    width: f32,
    hovered_index: Option<usize>,
) -> Vec<RenderCommand> {
    let mut commands = Vec::new();
    let item_height = 28.0;
    let separator_height = 8.0;
    let padding = 8.0;
    let corner = 8.0;

    // Calculate total height.
    let total_height: f32 = menu
        .iter()
        .map(|entry| match entry {
            ContextMenuEntry::Separator => separator_height,
            _ => item_height,
        })
        .sum::<f32>()
        + padding * 2.0;

    // Background shadow.
    commands.push(RenderCommand::BoxShadow {
        x,
        y,
        width,
        height: total_height,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 12.0,
        spread: 0.0,
        color: p.shadow(),
        corner_radii: CornerRadii::all(corner),
    });

    // Background.
    commands.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: total_height,
        color: p.base,
        corner_radii: CornerRadii::all(corner),
    });

    // Border.
    commands.push(RenderCommand::StrokeRect {
        x,
        y,
        width,
        height: total_height,
        color: p.surface1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(corner),
    });

    // Items.
    let mut cy = y + padding;
    for (i, entry) in menu.iter().enumerate() {
        match entry {
            ContextMenuEntry::Separator => {
                let sep_y = cy + separator_height / 2.0;
                commands.push(RenderCommand::Line {
                    x1: x + 12.0,
                    y1: sep_y,
                    x2: x + width - 12.0,
                    y2: sep_y,
                    color: p.surface0,
                    width: 1.0,
                });
                cy += separator_height;
            }
            ContextMenuEntry::Builtin(item) => {
                let hovered = hovered_index == Some(i);
                if hovered {
                    commands.push(RenderCommand::FillRect {
                        x: x + 4.0,
                        y: cy,
                        width: width - 8.0,
                        height: item_height,
                        color: p.surface0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Icon.
                commands.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: cy + 5.0,
                    text: item.icon().to_string(),
                    font_size: 13.0,
                    color: if hovered { p.text } else { p.subtext1 },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Label.
                commands.push(RenderCommand::Text {
                    x: x + 36.0,
                    y: cy + 6.0,
                    text: item.label().to_string(),
                    font_size: 13.0,
                    color: if hovered { p.text } else { p.subtext1 },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Shortcut hint.
                if let Some(shortcut) = item.shortcut() {
                    commands.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: cy + 7.0,
                        text: shortcut.to_string(),
                        font_size: 11.0,
                        color: p.overlay0,
                        font_weight: FontWeightHint::Light,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                cy += item_height;
            }
            ContextMenuEntry::Extension {
                label,
                icon,
                shortcut,
                slow,
                submenu,
                ..
            } => {
                let hovered = hovered_index == Some(i);
                if hovered {
                    commands.push(RenderCommand::FillRect {
                        x: x + 4.0,
                        y: cy,
                        width: width - 8.0,
                        height: item_height,
                        color: p.surface0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Icon (or app icon fallback).
                let icon_text = icon.as_deref().unwrap_or("\u{1F50C}");
                commands.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: cy + 5.0,
                    text: icon_text.to_string(),
                    font_size: 13.0,
                    color: if hovered { p.accent } else { p.subtext0 },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Label (with "loading..." indicator if slow).
                let display_label = if *slow {
                    format!("{label} (loading...)")
                } else {
                    label.clone()
                };
                commands.push(RenderCommand::Text {
                    x: x + 36.0,
                    y: cy + 6.0,
                    text: display_label,
                    font_size: 13.0,
                    color: if *slow {
                        p.overlay0
                    } else if hovered {
                        p.text
                    } else {
                        p.subtext1
                    },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Shortcut hint.
                if let Some(sc) = shortcut {
                    commands.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: cy + 7.0,
                        text: sc.clone(),
                        font_size: 11.0,
                        color: p.overlay0,
                        font_weight: FontWeightHint::Light,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                // Submenu arrow.
                if !submenu.is_empty() {
                    commands.push(RenderCommand::Text {
                        x: x + width - 20.0,
                        y: cy + 5.0,
                        text: "\u{25B6}".to_string(),
                        font_size: 10.0,
                        color: p.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                cy += item_height;
            }
        }
    }

    commands
}

// ============================================================================
// Settings UI
// ============================================================================

/// Settings panel for managing context menu extensions.
pub struct ExtensionSettingsUI {
    /// Reference to the extension manager (we render from a snapshot).
    extensions_snapshot: Vec<ContextMenuExtension>,
    /// App filter: show only extensions from this app (None = show all).
    pub app_filter: Option<String>,
    /// Search text.
    pub search_text: String,
    /// Scroll offset.
    pub scroll_y: f32,
    /// Selected extension ID.
    pub selected: Option<ExtensionId>,
}

impl ExtensionSettingsUI {
    pub fn new(extensions: &[ContextMenuExtension]) -> Self {
        Self {
            extensions_snapshot: extensions.to_vec(),
            app_filter: None,
            search_text: String::new(),
            scroll_y: 0.0,
            selected: None,
        }
    }

    /// Update the snapshot from the manager.
    pub fn refresh(&mut self, extensions: &[ContextMenuExtension]) {
        self.extensions_snapshot = extensions.to_vec();
    }

    /// Filtered list of extensions.
    pub fn filtered_extensions(&self) -> Vec<&ContextMenuExtension> {
        self.extensions_snapshot
            .iter()
            .filter(|e| {
                if let Some(app) = &self.app_filter
                    && e.app_name != *app
                {
                    return false;
                }
                if !self.search_text.is_empty() {
                    let lower = self.search_text.to_lowercase();
                    if !e.label.to_lowercase().contains(&lower)
                        && !e.app_name.to_lowercase().contains(&lower)
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Render the settings panel.
    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let padding = 12.0;
        let mut cy = y + padding - self.scroll_y;

        // Title.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Context Menu Extensions".to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 32.0;

        // Search bar.
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: width - padding * 2.0,
            height: 28.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_display = if self.search_text.is_empty() {
            "Search extensions...".to_string()
        } else {
            self.search_text.clone()
        };
        commands.push(RenderCommand::Text {
            x: x + padding + 10.0,
            y: cy + 6.0,
            text: search_display,
            font_size: 12.0,
            color: if self.search_text.is_empty() {
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 40.0;

        // Extension list.
        let filtered = self.filtered_extensions();
        if filtered.is_empty() {
            commands.push(RenderCommand::Text {
                x: x + padding,
                y: cy,
                text: "No extensions registered".to_string(),
                font_size: 13.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for ext in &filtered {
                let selected = self.selected == Some(ext.id);
                let row_h = 48.0;

                // Row background.
                let row_bg = if selected { p.surface0 } else { p.mantle };
                commands.push(RenderCommand::FillRect {
                    x: x + padding,
                    y: cy,
                    width: width - padding * 2.0,
                    height: row_h,
                    color: row_bg,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Enable/disable indicator.
                let status_color = if ext.enabled { p.green } else { p.red };
                commands.push(RenderCommand::FillRect {
                    x: x + padding + 8.0,
                    y: cy + 16.0,
                    width: 8.0,
                    height: 8.0,
                    color: status_color,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Extension label.
                commands.push(RenderCommand::Text {
                    x: x + padding + 24.0,
                    y: cy + 6.0,
                    text: ext.label.clone(),
                    font_size: 13.0,
                    color: if ext.enabled { p.text } else { p.subtext0 },
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // App name.
                commands.push(RenderCommand::Text {
                    x: x + padding + 24.0,
                    y: cy + 26.0,
                    text: ext.app_name.clone(),
                    font_size: 11.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Slow indicator.
                if ext.is_slow() {
                    commands.push(RenderCommand::Text {
                        x: x + width - padding - 60.0,
                        y: cy + 6.0,
                        text: "Slow".to_string(),
                        font_size: 10.0,
                        color: p.yellow,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                // Response time.
                if ext.invocation_count > 0 {
                    commands.push(RenderCommand::Text {
                        x: x + width - padding - 60.0,
                        y: cy + 26.0,
                        text: format!("{:.0}ms avg", ext.response_time_avg_ms),
                        font_size: 10.0,
                        color: p.overlay0,
                        font_weight: FontWeightHint::Light,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                cy += row_h + 4.0;
            }
        }

        commands
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
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    fn make_mgr() -> ContextMenuExtensionManager {
        ContextMenuExtensionManager::new()
    }

    // ---- TargetKind matching ----

    #[test]
    fn target_any_matches_all() {
        let t = TargetKind::Any;
        assert!(t.matches(&ContextTarget::file("test.txt", Some("txt"))));
        assert!(t.matches(&ContextTarget::directory("/home")));
        assert!(t.matches(&ContextTarget::desktop()));
    }

    #[test]
    fn target_any_file_matches_files() {
        let t = TargetKind::AnyFile;
        assert!(t.matches(&ContextTarget::file("test.txt", Some("txt"))));
        assert!(!t.matches(&ContextTarget::directory("/home")));
        assert!(!t.matches(&ContextTarget::desktop()));
    }

    #[test]
    fn target_directory_matches_dirs() {
        let t = TargetKind::Directory;
        assert!(!t.matches(&ContextTarget::file("test.txt", Some("txt"))));
        assert!(t.matches(&ContextTarget::directory("/home")));
    }

    #[test]
    fn target_desktop_matches_desktop() {
        let t = TargetKind::DesktopBackground;
        assert!(!t.matches(&ContextTarget::file("f.txt", Some("txt"))));
        assert!(t.matches(&ContextTarget::desktop()));
    }

    #[test]
    fn target_file_extensions_case_insensitive() {
        let t = TargetKind::FileExtensions(vec!["png".to_string(), "jpg".to_string()]);
        assert!(t.matches(&ContextTarget::file("img.PNG", Some("PNG"))));
        assert!(t.matches(&ContextTarget::file("img.jpg", Some("jpg"))));
        assert!(!t.matches(&ContextTarget::file("doc.pdf", Some("pdf"))));
    }

    #[test]
    fn target_multi_select() {
        let t = TargetKind::MultipleSelection;
        let mut ctx = ContextTarget::file("a.txt", Some("txt"));
        ctx.selection_count = 3;
        assert!(t.matches(&ctx));
        ctx.selection_count = 1;
        assert!(!t.matches(&ctx));
    }

    // ---- ContextTarget constructors ----

    #[test]
    fn context_target_file() {
        let t = ContextTarget::file("/tmp/test.rs", Some("rs"));
        assert!(t.is_file);
        assert!(!t.is_directory);
        assert_eq!(t.file_extension.as_deref(), Some("rs"));
        assert_eq!(t.selection_count, 1);
    }

    #[test]
    fn context_target_directory() {
        let t = ContextTarget::directory("/home/user");
        assert!(!t.is_file);
        assert!(t.is_directory);
        assert!(t.file_extension.is_none());
    }

    #[test]
    fn context_target_desktop() {
        let t = ContextTarget::desktop();
        assert!(!t.is_file);
        assert!(!t.is_directory);
        assert!(t.is_desktop);
        assert_eq!(t.selection_count, 0);
    }

    // ---- Extension creation ----

    #[test]
    fn extension_new() {
        let e = ContextMenuExtension::new(
            1,
            "Open in VSCode",
            TargetKind::AnyFile,
            "vscode",
            "code --open",
            42,
        );
        assert_eq!(e.id, 1);
        assert_eq!(e.label, "Open in VSCode");
        assert!(e.enabled);
        assert!(e.icon.is_none());
        assert_eq!(e.position, MenuPosition::Bottom);
    }

    #[test]
    fn extension_builder_methods() {
        let e = ContextMenuExtension::new(1, "Test", TargetKind::Any, "app", "cmd", 1)
            .with_icon("\u{2702}")
            .with_position(MenuPosition::Top)
            .with_shortcut("Ctrl+T")
            .with_multi_select();
        assert_eq!(e.icon.as_deref(), Some("\u{2702}"));
        assert_eq!(e.position, MenuPosition::Top);
        assert_eq!(e.shortcut_hint.as_deref(), Some("Ctrl+T"));
        assert!(e.supports_multi_select);
    }

    #[test]
    fn extension_submenu() {
        let mut e = ContextMenuExtension::new(1, "Test", TargetKind::Any, "app", "cmd", 1);
        e.add_submenu_item(SubMenuItem::new("Sub 1", "--sub1"));
        e.add_submenu_item(SubMenuItem::new("Sub 2", "--sub2").with_icon("\u{1F4C4}"));
        assert_eq!(e.submenu.len(), 2);
        assert_eq!(e.submenu[1].icon.as_deref(), Some("\u{1F4C4}"));
    }

    // ---- Response time tracking ----

    #[test]
    fn extension_response_time_tracking() {
        let mut e = ContextMenuExtension::new(1, "Test", TargetKind::Any, "app", "cmd", 1);
        assert!(!e.is_slow());

        // Record several fast invocations.
        for _ in 0..5 {
            e.record_invocation(50.0);
        }
        assert!(!e.is_slow());

        // Record slow invocations.
        for _ in 0..20 {
            e.record_invocation(500.0);
        }
        assert!(e.is_slow());
        assert!(e.loading_timeout);
    }

    // ---- Extension matching ----

    #[test]
    fn extension_matches_file() {
        let e = ContextMenuExtension::new(1, "Test", TargetKind::AnyFile, "app", "cmd", 1);
        assert!(e.matches_context(&ContextTarget::file("t.txt", Some("txt"))));
        assert!(!e.matches_context(&ContextTarget::directory("/tmp")));
    }

    #[test]
    fn disabled_extension_doesnt_match() {
        let mut e = ContextMenuExtension::new(1, "Test", TargetKind::Any, "app", "cmd", 1);
        e.enabled = false;
        assert!(!e.matches_context(&ContextTarget::file("t.txt", Some("txt"))));
    }

    #[test]
    fn multi_select_requires_support() {
        let e = ContextMenuExtension::new(1, "Test", TargetKind::AnyFile, "app", "cmd", 1);
        let mut ctx = ContextTarget::file("t.txt", Some("txt"));
        ctx.selection_count = 3;
        assert!(!e.matches_context(&ctx)); // doesn't support multi-select

        let e2 = ContextMenuExtension::new(2, "Test2", TargetKind::AnyFile, "app", "cmd", 1)
            .with_multi_select();
        assert!(e2.matches_context(&ctx));
    }

    // ---- Manager registration ----

    #[test]
    fn register_extension() {
        let mut mgr = make_mgr();
        let id = mgr.register("Open", TargetKind::AnyFile, "test_app", "cmd", 1);
        assert!(id.is_some());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn register_duplicate_label_rejected() {
        let mut mgr = make_mgr();
        mgr.register("Open", TargetKind::AnyFile, "test_app", "cmd", 1);
        let id2 = mgr.register("Open", TargetKind::AnyFile, "test_app", "cmd2", 2);
        assert!(id2.is_none());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn register_same_label_different_app_ok() {
        let mut mgr = make_mgr();
        mgr.register("Open", TargetKind::AnyFile, "app_a", "cmd1", 1);
        let id2 = mgr.register("Open", TargetKind::AnyFile, "app_b", "cmd2", 2);
        assert!(id2.is_some());
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn register_per_app_limit() {
        let mut mgr = make_mgr();
        mgr.max_per_app = 2;
        mgr.register("A", TargetKind::Any, "app", "cmd", 1);
        mgr.register("B", TargetKind::Any, "app", "cmd", 1);
        let id3 = mgr.register("C", TargetKind::Any, "app", "cmd", 1);
        assert!(id3.is_none());
    }

    // ---- Manager unregister ----

    #[test]
    fn unregister_extension() {
        let mut mgr = make_mgr();
        let id = mgr
            .register("Test", TargetKind::Any, "app", "cmd", 1)
            .unwrap();
        assert!(mgr.unregister(id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn unregister_nonexistent() {
        let mut mgr = make_mgr();
        assert!(!mgr.unregister(999));
    }

    #[test]
    fn unregister_app() {
        let mut mgr = make_mgr();
        mgr.register("A", TargetKind::Any, "app1", "cmd", 1);
        mgr.register("B", TargetKind::Any, "app1", "cmd", 1);
        mgr.register("C", TargetKind::Any, "app2", "cmd", 1);
        let removed = mgr.unregister_app("app1");
        assert_eq!(removed, 2);
        assert_eq!(mgr.count(), 1);
    }

    // ---- Manager enable/disable ----

    #[test]
    fn set_enabled() {
        let mut mgr = make_mgr();
        let id = mgr
            .register("Test", TargetKind::Any, "app", "cmd", 1)
            .unwrap();
        mgr.set_enabled(id, false);
        assert!(!mgr.get(id).unwrap().enabled);
        assert_eq!(mgr.enabled_count(), 0);
    }

    // ---- Manager query ----

    #[test]
    fn query_matches_file() {
        let mut mgr = make_mgr();
        mgr.register("File tool", TargetKind::AnyFile, "app", "cmd", 1);
        mgr.register("Dir tool", TargetKind::Directory, "app", "cmd2", 1);
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let matches = mgr.query(&ctx);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "File tool");
    }

    #[test]
    fn query_disabled_global() {
        let mut mgr = make_mgr();
        mgr.register("Test", TargetKind::Any, "app", "cmd", 1);
        mgr.extensions_enabled = false;
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        assert!(mgr.query(&ctx).is_empty());
    }

    #[test]
    fn query_sorted_by_position() {
        let mut mgr = make_mgr();
        let _id1 = mgr
            .register("Bottom", TargetKind::Any, "app", "cmd1", 1)
            .unwrap();
        let id2 = mgr
            .register("Top", TargetKind::Any, "app2", "cmd2", 1)
            .unwrap();
        mgr.get_mut(id2).unwrap().position = MenuPosition::Top;
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let matches = mgr.query(&ctx);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].label, "Top");
        assert_eq!(matches[1].label, "Bottom");
    }

    // ---- Manager stats ----

    #[test]
    fn extensions_by_app() {
        let mut mgr = make_mgr();
        mgr.register("A", TargetKind::Any, "app1", "cmd", 1);
        mgr.register("B", TargetKind::Any, "app1", "cmd2", 1);
        mgr.register("C", TargetKind::Any, "app2", "cmd", 1);
        let by_app = mgr.extensions_by_app();
        assert_eq!(by_app.len(), 2);
        assert_eq!(by_app[0], ("app1".to_string(), 2));
        assert_eq!(by_app[1], ("app2".to_string(), 1));
    }

    #[test]
    fn slow_count() {
        let mut mgr = make_mgr();
        let id = mgr
            .register("Slow", TargetKind::Any, "app", "cmd", 1)
            .unwrap();
        for _ in 0..20 {
            mgr.get_mut(id).unwrap().record_invocation(500.0);
        }
        assert_eq!(mgr.slow_count(), 1);
    }

    // ---- build_context_menu ----

    #[test]
    fn build_menu_file_has_builtins() {
        let mgr = make_mgr();
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let menu = build_context_menu(&ctx, &mgr);
        assert!(!menu.is_empty());
        // Should contain Open, Copy, etc.
        let has_open = menu
            .iter()
            .any(|e| matches!(e, ContextMenuEntry::Builtin(BuiltinMenuItem::Open)));
        assert!(has_open);
    }

    #[test]
    fn build_menu_desktop_has_builtins() {
        let mgr = make_mgr();
        let ctx = ContextTarget::desktop();
        let menu = build_context_menu(&ctx, &mgr);
        let has_refresh = menu
            .iter()
            .any(|e| matches!(e, ContextMenuEntry::Builtin(BuiltinMenuItem::Refresh)));
        assert!(has_refresh);
    }

    #[test]
    fn build_menu_includes_extensions() {
        let mut mgr = make_mgr();
        mgr.register("Open in Editor", TargetKind::AnyFile, "editor", "edit", 1);
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let menu = build_context_menu(&ctx, &mgr);
        let has_ext = menu.iter().any(
            |e| matches!(e, ContextMenuEntry::Extension { label, .. } if label == "Open in Editor"),
        );
        assert!(has_ext);
    }

    #[test]
    fn build_menu_separator_before_extensions() {
        let mut mgr = make_mgr();
        mgr.register("Ext", TargetKind::AnyFile, "app", "cmd", 1);
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let menu = build_context_menu(&ctx, &mgr);
        // Should have at least one separator (between builtins and extensions).
        let sep_count = menu
            .iter()
            .filter(|e| matches!(e, ContextMenuEntry::Separator))
            .count();
        assert!(sep_count >= 1);
    }

    // ---- render_context_menu ----

    #[test]
    fn render_menu_not_empty() {
        let mgr = make_mgr();
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let menu = build_context_menu(&ctx, &mgr);
        let cmds = render_context_menu(&menu, &Palette::for_mode(false), 100.0, 100.0, 250.0, None);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_menu_with_hover() {
        let mgr = make_mgr();
        let ctx = ContextTarget::file("test.txt", Some("txt"));
        let menu = build_context_menu(&ctx, &mgr);
        let cmds = render_context_menu(
            &menu,
            &Palette::for_mode(false),
            100.0,
            100.0,
            250.0,
            Some(0),
        );
        // Hovered item should produce extra FillRect.
        assert!(cmds.len() > 5);
    }

    // ---- BuiltinMenuItem ----

    #[test]
    fn builtin_file_items() {
        let items = BuiltinMenuItem::file_items();
        assert!(items.contains(&BuiltinMenuItem::Open));
        assert!(items.contains(&BuiltinMenuItem::Copy));
        assert!(items.contains(&BuiltinMenuItem::Properties));
    }

    #[test]
    fn builtin_directory_items() {
        let items = BuiltinMenuItem::directory_items();
        assert!(items.contains(&BuiltinMenuItem::OpenTerminalHere));
        assert!(items.contains(&BuiltinMenuItem::Paste));
    }

    #[test]
    fn builtin_desktop_items() {
        let items = BuiltinMenuItem::desktop_items();
        assert!(items.contains(&BuiltinMenuItem::NewFolder));
        assert!(items.contains(&BuiltinMenuItem::Refresh));
    }

    #[test]
    fn builtin_labels_not_empty() {
        for item in BuiltinMenuItem::file_items() {
            assert!(!item.label().is_empty());
            assert!(!item.icon().is_empty());
        }
    }

    // ---- Settings UI ----

    #[test]
    fn settings_ui_empty() {
        let ui = ExtensionSettingsUI::new(&[]);
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_ui_with_extensions() {
        let ext = ContextMenuExtension::new(1, "Test Ext", TargetKind::Any, "test_app", "cmd", 1);
        let ui = ExtensionSettingsUI::new(&[ext]);
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 400.0);
        assert!(cmds.len() > 5);
    }

    #[test]
    fn settings_ui_filter_by_search() {
        let ext1 =
            ContextMenuExtension::new(1, "Open in Editor", TargetKind::Any, "editor", "cmd", 1);
        let ext2 = ContextMenuExtension::new(2, "Compress", TargetKind::Any, "archiver", "cmd", 1);
        let mut ui = ExtensionSettingsUI::new(&[ext1, ext2]);
        ui.search_text = "editor".to_string();
        let filtered = ui.filtered_extensions();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].label, "Open in Editor");
    }

    #[test]
    fn settings_ui_filter_by_app() {
        let ext1 = ContextMenuExtension::new(1, "A", TargetKind::Any, "app1", "cmd", 1);
        let ext2 = ContextMenuExtension::new(2, "B", TargetKind::Any, "app2", "cmd", 1);
        let mut ui = ExtensionSettingsUI::new(&[ext1, ext2]);
        ui.app_filter = Some("app1".to_string());
        let filtered = ui.filtered_extensions();
        assert_eq!(filtered.len(), 1);
    }

    // ========================================================================
    // The palette conversion
    // ========================================================================

    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;
    // Only the tests name a colour type now. Production code here reads
    // roles off the palette it was handed and never constructs one.
    use guitk::color::Color;

    const MX: f32 = 100.0;
    const MY: f32 = 100.0;
    const MW: f32 = 250.0;

    /// The accents a test may sweep without a frozen neighbour coinciding with
    /// the accented site on exactly the accent that matters. `YELLOW`, `GREEN`
    /// and `RED` are excluded because this module freezes all three.
    const SAFE_ACCENTS: [Color; 4] = [
        appearance::BLUE,
        appearance::MAUVE,
        appearance::TEAL,
        appearance::SAPPHIRE,
    ];

    /// A glyph no built-in item uses.
    ///
    /// It matters that this is distinctive: a built-in item's icon and an
    /// extension's are drawn at the same x and the same font size, so geometry
    /// cannot tell them apart and the extractor keys on the glyph instead.
    const EXT_ICON: &str = "\u{2699}";

    /// A menu that takes every branch `render_context_menu` has.
    ///
    /// The shape is not arbitrary and must not be trimmed. Every optional part
    /// of a row — a built-in's shortcut hint, an extension's shortcut hint, the
    /// icon fallback, the "loading..." label a slow extension gets, the submenu
    /// arrow — is drawn in its own colour, so an entry that omits one takes a
    /// colour site out of *every* test in this module at once. See
    /// `the_fixture_menu_takes_every_branch_the_renderer_has`, which pins it.
    ///
    /// `Open` has no shortcut and `Copy` does, so both halves of that branch
    /// are covered; `Compress` is the row the accent test hovers, and carries
    /// `EXT_ICON` so the extractor can find it; `Scan` is slow, has no icon of
    /// its own, and has a submenu.
    fn ext_menu() -> Vec<ContextMenuEntry> {
        vec![
            ContextMenuEntry::Builtin(BuiltinMenuItem::Open),
            ContextMenuEntry::Builtin(BuiltinMenuItem::Copy),
            ContextMenuEntry::Separator,
            ContextMenuEntry::Extension {
                id: 1,
                label: "Compress".to_string(),
                icon: Some(EXT_ICON.to_string()),
                shortcut: Some("Ctrl+K".to_string()),
                slow: false,
                submenu: Vec::new(),
            },
            ContextMenuEntry::Extension {
                id: 2,
                label: "Scan".to_string(),
                icon: None,
                shortcut: None,
                slow: true,
                submenu: vec![SubMenuItem::new("Quick scan", "quick")],
            },
        ]
    }

    /// The index of the extension entry, which is what `hovered_index` takes.
    fn ext_index(menu: &[ContextMenuEntry]) -> usize {
        menu.iter()
            .position(|e| matches!(e, ContextMenuEntry::Extension { .. }))
            .expect("the fixture menu has an extension entry")
    }

    /// A settings panel holding one enabled extension, one disabled one, and
    /// one slow enough to earn the "Slow" badge.
    fn settings_ui() -> ExtensionSettingsUI {
        let enabled = ContextMenuExtension::new(1, "Compress", TargetKind::Any, "archiver", "c", 1);
        let mut disabled =
            ContextMenuExtension::new(2, "Open in Editor", TargetKind::Any, "editor", "c", 1);
        disabled.enabled = false;
        let mut slow = ContextMenuExtension::new(3, "Scan", TargetKind::Any, "scanner", "c", 1);
        // Twice, because the average is exponential and one invocation is not
        // enough to carry it over the 200ms threshold from a start of zero.
        slow.record_invocation(2000.0);
        slow.record_invocation(2000.0);
        assert!(slow.is_slow(), "the slow fixture is not actually slow");
        ExtensionSettingsUI::new(&[enabled, disabled, slow])
    }

    /// Every render this module can produce, in one mode.
    fn every_render(p: &Palette) -> Vec<(Vec<RenderCommand>, String)> {
        let menu = ext_menu();
        let ext_at = ext_index(&menu);
        let mut ui_selected = settings_ui();
        ui_selected.selected = Some(1);
        let mut ui_searching = settings_ui();
        ui_searching.search_text = "compress".to_string();
        vec![
            (
                render_context_menu(&menu, p, MX, MY, MW, None),
                "menu at rest".to_string(),
            ),
            (
                render_context_menu(&menu, p, MX, MY, MW, Some(0)),
                "menu, built-in hovered".to_string(),
            ),
            (
                render_context_menu(&menu, p, MX, MY, MW, Some(ext_at)),
                "menu, extension hovered".to_string(),
            ),
            (
                settings_ui().render(p, 0.0, 0.0, 400.0),
                "settings".to_string(),
            ),
            (
                ui_selected.render(p, 0.0, 0.0, 400.0),
                "settings, row selected".to_string(),
            ),
            (
                ui_searching.render(p, 0.0, 0.0, 400.0),
                "settings, searching".to_string(),
            ),
            (
                ExtensionSettingsUI::new(&[]).render(p, 0.0, 0.0, 400.0),
                "settings, no extensions".to_string(),
            ),
        ]
    }

    fn fills(cmds: &[RenderCommand], keep: impl Fn(f32, f32, f32, f32) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if keep(*x, *y, *width, *height) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of every `Text` command whose glyph is exactly `want`.
    fn texts_saying(cmds: &[RenderCommand], want: &str) -> Vec<Color> {
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, color, .. } if text == want => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The status dot beside each row: the only 8x8 fill the panel draws.
    fn status_dots(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 8.0 && h == 8.0)
    }

    // -- The sweep -----------------------------------------------------------

    /// Nothing this module draws is outside the palette it was handed.
    ///
    /// Rendered in both modes; the light render is the one that matters, since
    /// every constant deleted here was a Mocha value and Mocha is not a subset
    /// of Latte. `p.shadow()` is black, which the sweep allows at any alpha.
    #[test]
    fn every_colour_the_context_menu_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (cmds, what) in every_render(&p) {
                assert_drawn_from(&p, &cmds, &[], &what);
            }
        }
    }

    /// Nothing is drawn that covers no pixels or is immediately painted over.
    #[test]
    fn the_menu_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (cmds, what) in every_render(&p) {
                assert_nothing_is_drawn_and_never_seen(&cmds, &what);
            }
        }
    }

    /// The fixture menu draws every branch the renderer has.
    ///
    /// Three defects escaped the sweep on this module's first proof run — a
    /// built-in's shortcut hint, a slow extension's label, and the submenu
    /// arrow. None of them was a weak assertion: the fixture simply never
    /// produced those commands, so the sweep was handed a render in which those
    /// three colours did not appear. **The sweep is only as wide as the render
    /// it is given**, which makes fixture coverage a property the whole module
    /// rests on and nothing else was checking.
    ///
    /// So it is checked here, and checked against the *render* rather than
    /// against the fixture's shape, because what matters is that the commands
    /// come out — a branch can stop drawing without the entry that feeds it
    /// changing at all. The two `shortcut()` assertions are the exception, and
    /// deliberately so: whether a built-in has a shortcut is a fact about the
    /// item, and both arms of that `if let` have to be taken by *some* row.
    #[test]
    fn the_fixture_menu_takes_every_branch_the_renderer_has() {
        assert!(
            BuiltinMenuItem::Open.shortcut().is_none(),
            "the fixture no longer covers a built-in without a shortcut"
        );
        assert!(
            BuiltinMenuItem::Copy.shortcut().is_some(),
            "the fixture no longer covers a built-in with a shortcut"
        );

        let p = Palette::for_mode(false);
        let menu = ext_menu();
        let cmds = render_context_menu(&menu, &p, MX, MY, MW, None);

        assert!(
            cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. })),
            "no separator is drawn"
        );
        for (glyph, what) in [
            ("Ctrl+C", "a built-in's shortcut hint"),
            ("Ctrl+K", "an extension's shortcut hint"),
            (
                "\u{1F50C}",
                "the icon an extension without one falls back to",
            ),
            ("Scan (loading...)", "a slow extension's label"),
            ("\u{25B6}", "the submenu arrow"),
        ] {
            assert_eq!(
                texts_saying(&cmds, glyph).len(),
                1,
                "{what} is not drawn, so no test in this module checks its colour"
            );
        }
    }

    // -- The one accent site -------------------------------------------------

    /// Pointing at an extension item marks its icon with the accent.
    ///
    /// This is the module's only accent site, and it is one because the colour
    /// appears in exactly one state — hover — and a colour that appears only in
    /// one state marks that state. Hover is a position, which is what the
    /// accent is for. Asserting *equality with the accent* rather than
    /// inequality with the old hardcoded blue is deliberate: an assertion whose
    /// truth depends on which accent the user picked is a bug in the test.
    #[test]
    fn a_hovered_extensions_icon_follows_the_accent() {
        let menu = ext_menu();
        let ext_at = ext_index(&menu);
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let hovered = render_context_menu(&menu, &p, MX, MY, MW, Some(ext_at));
                let icon = texts_saying(&hovered, EXT_ICON);
                assert_eq!(icon.len(), 1, "expected one extension icon (light={light})");
                assert_eq!(
                    icon[0], p.accent,
                    "a hovered extension's icon is not the accent (light={light})"
                );

                // And it is the accent only while hovered: at rest it is the
                // recessed body colour, so the accent is carrying the state
                // rather than decorating the row.
                let at_rest = render_context_menu(&menu, &p, MX, MY, MW, None);
                let idle = texts_saying(&at_rest, EXT_ICON);
                assert_eq!(idle.len(), 1, "expected one extension icon at rest");
                assert_eq!(
                    idle[0], p.subtext0,
                    "an unhovered extension's icon is not p.subtext0 (light={light})"
                );
            }
        }
    }

    // -- The frozen reports --------------------------------------------------

    /// Nothing that reports a fact about an extension follows the accent.
    ///
    /// Two kinds of site: the dot that says whether an extension is enabled,
    /// and the "Slow" badge. Each answers a question *about the extension*, and
    /// an answer that changes because the user picked a different accent has
    /// stopped being an answer. The sweep cannot see this — every one of these
    /// is a palette role either way — so it needs its own assertion.
    #[test]
    fn nothing_that_reports_a_fact_follows_the_accent() {
        for light in [false, true] {
            let baseline = {
                let p = Palette::for_mode(light);
                let cmds = settings_ui().render(&p, 0.0, 0.0, 400.0);
                (status_dots(&cmds), texts_saying(&cmds, "Slow"))
            };
            assert_eq!(baseline.0.len(), 3, "expected three status dots");
            assert_eq!(baseline.1.len(), 1, "expected one Slow badge");

            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = settings_ui().render(&p, 0.0, 0.0, 400.0);
                assert_eq!(
                    status_dots(&cmds),
                    baseline.0,
                    "a status dot moved with the accent (light={light})"
                );
                assert_eq!(
                    texts_saying(&cmds, "Slow"),
                    baseline.1,
                    "the Slow badge moved with the accent (light={light})"
                );
            }
        }
    }

    /// An enabled extension and a disabled one never look alike.
    ///
    /// The dot is the only thing that tells them apart at a glance, so the two
    /// colours must stay distinct in both modes — and each must be the role it
    /// claims, not merely some member of the palette.
    #[test]
    fn an_enabled_extension_and_a_disabled_one_never_look_alike() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = settings_ui().render(&p, 0.0, 0.0, 400.0);
            let dots = status_dots(&cmds);
            assert_eq!(dots.len(), 3, "expected three status dots");
            // Rows are drawn in fixture order: enabled, disabled, slow-enabled.
            assert_eq!(dots[0], p.green, "an enabled dot is not p.green");
            assert_eq!(dots[1], p.red, "a disabled dot is not p.red");
            assert_ne!(
                dots[0], dots[1],
                "enabled and disabled are the same colour (light={light})"
            );
        }
    }

    // -- The surfaces --------------------------------------------------------

    /// The menu's own surfaces are the palette's, not merely *a* member of it.
    ///
    /// The sweep proves membership; this proves identity, which is strictly
    /// stronger and, unlike membership, can fail in dark mode too.
    #[test]
    fn the_menus_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);

            let menu = ext_menu();
            let cmds = render_context_menu(&menu, &p, MX, MY, MW, None);

            let background = fills(&cmds, |_, _, w, _| w == MW);
            assert_eq!(background.len(), 1, "expected one menu background");
            assert_eq!(background[0], p.base, "the menu is not p.base");

            let border: Vec<Color> = cmds
                .iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(border.len(), 1, "expected one menu border");
            assert_eq!(border[0], p.surface1, "the menu border is not p.surface1");

            // Pointing at a row lifts it one surface step above the menu.
            let hovered = render_context_menu(&menu, &p, MX, MY, MW, Some(0));
            let highlight = fills(&hovered, |_, _, w, _| w == MW - 8.0);
            assert_eq!(highlight.len(), 1, "expected one hovered row highlight");
            assert_eq!(
                highlight[0], p.surface0,
                "the hovered row is not p.surface0 (light={light})"
            );
            assert_ne!(
                highlight[0], background[0],
                "the hovered row is the same colour as the menu under it, so \
                 pointing at a row says nothing (light={light})"
            );

            // A selected settings row does the same over p.mantle.
            let mut ui = settings_ui();
            ui.selected = Some(1);
            let rows = fills(&ui.render(&p, 0.0, 0.0, 400.0), |_, _, _, h| h == 48.0);
            assert_eq!(rows.len(), 3, "expected three settings rows");
            assert_eq!(rows[0], p.surface0, "the selected row is not p.surface0");
            assert_eq!(rows[1], p.mantle, "an unselected row is not p.mantle");
        }
    }

    // -- The shadow ----------------------------------------------------------

    /// The menu casts the shared popup shadow, at the shared depth.
    ///
    /// It was `rgba(0, 0, 0, 80)`, chosen alone; `Palette::shadow()` is the one
    /// shadow for every popup that sits on top of a window. The sweep allows
    /// black at *any* alpha — it has to, since a shadow is not a role — so
    /// nothing but this assertion would notice the depth drifting back.
    #[test]
    fn the_menu_casts_the_shared_popup_shadow() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let menu = ext_menu();
            let shadows: Vec<Color> = render_context_menu(&menu, &p, MX, MY, MW, None)
                .iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::BoxShadow { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(shadows.len(), 1, "expected one menu shadow");
            assert_eq!(
                shadows[0],
                p.shadow(),
                "the menu's shadow is not the shared popup shadow (light={light})"
            );
            assert!(
                shadows[0].a < 255,
                "the menu's shadow is opaque, so it is no longer a shadow"
            );
        }
    }
}
