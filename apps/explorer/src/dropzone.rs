//! Drop zone system for drag-and-drop in the file explorer.
//!
//! Handles drops onto empty space (copy/move to current directory), onto folder
//! entries (copy/move into that folder), and onto sidebar items. Integrates with
//! the toolkit's DnD infrastructure and the explorer's `fileops` module.
//!
//! # Architecture
//!
//! Each frame the file list and sidebar rendering code calls
//! [`DropZoneManager::register_file_row`] and
//! [`DropZoneManager::register_sidebar_item`] to describe the current on-screen
//! layout. When the user drags files over the explorer, [`DropZoneManager::find_zone`]
//! hit-tests registered zones and [`DropZoneManager::determine_operation`]
//! computes the correct Copy/Move/Link based on source/target drives and
//! modifier keys.
//!
//! The caller is `ExplorerState`: `render` rebuilds the zones as it draws,
//! `drag_over` moves the hover, and `drop_at` turns the verdict into a
//! `fileops` plan. This paragraph is not decoration — the module carried the
//! architecture note above while nothing at all called it, and the note read
//! exactly the same either way.
//!
//! Note there is no `#![allow(dead_code)]` here. There was, and it hid the
//! fact that *every* item in the file was unused; the module now has a caller
//! for each one, and the absence of the blanket is what keeps that true.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::theme::with_alpha;

use std::path::{Path, PathBuf};

// ============================================================================
// Rect helper
// ============================================================================

/// Axis-aligned rectangle used for zone bounding boxes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Create a new rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns `true` if the point `(px, py)` lies inside this rectangle.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

// ============================================================================
// Drop zone enum
// ============================================================================

/// Identifies the zone the pointer is currently hovering over.
#[derive(Clone, Debug, PartialEq)]
pub enum DropZone {
    /// Over empty space in the file list -- drop into the current directory.
    CurrentDirectory,
    /// Over a folder row in the file list.
    Folder { path: PathBuf, rect: Rect },
    /// Over a sidebar entry.
    Sidebar { path: PathBuf, rect: Rect },
    /// Not over any valid drop zone.
    None,
}

// ============================================================================
// Drop operation
// ============================================================================

/// The operation that will be performed on drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropOperation {
    Copy,
    Move,
    Link,
    None,
}

// ============================================================================
// Modifier keys snapshot
// ============================================================================

/// Modifier keys held during a drag. Used to override the default operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DragModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

// ============================================================================
// Drop result / conflict info
// ============================================================================

/// Result of validating a drop before executing it.
#[derive(Clone, Debug)]
pub struct DropResult {
    /// The operation that will be performed.
    pub operation: DropOperation,
    /// Target directory for the operation.
    pub target_dir: PathBuf,
    /// Source files to operate on.
    pub sources: Vec<PathBuf>,
    /// Paths that already exist in the target and would conflict.
    pub conflicts: Vec<PathBuf>,
    /// Whether the drop is valid (no nested-drop violations, etc.).
    pub valid: bool,
    /// Human-readable reason when `valid` is `false`.
    pub invalid_reason: Option<String>,
}

// ============================================================================
// Drop zone events
// ============================================================================

/// Events produced as the drag moves across zones.
#[derive(Clone, Debug)]
pub enum DropZoneEvent {
    /// Drag entered a new zone.
    DragEnter {
        zone: DropZone,
        operation: DropOperation,
    },
    /// Drag moved within a zone (operation may have changed due to modifiers).
    DragOver {
        zone: DropZone,
        operation: DropOperation,
    },
    /// Drag left all valid zones.
    DragLeave,
    /// Files were dropped.
    Drop {
        zone: DropZone,
        sources: Vec<PathBuf>,
        operation: DropOperation,
    },
}

// ============================================================================
// Registered zone entry (internal)
// ============================================================================

/// A single registered zone -- either a file row or a sidebar item.
#[derive(Clone, Debug)]
struct RegisteredZone {
    path: PathBuf,
    rect: Rect,
    is_dir: bool,
    kind: ZoneKind,
    /// For a [`ZoneKind::FileRow`], its index in the listing; `None` for a
    /// sidebar item, which is a place rather than a row.
    ///
    /// Kept so that [`DropZoneManager::find_file_row`] can answer "which entry
    /// did the user click?" from the rectangles the renderer actually emitted.
    /// The alternative is a second copy of the icon-grid and list-row layout
    /// arithmetic living in the click handler, which is two computations of one
    /// rectangle to keep in agreement — and when they disagree the user clicks
    /// one file and opens another.
    index: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoneKind {
    FileRow,
    SidebarItem,
}

// ============================================================================
// DropZoneManager
// ============================================================================

/// Manages registered drop zones, hit-testing, and visual feedback.
///
/// Zones are rebuilt every frame: the rendering code calls [`clear_zones`] at
/// the start of a frame, then [`register_file_row`] / [`register_sidebar_item`]
/// for each visible element. During a drag the manager is queried with
/// [`find_zone`] and [`determine_operation`].
pub struct DropZoneManager {
    /// Registered zones for the current frame.
    zones: Vec<RegisteredZone>,
    /// Bounding rectangle of the entire file list area (for "current directory"
    /// hits on empty space).
    list_area: Option<Rect>,
    /// Currently hovered zone (for visual feedback tracking).
    current_hover: DropZone,
    /// Path of the current directory (needed for nested-drop checks and
    /// same-device detection).
    current_dir: PathBuf,
}

impl DropZoneManager {
    /// Create a new manager for the given current directory.
    pub fn new(current_dir: PathBuf) -> Self {
        Self {
            zones: Vec::new(),
            list_area: None,
            current_hover: DropZone::None,
            current_dir,
        }
    }

