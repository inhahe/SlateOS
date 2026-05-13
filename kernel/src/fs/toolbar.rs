//! File explorer toolbar / command bar.
//!
//! Defines the toolbar buttons and layout for the file explorer window.
//! The toolbar adapts based on:
//! - Current view mode (icons, details, etc.)
//! - Current selection (cut/copy/delete enabled when items selected)
//! - Current directory (e.g., Trash shows "Restore" instead of "Delete")
//! - Search state
//!
//! ## Architecture
//!
//! ```text
//! File explorer window
//!   → toolbar::build(context) generates ToolbarLayout
//!     → Navigation section: back/forward/up/path
//!     → Actions section: cut/copy/paste/delete/rename
//!     → View section: view mode toggle, sort, columns
//!     → New section: new folder, new file
//!   → GUI renders toolbar buttons
//!   → Click → toolbar::execute(action_id)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A toolbar section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarSection {
    /// Navigation (back, forward, up).
    Navigation,
    /// File actions (cut, copy, paste, delete, rename).
    Actions,
    /// View controls (view mode, sort, columns).
    View,
    /// Create new items (new folder, new file).
    New,
    /// Search controls.
    Search,
}

/// A toolbar button.
#[derive(Debug, Clone)]
pub struct ToolbarButton {
    /// Unique action identifier.
    pub action: String,
    /// Display label.
    pub label: String,
    /// Icon identifier.
    pub icon: String,
    /// Tooltip text.
    pub tooltip: String,
    /// Keyboard shortcut.
    pub shortcut: String,
    /// Whether the button is enabled.
    pub enabled: bool,
    /// Whether this is a toggle button (pressed state).
    pub toggled: bool,
    /// Section this button belongs to.
    pub section: ToolbarSection,
    /// Whether this is a dropdown button.
    pub has_dropdown: bool,
    /// Dropdown items (if has_dropdown).
    pub dropdown: Vec<DropdownItem>,
}

/// A dropdown menu item.
#[derive(Debug, Clone)]
pub struct DropdownItem {
    /// Action identifier.
    pub action: String,
    /// Display label.
    pub label: String,
    /// Whether checked/active.
    pub checked: bool,
}

/// Context for toolbar generation.
#[derive(Debug, Clone)]
pub struct ToolbarContext {
    /// Whether anything is selected.
    pub has_selection: bool,
    /// Number of selected items.
    pub selection_count: usize,
    /// Whether clipboard has content to paste.
    pub can_paste: bool,
    /// Whether we can navigate back.
    pub can_go_back: bool,
    /// Whether we can navigate forward.
    pub can_go_forward: bool,
    /// Whether we can go up.
    pub can_go_up: bool,
    /// Current view mode.
    pub view_mode: &'static str,
    /// Whether hidden files are shown.
    pub show_hidden: bool,
    /// Whether extensions are shown.
    pub show_extensions: bool,
    /// Whether in trash directory.
    pub is_trash: bool,
    /// Whether searching.
    pub is_searching: bool,
    /// Whether directory is read-only.
    pub read_only: bool,
}

impl Default for ToolbarContext {
    fn default() -> Self {
        Self {
            has_selection: false,
            selection_count: 0,
            can_paste: false,
            can_go_back: false,
            can_go_forward: false,
            can_go_up: true,
            view_mode: "Details",
            show_hidden: false,
            show_extensions: true,
            is_trash: false,
            is_searching: false,
            read_only: false,
        }
    }
}

