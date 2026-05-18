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
//! hit-tests registered zones and [`determine_operation`] computes the correct
//! Copy/Move/Link based on source/target drives and modifier keys.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
        Self { x, y, width, height }
    }

    /// Returns `true` if the point `(px, py)` lies inside this rectangle.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x
            && px < self.x + self.width
            && py >= self.y
            && py < self.y + self.height
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
    Folder {
        path: String,
        rect: Rect,
    },
    /// Over a sidebar entry.
    Sidebar {
        path: String,
        rect: Rect,
    },
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
        sources: Vec<String>,
        operation: DropOperation,
    },
}

// ============================================================================
// Registered zone entry (internal)
// ============================================================================

/// A single registered zone -- either a file row or a sidebar item.
#[derive(Clone, Debug)]
struct RegisteredZone {
    path: String,
    rect: Rect,
    is_dir: bool,
    kind: ZoneKind,
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

    /// Register a visible file row.
    ///
    /// * `_index` -- row index in the file list (reserved for future use).
    /// * `path` -- absolute path of the file/folder.
    /// * `rect` -- bounding rectangle of the row.
    /// * `is_dir` -- whether the entry is a directory.
    pub fn register_file_row(&mut self, _index: usize, path: &str, rect: Rect, is_dir: bool) {
        self.zones.push(RegisteredZone {
            path: path.to_string(),
            rect,
            is_dir,
            kind: ZoneKind::FileRow,
        });
    }

    /// Register a sidebar item.
    pub fn register_sidebar_item(&mut self, path: &str, rect: Rect) {
        self.zones.push(RegisteredZone {
            path: path.to_string(),
            rect,
            is_dir: true, // sidebar items are always directories
            kind: ZoneKind::SidebarItem,
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
        if let Some(ref list) = self.list_area {
            if list.contains(x, y) {
                return DropZone::CurrentDirectory;
            }
        }

        DropZone::None
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
        sources: &[String],
    ) -> Option<DropZoneEvent> {
        let new_zone = self.find_zone(x, y);
        let operation = self.determine_operation(sources, &new_zone, modifiers);

        let event = if new_zone != self.current_hover {
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
        };

        event
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
        sources: &[String],
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

        let target_path = match zone {
            DropZone::CurrentDirectory => self.current_dir.to_string_lossy().to_string(),
            DropZone::Folder { path, .. } | DropZone::Sidebar { path, .. } => path.clone(),
            DropZone::None => return DropOperation::None,
        };

        // Default: same device -> Move, different device -> Copy.
        let target = Path::new(&target_path);
        if let Some(first_source) = sources.first() {
            if same_device(Path::new(first_source), target) {
                DropOperation::Move
            } else {
                DropOperation::Copy
            }
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
        sources: &[String],
        modifiers: DragModifiers,
    ) -> DropResult {
        let zone = self.find_zone(x, y);
        let operation = self.determine_operation(sources, &zone, modifiers);

        let target_dir = match &zone {
            DropZone::CurrentDirectory => self.current_dir.clone(),
            DropZone::Folder { path, .. } | DropZone::Sidebar { path, .. } => {
                PathBuf::from(path)
            }
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

        let source_paths: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();

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

/// Best-effort same-device check by comparing the first path component.
///
/// On the real OS this would compare device IDs from `stat`. Here we use
/// the same heuristic as `fileops::same_device`.
fn same_device(a: &Path, b: &Path) -> bool {
    let root_a = a.components().next();
    let root_b = b.components().next();
    root_a == root_b
}

/// Check whether dropping `sources` into `target_dir` would create a nested
/// drop (folder dropped into itself or one of its descendants).
///
/// Returns `Some(reason)` if the drop is invalid, `None` if it is fine.
fn check_nested_drop(sources: &[PathBuf], target_dir: &Path) -> Option<String> {
    for src in sources {
        // Exact self-drop: can't drop /foo into /foo.
        if src == target_dir {
            return Some(format!(
                "Cannot drop '{}' into itself",
                src.display()
            ));
        }

        // Ancestor check: can't drop /foo into /foo/bar/baz.
        if target_dir.starts_with(src) {
            return Some(format!(
                "Cannot drop '{}' into its own subdirectory '{}'",
                src.display(),
                target_dir.display()
            ));
        }
    }
    None
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
struct FeedbackColors;

impl FeedbackColors {
    /// Translucent blue overlay for valid drop zones.
    const HIGHLIGHT: Color = Color::rgba(50, 120, 220, 40);
    /// Stronger blue for the underline on a hovered folder row.
    const UNDERLINE: Color = Color::rgba(50, 120, 220, 180);
    /// Red overlay for invalid drop targets.
    const INVALID: Color = Color::rgba(220, 50, 50, 50);
    /// Red underline for invalid targets.
    const INVALID_UNDERLINE: Color = Color::rgba(220, 50, 50, 180);
    /// Semi-transparent background for the operation label.
    const LABEL_BG: Color = Color::rgba(30, 30, 30, 200);
    /// White text for the operation label.
    const LABEL_FG: Color = Color::WHITE;
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
            // Show just the last component for brevity.
            Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone())
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
) -> Vec<RenderCommand> {
    let mut cmds: Vec<RenderCommand> = Vec::new();

    let (highlight, underline) = if valid {
        (FeedbackColors::HIGHLIGHT, FeedbackColors::UNDERLINE)
    } else {
        (FeedbackColors::INVALID, FeedbackColors::INVALID_UNDERLINE)
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
        let estimated_width = label.len() as f32 * 7.0 + 16.0;
        let label_height = 22.0;

        // Background pill.
        cmds.push(RenderCommand::FillRect {
            x: label_x,
            y: label_y,
            width: estimated_width,
            height: label_height,
            color: FeedbackColors::LABEL_BG,
            corner_radii: CornerRadii::all(4.0),
        });

        // Text.
        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: label_y + 4.0,
            text: label,
            color: FeedbackColors::LABEL_FG,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
            "/home/user/Documents",
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        let zone = mgr.find_zone(400.0, 90.0);
        assert!(matches!(zone, DropZone::Folder { ref path, .. } if path == "/home/user/Documents"));
    }

    #[test]
    fn find_zone_file_row_falls_through() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        // Register a file (not a directory).
        mgr.register_file_row(
            0,
            "/home/user/readme.txt",
            Rect::new(200.0, 86.0, 700.0, 22.0),
            false,
        );

        // Hovering over a file row should give CurrentDirectory.
        assert_eq!(mgr.find_zone(400.0, 90.0), DropZone::CurrentDirectory);
    }

    #[test]
    fn find_zone_sidebar() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.register_sidebar_item("/tmp", Rect::new(0.0, 120.0, 200.0, 24.0));

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
        mgr.register_sidebar_item("/var", Rect::new(0.0, 100.0, 200.0, 30.0));
        mgr.register_sidebar_item("/tmp", Rect::new(0.0, 100.0, 200.0, 30.0));

        let zone = mgr.find_zone(100.0, 115.0);
        assert!(matches!(zone, DropZone::Sidebar { ref path, .. } if path == "/tmp"));
    }

    // ------------------------------------------------------------------
    // Operation determination (same/different drive, modifiers)
    // ------------------------------------------------------------------

    #[test]
    fn operation_same_drive_default_is_move() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::CurrentDirectory,
            DragModifiers::default(),
        );
        assert_eq!(op, DropOperation::Move);
    }

    #[test]
    fn operation_ctrl_forces_copy() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::CurrentDirectory,
            DragModifiers { ctrl: true, ..Default::default() },
        );
        assert_eq!(op, DropOperation::Copy);
    }

