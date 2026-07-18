//! File explorer sidebar / navigation panel.
//!
//! Assembles the left-hand navigation panel for the file explorer,
//! combining data from multiple sources into a unified tree:
//!
//! - **Quick Access**: pinned folders from bookmarks (user-customizable)
//! - **This PC**: drives and mount points from VFS
//! - **Network**: network locations (placeholder for now)
//! - **Recent**: recently accessed directories from fs::recent
//! - **Tags**: tagged files from fs::tags (if any)
//!
//! ## Architecture
//!
//! ```text
//! File explorer sidebar
//!   → sidebar::build() assembles all sections
//!     → bookmarks module for Quick Access
//!     → VFS mount info for This PC / drives
//!     → recent module for Recent Files
//!     → tags module for Tags
//!   → GUI renders the tree with expand/collapse
//! ```
//!
//! The sidebar is dynamic — it updates when:
//! - User pins/unpins folders
//! - Drives are mounted/unmounted
//! - Recent files change

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum sidebar sections.
const MAX_SECTIONS: usize = 16;

/// Maximum items per section.
const MAX_ITEMS_PER_SECTION: usize = 64;

/// Maximum recent directories shown.
const MAX_RECENT_DIRS: usize = 10;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of sidebar section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// Quick Access / pinned folders.
    QuickAccess,
    /// This PC / drives.
    ThisPC,
    /// Network locations.
    Network,
    /// Recent directories.
    Recent,
    /// Tags.
    Tags,
    /// Custom user section.
    Custom,
}

impl SectionKind {
    /// Default display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::QuickAccess => "Quick Access",
            Self::ThisPC => "This PC",
            Self::Network => "Network",
            Self::Recent => "Recent",
            Self::Tags => "Tags",
            Self::Custom => "Custom",
        }
    }

    /// Display priority (lower = higher in sidebar).
    pub fn priority(self) -> u32 {
        match self {
            Self::QuickAccess => 100,
            Self::ThisPC => 200,
            Self::Network => 300,
            Self::Recent => 400,
            Self::Tags => 500,
            Self::Custom => 600,
        }
    }
}

/// A single item in a sidebar section.
#[derive(Debug, Clone)]
pub struct SidebarItem {
    /// Display label.
    pub label: String,
    /// Navigation path.
    pub path: String,
    /// Icon identifier.
    pub icon: String,
    /// Whether this item can be unpinned/removed.
    pub removable: bool,
    /// Whether this item supports drag-drop.
    pub droppable: bool,
    /// Usage info (e.g., "45 GB free of 120 GB" for drives).
    pub usage_info: String,
    /// Sort priority within section.
    pub priority: u32,
}

/// A sidebar section.
#[derive(Debug, Clone)]
pub struct SidebarSection {
    /// Section kind.
    pub kind: SectionKind,
    /// Display label (may be customized).
    pub label: String,
    /// Whether the section is expanded.
    pub expanded: bool,
    /// Whether the section can be hidden.
    pub hideable: bool,
    /// Items in this section.
    pub items: Vec<SidebarItem>,
}