/// Complete toolbar layout.
#[derive(Debug, Clone)]
pub struct ToolbarLayout {
    /// All buttons in display order.
    pub buttons: Vec<ToolbarButton>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static ACTION_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Build the toolbar for the current context.
pub fn build(ctx: &ToolbarContext) -> ToolbarLayout {
    BUILD_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut buttons = Vec::new();

    // Navigation section.
    buttons.push(ToolbarButton {
        action: String::from("back"),
        label: String::new(),
        icon: String::from("icon-back"),
        tooltip: String::from("Back (Alt+Left)"),
        shortcut: String::from("Alt+Left"),
        enabled: ctx.can_go_back,
        toggled: false,
        section: ToolbarSection::Navigation,
        has_dropdown: false,
        dropdown: Vec::new(),
    });
    buttons.push(ToolbarButton {
        action: String::from("forward"),
        label: String::new(),
        icon: String::from("icon-forward"),
        tooltip: String::from("Forward (Alt+Right)"),
        shortcut: String::from("Alt+Right"),
        enabled: ctx.can_go_forward,
        toggled: false,
        section: ToolbarSection::Navigation,
        has_dropdown: false,
        dropdown: Vec::new(),
    });
    buttons.push(ToolbarButton {
        action: String::from("up"),
        label: String::new(),
        icon: String::from("icon-up"),
        tooltip: String::from("Up (Alt+Up)"),
        shortcut: String::from("Alt+Up"),
        enabled: ctx.can_go_up,
        toggled: false,
        section: ToolbarSection::Navigation,
        has_dropdown: false,
        dropdown: Vec::new(),
    });

    // New section.
    if !ctx.read_only && !ctx.is_trash {
        buttons.push(ToolbarButton {
            action: String::from("new_folder"),
            label: String::from("New Folder"),
            icon: String::from("icon-new-folder"),
            tooltip: String::from("Create new folder (Ctrl+Shift+N)"),
            shortcut: String::from("Ctrl+Shift+N"),
            enabled: true,
            toggled: false,
            section: ToolbarSection::New,
            has_dropdown: true,
            dropdown: alloc::vec![
                DropdownItem {
                    action: String::from("new_folder"),
                    label: String::from("Folder"),
                    checked: false,
                },
                DropdownItem {
                    action: String::from("new_text"),
                    label: String::from("Text Document"),
                    checked: false,
                },
            ],
        });
    }

    // Actions section.
    buttons.push(ToolbarButton {
        action: String::from("cut"),
        label: String::from("Cut"),
        icon: String::from("icon-cut"),
        tooltip: String::from("Cut (Ctrl+X)"),
        shortcut: String::from("Ctrl+X"),
        enabled: ctx.has_selection && !ctx.read_only,
        toggled: false,
        section: ToolbarSection::Actions,
        has_dropdown: false,
        dropdown: Vec::new(),
    });
    buttons.push(ToolbarButton {
        action: String::from("copy"),
        label: String::from("Copy"),
        icon: String::from("icon-copy"),
        tooltip: String::from("Copy (Ctrl+C)"),
        shortcut: String::from("Ctrl+C"),
        enabled: ctx.has_selection,
        toggled: false,
        section: ToolbarSection::Actions,
        has_dropdown: false,
        dropdown: Vec::new(),
    });
    buttons.push(ToolbarButton {
        action: String::from("paste"),
        label: String::from("Paste"),
        icon: String::from("icon-paste"),
        tooltip: String::from("Paste (Ctrl+V)"),
        shortcut: String::from("Ctrl+V"),
        enabled: ctx.can_paste && !ctx.read_only,
        toggled: false,
        section: ToolbarSection::Actions,
        has_dropdown: false,
        dropdown: Vec::new(),
    });

    // Delete or Restore (for trash).
    if ctx.is_trash {
        buttons.push(ToolbarButton {
            action: String::from("restore"),
            label: String::from("Restore"),
            icon: String::from("icon-restore"),
            tooltip: String::from("Restore selected items"),
            shortcut: String::new(),
            enabled: ctx.has_selection,
            toggled: false,
            section: ToolbarSection::Actions,
            has_dropdown: false,
            dropdown: Vec::new(),
        });
        buttons.push(ToolbarButton {
            action: String::from("empty_trash"),
            label: String::from("Empty Trash"),
            icon: String::from("icon-empty-trash"),
            tooltip: String::from("Permanently delete all items"),
            shortcut: String::new(),
            enabled: true,
            toggled: false,
            section: ToolbarSection::Actions,
            has_dropdown: false,
            dropdown: Vec::new(),
        });
    } else {
        buttons.push(ToolbarButton {
            action: String::from("delete"),
            label: String::from("Delete"),
            icon: String::from("icon-delete"),
            tooltip: String::from("Move to Trash (Del)"),
            shortcut: String::from("Del"),
            enabled: ctx.has_selection && !ctx.read_only,
            toggled: false,
            section: ToolbarSection::Actions,
            has_dropdown: false,
            dropdown: Vec::new(),
        });
        buttons.push(ToolbarButton {
            action: String::from("rename"),
            label: String::from("Rename"),
            icon: String::from("icon-rename"),
            tooltip: String::from("Rename (F2)"),
            shortcut: String::from("F2"),
            enabled: ctx.selection_count == 1 && !ctx.read_only,
            toggled: false,
            section: ToolbarSection::Actions,
            has_dropdown: false,
            dropdown: Vec::new(),
        });
    }

    // View section.
    let view_modes = alloc::vec![
        ("LargeIcons", "Large Icons"),
        ("SmallIcons", "Small Icons"),
        ("List", "List"),
        ("Details", "Details"),
        ("Tiles", "Tiles"),
    ];
    let view_dropdown: Vec<DropdownItem> = view_modes.iter().map(|(id, label)| {
        DropdownItem {
            action: alloc::format!("view_{}", id),
            label: String::from(*label),
            checked: ctx.view_mode == *id,
        }
    }).collect();

    buttons.push(ToolbarButton {
        action: String::from("view_mode"),
        label: String::from("View"),
        icon: String::from("icon-view"),
        tooltip: String::from("Change view mode"),
        shortcut: String::new(),
        enabled: true,
        toggled: false,
        section: ToolbarSection::View,
        has_dropdown: true,
        dropdown: view_dropdown,
    });

    // Sort dropdown.
    buttons.push(ToolbarButton {
        action: String::from("sort"),
        label: String::from("Sort"),
        icon: String::from("icon-sort"),
        tooltip: String::from("Sort items"),
        shortcut: String::new(),
        enabled: true,
        toggled: false,
        section: ToolbarSection::View,
        has_dropdown: true,
        dropdown: alloc::vec![
            DropdownItem { action: String::from("sort_name"), label: String::from("Name"), checked: true },
            DropdownItem { action: String::from("sort_date"), label: String::from("Date modified"), checked: false },
            DropdownItem { action: String::from("sort_size"), label: String::from("Size"), checked: false },
            DropdownItem { action: String::from("sort_type"), label: String::from("Type"), checked: false },
        ],
    });

    // Toggle buttons.
    buttons.push(ToolbarButton {
        action: String::from("toggle_hidden"),
        label: String::from("Hidden"),
        icon: String::from("icon-hidden"),
        tooltip: String::from("Show/hide hidden files"),
        shortcut: String::from("Ctrl+H"),
        enabled: true,
        toggled: ctx.show_hidden,
        section: ToolbarSection::View,
        has_dropdown: false,
        dropdown: Vec::new(),
    });

    // Search.
    buttons.push(ToolbarButton {
        action: String::from("search"),
        label: String::from("Search"),
        icon: String::from("icon-search"),
        tooltip: String::from("Search in this folder (Ctrl+F)"),
        shortcut: String::from("Ctrl+F"),
        enabled: true,
        toggled: ctx.is_searching,
        section: ToolbarSection::Search,
        has_dropdown: false,
        dropdown: Vec::new(),
    });

    ToolbarLayout { buttons }
}

/// Get buttons for a specific section.
pub fn section_buttons(layout: &ToolbarLayout, section: ToolbarSection) -> Vec<&ToolbarButton> {
    layout.buttons.iter().filter(|b| b.section == section).collect()
}

/// Check if an action is valid in the current context.
pub fn is_action_enabled(layout: &ToolbarLayout, action: &str) -> bool {
    layout.buttons.iter()
        .find(|b| b.action == action)
        .map(|b| b.enabled)
        .unwrap_or(false)
}

/// Record an action execution.
pub fn record_action(_action: &str) {
    ACTION_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (build_count, action_count).
pub fn stats() -> (u64, u64) {
    (
        BUILD_COUNT.load(Ordering::Relaxed),
        ACTION_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    BUILD_COUNT.store(0, Ordering::Relaxed);
    ACTION_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the toolbar module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: default toolbar.
    {
        let ctx = ToolbarContext::default();
        let layout = build(&ctx);
        assert!(!layout.buttons.is_empty());
        // Should have navigation buttons.
        assert!(layout.buttons.iter().any(|b| b.action == "back"));
        assert!(layout.buttons.iter().any(|b| b.action == "forward"));
        assert!(layout.buttons.iter().any(|b| b.action == "up"));
        serial_println!("[toolbar] test 1 passed: default build ({} buttons)", layout.buttons.len());
    }

    // Test 2: selection enables cut/copy/delete.
    {
        let ctx = ToolbarContext {
            has_selection: true,
            selection_count: 3,
            ..Default::default()
        };
        let layout = build(&ctx);
        assert!(is_action_enabled(&layout, "cut"));
        assert!(is_action_enabled(&layout, "copy"));
        assert!(is_action_enabled(&layout, "delete"));
        serial_println!("[toolbar] test 2 passed: selection enables actions");
    }

    // Test 3: no selection disables cut/copy/delete.
    {
        let ctx = ToolbarContext::default();
        let layout = build(&ctx);
        assert!(!is_action_enabled(&layout, "cut"));
        assert!(!is_action_enabled(&layout, "copy"));
        assert!(!is_action_enabled(&layout, "delete"));
        serial_println!("[toolbar] test 3 passed: no selection disables actions");
    }

    // Test 4: trash mode shows restore instead of delete.
    {
        let ctx = ToolbarContext {
            has_selection: true,
            selection_count: 1,
            is_trash: true,
            ..Default::default()
        };
        let layout = build(&ctx);
        assert!(layout.buttons.iter().any(|b| b.action == "restore"));
        assert!(layout.buttons.iter().any(|b| b.action == "empty_trash"));
        assert!(!layout.buttons.iter().any(|b| b.action == "delete"));
        serial_println!("[toolbar] test 4 passed: trash mode");
    }

    // Test 5: read-only disables write operations.
    {
        let ctx = ToolbarContext {
            has_selection: true,
            selection_count: 1,
            read_only: true,
            ..Default::default()
        };
        let layout = build(&ctx);
        assert!(!is_action_enabled(&layout, "cut"));
        assert!(!is_action_enabled(&layout, "delete"));
        assert!(!is_action_enabled(&layout, "paste"));
        // Copy should still work.
        assert!(is_action_enabled(&layout, "copy"));
        serial_println!("[toolbar] test 5 passed: read-only mode");
    }

    // Test 6: section filtering.
    {
        let ctx = ToolbarContext::default();
        let layout = build(&ctx);
        let nav = section_buttons(&layout, ToolbarSection::Navigation);
        assert_eq!(nav.len(), 3); // back, forward, up
        serial_println!("[toolbar] test 6 passed: section filtering");
    }

    // Test 7: view mode dropdown.
    {
        let ctx = ToolbarContext {
            view_mode: "Details",
            ..Default::default()
        };
        let layout = build(&ctx);
        let view_btn = layout.buttons.iter().find(|b| b.action == "view_mode");
        assert!(view_btn.is_some());
        let empty = Vec::new();
        let dropdown = view_btn.map(|b| &b.dropdown).unwrap_or(&empty);
        assert!(!dropdown.is_empty());
        assert!(dropdown.iter().any(|d| d.label == "Details" && d.checked));
        serial_println!("[toolbar] test 7 passed: view mode dropdown");
    }

    serial_println!("[toolbar] all 7 self-tests passed");
    Ok(())
}