    /// Update the current directory (e.g. after navigation).
    pub fn set_current_dir(&mut self, path: PathBuf) {
        self.current_dir = path;
    }

    /// Return a reference to the current directory.
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    // ------------------------------------------------------------------
    // Zone registration (called each frame during render)
    // ------------------------------------------------------------------

    /// Remove all registered zones. Call at the start of each frame before
    /// re-registering.
    pub fn clear_zones(&mut self) {
        self.zones.clear();
        self.list_area = None;
    }

    /// Set the bounding rectangle of the entire file-list area. Hits inside
    /// this area that do not land on a folder row produce
    /// [`DropZone::CurrentDirectory`].
    pub fn set_list_area(&mut self, rect: Rect) {
        self.list_area = Some(rect);
    }

    /// The file-list area registered for this frame, if one has been.
    ///
    /// [`render_drop_feedback`] needs it to tint the pane for a
    /// [`DropZone::CurrentDirectory`] hover, and it should read the rectangle
    /// the renderer actually registered rather than recompute the layout —
    /// two computations of the same rectangle are two rectangles to keep in
    /// agreement.
    pub fn list_area(&self) -> Option<Rect> {
        self.list_area
    }

    /// Register a visible file row.
    ///
    /// * `index` -- row index in the file list, as [`find_file_row`] reports it
    ///   back.
    /// * `path` -- absolute path of the file/folder.
    /// * `rect` -- bounding rectangle of the row.
    /// * `is_dir` -- whether the entry is a directory.
    ///
    /// [`find_file_row`]: Self::find_file_row
    pub fn register_file_row(&mut self, index: usize, path: &Path, rect: Rect, is_dir: bool) {
        self.zones.push(RegisteredZone {
            path: path.to_path_buf(),
            rect,
            is_dir,
            kind: ZoneKind::FileRow,
            index: Some(index),
        });
    }

    /// Register a sidebar item.
    pub fn register_sidebar_item(&mut self, path: &Path, rect: Rect) {
        self.zones.push(RegisteredZone {
            path: path.to_path_buf(),
            rect,
            is_dir: true, // sidebar items are always directories
            kind: ZoneKind::SidebarItem,
            index: None,
        });
    }

    // ------------------------------------------------------------------
    // Hit testing
    // ------------------------------------------------------------------

    /// Find which drop zone the pointer at `(x, y)` is over.
    ///
    /// Precedence: file rows and sidebar items are tested first (front to
    /// back, i.e. later registrations win). If no specific zone matches but
    /// the point is inside the list area, [`DropZone::CurrentDirectory`] is
    /// returned. Otherwise [`DropZone::None`].
    pub fn find_zone(&self, x: f32, y: f32) -> DropZone {
        // Check registered zones in reverse order (last registered = on top).
        for zone in self.zones.iter().rev() {
            if zone.rect.contains(x, y) {
                return match zone.kind {
                    ZoneKind::FileRow => {
                        if zone.is_dir {
                            DropZone::Folder {
                                path: zone.path.clone(),
                                rect: zone.rect,
                            }
                        } else {
                            // Hovering over a file row falls through to "current
                            // directory" -- you can't drop into a file.
                            DropZone::CurrentDirectory
                        }
                    }
                    ZoneKind::SidebarItem => DropZone::Sidebar {
                        path: zone.path.clone(),
                        rect: zone.rect,
                    },
                };
            }
        }

        // Empty space inside the list area counts as current directory.
        if let Some(ref list) = self.list_area
            && list.contains(x, y)
        {
            return DropZone::CurrentDirectory;
        }

        DropZone::None
    }

    /// Which listing row the pointer at `(x, y)` is over, if any.
    ///
    /// **This is how a click finds the file it landed on.** The rectangles were
    /// put here by the same code that drew the rows, so the row the user sees
    /// under the pointer is the row this returns — including in the icon view,
    /// whose cells are a grid whose column count depends on the pane width, and
    /// after a scroll or a resize that moved every row. Recomputing the layout
    /// in the click handler instead would be a second copy of that arithmetic,
    /// and the first time the two disagreed the user would click one file and
    /// open another.
    ///
    /// Reverse order, matching [`Self::find_zone`]: whatever was registered
    /// last was drawn last and so is on top.
    ///
    /// Unlike `find_zone` this does *not* fall through to the list area — a
    /// click on empty space is a click on no file, which is what clears a
    /// selection rather than what extends one.
    #[must_use]
    pub fn find_file_row(&self, x: f32, y: f32) -> Option<usize> {
        self.zones
            .iter()
            .rev()
            .find(|z| z.kind == ZoneKind::FileRow && z.rect.contains(x, y))
            .and_then(|z| z.index)
    }

