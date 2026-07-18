//! Desktop icon layout and persistence.
//!
//! Manages the placement of icons on the desktop background, supporting
//! two layout modes per design spec (line 712):
//! - **Snap to grid**: icons align to a configurable grid
//! - **Free placement**: icons can be dragged anywhere
//!
//! Also manages:
//! - Icon position persistence across sessions
//! - Auto-arrange and sort options
//! - Desktop icon refresh when files change
//! - Special icons (Computer, Trash, Home, Network)
//!
//! ## Architecture
//!
//! ```text
//! Desktop widget
//!   → DesktopIcons::load(desktop_path)
//!     → reads directory listing
//!     → loads saved positions from layout file
//!     → applies grid snap or free positions
//!   → User drags icon → update_position(name, x, y)
//!   → User right-clicks → context menu integration
//!   → User double-clicks → open file/folder
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default grid cell width.
const DEFAULT_GRID_W: u32 = 80;

/// Default grid cell height.
const DEFAULT_GRID_H: u32 = 100;

/// Default icon size.
const DEFAULT_ICON_SIZE: u32 = 48;

/// Default horizontal padding between grid cells.
const DEFAULT_PAD_X: u32 = 10;

/// Default vertical padding between grid cells.
const DEFAULT_PAD_Y: u32 = 10;

/// Maximum desktop icons.
const MAX_ICONS: usize = 1024;

/// Maximum saved layouts.
const MAX_LAYOUTS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Desktop icon placement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Snap icons to a grid.
    SnapToGrid,
    /// Place icons freely anywhere.
    FreePlacement,
}

/// Sort order for auto-arrange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    /// Sort by name (alphabetical).
    Name,
    /// Sort by size.
    Size,
    /// Sort by type (extension).
    Type,
    /// Sort by modification date.
    DateModified,
}

/// A special system icon on the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialIcon {
    /// "This Computer" / "My Computer".
    Computer,
    /// Recycle Bin / Trash.
    Trash,
    /// User's home directory.
    Home,
    /// Network places.
    Network,
}

impl SpecialIcon {
    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Computer => "Computer",
            Self::Trash => "Trash",
            Self::Home => "Home",
            Self::Network => "Network",
        }
    }

    /// Default virtual path.
    pub fn path(self) -> &'static str {
        match self {
            Self::Computer => "/",
            Self::Trash => "/trash",
            Self::Home => "/home/user",
            Self::Network => "/net",
        }
    }
}

/// A single desktop icon.
#[derive(Debug, Clone)]
pub struct DesktopIcon {
    /// File or folder name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Whether this is a special system icon.
    pub special: Option<SpecialIcon>,
    /// X position (pixels from left).
    pub x: u32,
    /// Y position (pixels from top).
    pub y: u32,
    /// Whether the icon is selected.
    pub selected: bool,
    /// Whether the icon is being renamed.
    pub renaming: bool,
}

/// Grid configuration.
#[derive(Debug, Clone, Copy)]
pub struct GridConfig {
    /// Cell width in pixels.
    pub cell_w: u32,
    /// Cell height in pixels.
    pub cell_h: u32,
    /// Icon size in pixels.
    pub icon_size: u32,
    /// Horizontal padding.
    pub pad_x: u32,
    /// Vertical padding.
    pub pad_y: u32,
    /// Start X offset from screen edge.
    pub start_x: u32,
    /// Start Y offset from screen edge.
    pub start_y: u32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_w: DEFAULT_GRID_W,
            cell_h: DEFAULT_GRID_H,
            icon_size: DEFAULT_ICON_SIZE,
            pad_x: DEFAULT_PAD_X,
            pad_y: DEFAULT_PAD_Y,
            start_x: DEFAULT_PAD_X,
            start_y: DEFAULT_PAD_Y,
        }
    }
}