/// Complete sidebar state.
#[derive(Debug, Clone)]
pub struct Sidebar {
    /// All sections in display order.
    pub sections: Vec<SidebarSection>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;

/// Section visibility preferences.
static HIDDEN_SECTIONS: Mutex<Vec<SectionKind>> = Mutex::new(Vec::new());

/// Section expanded state.
static EXPANDED_STATE: Mutex<Vec<(SectionKind, bool)>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Build the complete sidebar.
///
/// Assembles all sections from their data sources.
pub fn build() -> Sidebar {
    BUILD_COUNT.fetch_add(1, Ordering::Relaxed);

    let hidden = HIDDEN_SECTIONS.lock();
    let expanded = EXPANDED_STATE.lock();
    let mut sections = Vec::new();

    // Quick Access section.
    if !hidden.contains(&SectionKind::QuickAccess) {
        sections.push(build_quick_access(&expanded));
    }

    // This PC section.
    if !hidden.contains(&SectionKind::ThisPC) {
        sections.push(build_this_pc(&expanded));
    }

    // Network section.
    if !hidden.contains(&SectionKind::Network) {
        sections.push(build_network(&expanded));
    }

    // Recent section.
    if !hidden.contains(&SectionKind::Recent) {
        sections.push(build_recent(&expanded));
    }

    // Tags section.
    if !hidden.contains(&SectionKind::Tags) {
        sections.push(build_tags(&expanded));
    }

    // Sort by priority.
    sections.sort_by_key(|s| s.kind.priority());

    Sidebar { sections }
}

/// Hide a section.
pub fn hide_section(kind: SectionKind) {
    let mut hidden = HIDDEN_SECTIONS.lock();
    if !hidden.contains(&kind) {
        hidden.push(kind);
    }
}

/// Show a section.
pub fn show_section(kind: SectionKind) {
    let mut hidden = HIDDEN_SECTIONS.lock();
    hidden.retain(|k| *k != kind);
}

/// Set a section's expanded state.
pub fn set_expanded(kind: SectionKind, expanded: bool) {
    let mut state = EXPANDED_STATE.lock();
    if let Some(entry) = state.iter_mut().find(|(k, _)| *k == kind) {
        entry.1 = expanded;
    } else {
        state.push((kind, expanded));
    }
}

/// Toggle a section's expanded state.
pub fn toggle_expanded(kind: SectionKind) -> bool {
    let mut state = EXPANDED_STATE.lock();
    if let Some(entry) = state.iter_mut().find(|(k, _)| *k == kind) {
        entry.1 = !entry.1;
        entry.1
    } else {
        // Default is expanded, so toggling makes it collapsed.
        state.push((kind, false));
        false
    }
}

/// Pin a folder to Quick Access.
pub fn pin_to_quick_access(path: &str, label: &str) -> KernelResult<()> {
    // Use bookmarks module under the hood.
    let name = alloc::format!("qa_{}", label.replace(' ', "_").to_lowercase());
    crate::fs::bookmarks::add(
        &name,
        path,
        label,
        crate::fs::bookmarks::Category::Favorites,
    )
}

/// Unpin a folder from Quick Access.
pub fn unpin_from_quick_access(path: &str) -> KernelResult<()> {
    // Find bookmark by path.
    let bookmarks = crate::fs::bookmarks::list_category(crate::fs::bookmarks::Category::Favorites);
    for bm in &bookmarks {
        if bm.path == path {
            return crate::fs::bookmarks::remove(&bm.name);
        }
    }
    Err(KernelError::NotFound)
}

/// Get the section count.
pub fn section_count() -> usize {
    let hidden = HIDDEN_SECTIONS.lock();
    5usize.saturating_sub(hidden.len()) // 5 default sections minus hidden.
}

// ---------------------------------------------------------------------------
// Section builders
// ---------------------------------------------------------------------------

fn is_expanded(expanded: &[(SectionKind, bool)], kind: SectionKind) -> bool {
    expanded.iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, e)| *e)
        .unwrap_or(true) // Default expanded.
}

fn build_quick_access(expanded: &[(SectionKind, bool)]) -> SidebarSection {
    let mut items = Vec::new();

    // Get quick-access bookmarks.
    let bookmarks = crate::fs::bookmarks::list_category(crate::fs::bookmarks::Category::Favorites);
    for (idx, bm) in bookmarks.iter().enumerate() {
        if idx >= MAX_ITEMS_PER_SECTION {
            break;
        }
        items.push(SidebarItem {
            label: bm.label.clone(),
            path: bm.path.clone(),
            icon: bm.icon.clone(),
            removable: true,
            droppable: true,
            usage_info: String::new(),
            priority: idx as u32,
        });
    }

    // If no bookmarks, add some defaults.
    if items.is_empty() {
        let defaults = [
            ("Desktop", "/home/user/Desktop"),
            ("Downloads", "/home/user/Downloads"),
            ("Documents", "/home/user/Documents"),
            ("Pictures", "/home/user/Pictures"),
            ("Music", "/home/user/Music"),
            ("Videos", "/home/user/Videos"),
        ];
        for (idx, (label, path)) in defaults.iter().enumerate() {
            items.push(SidebarItem {
                label: String::from(*label),
                path: String::from(*path),
                icon: String::new(),
                removable: true,
                droppable: true,
                usage_info: String::new(),
                priority: idx as u32,
            });
        }
    }

    SidebarSection {
        kind: SectionKind::QuickAccess,
        label: String::from("Quick Access"),
        expanded: is_expanded(expanded, SectionKind::QuickAccess),
        hideable: false, // Always visible.
        items,
    }
}