    /// Which sidebar place the pointer at `(x, y)` is over, if any.
    ///
    /// The navigation counterpart of [`Self::find_file_row`], and registered by
    /// the same renderer for the same reason.
    #[must_use]
    pub fn find_sidebar_item(&self, x: f32, y: f32) -> Option<&Path> {
        self.zones
            .iter()
            .rev()
            .find(|z| z.kind == ZoneKind::SidebarItem && z.rect.contains(x, y))
            .map(|z| z.path.as_path())
    }

    // ------------------------------------------------------------------
    // Hover tracking
    // ------------------------------------------------------------------

    /// Update the hover zone and return the appropriate event. Call on every
    /// mouse-move during a drag.
    pub fn update_hover(
        &mut self,
        x: f32,
        y: f32,
        modifiers: DragModifiers,
        sources: &[PathBuf],
    ) -> Option<DropZoneEvent> {
        let new_zone = self.find_zone(x, y);
        let operation = self.determine_operation(sources, &new_zone, modifiers);

        if new_zone != self.current_hover {
            if new_zone == DropZone::None {
                self.current_hover = DropZone::None;
                Some(DropZoneEvent::DragLeave)
            } else {
                self.current_hover = new_zone.clone();
                Some(DropZoneEvent::DragEnter {
                    zone: new_zone,
                    operation,
                })
            }
        } else if new_zone != DropZone::None {
            Some(DropZoneEvent::DragOver {
                zone: new_zone,
                operation,
            })
        } else {
            Option::None
        }
    }

    /// Clear the current hover state (call when drag ends or is cancelled).
    pub fn clear_hover(&mut self) {
        self.current_hover = DropZone::None;
    }

    /// Return the currently hovered zone.
    pub fn current_hover(&self) -> &DropZone {
        &self.current_hover
    }

    // ------------------------------------------------------------------
    // Operation determination
    // ------------------------------------------------------------------

    /// Decide the operation (Copy/Move/Link) for a drop of `sources` onto
    /// `zone`, given the held modifier keys.
    ///
    /// Rules:
    /// * Ctrl held -> always Copy
    /// * Shift held -> always Move
    /// * Alt held -> always Link
    /// * Otherwise: same root component -> Move, different -> Copy.
    pub fn determine_operation(
        &self,
        sources: &[PathBuf],
        zone: &DropZone,
        modifiers: DragModifiers,
    ) -> DropOperation {
        // Explicit modifier overrides.
        if modifiers.ctrl {
            return DropOperation::Copy;
        }
        if modifiers.shift {
            return DropOperation::Move;
        }
        if modifiers.alt {
            return DropOperation::Link;
        }

        let target: &Path = match zone {
            DropZone::CurrentDirectory => &self.current_dir,
            DropZone::Folder { path, .. } | DropZone::Sidebar { path, .. } => path,
            DropZone::None => return DropOperation::None,
        };

        // Default: same drive -> Move, different drive -> Copy.
        if let Some(first_source) = sources.first() {
            default_drop_operation(crate::drives::same_drive(first_source, target))
        } else {
            DropOperation::None
        }
    }

    // ------------------------------------------------------------------
    // Drop handling
    // ------------------------------------------------------------------