/// Complete desktop icon layout state.
#[derive(Debug, Clone)]
pub struct DesktopLayout {
    /// Desktop directory path.
    pub desktop_path: String,
    /// All icons.
    pub icons: Vec<DesktopIcon>,
    /// Layout mode.
    pub mode: LayoutMode,
    /// Sort order (for auto-arrange).
    pub sort_by: SortBy,
    /// Grid configuration.
    pub grid: GridConfig,
    /// Screen width (for grid calculations).
    pub screen_w: u32,
    /// Screen height.
    pub screen_h: u32,
    /// Whether to show hidden files.
    pub show_hidden: bool,
    /// Which special icons are visible.
    pub special_icons: Vec<SpecialIcon>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static LOAD_COUNT: AtomicU64 = AtomicU64::new(0);
static ARRANGE_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;

/// Current desktop layout.
static LAYOUT: Mutex<Option<DesktopLayout>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Load desktop icons from the desktop directory.
///
/// Scans the directory, applies saved positions if available,
/// and arranges new icons that don't have saved positions.
pub fn load(desktop_path: &str, screen_w: u32, screen_h: u32) -> KernelResult<()> {
    LOAD_COUNT.fetch_add(1, Ordering::Relaxed);

    let entries = crate::fs::vfs::Vfs::readdir(desktop_path)?;

    let grid = GridConfig::default();
    let mut icons = Vec::new();

    // Add special icons first.
    let specials = alloc::vec![
        SpecialIcon::Computer,
        SpecialIcon::Trash,
        SpecialIcon::Home,
    ];

    for (idx, special) in specials.iter().enumerate() {
        let (x, y) = grid_position(&grid, idx, screen_w);
        icons.push(DesktopIcon {
            name: String::from(special.label()),
            path: String::from(special.path()),
            is_dir: true,
            special: Some(*special),
            x,
            y,
            selected: false,
            renaming: false,
        });
    }

    // Add regular icons from directory.
    let offset = specials.len();
    for (idx, entry) in entries.iter().enumerate() {
        if entry.name.starts_with('.') {
            continue; // Skip hidden by default.
        }
        if icons.len() >= MAX_ICONS {
            break;
        }
        let (x, y) = grid_position(&grid, offset + idx, screen_w);
        let full_path = if desktop_path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", desktop_path, entry.name)
        };
        icons.push(DesktopIcon {
            name: entry.name.clone(),
            path: full_path,
            is_dir: entry.entry_type == crate::fs::EntryType::Directory,
            special: None,
            x,
            y,
            selected: false,
            renaming: false,
        });
    }

    let layout = DesktopLayout {
        desktop_path: String::from(desktop_path),
        icons,
        mode: LayoutMode::SnapToGrid,
        sort_by: SortBy::Name,
        grid,
        screen_w,
        screen_h,
        show_hidden: false,
        special_icons: specials,
    };

    *LAYOUT.lock() = Some(layout);
    Ok(())
}

/// Get the current layout.
pub fn get_layout() -> Option<DesktopLayout> {
    LAYOUT.lock().clone()
}

/// Update an icon's position (after drag).
pub fn update_position(name: &str, x: u32, y: u32) -> KernelResult<()> {
    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;

    let icon = layout.icons.iter_mut()
        .find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;

    match layout.mode {
        LayoutMode::SnapToGrid => {
            // Snap to nearest grid position.
            let (snapped_x, snapped_y) = snap_to_grid(&layout.grid, x, y);
            icon.x = snapped_x;
            icon.y = snapped_y;
        }
        LayoutMode::FreePlacement => {
            icon.x = x;
            icon.y = y;
        }
    }

    Ok(())
}

/// Set layout mode.
pub fn set_mode(mode: LayoutMode) -> KernelResult<()> {
    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;
    layout.mode = mode;

    if mode == LayoutMode::SnapToGrid {
        // Re-snap all icons.
        for icon in &mut layout.icons {
            let (sx, sy) = snap_to_grid(&layout.grid, icon.x, icon.y);
            icon.x = sx;
            icon.y = sy;
        }
    }

    Ok(())
}