fn build_this_pc(expanded: &[(SectionKind, bool)]) -> SidebarSection {
    let mut items = Vec::new();

    // Root filesystem.
    items.push(SidebarItem {
        label: String::from("Local Disk (/)"),
        path: String::from("/"),
        icon: String::from("drive"),
        removable: false,
        droppable: true,
        usage_info: disk_usage_string("/"),
        priority: 0,
    });

    // Check for common mount points.
    let mount_points = ["/home", "/tmp", "/boot"];
    let mut prio = 1u32;
    for mp in &mount_points {
        if crate::fs::vfs::Vfs::metadata(mp).is_ok() {
            let label = mp.to_string();
            items.push(SidebarItem {
                label,
                path: String::from(*mp),
                icon: String::from("folder"),
                removable: false,
                droppable: true,
                usage_info: String::new(),
                priority: prio,
            });
            prio = prio.saturating_add(1);
        }
    }

    SidebarSection {
        kind: SectionKind::ThisPC,
        label: String::from("This PC"),
        expanded: is_expanded(expanded, SectionKind::ThisPC),
        hideable: false,
        items,
    }
}

fn build_network(expanded: &[(SectionKind, bool)]) -> SidebarSection {
    // Network is a placeholder for now.
    SidebarSection {
        kind: SectionKind::Network,
        label: String::from("Network"),
        expanded: is_expanded(expanded, SectionKind::Network),
        hideable: true,
        items: Vec::new(),
    }
}

fn build_recent(expanded: &[(SectionKind, bool)]) -> SidebarSection {
    let mut items = Vec::new();

    // Get recent directories from the recent module.
    let recent = crate::fs::recent::query(&crate::fs::recent::RecentFilter {
        access_type: Some(crate::fs::recent::AccessType::Open),
        limit: MAX_RECENT_DIRS * 3, // Over-fetch since we filter to dirs.
        min_age_ns: 0,
        pattern: String::new(),
    });

    // Filter to directories only (heuristic: no extension).
    let mut prio = 0u32;
    for entry in &recent {
        // Check if this is a directory.
        if let Ok(meta) = crate::fs::vfs::Vfs::metadata(&entry.path) {
            if meta.entry_type == crate::fs::EntryType::Directory {
                let label = entry.path.rsplit('/').next().unwrap_or(&entry.path);
                items.push(SidebarItem {
                    label: String::from(label),
                    path: entry.path.clone(),
                    icon: String::from("folder"),
                    removable: true,
                    droppable: false,
                    usage_info: String::new(),
                    priority: prio,
                });
                prio = prio.saturating_add(1);
                if items.len() >= MAX_RECENT_DIRS {
                    break;
                }
            }
        }
    }

    SidebarSection {
        kind: SectionKind::Recent,
        label: String::from("Recent"),
        expanded: is_expanded(expanded, SectionKind::Recent),
        hideable: true,
        items,
    }
}