    #[test]
    fn operation_shift_forces_move() {
        let mgr = DropZoneManager::new(PathBuf::from("/mnt/usb"));
        // Different device -- default would be Copy, but Shift overrides.
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::CurrentDirectory,
            DragModifiers { shift: true, ..Default::default() },
        );
        assert_eq!(op, DropOperation::Move);
    }

    #[test]
    fn operation_alt_forces_link() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::CurrentDirectory,
            DragModifiers { alt: true, ..Default::default() },
        );
        assert_eq!(op, DropOperation::Link);
    }

    #[test]
    fn operation_none_zone_returns_none() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::None,
            DragModifiers::default(),
        );
        assert_eq!(op, DropOperation::None);
    }

    #[test]
    fn operation_no_sources_returns_none() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &[],
            &DropZone::CurrentDirectory,
            DragModifiers::default(),
        );
        assert_eq!(op, DropOperation::None);
    }

    #[test]
    fn operation_folder_target() {
        let mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        let op = mgr.determine_operation(
            &["/home/user/file.txt".to_string()],
            &DropZone::Folder {
                path: "/home/user/Documents".to_string(),
                rect: Rect::new(0.0, 0.0, 100.0, 22.0),
            },
            DragModifiers::default(),
        );
        // Same root component -> Move.
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
        assert!(result
            .as_ref()
            .is_some_and(|r| r.contains("into itself")));
    }

    #[test]
    fn nested_drop_parent_into_child() {
        let sources = vec![PathBuf::from("/home/user")];
        let target = Path::new("/home/user/Documents/sub");
        let result = check_nested_drop(&sources, target);
        assert!(result.is_some());
        assert!(result
            .as_ref()
            .is_some_and(|r| r.contains("subdirectory")));
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

    // ------------------------------------------------------------------
    // Zone registration and clearing
    // ------------------------------------------------------------------

    #[test]
    fn clear_zones_removes_all() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(0, "/home/user/a", Rect::new(200.0, 86.0, 700.0, 22.0), true);
        mgr.register_sidebar_item("/tmp", Rect::new(0.0, 100.0, 200.0, 24.0));

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
        mgr.register_file_row(0, "/home/user/old", Rect::new(200.0, 86.0, 700.0, 22.0), true);
        assert!(matches!(mgr.find_zone(400.0, 90.0), DropZone::Folder { ref path, .. } if path == "/home/user/old"));

        // Frame 2 -- clear and register different zones.
        mgr.clear_zones();
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(0, "/home/user/new", Rect::new(200.0, 86.0, 700.0, 22.0), true);
        assert!(matches!(mgr.find_zone(400.0, 90.0), DropZone::Folder { ref path, .. } if path == "/home/user/new"));
    }

    // ------------------------------------------------------------------
    // Drop handling (integration-level)
    // ------------------------------------------------------------------

    #[test]
    fn handle_drop_valid() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));

        let result = mgr.handle_drop(
            400.0,
            200.0,
            &["/home/user/file.txt".to_string()],
            DragModifiers::default(),
        );

        assert!(result.valid);
        assert_eq!(result.operation, DropOperation::Move);
        assert_eq!(result.target_dir, PathBuf::from("/home/user"));
    }

    #[test]
    fn handle_drop_nested_invalid() {
        let mut mgr = DropZoneManager::new(PathBuf::from("/home/user"));
        mgr.set_list_area(Rect::new(200.0, 64.0, 700.0, 500.0));
        mgr.register_file_row(
            0,
            "/home/user/Documents",
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        // Drop /home/user/Documents onto itself.
        let result = mgr.handle_drop(
            400.0,
            90.0,
            &["/home/user/Documents".to_string()],
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
            &["/home/user/file.txt".to_string()],
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
            "/home/user/Documents",
            Rect::new(200.0, 86.0, 700.0, 22.0),
            true,
        );

        // Enter the folder zone.
        let event = mgr.update_hover(
            400.0,
            90.0,
            DragModifiers::default(),
            &["/home/user/file.txt".to_string()],
        );
        assert!(matches!(event, Some(DropZoneEvent::DragEnter { .. })));

        // Move within the same zone.
        let event = mgr.update_hover(
            500.0,
            95.0,
            DragModifiers::default(),
            &["/home/user/file.txt".to_string()],
        );
        assert!(matches!(event, Some(DropZoneEvent::DragOver { .. })));

        // Move outside all zones.
        let event = mgr.update_hover(
            50.0,
            50.0,
            DragModifiers::default(),
            &["/home/user/file.txt".to_string()],
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
        );
        // Should have: list area overlay + label background + label text.
        assert_eq!(cmds.len(), 3);
        // First command is a FillRect covering the list area.
        assert!(matches!(&cmds[0], RenderCommand::FillRect { x, y, width, height, .. }
            if (*x - 200.0).abs() < f32::EPSILON
            && (*y - 64.0).abs() < f32::EPSILON
            && (*width - 700.0).abs() < f32::EPSILON
            && (*height - 500.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn render_feedback_folder_zone() {
        let rect = Rect::new(200.0, 86.0, 700.0, 22.0);
        let cmds = render_drop_feedback(
            &DropZone::Folder {
                path: "/home/user/Documents".to_string(),
                rect,
            },
            DropOperation::Copy,
            400.0,
            90.0,
            None,
            true,
        );
        // Should have: overlay + underline + label bg + label text.
        assert_eq!(cmds.len(), 4);
    }

    #[test]
    fn render_feedback_invalid_uses_red() {
        let rect = Rect::new(200.0, 86.0, 700.0, 22.0);
        let cmds = render_drop_feedback(
            &DropZone::Folder {
                path: "/home/user/Documents".to_string(),
                rect,
            },
            DropOperation::None,
            400.0,
            90.0,
            None,
            false,
        );
        // The overlay should use the invalid (red) colour.
        if let RenderCommand::FillRect { color, .. } = &cmds[0] {
            assert_eq!(color.r, 220);
            assert_eq!(color.g, 50);
            assert_eq!(color.b, 50);
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
        );
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_feedback_sidebar() {
        let rect = Rect::new(0.0, 120.0, 200.0, 24.0);
        let cmds = render_drop_feedback(
            &DropZone::Sidebar {
                path: "/tmp".to_string(),
                rect,
            },
            DropOperation::Copy,
            100.0,
            130.0,
            None,
            true,
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
                path: "/home/user/Documents".to_string(),
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
                path: "/tmp".to_string(),
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
    fn same_device_same_root() {
        assert!(same_device(
            Path::new("/home/user/a"),
            Path::new("/home/other/b")
        ));
    }

    #[test]
    fn same_device_both_empty() {
        assert!(same_device(Path::new(""), Path::new("")));
    }
}