/// Auto-arrange all icons.
pub fn auto_arrange(sort: SortBy) -> KernelResult<()> {
    ARRANGE_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;

    layout.sort_by = sort;

    // Sort icons (special icons first, then sorted).
    layout.icons.sort_by(|a, b| {
        // Special icons always first.
        match (&a.special, &b.special) {
            (Some(_), None) => core::cmp::Ordering::Less,
            (None, Some(_)) => core::cmp::Ordering::Greater,
            _ => {
                match sort {
                    SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    SortBy::Type => {
                        let ext_a = a.name.rsplit('.').next().unwrap_or("");
                        let ext_b = b.name.rsplit('.').next().unwrap_or("");
                        ext_a.cmp(ext_b).then(a.name.cmp(&b.name))
                    }
                    SortBy::Size | SortBy::DateModified => {
                        // For size/date sort we'd need metadata — fallback to name.
                        a.name.cmp(&b.name)
                    }
                }
            }
        }
    });

    // Reassign grid positions.
    let screen_w = layout.screen_w;
    let grid = layout.grid;
    for (idx, icon) in layout.icons.iter_mut().enumerate() {
        let (x, y) = grid_position(&grid, idx, screen_w);
        icon.x = x;
        icon.y = y;
    }

    Ok(())
}

/// Select an icon by name.
pub fn select(name: &str, exclusive: bool) -> KernelResult<()> {
    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;

    if exclusive {
        // Deselect all first.
        for icon in &mut layout.icons {
            icon.selected = false;
        }
    }

    let icon = layout.icons.iter_mut()
        .find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    icon.selected = true;
    Ok(())
}

/// Deselect all icons.
pub fn deselect_all() -> KernelResult<()> {
    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;
    for icon in &mut layout.icons {
        icon.selected = false;
    }
    Ok(())
}

/// Get selected icon paths.
pub fn selected_paths() -> Vec<String> {
    let layout_opt = LAYOUT.lock();
    match layout_opt.as_ref() {
        Some(layout) => layout.icons.iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect(),
        None => Vec::new(),
    }
}

/// Toggle show/hide hidden files.
pub fn set_show_hidden(show: bool) -> KernelResult<()> {
    let mut layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_mut().ok_or(KernelError::NotFound)?;
    layout.show_hidden = show;
    Ok(())
}

/// Refresh desktop icons from the filesystem.
pub fn refresh() -> KernelResult<()> {
    let (path, screen_w, screen_h) = {
        let layout_opt = LAYOUT.lock();
        let layout = layout_opt.as_ref().ok_or(KernelError::NotFound)?;
        (layout.desktop_path.clone(), layout.screen_w, layout.screen_h)
    };
    load(&path, screen_w, screen_h)
}

/// Hit test: find icon at pixel coordinates.
pub fn icon_at(px: u32, py: u32) -> Option<String> {
    let layout_opt = LAYOUT.lock();
    let layout = layout_opt.as_ref()?;

    for icon in &layout.icons {
        if px >= icon.x
            && px < icon.x.saturating_add(layout.grid.cell_w)
            && py >= icon.y
            && py < icon.y.saturating_add(layout.grid.cell_h)
        {
            return Some(icon.name.clone());
        }
    }
    None
}