fn build_tags(expanded: &[(SectionKind, bool)]) -> SidebarSection {
    let mut items = Vec::new();

    // Get unique tags from tags module.
    let tag_list = crate::fs::tags::list_tags();
    let mut prio = 0u32;
    for (tag, _count) in &tag_list {
        if prio >= MAX_ITEMS_PER_SECTION as u32 {
            break;
        }
        items.push(SidebarItem {
            label: tag.clone(),
            // Tags navigate to a virtual search path.
            path: alloc::format!("/tags/{}", tag),
            icon: String::from("tag"),
            removable: false,
            droppable: false,
            usage_info: String::new(),
            priority: prio,
        });
        prio = prio.saturating_add(1);
    }

    SidebarSection {
        kind: SectionKind::Tags,
        label: String::from("Tags"),
        expanded: is_expanded(expanded, SectionKind::Tags),
        hideable: true,
        items,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get disk usage string for a path.
fn disk_usage_string(path: &str) -> String {
    // Use VFS fsinfo if available.
    match crate::fs::vfs::Vfs::statvfs(path) {
        Ok(info) => {
            let free_bytes = info.free_blocks.saturating_mul(info.block_size);
            let total_bytes = info.total_blocks.saturating_mul(info.block_size);
            let free_mib = free_bytes / (1024 * 1024);
            let total_mib = total_bytes / (1024 * 1024);
            if total_mib >= 1024 {
                alloc::format!("{:.1} GiB free of {:.1} GiB",
                               free_mib as f64 / 1024.0,
                               total_mib as f64 / 1024.0)
            } else {
                alloc::format!("{} MiB free of {} MiB", free_mib, total_mib)
            }
        }
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (build_count, section_count, hidden_count).
pub fn stats() -> (u64, usize, usize) {
    (
        BUILD_COUNT.load(Ordering::Relaxed),
        section_count(),
        HIDDEN_SECTIONS.lock().len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    BUILD_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the sidebar module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: build sidebar.
    {
        let sidebar = build();
        assert!(!sidebar.sections.is_empty());
        // Should have Quick Access and This PC at minimum.
        assert!(sidebar.sections.iter().any(|s| s.kind == SectionKind::QuickAccess));
        assert!(sidebar.sections.iter().any(|s| s.kind == SectionKind::ThisPC));
        serial_println!("[sidebar] test 1 passed: build ({} sections)", sidebar.sections.len());
    }

    // Test 2: section ordering.
    {
        let sidebar = build();
        let priorities: Vec<u32> = sidebar.sections.iter().map(|s| s.kind.priority()).collect();
        for w in priorities.windows(2) {
            assert!(w[0] <= w[1], "sections should be priority-ordered");
        }
        serial_println!("[sidebar] test 2 passed: section ordering");
    }

    // Test 3: hide/show section.
    {
        hide_section(SectionKind::Network);
        let sidebar = build();
        assert!(!sidebar.sections.iter().any(|s| s.kind == SectionKind::Network));

        show_section(SectionKind::Network);
        let sidebar2 = build();
        assert!(sidebar2.sections.iter().any(|s| s.kind == SectionKind::Network));
        serial_println!("[sidebar] test 3 passed: hide/show section");
    }

    // Test 4: toggle expanded.
    {
        // Default is expanded.
        let new_state = toggle_expanded(SectionKind::QuickAccess);
        assert!(!new_state); // Should now be collapsed.

        let new_state2 = toggle_expanded(SectionKind::QuickAccess);
        assert!(new_state2); // Should be expanded again.
        serial_println!("[sidebar] test 4 passed: toggle expanded");
    }

    // Test 5: Quick Access defaults.
    {
        let sidebar = build();
        let qa = sidebar.sections.iter().find(|s| s.kind == SectionKind::QuickAccess);
        assert!(qa.is_some());
        // Should have default items (Desktop, Downloads, etc.) or bookmarks.
        assert!(!qa.map(|s| s.items.is_empty()).unwrap_or(true));
        serial_println!("[sidebar] test 5 passed: quick access items");
    }

    // Test 6: This PC has root.
    {
        let sidebar = build();
        let pc = sidebar.sections.iter().find(|s| s.kind == SectionKind::ThisPC);
        assert!(pc.is_some());
        let has_root = pc.map(|s| s.items.iter().any(|i| i.path == "/")).unwrap_or(false);
        assert!(has_root);
        serial_println!("[sidebar] test 6 passed: this PC root drive");
    }

    // Test 7: stats.
    {
        let (builds, sections, _hidden) = stats();
        assert!(builds > 0);
        assert!(sections > 0);
        serial_println!("[sidebar] test 7 passed: stats");
    }

    serial_println!("[sidebar] all 7 self-tests passed");
    Ok(())
}