    /// Handle a drop at `(x, y)` of the given `sources`.
    ///
    /// Validates the drop (checks for nested-drop violations and conflicts),
    /// returning a [`DropResult`] describing what would happen. The caller is
    /// responsible for executing the operation (e.g. via `fileops`).
    pub fn handle_drop(
        &self,
        x: f32,
        y: f32,
        sources: &[PathBuf],
        modifiers: DragModifiers,
    ) -> DropResult {
        let zone = self.find_zone(x, y);
        let operation = self.determine_operation(sources, &zone, modifiers);

        let target_dir = match &zone {
            DropZone::CurrentDirectory => self.current_dir.clone(),
            DropZone::Folder { path, .. } | DropZone::Sidebar { path, .. } => PathBuf::from(path),
            DropZone::None => {
                return DropResult {
                    operation: DropOperation::None,
                    target_dir: PathBuf::new(),
                    sources: Vec::new(),
                    conflicts: Vec::new(),
                    valid: false,
                    invalid_reason: Some("Not over a valid drop target".to_string()),
                };
            }
        };

        let source_paths: Vec<PathBuf> = sources.to_vec();

        // Nested-drop check: can't drop a folder into itself or a descendant.
        if let Some(reason) = check_nested_drop(&source_paths, &target_dir) {
            return DropResult {
                operation,
                target_dir,
                sources: source_paths,
                conflicts: Vec::new(),
                valid: false,
                invalid_reason: Some(reason),
            };
        }

        // Conflict detection: check which sources already exist in target.
        let conflicts = detect_conflicts(&source_paths, &target_dir);

        DropResult {
            operation,
            target_dir,
            sources: source_paths,
            conflicts,
            valid: true,
            invalid_reason: None,
        }
    }
}

// ============================================================================
// Path helpers
// ============================================================================

/// What a plain drag does, given whether the two ends share a drive.
///
/// The convention every desktop uses: a drag within one drive moves, a drag
/// across drives copies. `Ctrl` overrides it, which is why this is only the
/// default.
///
/// **`None` means Copy.** A drive this machine cannot name is not a reason to
/// delete the user's file: a needless copy is a duplicate they can remove,
/// while a needless move off a card reader is a photo that is no longer on the
/// card. The cost of the two mistakes is not symmetric, so the unknown case is
/// resolved towards the one that is recoverable. (`OperationQueue` reads the
/// same `None` the other way round, and says so at its own call site -- see
/// `crate::drives`.)
fn default_drop_operation(same_drive: Option<bool>) -> DropOperation {
    if same_drive == Some(true) {
        DropOperation::Move
    } else {
        DropOperation::Copy
    }
}

/// Check whether dropping `sources` into `target_dir` would create a nested
/// drop (folder dropped into itself or one of its descendants).
///
/// Returns `Some(reason)` if the drop is invalid, `None` if it is fine.
fn check_nested_drop(sources: &[PathBuf], target_dir: &Path) -> Option<String> {
    // Compared after resolving symlinks and `..`, because `PathBuf::starts_with`
    // is a *textual* component test: a target that only reaches inside the
    // source by way of a symlink looks unrelated to it, and letting that drop
    // through makes `fileops::copy_tree` write into the directory it is still
    // walking — which does not terminate, and fills the disk trying.
    //
    // A path that cannot be canonicalised (it may not exist yet, or be on a
    // device that refuses) falls back to its literal form: that is exactly the
    // old behaviour, so the check never becomes weaker than it was.
    let real_target = canonical_or_literal(target_dir);
    for src in sources {
        let real_src = canonical_or_literal(src);

        // Exact self-drop: can't drop /foo into /foo.
        if real_src == real_target {
            return Some(format!("Cannot drop '{}' into itself", src.display()));
        }

        // Ancestor check: can't drop /foo into /foo/bar/baz.
        if real_target.starts_with(&real_src) {
            return Some(format!(
                "Cannot drop '{}' into its own subdirectory '{}'",
                src.display(),
                target_dir.display()
            ));
        }
    }
    None
}

/// The path with symlinks and `.`/`..` resolved, or the path itself if the
/// filesystem cannot answer.
fn canonical_or_literal(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Check which source file names already exist in `target_dir`.
fn detect_conflicts(sources: &[PathBuf], target_dir: &Path) -> Vec<PathBuf> {
    let mut conflicts = Vec::new();
    for src in sources {
        if let Some(name) = src.file_name() {
            let dest = target_dir.join(name);
            if dest.exists() {
                conflicts.push(dest);
            }
        }
    }
    conflicts
}

// ============================================================================
// Visual feedback rendering
// ============================================================================

/// Colours used for drop zone feedback.
struct FeedbackColors {
    highlight: Color,
    underline: Color,
    invalid: Color,
    invalid_underline: Color,
    label_bg: Color,
    label_fg: Color,
}

impl FeedbackColors {
    fn new(p: &Palette) -> Self {
        Self {
            // The accent at two weights: a wash over the zone, a firm line
            // under the row. The fixed blue they used to be was the old
            // accent, frozen.
            highlight: with_alpha(p.accent, 40),
            underline: with_alpha(p.accent, 180),
            // Refusal is `red` for the reason every categorical colour is a
            // role: it has to read as refusal under any accent, including a
            // red one.
            invalid: with_alpha(p.red, 50),
            invalid_underline: with_alpha(p.red, 180),
            // Deliberately NOT a palette surface. This chip is drawn over
            // whatever is being dragged across -- icons, thumbnails, someone's
            // photographs -- so its ground is unknown, and a themed surface
            // would be pale-on-pale half the time. A near-black scrim with
            // white on it is the answer subtitles reach, and `palette_check`
            // already exempts black at any alpha for this class.
            label_bg: Color::rgba(0, 0, 0, 200),
            label_fg: Color::WHITE,
        }
    }
}

/// Build a human-readable label like "Copy to Documents" or "Move to ~/Projects".
fn operation_label(operation: DropOperation, zone: &DropZone) -> String {
    let verb = match operation {
        DropOperation::Copy => "Copy to",
        DropOperation::Move => "Move to",
        DropOperation::Link => "Link in",
        DropOperation::None => "Cannot drop in",
    };

    let target = match zone {
        DropZone::CurrentDirectory => "current folder".to_string(),
        DropZone::Folder { path, .. } | DropZone::Sidebar { path, .. } => {
            // Show just the last component for brevity. This is the one place
            // a lossy rendering is right rather than wrong: the string is drawn
            // for a human and never used to reach the file, so an undecodable
            // byte should become U+FFFD on screen instead of hiding the label.
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .to_string()
        }
        DropZone::None => return String::new(),
    };

    format!("{verb} {target}")
}

/// Render visual feedback for the current drop zone.
///
/// Returns a list of [`RenderCommand`]s that should be drawn on top of the
/// normal explorer content.
///
/// * `zone` -- the zone the pointer is hovering over.
/// * `operation` -- the operation that would be performed.
/// * `drag_x`, `drag_y` -- current pointer position (for the label).
/// * `list_area` -- the file-list bounding rectangle (for CurrentDirectory
///   overlay).
/// * `valid` -- whether the drop is valid (false shows red feedback).
pub fn render_drop_feedback(
    zone: &DropZone,
    operation: DropOperation,
    drag_x: f32,
    drag_y: f32,
    list_area: Option<Rect>,
    valid: bool,
    p: &Palette,
) -> Vec<RenderCommand> {
    let mut cmds: Vec<RenderCommand> = Vec::new();
    let c = FeedbackColors::new(p);

    let (highlight, underline) = if valid {
        (c.highlight, c.underline)
    } else {
        (c.invalid, c.invalid_underline)
    };

    match zone {
        DropZone::CurrentDirectory => {
            // Tint the entire list area.
            if let Some(area) = list_area {
                cmds.push(RenderCommand::FillRect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: area.height,
                    color: highlight,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        DropZone::Folder { rect, .. } => {
            // Overlay on the folder row.
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color: highlight,
                corner_radii: CornerRadii::ZERO,
            });
            // Underline at the bottom of the row.
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y + rect.height - 2.0,
                width: rect.width,
                height: 2.0,
                color: underline,
                corner_radii: CornerRadii::ZERO,
            });
        }
        DropZone::Sidebar { rect, .. } => {
            // Highlight the sidebar item.
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color: highlight,
                corner_radii: CornerRadii::ZERO,
            });
            // Underline.
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y + rect.height - 2.0,
                width: rect.width,
                height: 2.0,
                color: underline,
                corner_radii: CornerRadii::ZERO,
            });
        }
        DropZone::None => {
            return cmds;
        }
    }

    // Operation label near the cursor.
    let label = operation_label(operation, zone);
    if !label.is_empty() {
        let label_x = drag_x + 16.0;
        let label_y = drag_y + 16.0;
        let pill_width = text::padded_width(&label, 8.0, 12.0, FontWeightHint::Regular);
        let label_height = 22.0;

        // Background pill.
        cmds.push(RenderCommand::FillRect {
            x: label_x,
            y: label_y,
            width: pill_width,
            height: label_height,
            color: c.label_bg,
            corner_radii: CornerRadii::all(4.0),
        });

        // Text.
        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: label_y + 4.0,
            text: label,
            color: c.label_fg,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use scratchdir::ScratchDir;
    use std::fs;

    // ------------------------------------------------------------------
    // Rect hit testing
    // ------------------------------------------------------------------

    #[test]
    fn rect_contains_inside() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(10.0, 20.0));
        assert!(r.contains(50.0, 40.0));
        assert!(r.contains(109.9, 69.9));
    }

    #[test]
    fn rect_contains_outside() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(!r.contains(9.9, 20.0));
        assert!(!r.contains(10.0, 19.9));
        assert!(!r.contains(110.0, 40.0));
        assert!(!r.contains(50.0, 70.0));
    }

    // ------------------------------------------------------------------
    // Zone hit testing (empty space, folder row, sidebar)
    // ------------------------------------------------------------------

    #[test]
    fn find_zone_empty_space() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        // No file rows registered -- hit in list area = CurrentDirectory.
        assert_eq!(mgr.find_zone(400.0, 200.0), DropZone::CurrentDirectory);
    }

    #[test]
    fn find_zone_folder_row() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/Documents"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        let zone = mgr.find_zone(400.0, 90.0);
        assert!(
            matches!(zone, DropZone::Folder { ref path, .. } if path == "/home/user/Documents")
        );
    }

    #[test]
    fn find_zone_file_row_falls_through() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        // Register a file (not a directory).
        mgr.register_file_row(
            0,
            Path::new("/home/user/readme.txt"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            false,
        );

        // Hovering over a file row should give CurrentDirectory.
        assert_eq!(mgr.find_zone(400.0, 90.0), DropZone::CurrentDirectory);
    }

    #[test]
    fn find_zone_sidebar() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.register_sidebar_item(Path::new("/tmp"), Rect::new(0.0, 120.0, 200.0, 24.0));

        let zone = mgr.find_zone(100.0, 130.0);
        assert!(matches!(zone, DropZone::Sidebar { ref path, .. } if path == "/tmp"));
    }

    #[test]
    fn find_zone_outside_everything() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        // No list area, no zones registered.
        assert_eq!(mgr.find_zone(500.0, 500.0), DropZone::None);
    }

    #[test]
    fn find_zone_later_registration_wins() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        // Two overlapping zones -- second one should win.
        mgr.register_sidebar_item(Path::new("/var"), Rect::new(0.0, 100.0, 200.0, 30.0));
        mgr.register_sidebar_item(Path::new("/tmp"), Rect::new(0.0, 100.0, 200.0, 30.0));

        let zone = mgr.find_zone(100.0, 115.0);
        assert!(matches!(zone, DropZone::Sidebar { ref path, .. } if path == "/tmp"));
    }

    // ------------------------------------------------------------------
    // Operation determination (same/different drive, modifiers)
    // ------------------------------------------------------------------

    #[test]
    fn operation_same_drive_default_is_move() {
        // Real paths, not `/home/user`: the drive a path is on is a fact about
        // the machine, and a path that does not exist has no drive to be on.
        // This test used to pass against a *made-up* path because the check it
        // rested on compared first components and never touched the disk --
        // which is exactly the bug. See `crate::drives`.
        let scratch = ScratchDir::new("dropzone_same_drive");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("file.txt"), "x").unwrap();

        let mgr = DropZoneManager::new(root.clone());
        let op = mgr.determine_operation(
            &[root.join("file.txt")],
            &DropZone::CurrentDirectory,
            DragModifiers::default(),
        );
        assert_eq!(op, DropOperation::Move);
    }

    #[test]
    fn operation_ctrl_forces_copy() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &[PathBuf::from("/home/user/file.txt")],
            &DropZone::CurrentDirectory,
            DragModifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(op, DropOperation::Copy);
    }

    #[test]
    fn operation_shift_forces_move() {
        let mgr = DropZoneManager::new(PathBuf::from("/mnt/usb"));
        // Different device -- default would be Copy, but Shift overrides.
        let op = mgr.determine_operation(
            &[PathBuf::from("/home/user/file.txt")],
            &DropZone::CurrentDirectory,
            DragModifiers {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(op, DropOperation::Move);
    }

    #[test]
    fn operation_alt_forces_link() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &[PathBuf::from("/home/user/file.txt")],
            &DropZone::CurrentDirectory,
            DragModifiers {
                alt: true,
                ..Default::default()
            },
        );
        assert_eq!(op, DropOperation::Link);
    }

    #[test]
    fn operation_none_zone_returns_none() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &[PathBuf::from("/home/user/file.txt")],
            &DropZone::None,
            DragModifiers::default(),
        );
        assert_eq!(op, DropOperation::None);
    }

    #[test]
    fn operation_no_sources_returns_none() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op =
            mgr.determine_operation(&[], &DropZone::CurrentDirectory, DragModifiers::default());
        assert_eq!(op, DropOperation::None);
    }

    #[test]
    fn operation_folder_target() {
        let scratch = ScratchDir::new("dropzone_folder_target");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("file.txt"), "x").unwrap();
        fs::create_dir(root.join("Documents")).unwrap();

        let mgr = DropZoneManager::new(root.clone());
        let op = mgr.determine_operation(
            &[root.join("file.txt")],
            &DropZone::Folder {
                path: root.join("Documents"),
                rect: Rect::new(0.0, 0.0, 100.0, 22.0),
            },
            DragModifiers::default(),
        );
        // One drive, so a drag inside it moves.
        assert_eq!(op, DropOperation::Move);
    }

    // ------------------------------------------------------------------
    // Nested drop prevention
    // ------------------------------------------------------------------

    #[test]
    fn nested_drop_self() {
        let sources = vec![PathBuf::from("/home/user/Documents")];
        let target = Path::new("/home/user/Documents");
        let result = check_nested_drop(&sources, target);
        assert!(result.is_some());
        assert!(result.as_ref().is_some_and(|r| r.contains("into itself")));
    }

    #[test]
    fn nested_drop_parent_into_child() {
        let sources = vec![PathBuf::from("/home/user")];
        let target = Path::new("/home/user/Documents/sub");
        let result = check_nested_drop(&sources, target);
        assert!(result.is_some());
        assert!(result.as_ref().is_some_and(|r| r.contains("subdirectory")));
    }

    #[test]
    fn nested_drop_valid() {
        let sources = vec![PathBuf::from("/home/user/file.txt")];
        let target = Path::new("/home/user/Documents");
        let result = check_nested_drop(&sources, target);
        assert!(result.is_none());
    }

    #[test]
    fn nested_drop_sibling_is_valid() {
        // Dropping /home/user/A into /home/user/B is valid.
        let sources = vec![PathBuf::from("/home/user/A")];
        let target = Path::new("/home/user/B");
        let result = check_nested_drop(&sources, target);
        assert!(result.is_none());
    }

    /// `..` makes a target that is textually unlike the source but is in fact
    /// inside it. `starts_with` alone said the drop was fine.
    #[test]
    fn nested_drop_sees_through_a_parent_component() {
        // Named from the process id and a per-process counter rather than from
        // the clock: `cargo test` runs this binary's tests as threads of one
        // process, and the clock they read only advances on a timer interrupt,
        // so a nanosecond tag is shared by every test that starts in the same
        // tick and they would scribble on each other's trees.
        let scratch = scratchdir::ScratchDir::new("dropzone_nested");
        let dir = scratch.dir();
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("sub")).expect("tree");

        // `<dir>/project/sub/../sub` is `<dir>/project/sub` — inside `project`.
        let target = project.join("sub").join("..").join("sub");
        let sources = vec![project.clone()];

        assert!(
            check_nested_drop(&sources, &target).is_some(),
            "a target that resolves inside the source must be refused"
        );

        // And the honest sibling case still passes.
        let sibling = dir.join("elsewhere");
        std::fs::create_dir_all(&sibling).expect("sibling");
        assert!(check_nested_drop(&sources, &sibling).is_none());
    }

    // ------------------------------------------------------------------
    // Zone registration and clearing
    // ------------------------------------------------------------------

    #[test]
    fn clear_zones_removes_all() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/a"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );
        mgr.register_sidebar_item(Path::new("/tmp"), Rect::new(0.0, 100.0, 200.0, 24.0));

        mgr.clear_zones();

        // After clearing, the list area should also be gone.
        assert_eq!(mgr.find_zone(400.0, 200.0), DropZone::None);
        assert_eq!(mgr.find_zone(100.0, 112.0), DropZone::None);
    }

    #[test]
    fn zones_rebuilt_each_frame() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));

        // Frame 1.
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/old"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );
        assert!(
            matches!(mgr.find_zone(400.0, 90.0), DropZone::Folder { ref path, .. } if path == "/home/user/old")
        );

        // Frame 2 -- clear and register different zones.
        mgr.clear_zones();
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/new"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );
        assert!(
            matches!(mgr.find_zone(400.0, 90.0), DropZone::Folder { ref path, .. } if path == "/home/user/new")
        );
    }

    // ------------------------------------------------------------------
    // Drop handling (integration-level)
    // ------------------------------------------------------------------

    #[test]
    fn handle_drop_valid() {
        let scratch = ScratchDir::new("dropzone_handle_drop");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("file.txt"), "x").unwrap();

        let mut mgr = DropZoneManager::new(root.clone());
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));

        let result = mgr.handle_drop(
            400.0,
            200.0,
            &[root.join("file.txt")],
            DragModifiers::default(),
        );

        assert!(result.valid);
        assert_eq!(result.operation, DropOperation::Move);
        assert_eq!(result.target_dir, root);
    }

    #[test]
    fn handle_drop_nested_invalid() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/Documents"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        // Drop /home/user/Documents onto itself.
        let result = mgr.handle_drop(
            400.0,
            90.0,
            &[PathBuf::from("/home/user/Documents")],
            DragModifiers::default(),
        );

        assert!(!result.valid);
        assert!(result.invalid_reason.is_some());
    }

    #[test]
    fn handle_drop_on_none_zone() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        // No list area, no zones -- drop on nothing.
        let result = mgr.handle_drop(
            500.0,
            500.0,
            &[PathBuf::from("/home/user/file.txt")],
            DragModifiers::default(),
        );
        assert!(!result.valid);
        assert_eq!(result.operation, DropOperation::None);
    }

    // ------------------------------------------------------------------
    // Hover tracking
    // ------------------------------------------------------------------

    #[test]
    fn update_hover_enter_and_leave() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            Path::new("/home/user/Documents"),
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        // Enter the folder zone.
        let event = mgr.update_hover(
            400.0,
            90.0,
            DragModifiers::default(),
            &[PathBuf::from("/home/user/file.txt")],
        );
        assert!(matches!(event, Some(DropZoneEvent::DragEnter { .. })));

        // Move within the same zone.
        let event = mgr.update_hover(
            500.0,
            95.0,
            DragModifiers::default(),
            &[PathBuf::from("/home/user/file.txt")],
        );
        assert!(matches!(event, Some(DropZoneEvent::DragOver { .. })));

        // Move outside all zones.
        let event = mgr.update_hover(
            50.0,
            50.0,
            DragModifiers::default(),
            &[PathBuf::from("/home/user/file.txt")],
        );
        assert!(matches!(event, Some(DropZoneEvent::DragLeave)));
    }

    // ------------------------------------------------------------------
    // Visual feedback rendering
    // ------------------------------------------------------------------

    #[test]
    fn render_feedback_current_directory() {
        let list_area = Some(Rect::new(200.0, 64.0, 700.0, 500.0));
        let cmds = render_drop_feedback(
            &DropZone::CurrentDirectory,
            DropOperation::Move,
            400.0,
            200.0,
            list_area,
            true,
            &Palette::for_mode(false),
        );
        // Should have: list area overlay + label background + label text.
        assert_eq!(cmds.len(), 3);
        // First command is a FillRect covering the list area.
        assert!(
            matches!(&cmds[0], RenderCommand::FillRect { x, y, width, height, .. }
                if (*x - 200.0).abs() < f32::EPSILON
                && (*y - 64.0).abs() < f32::EPSILON
                && (*width - 700.0).abs() < f32::EPSILON
                && (*height - 500.0).abs() < f32::EPSILON
            )
        );
    }

    #[test]
    fn render_feedback_folder_zone() {
        let rect = Rect::new(200.0, 86.0, 700.0, 22.0);
        let cmds = render_drop_feedback(
            &DropZone::Folder {
                path: PathBuf::from("/home/user/Documents"),
                rect,
            },
            DropOperation::Copy,
            400.0,
            90.0,
            None,
            true,
            &Palette::for_mode(false),
        );
        // Should have: overlay + underline + label bg + label text.
        assert_eq!(cmds.len(), 4);
    }

    #[test]
    fn render_feedback_invalid_uses_red() {
        let rect = Rect::new(200.0, 86.0, 700.0, 22.0);
        let cmds = render_drop_feedback(
            &DropZone::Folder {
                path: PathBuf::from("/home/user/Documents"),
                rect,
            },
            DropOperation::None,
            400.0,
            90.0,
            None,
            false,
            &Palette::for_mode(false),
        );
        // The overlay reports refusal, so it is the palette's `red` -- checked
        // as the role rather than as an RGB triple, which is the difference
        // between "this means refusal" and "this is the colour someone typed
        // in 2026". A literal here is what made this test fail when the
        // hardcoded red became a role, and it would have gone on passing if
        // the arm had been swapped for a green one of the same value.
        let p = Palette::for_mode(false);
        if let RenderCommand::FillRect { color, .. } = &cmds[0] {
            assert_eq!((color.r, color.g, color.b), (p.red.r, p.red.g, p.red.b));
            assert_eq!(color.a, 50, "the overlay is a wash, not a fill");
        } else {
            panic!("expected FillRect as first command");
        }
    }

    #[test]
    fn render_feedback_none_zone_empty() {
        let cmds = render_drop_feedback(
            &DropZone::None,
            DropOperation::None,
            0.0,
            0.0,
            None,
            true,
            &Palette::for_mode(false),
        );
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_feedback_sidebar() {
        let rect = Rect::new(0.0, 120.0, 200.0, 24.0);
        let cmds = render_drop_feedback(
            &DropZone::Sidebar {
                path: PathBuf::from("/tmp"),
                rect,
            },
            DropOperation::Copy,
            100.0,
            130.0,
            None,
            true,
            &Palette::for_mode(false),
        );
        // overlay + underline + label bg + label text.
        assert_eq!(cmds.len(), 4);
    }

    // ------------------------------------------------------------------
    // Operation label
    // ------------------------------------------------------------------

    #[test]
    fn operation_label_copy() {
        let label = operation_label(
            DropOperation::Copy,
            &DropZone::Folder {
                path: PathBuf::from("/home/user/Documents"),
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            },
        );
        assert_eq!(label, "Copy to Documents");
    }

    #[test]
    fn operation_label_move_current_dir() {
        let label = operation_label(DropOperation::Move, &DropZone::CurrentDirectory);
        assert_eq!(label, "Move to current folder");
    }

    #[test]
    fn operation_label_link() {
        let label = operation_label(
            DropOperation::Link,
            &DropZone::Sidebar {
                path: PathBuf::from("/tmp"),
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            },
        );
        assert_eq!(label, "Link in tmp");
    }

    #[test]
    fn operation_label_none_zone() {
        let label = operation_label(DropOperation::None, &DropZone::None);
        assert!(label.is_empty());
    }

    // ------------------------------------------------------------------
    // Same-device helper
    // ------------------------------------------------------------------

    #[test]
    fn a_drag_within_one_drive_moves_and_across_drives_copies() {
        assert_eq!(default_drop_operation(Some(true)), DropOperation::Move);
        assert_eq!(default_drop_operation(Some(false)), DropOperation::Copy);
    }

    /// **Not knowing must not cost the user a file.**
    ///
    /// This replaces `same_device_same_root`, which asserted that
    /// `/home/user/a` and `/home/other/b` are one device because they share a
    /// first component -- true of *every* pair of absolute paths on a
    /// Unix-shaped filesystem, so on the OS this is written for the answer was
    /// "Move" for every drag ever made, including one off a camera card.
    #[test]
    fn a_drag_whose_drives_are_unknown_copies_rather_than_moves() {
        assert_eq!(default_drop_operation(None), DropOperation::Copy);
    }

    #[test]
    fn an_empty_path_resolves_to_nothing_rather_than_to_everything() {
        // It used to assert that two empty paths are the same device, which
        // followed from comparing first components: neither has one, so they
        // matched. An empty path names no file and therefore no drive.
        assert_eq!(
            crate::drives::same_drive(Path::new(""), Path::new("")),
            None
        );
        assert_eq!(default_drop_operation(None), DropOperation::Copy);
    }
}