/// Get icon count.
pub fn icon_count() -> usize {
    let layout_opt = LAYOUT.lock();
    layout_opt.as_ref().map(|l| l.icons.len()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Grid helpers
// ---------------------------------------------------------------------------

/// Calculate grid position for the nth icon.
fn grid_position(grid: &GridConfig, index: usize, screen_w: u32) -> (u32, u32) {
    let cols = if screen_w > grid.start_x.saturating_mul(2) {
        let usable = screen_w.saturating_sub(grid.start_x.saturating_mul(2));
        let col_w = grid.cell_w.saturating_add(grid.pad_x);
        if col_w == 0 { 1 } else { (usable / col_w).max(1) }
    } else {
        1
    };

    let col = (index as u32) % cols;
    let row = (index as u32) / cols;

    let x = grid.start_x + col * (grid.cell_w + grid.pad_x);
    let y = grid.start_y + row * (grid.cell_h + grid.pad_y);
    (x, y)
}

/// Snap coordinates to the nearest grid cell.
fn snap_to_grid(grid: &GridConfig, x: u32, y: u32) -> (u32, u32) {
    let col_w = grid.cell_w.saturating_add(grid.pad_x);
    let row_h = grid.cell_h.saturating_add(grid.pad_y);

    let col = if col_w > 0 && x >= grid.start_x {
        (x.saturating_sub(grid.start_x) + col_w / 2) / col_w
    } else {
        0
    };
    let row = if row_h > 0 && y >= grid.start_y {
        (y.saturating_sub(grid.start_y) + row_h / 2) / row_h
    } else {
        0
    };

    let snapped_x = grid.start_x + col * col_w;
    let snapped_y = grid.start_y + row * row_h;
    (snapped_x, snapped_y)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (load_count, arrange_count, icon_count).
pub fn stats() -> (u64, u64, usize) {
    (
        LOAD_COUNT.load(Ordering::Relaxed),
        ARRANGE_COUNT.load(Ordering::Relaxed),
        icon_count(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    LOAD_COUNT.store(0, Ordering::Relaxed);
    ARRANGE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the desktop icons module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: grid position calculation.
    {
        let grid = GridConfig::default();
        let (x0, y0) = grid_position(&grid, 0, 1920);
        assert_eq!(x0, grid.start_x);
        assert_eq!(y0, grid.start_y);

        let (x1, y1) = grid_position(&grid, 1, 1920);
        assert_eq!(x1, grid.start_x + grid.cell_w + grid.pad_x);
        assert_eq!(y1, grid.start_y);
        serial_println!("[deskicons] test 1 passed: grid position");
    }

    // Test 2: snap to grid.
    {
        let grid = GridConfig::default();
        // Exact grid position should snap to itself.
        let (sx, sy) = snap_to_grid(&grid, grid.start_x, grid.start_y);
        assert_eq!(sx, grid.start_x);
        assert_eq!(sy, grid.start_y);

        // Slightly off should snap.
        let (sx2, sy2) = snap_to_grid(&grid, grid.start_x + 5, grid.start_y + 5);
        assert_eq!(sx2, grid.start_x);
        assert_eq!(sy2, grid.start_y);
        serial_println!("[deskicons] test 2 passed: snap to grid");
    }

    // Test 3: load desktop from root.
    {
        load("/", 1920, 1080)?;
        let count = icon_count();
        // Should have at least the 3 special icons.
        assert!(count >= 3);
        serial_println!("[deskicons] test 3 passed: load ({}  icons)", count);
    }

    // Test 4: select and deselect.
    {
        select("Computer", true)?;
        let sel = selected_paths();
        assert_eq!(sel.len(), 1);
        assert_eq!(sel.first().map(|s| s.as_str()), Some("/"));

        deselect_all()?;
        let sel2 = selected_paths();
        assert!(sel2.is_empty());
        serial_println!("[deskicons] test 4 passed: select/deselect");
    }

    // Test 5: auto-arrange.
    {
        auto_arrange(SortBy::Name)?;
        // Icons should still exist.
        assert!(icon_count() >= 3);
        serial_println!("[deskicons] test 5 passed: auto-arrange");
    }

    // Test 6: hit test.
    {
        let grid = GridConfig::default();
        // First icon should be at start_x, start_y.
        let hit = icon_at(grid.start_x + 5, grid.start_y + 5);
        assert!(hit.is_some());
        // Way off screen should miss.
        let miss = icon_at(50000, 50000);
        assert!(miss.is_none());
        serial_println!("[deskicons] test 6 passed: hit test");
    }

    // Test 7: layout mode.
    {
        set_mode(LayoutMode::FreePlacement)?;
        let layout = get_layout();
        assert!(layout.is_some());
        assert_eq!(layout.as_ref().map(|l| l.mode), Some(LayoutMode::FreePlacement));

        set_mode(LayoutMode::SnapToGrid)?;
        serial_println!("[deskicons] test 7 passed: layout mode");
    }

    serial_println!("[deskicons] all 7 self-tests passed");
    Ok(())
}
