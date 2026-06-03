//! Context menu extension API.
//!
//! Provides a capability-gated, lazy-loading context menu extension system
//! that allows applications to register items in context menus (e.g.,
//! "Open with...", "Compress to .zip"). Extensions declare file patterns
//! for matching, capability requirements, priority ordering, and optional
//! submenus.
//!
//! # Architecture
//!
//! - **`ContextMenuExtension`** — what an app registers (patterns, labels, capabilities).
//! - **`ExtensionRegistry`** — manages all registered extensions with lookup methods.
//! - **`CapabilityChecker`** — trait for gating extension visibility by capability token.
//! - **`ExtensionLoader`** — trait for lazy-loading extension manifests from directories.
//! - **`TimeoutPolicy`** — time-bounds extension loading to prevent UI stalls.
//! - **`build_context_menu`** — assembles base items + extension items into a final menu.

#![allow(dead_code)]

use crate::menu::MenuItem;

use core::sync::atomic::{AtomicU64, Ordering};

// ─── Identifiers ───────────────────────────────────────────────────────────

/// Unique identifier for a registered extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExtensionId(u64);

impl ExtensionId {
    /// Generate a new unique extension ID.
    fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

// ─── Extension menu item (for submenus) ────────────────────────────────────

/// A menu item within an extension's submenu.
#[derive(Clone, Debug)]
pub struct ExtensionMenuItem {
    /// Display label.
    pub label: String,
    /// Icon identifier.
    pub icon: Option<String>,
    /// Unique action tag (used in activation events).
    pub action: String,
    /// Whether this item is enabled.
    pub enabled: bool,
}

// ─── Core extension struct ─────────────────────────────────────────────────

/// Describes a context menu extension registered by an application.
#[derive(Clone, Debug)]
pub struct ContextMenuExtension {
    /// Unique ID assigned on registration.
    pub id: ExtensionId,
    /// Name of the registering application.
    pub app_name: String,
    /// Display text shown in the context menu.
    pub label: String,
    /// Icon identifier (e.g., icon name or path).
    pub icon: Option<String>,
    /// Glob patterns determining when this extension appears.
    /// Patterns like `"*.rs"`, `"*.txt"`, `"*"` (match all), or
    /// `"dir:*"` (match directories).
    pub file_patterns: Vec<String>,
    /// Capability token required for this extension to be visible.
    /// `None` means no capability needed.
    pub capability_required: Option<String>,
    /// Sort priority. Lower values appear higher in the menu.
    pub priority: i32,
    /// Optional nested submenu items.
    pub submenu: Option<Vec<ExtensionMenuItem>>,
}

// ─── Extension manifest (for parsing from config) ──────────────────────────

/// A parsed extension manifest from a configuration file.
/// Used by `ExtensionLoader` implementations to describe extensions
/// before they are assigned IDs and registered.
#[derive(Clone, Debug)]
pub struct ExtensionManifest {
    /// Name of the registering application.
    pub app_name: String,
    /// Display text shown in the context menu.
    pub label: String,
    /// Icon identifier.
    pub icon: Option<String>,
    /// Glob patterns for file matching.
    pub file_patterns: Vec<String>,
    /// Capability token required.
    pub capability_required: Option<String>,
    /// Sort priority.
    pub priority: i32,
    /// Optional submenu items.
    pub submenu: Option<Vec<ExtensionMenuItem>>,
}

// ─── Capability checker trait ──────────────────────────────────────────────

/// Trait for checking whether a required capability is available.
///
/// The default behavior (when no checker is installed) grants all capabilities.
pub trait CapabilityChecker {
    /// Returns `true` if the given capability token is granted.
    fn check(&self, capability: &str) -> bool;
}

/// Default capability checker that grants everything.
struct AllowAllCapabilities;

impl CapabilityChecker for AllowAllCapabilities {
    fn check(&self, _capability: &str) -> bool {
        true
    }
}

// ─── Extension loader trait ────────────────────────────────────────────────

/// Trait for lazily loading extension manifests from a directory.
pub trait ExtensionLoader {
    /// Scan the given path and return all discovered extension manifests.
    fn load_extensions(&self, path: &str) -> Vec<ExtensionManifest>;
}

// ─── Timeout policy ────────────────────────────────────────────────────────

/// Controls the time budget for extension loading during menu construction.
#[derive(Clone, Debug)]
pub struct TimeoutPolicy {
    /// Maximum duration in milliseconds for loading extensions.
    pub timeout_ms: u64,
}

impl TimeoutPolicy {
    /// Create a timeout policy with the given duration.
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self { timeout_ms: 200 }
    }
}

// ─── Events ────────────────────────────────────────────────────────────────

/// Events emitted by the extension system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionEvent {
    /// User clicked an extension item.
    Activated {
        /// The extension that was activated.
        extension_id: ExtensionId,
        /// The file paths the extension was invoked on.
        file_paths: Vec<String>,
        /// The specific action (for submenu items).
        action: Option<String>,
    },
    /// Extension loading exceeded the timeout.
    LoadTimeout {
        /// The extension that timed out.
        extension_id: ExtensionId,
    },
}

// ─── Extension registry ────────────────────────────────────────────────────

/// Manages all registered context menu extensions.
pub struct ExtensionRegistry {
    extensions: Vec<ContextMenuExtension>,
    capability_checker: Box<dyn CapabilityChecker>,
    loader: Option<Box<dyn ExtensionLoader>>,
    loader_paths: Vec<String>,
    loaded: bool,
    timeout_policy: TimeoutPolicy,
}

impl ExtensionRegistry {
    /// Create a new empty registry with default settings.
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
            capability_checker: Box::new(AllowAllCapabilities),
            loader: None,
            loader_paths: Vec::new(),
            loaded: false,
            timeout_policy: TimeoutPolicy::default(),
        }
    }

    /// Set a custom capability checker.
    pub fn set_capability_checker(&mut self, checker: Box<dyn CapabilityChecker>) {
        self.capability_checker = checker;
    }

    /// Set an extension loader for lazy discovery.
    pub fn set_loader(&mut self, loader: Box<dyn ExtensionLoader>, paths: Vec<String>) {
        self.loader = Some(loader);
        self.loader_paths = paths;
        self.loaded = false;
    }

    /// Set the timeout policy for extension loading.
    pub fn set_timeout_policy(&mut self, policy: TimeoutPolicy) {
        self.timeout_policy = policy;
    }

    /// Get the current timeout policy.
    pub fn timeout_policy(&self) -> &TimeoutPolicy {
        &self.timeout_policy
    }

    /// Register an extension from a manifest, assigning it an ID.
    /// Returns the assigned `ExtensionId`.
    pub fn register(&mut self, manifest: ExtensionManifest) -> ExtensionId {
        let id = ExtensionId::next();
        self.extensions.push(ContextMenuExtension {
            id,
            app_name: manifest.app_name,
            label: manifest.label,
            icon: manifest.icon,
            file_patterns: manifest.file_patterns,
            capability_required: manifest.capability_required,
            priority: manifest.priority,
            submenu: manifest.submenu,
        });
        id
    }

    /// Register a pre-built extension directly.
    /// Returns the extension's ID.
    pub fn register_extension(&mut self, ext: ContextMenuExtension) -> ExtensionId {
        let id = ext.id;
        self.extensions.push(ext);
        id
    }

    /// Unregister an extension by ID.
    /// Returns `true` if the extension was found and removed.
    pub fn unregister(&mut self, id: ExtensionId) -> bool {
        let before = self.extensions.len();
        self.extensions.retain(|e| e.id != id);
        self.extensions.len() < before
    }

    /// Trigger lazy loading from configured loader paths.
    /// Does nothing if already loaded or no loader is set.
    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;

        // Take the loader temporarily to avoid borrow issues.
        let Some(loader) = self.loader.as_ref() else {
            return;
        };

        let mut manifests = Vec::new();
        for path in &self.loader_paths {
            manifests.extend(loader.load_extensions(path));
        }

        for manifest in manifests {
            self.register(manifest);
        }
    }

    /// Force a reload from all configured loader paths.
    /// Clears previously loaded extensions and re-scans.
    pub fn refresh(&mut self) {
        // Remove all extensions that were auto-loaded (keep manually registered ones).
        // Since we can't distinguish them easily, refresh clears everything and reloads.
        self.extensions.clear();
        self.loaded = false;
        self.ensure_loaded();
    }

    /// Get extensions matching a file path, filtered by capabilities.
    pub fn get_for_file(&self, path: &str) -> Vec<&ContextMenuExtension> {
        self.extensions
            .iter()
            .filter(|ext| self.has_capability(ext))
            .filter(|ext| ext.file_patterns.iter().any(|pat| match_file_pattern(pat, path)))
            .collect()
    }

    /// Get extensions matching a directory path, filtered by capabilities.
    pub fn get_for_directory(&self, path: &str) -> Vec<&ContextMenuExtension> {
        self.extensions
            .iter()
            .filter(|ext| self.has_capability(ext))
            .filter(|ext| {
                ext.file_patterns.iter().any(|pat| {
                    match_directory_pattern(pat, path)
                })
            })
            .collect()
    }

    /// Get extensions matching all paths in a multi-selection.
    /// Returns only extensions that match every path in the selection.
    pub fn get_for_selection(&self, paths: &[&str]) -> Vec<&ContextMenuExtension> {
        if paths.is_empty() {
            return Vec::new();
        }

        self.extensions
            .iter()
            .filter(|ext| self.has_capability(ext))
            .filter(|ext| {
                paths.iter().all(|path| {
                    ext.file_patterns.iter().any(|pat| match_file_pattern(pat, path))
                })
            })
            .collect()
    }

    /// Return the total number of registered extensions.
    pub fn len(&self) -> usize {
        self.extensions.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    /// Check if an extension's required capability is granted.
    fn has_capability(&self, ext: &ContextMenuExtension) -> bool {
        match &ext.capability_required {
            None => true,
            Some(cap) => self.capability_checker.check(cap),
        }
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Glob pattern matching ─────────────────────────────────────────────────

/// Match a file pattern against a file path.
///
/// Supported patterns:
/// - `"*"` — matches everything
/// - `"*.ext"` — matches files ending with `.ext`
/// - `"prefix*"` — matches files starting with `prefix`
/// - `"exact_name"` — exact filename match
/// - `"dir:*"` — only matches directories (use `match_directory_pattern`)
fn match_file_pattern(pattern: &str, path: &str) -> bool {
    // Directory-only patterns never match files.
    if pattern.starts_with("dir:") {
        return false;
    }

    // Extract filename from path for matching.
    let filename = path.rsplit('/').next().unwrap_or(path);
    // Also try backslash separator (Windows-style paths).
    let filename = if filename == path {
        path.rsplit('\\').next().unwrap_or(path)
    } else {
        filename
    };

    glob_match(pattern, filename)
}

/// Match a directory pattern against a directory path.
///
/// Supports `"dir:*"` prefix for directory-only patterns,
/// as well as regular `"*"` patterns.
fn match_directory_pattern(pattern: &str, path: &str) -> bool {
    if let Some(dir_pattern) = pattern.strip_prefix("dir:") {
        // Directory-specific pattern.
        let dirname = path.rsplit('/').next().unwrap_or(path);
        let dirname = if dirname == path {
            path.rsplit('\\').next().unwrap_or(path)
        } else {
            dirname
        };
        glob_match(dir_pattern, dirname)
    } else {
        // Regular pattern also matches directories (e.g., "*" matches all).
        let dirname = path.rsplit('/').next().unwrap_or(path);
        let dirname = if dirname == path {
            path.rsplit('\\').next().unwrap_or(path)
        } else {
            dirname
        };
        glob_match(pattern, dirname)
    }
}

/// Simple glob matching supporting `*` as a wildcard.
///
/// - `"*"` matches any string.
/// - `"*.ext"` matches any string ending with `.ext`.
/// - `"prefix*"` matches any string starting with `prefix`.
/// - `"pre*suf"` matches strings starting with `pre` and ending with `suf`.
/// - Exact match otherwise.
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Count wildcards. We support a single `*` for simplicity.
    let star_count = pattern.chars().filter(|&c| c == '*').count();
    if star_count == 0 {
        // Exact match (case-sensitive, per OS design spec).
        return pattern == text;
    }

    if star_count == 1 {
        if let Some(suffix) = pattern.strip_prefix('*') {
            // Pattern like `*.rs` — match suffix.
            return text.ends_with(suffix);
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            // Pattern like `Makefile*` — match prefix.
            return text.starts_with(prefix);
        }
        // Pattern like `pre*suf` — split and check both.
        if let Some(star_pos) = pattern.find('*') {
            let prefix = &pattern[..star_pos];
            let suffix = &pattern[star_pos + 1..];
            return text.starts_with(prefix)
                && text.ends_with(suffix)
                && text.len() >= prefix.len() + suffix.len();
        }
    }

    // Multiple wildcards: use a recursive approach.
    glob_match_recursive(pattern.as_bytes(), text.as_bytes())
}

/// Recursive glob matching for patterns with multiple `*` wildcards.
fn glob_match_recursive(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if pattern[0] == b'*' {
        // `*` can match zero or more characters.
        // Try matching rest of pattern against each suffix of text.
        let rest_pattern = &pattern[1..];
        for i in 0..=text.len() {
            if glob_match_recursive(rest_pattern, &text[i..]) {
                return true;
            }
        }
        false
    } else if text.is_empty() {
        false
    } else if pattern[0] == text[0] {
        glob_match_recursive(&pattern[1..], &text[1..])
    } else {
        false
    }
}

// ─── Context menu building ─────────────────────────────────────────────────

/// A tracking entry for timeout monitoring during menu construction.
#[derive(Clone, Debug)]
pub struct LoadingEntry {
    /// Extension ID being loaded.
    pub extension_id: ExtensionId,
    /// Whether loading completed in time.
    pub completed: bool,
    /// Elapsed time in milliseconds (if tracked externally).
    pub elapsed_ms: u64,
}

/// Build a context menu combining base items with extension items.
///
/// Base items (Cut/Copy/Paste/Delete/etc.) always appear first.
/// Extension items are added below a separator, grouped by application,
/// sorted by priority. An "Open with..." submenu is auto-generated
/// from matching extensions that have submenu items.
///
/// # Arguments
///
/// - `base_items` — Standard menu items (always shown first).
/// - `file_path` — The file/directory the context menu is for.
/// - `registry` — The extension registry to query.
/// - `is_directory` — Whether the target is a directory.
///
/// # Returns
///
/// A combined `Vec<MenuItem>` ready for display.
pub fn build_context_menu(
    base_items: Vec<MenuItem>,
    file_path: &str,
    registry: &ExtensionRegistry,
    is_directory: bool,
) -> Vec<MenuItem> {
    let matching = if is_directory {
        registry.get_for_directory(file_path)
    } else {
        registry.get_for_file(file_path)
    };

    if matching.is_empty() {
        return base_items;
    }

    // Sort by priority (lower = higher in menu), then by app name for stability.
    let mut sorted: Vec<&ContextMenuExtension> = matching;
    sorted.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then_with(|| a.app_name.cmp(&b.app_name))
    });

    let mut result = base_items;

    // Add separator between base items and extensions.
    if !result.is_empty() {
        result.push(MenuItem::Separator);
    }

    // Collect extensions with submenus for "Open with..." generation.
    let mut open_with_items: Vec<&ContextMenuExtension> = Vec::new();
    let mut regular_items: Vec<&ContextMenuExtension> = Vec::new();

    for ext in &sorted {
        if ext.submenu.is_some() {
            open_with_items.push(ext);
        } else {
            regular_items.push(ext);
        }
    }

    // Add regular extension items.
    let mut menu_id_counter: u64 = 10_000; // Start extension IDs high to avoid conflicts.
    for ext in &regular_items {
        result.push(MenuItem::Action {
            id: menu_id_counter,
            label: ext.label.clone(),
            shortcut: None,
            icon: ext.icon.clone(),
            enabled: true,
            checked: None,
        });
        menu_id_counter += 1;
    }

    // Build "Open with..." submenu if there are matching extensions with submenus.
    if !open_with_items.is_empty() {
        let mut open_with_children = Vec::new();
        for ext in &open_with_items {
            if let Some(sub_items) = &ext.submenu {
                for sub_item in sub_items {
                    open_with_children.push(MenuItem::Action {
                        id: menu_id_counter,
                        label: sub_item.label.clone(),
                        shortcut: None,
                        icon: sub_item.icon.clone(),
                        enabled: sub_item.enabled,
                        checked: None,
                    });
                    menu_id_counter += 1;
                }
            } else {
                // Extension without explicit submenu items gets a single entry.
                open_with_children.push(MenuItem::Action {
                    id: menu_id_counter,
                    label: ext.label.clone(),
                    shortcut: None,
                    icon: ext.icon.clone(),
                    enabled: true,
                    checked: None,
                });
                menu_id_counter += 1;
            }
        }

        result.push(MenuItem::Submenu {
            id: menu_id_counter,
            label: "Open with...".to_string(),
            icon: None,
            enabled: true,
            children: open_with_children,
        });
    }

    result
}

/// Build a context menu for a multi-file selection.
///
/// Only shows extensions that match ALL selected paths.
pub fn build_context_menu_for_selection(
    base_items: Vec<MenuItem>,
    paths: &[&str],
    registry: &ExtensionRegistry,
) -> Vec<MenuItem> {
    let matching = registry.get_for_selection(paths);

    if matching.is_empty() {
        return base_items;
    }

    let mut sorted: Vec<&ContextMenuExtension> = matching;
    sorted.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then_with(|| a.app_name.cmp(&b.app_name))
    });

    let mut result = base_items;

    if !result.is_empty() {
        result.push(MenuItem::Separator);
    }

    for (menu_id_counter, ext) in (10_000_u64..).zip(sorted.iter()) {
        result.push(MenuItem::Action {
            id: menu_id_counter,
            label: ext.label.clone(),
            shortcut: None,
            icon: ext.icon.clone(),
            enabled: true,
            checked: None,
        });
    }

    result
}

/// Create a "Loading..." placeholder menu item for slow extensions.
pub fn loading_placeholder(id: u64) -> MenuItem {
    MenuItem::Action {
        id,
        label: "Loading...".to_string(),
        shortcut: None,
        icon: None,
        enabled: false,
        checked: None,
    }
}

/// Check if extension loading has exceeded the timeout.
///
/// Returns timeout events for extensions that took too long.
pub fn check_timeouts(
    entries: &[LoadingEntry],
    policy: &TimeoutPolicy,
) -> Vec<ExtensionEvent> {
    entries
        .iter()
        .filter(|e| !e.completed && e.elapsed_ms > policy.timeout_ms)
        .map(|e| ExtensionEvent::LoadTimeout {
            extension_id: e.extension_id,
        })
        .collect()
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helpers ────────────────────────────────────────────────────────────

    fn make_extension(
        label: &str,
        patterns: &[&str],
        priority: i32,
    ) -> ExtensionManifest {
        ExtensionManifest {
            app_name: label.to_string(),
            label: label.to_string(),
            icon: None,
            file_patterns: patterns.iter().map(|s| s.to_string()).collect(),
            capability_required: None,
            priority,
            submenu: None,
        }
    }

    fn make_extension_with_cap(
        label: &str,
        patterns: &[&str],
        capability: &str,
    ) -> ExtensionManifest {
        ExtensionManifest {
            app_name: label.to_string(),
            label: label.to_string(),
            icon: None,
            file_patterns: patterns.iter().map(|s| s.to_string()).collect(),
            capability_required: Some(capability.to_string()),
            priority: 0,
            submenu: None,
        }
    }

    fn make_extension_with_submenu(
        label: &str,
        patterns: &[&str],
        sub_items: Vec<(&str, &str)>,
    ) -> ExtensionManifest {
        ExtensionManifest {
            app_name: label.to_string(),
            label: label.to_string(),
            icon: None,
            file_patterns: patterns.iter().map(|s| s.to_string()).collect(),
            capability_required: None,
            priority: 0,
            submenu: Some(
                sub_items
                    .into_iter()
                    .map(|(lbl, act)| ExtensionMenuItem {
                        label: lbl.to_string(),
                        icon: None,
                        action: act.to_string(),
                        enabled: true,
                    })
                    .collect(),
            ),
        }
    }

    fn base_items() -> Vec<MenuItem> {
        vec![
            MenuItem::Action {
                id: 1,
                label: "Cut".to_string(),
                shortcut: Some("Ctrl+X".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Action {
                id: 2,
                label: "Copy".to_string(),
                shortcut: Some("Ctrl+C".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Action {
                id: 3,
                label: "Paste".to_string(),
                shortcut: Some("Ctrl+V".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Action {
                id: 4,
                label: "Delete".to_string(),
                shortcut: Some("Del".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
        ]
    }

    /// Capability checker that denies a specific capability.
    struct DenyCapability {
        denied: String,
    }

    impl DenyCapability {
        fn new(denied: &str) -> Self {
            Self { denied: denied.to_string() }
        }
    }

    impl CapabilityChecker for DenyCapability {
        fn check(&self, capability: &str) -> bool {
            capability != self.denied
        }
    }

    /// Capability checker that only allows specific capabilities.
    struct AllowList {
        allowed: Vec<String>,
    }

    impl AllowList {
        fn new(allowed: &[&str]) -> Self {
            Self {
                allowed: allowed.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl CapabilityChecker for AllowList {
        fn check(&self, capability: &str) -> bool {
            self.allowed.iter().any(|a| a == capability)
        }
    }

    /// Test extension loader that returns predetermined manifests.
    struct TestLoader {
        manifests: Vec<ExtensionManifest>,
    }

    impl TestLoader {
        fn new(manifests: Vec<ExtensionManifest>) -> Self {
            Self { manifests }
        }
    }

    impl ExtensionLoader for TestLoader {
        fn load_extensions(&self, _path: &str) -> Vec<ExtensionManifest> {
            self.manifests.clone()
        }
    }

    // ─── Pattern matching tests ─────────────────────────────────────────────

    #[test]
    fn pattern_star_matches_everything() {
        assert!(match_file_pattern("*", "/home/user/document.txt"));
        assert!(match_file_pattern("*", "file.rs"));
        assert!(match_file_pattern("*", "no-extension"));
    }

    #[test]
    fn pattern_extension_match() {
        assert!(match_file_pattern("*.rs", "/home/user/main.rs"));
        assert!(match_file_pattern("*.rs", "lib.rs"));
        assert!(!match_file_pattern("*.rs", "file.txt"));
        assert!(!match_file_pattern("*.rs", "rs")); // No dot.
    }

    #[test]
    fn pattern_multiple_extensions() {
        assert!(match_file_pattern("*.tar.gz", "/path/to/archive.tar.gz"));
        assert!(!match_file_pattern("*.tar.gz", "archive.gz"));
    }

    #[test]
    fn pattern_prefix_match() {
        assert!(match_file_pattern("Makefile*", "Makefile"));
        assert!(match_file_pattern("Makefile*", "Makefile.bak"));
        assert!(!match_file_pattern("Makefile*", "notMakefile"));
    }

    #[test]
    fn pattern_exact_match() {
        assert!(match_file_pattern("Cargo.toml", "/project/Cargo.toml"));
        assert!(!match_file_pattern("Cargo.toml", "cargo.toml")); // Case-sensitive.
    }

    #[test]
    fn pattern_middle_wildcard() {
        assert!(glob_match("test*file", "test_my_file"));
        assert!(glob_match("test*file", "testfile"));
        assert!(!glob_match("test*file", "test_my_files"));
    }

    #[test]
    fn pattern_directory_only_does_not_match_files() {
        assert!(!match_file_pattern("dir:*", "/home/user/file.txt"));
        assert!(!match_file_pattern("dir:src", "/project/src/main.rs"));
    }

    #[test]
    fn pattern_directory_match() {
        assert!(match_directory_pattern("dir:*", "/home/user/Documents"));
        assert!(match_directory_pattern("dir:src", "/project/src"));
        assert!(!match_directory_pattern("dir:src", "/project/lib"));
    }

    #[test]
    fn pattern_directory_wildcard() {
        assert!(match_directory_pattern("dir:.*", ".hidden"));
        assert!(!match_directory_pattern("dir:.*", "visible"));
    }

    #[test]
    fn pattern_regular_matches_directory() {
        // Regular patterns (non dir:-prefixed) also match directories.
        assert!(match_directory_pattern("*", "/home/user/Documents"));
        assert!(match_directory_pattern("*.d", "/etc/conf.d"));
    }

    // ─── Registration tests ─────────────────────────────────────────────────

    #[test]
    fn register_and_find() {
        let mut registry = ExtensionRegistry::new();
        let id = registry.register(make_extension("Compress", &["*"], 0));

        let results = registry.get_for_file("anything.txt");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[test]
    fn register_multiple_and_find_matching() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Rust Analyzer", &["*.rs"], 0));
        registry.register(make_extension("Text Editor", &["*.txt"], 0));
        registry.register(make_extension("Compress", &["*"], 10));

        let results = registry.get_for_file("/project/src/main.rs");
        assert_eq!(results.len(), 2); // Rust Analyzer + Compress
        assert!(results.iter().any(|e| e.label == "Rust Analyzer"));
        assert!(results.iter().any(|e| e.label == "Compress"));
    }

    #[test]
    fn unregister_removes_extension() {
        let mut registry = ExtensionRegistry::new();
        let id = registry.register(make_extension("Test", &["*"], 0));
        assert_eq!(registry.len(), 1);

        let removed = registry.unregister(id);
        assert!(removed);
        assert_eq!(registry.len(), 0);
        assert!(registry.get_for_file("anything").is_empty());
    }

    #[test]
    fn unregister_nonexistent_returns_false() {
        let mut registry = ExtensionRegistry::new();
        let fake_id = ExtensionId(9999);
        assert!(!registry.unregister(fake_id));
    }

    #[test]
    fn registry_len_and_is_empty() {
        let mut registry = ExtensionRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        registry.register(make_extension("A", &["*"], 0));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    // ─── Capability filtering tests ─────────────────────────────────────────

    #[test]
    fn capability_no_requirement_always_visible() {
        let mut registry = ExtensionRegistry::new();
        registry.set_capability_checker(Box::new(DenyCapability::new("everything")));
        registry.register(make_extension("NoCapNeeded", &["*"], 0));

        let results = registry.get_for_file("test.txt");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn capability_denied_hides_extension() {
        let mut registry = ExtensionRegistry::new();
        registry.set_capability_checker(Box::new(DenyCapability::new("admin")));
        registry.register(make_extension_with_cap("AdminTool", &["*"], "admin"));

        let results = registry.get_for_file("test.txt");
        assert!(results.is_empty());
    }

    #[test]
    fn capability_granted_shows_extension() {
        let mut registry = ExtensionRegistry::new();
        registry.set_capability_checker(Box::new(AllowList::new(&["file.compress"])));
        registry.register(make_extension_with_cap("Compress", &["*"], "file.compress"));

        let results = registry.get_for_file("archive.zip");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn capability_mixed_filters_correctly() {
        let mut registry = ExtensionRegistry::new();
        registry.set_capability_checker(Box::new(AllowList::new(&["file.read"])));

        registry.register(make_extension_with_cap("Reader", &["*"], "file.read"));
        registry.register(make_extension_with_cap("Writer", &["*"], "file.write"));
        registry.register(make_extension("NoCap", &["*"], 5));

        let results = registry.get_for_file("test.txt");
        // Reader (cap granted) + NoCap (no cap needed) = 2. Writer filtered out.
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|e| e.label == "Reader"));
        assert!(results.iter().any(|e| e.label == "NoCap"));
    }

    #[test]
    fn default_checker_grants_all() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension_with_cap("Anything", &["*"], "some.rare.cap"));

        let results = registry.get_for_file("file.bin");
        assert_eq!(results.len(), 1);
    }

    // ─── Priority sorting tests ─────────────────────────────────────────────

    #[test]
    fn priority_sorting_in_file_results() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Low Priority", &["*"], 100));
        registry.register(make_extension("High Priority", &["*"], -10));
        registry.register(make_extension("Medium Priority", &["*"], 50));

        let mut results = registry.get_for_file("test.txt");
        results.sort_by_key(|e| e.priority);

        assert_eq!(results[0].label, "High Priority");
        assert_eq!(results[1].label, "Medium Priority");
        assert_eq!(results[2].label, "Low Priority");
    }

    #[test]
    fn same_priority_sorted_by_app_name() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Zebra", &["*"], 0));
        registry.register(make_extension("Alpha", &["*"], 0));
        registry.register(make_extension("Middle", &["*"], 0));

        let mut results = registry.get_for_file("test.txt");
        results.sort_by(|a, b| {
            a.priority.cmp(&b.priority).then_with(|| a.app_name.cmp(&b.app_name))
        });

        assert_eq!(results[0].label, "Alpha");
        assert_eq!(results[1].label, "Middle");
        assert_eq!(results[2].label, "Zebra");
    }

    // ─── File vs directory matching tests ───────────────────────────────────

    #[test]
    fn file_extensions_do_not_match_directory_patterns() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("DirOnly", &["dir:*"], 0));

        let results = registry.get_for_file("/path/to/file.txt");
        assert!(results.is_empty());
    }

    #[test]
    fn directory_extensions_match_directory_patterns() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("DirHandler", &["dir:*"], 0));

        let results = registry.get_for_directory("/home/user/Documents");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn wildcard_matches_both_files_and_directories() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Universal", &["*"], 0));

        let file_results = registry.get_for_file("/path/file.txt");
        let dir_results = registry.get_for_directory("/path/dir");
        assert_eq!(file_results.len(), 1);
        assert_eq!(dir_results.len(), 1);
    }

    // ─── Multi-selection intersection tests ─────────────────────────────────

    #[test]
    fn selection_intersection_all_match() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Compress", &["*"], 0));

        let results = registry.get_for_selection(&["file1.txt", "file2.rs", "file3.png"]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn selection_intersection_partial_match_excluded() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("RustTool", &["*.rs"], 0));
        registry.register(make_extension("Universal", &["*"], 5));

        let results = registry.get_for_selection(&["main.rs", "readme.txt"]);
        // RustTool only matches *.rs, doesn't match readme.txt => excluded.
        // Universal matches all => included.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "Universal");
    }

    #[test]
    fn selection_intersection_all_same_type() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("RustTool", &["*.rs"], 0));

        let results = registry.get_for_selection(&["main.rs", "lib.rs", "mod.rs"]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "RustTool");
    }

    #[test]
    fn selection_empty_returns_empty() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Something", &["*"], 0));

        let results = registry.get_for_selection(&[]);
        assert!(results.is_empty());
    }

    // ─── Timeout tracking tests ─────────────────────────────────────────────

    #[test]
    fn timeout_not_exceeded() {
        let policy = TimeoutPolicy::new(200);
        let entries = vec![
            LoadingEntry {
                extension_id: ExtensionId(1),
                completed: false,
                elapsed_ms: 100,
            },
        ];

        let events = check_timeouts(&entries, &policy);
        assert!(events.is_empty());
    }

    #[test]
    fn timeout_exceeded_generates_event() {
        let policy = TimeoutPolicy::new(200);
        let entries = vec![
            LoadingEntry {
                extension_id: ExtensionId(1),
                completed: false,
                elapsed_ms: 250,
            },
        ];

        let events = check_timeouts(&entries, &policy);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            ExtensionEvent::LoadTimeout { extension_id: ExtensionId(1) }
        );
    }

    #[test]
    fn timeout_completed_not_reported() {
        let policy = TimeoutPolicy::new(200);
        let entries = vec![
            LoadingEntry {
                extension_id: ExtensionId(1),
                completed: true,
                elapsed_ms: 500, // Over limit but already completed.
            },
        ];

        let events = check_timeouts(&entries, &policy);
        assert!(events.is_empty());
    }

    #[test]
    fn timeout_multiple_entries() {
        let policy = TimeoutPolicy::new(200);
        let entries = vec![
            LoadingEntry { extension_id: ExtensionId(1), completed: false, elapsed_ms: 50 },
            LoadingEntry { extension_id: ExtensionId(2), completed: false, elapsed_ms: 300 },
            LoadingEntry { extension_id: ExtensionId(3), completed: true, elapsed_ms: 400 },
            LoadingEntry { extension_id: ExtensionId(4), completed: false, elapsed_ms: 201 },
        ];

        let events = check_timeouts(&entries, &policy);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn default_timeout_is_200ms() {
        let policy = TimeoutPolicy::default();
        assert_eq!(policy.timeout_ms, 200);
    }

    // ─── Menu building tests ────────────────────────────────────────────────

    #[test]
    fn build_menu_no_extensions_returns_base() {
        let registry = ExtensionRegistry::new();
        let base = base_items();
        let result = build_context_menu(base.clone(), "file.txt", &registry, false);
        assert_eq!(result.len(), base.len());
    }

    #[test]
    fn build_menu_adds_separator_and_extensions() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Compress", &["*"], 0));

        let base = base_items();
        let base_len = base.len();
        let result = build_context_menu(base, "file.txt", &registry, false);

        // base_items + separator + extension item
        assert_eq!(result.len(), base_len + 2);
        assert!(matches!(result[base_len], MenuItem::Separator));
    }

    #[test]
    fn build_menu_extensions_sorted_by_priority() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Low", &["*"], 100));
        registry.register(make_extension("High", &["*"], -5));
        registry.register(make_extension("Med", &["*"], 50));

        let result = build_context_menu(Vec::new(), "file.txt", &registry, false);

        // No base items, so no separator — just the 3 extension items.
        assert_eq!(result.len(), 3);
        let labels: Vec<&str> = result
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec!["High", "Med", "Low"]);
    }

    #[test]
    fn build_menu_open_with_submenu() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension_with_submenu(
            "Editors",
            &["*.txt"],
            vec![("VSCode", "open_vscode"), ("Vim", "open_vim")],
        ));

        let result = build_context_menu(base_items(), "readme.txt", &registry, false);

        // Should have an "Open with..." submenu at the end.
        let last = result.last().expect("menu should not be empty");
        match last {
            MenuItem::Submenu { label, children, .. } => {
                assert_eq!(label, "Open with...");
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected Submenu for 'Open with...'"),
        }
    }

    #[test]
    fn build_menu_mixed_regular_and_submenu() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("Compress", &["*"], 0));
        registry.register(make_extension_with_submenu(
            "Editors",
            &["*"],
            vec![("Notepad", "open_notepad")],
        ));

        let base = base_items();
        let base_len = base.len();
        let result = build_context_menu(base, "file.txt", &registry, false);

        // base_items + separator + "Compress" action + "Open with..." submenu
        assert_eq!(result.len(), base_len + 3);

        // The "Compress" action should be before the submenu.
        match &result[base_len + 1] {
            MenuItem::Action { label, .. } => assert_eq!(label, "Compress"),
            _ => panic!("Expected action item"),
        }
        match &result[base_len + 2] {
            MenuItem::Submenu { label, .. } => assert_eq!(label, "Open with..."),
            _ => panic!("Expected submenu"),
        }
    }

    #[test]
    fn build_menu_only_matching_extensions_shown() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("RustTool", &["*.rs"], 0));
        registry.register(make_extension("PythonTool", &["*.py"], 0));

        let result = build_context_menu(Vec::new(), "main.rs", &registry, false);
        assert_eq!(result.len(), 1);
        match &result[0] {
            MenuItem::Action { label, .. } => assert_eq!(label, "RustTool"),
            _ => panic!("Expected action"),
        }
    }

    #[test]
    fn build_menu_directory_mode() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("DirTool", &["dir:*"], 0));
        registry.register(make_extension("FileTool", &["*.txt"], 0));

        // File mode should not see DirTool.
        let file_result = build_context_menu(Vec::new(), "file.txt", &registry, false);
        assert_eq!(file_result.len(), 1);
        match &file_result[0] {
            MenuItem::Action { label, .. } => assert_eq!(label, "FileTool"),
            _ => panic!("Expected FileTool"),
        }

        // Directory mode should see DirTool but not FileTool (*.txt won't match dir name).
        let dir_result = build_context_menu(Vec::new(), "/home/user/src", &registry, true);
        assert_eq!(dir_result.len(), 1);
        match &dir_result[0] {
            MenuItem::Action { label, .. } => assert_eq!(label, "DirTool"),
            _ => panic!("Expected DirTool"),
        }
    }

    // ─── Lazy loading tests ─────────────────────────────────────────────────

    #[test]
    fn lazy_loader_populates_registry() {
        let manifests = vec![
            make_extension("Loaded1", &["*"], 0),
            make_extension("Loaded2", &["*.rs"], 5),
        ];

        let mut registry = ExtensionRegistry::new();
        registry.set_loader(
            Box::new(TestLoader::new(manifests)),
            vec!["/extensions".to_string()],
        );

        // Not loaded yet.
        assert!(registry.is_empty());

        // Trigger lazy load.
        registry.ensure_loaded();
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn ensure_loaded_only_loads_once() {
        let manifests = vec![make_extension("Once", &["*"], 0)];

        let mut registry = ExtensionRegistry::new();
        registry.set_loader(
            Box::new(TestLoader::new(manifests)),
            vec!["/ext".to_string()],
        );

        registry.ensure_loaded();
        assert_eq!(registry.len(), 1);

        // Calling again should not double-load.
        registry.ensure_loaded();
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn refresh_reloads_extensions() {
        let manifests = vec![make_extension("Reloaded", &["*"], 0)];

        let mut registry = ExtensionRegistry::new();
        registry.set_loader(
            Box::new(TestLoader::new(manifests)),
            vec!["/ext".to_string()],
        );

        registry.ensure_loaded();
        assert_eq!(registry.len(), 1);

        // Refresh clears and reloads.
        registry.refresh();
        assert_eq!(registry.len(), 1);
    }

    // ─── Loading placeholder test ───────────────────────────────────────────

    #[test]
    fn loading_placeholder_is_disabled() {
        let item = loading_placeholder(999);
        match item {
            MenuItem::Action { id, label, enabled, .. } => {
                assert_eq!(id, 999);
                assert_eq!(label, "Loading...");
                assert!(!enabled);
            }
            _ => panic!("Expected disabled action item"),
        }
    }

    // ─── Selection menu building test ───────────────────────────────────────

    #[test]
    fn build_selection_menu_intersection() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("AllFiles", &["*"], 0));
        registry.register(make_extension("RustOnly", &["*.rs"], 5));

        let result = build_context_menu_for_selection(
            base_items(),
            &["main.rs", "lib.rs"],
            &registry,
        );

        // base_items + separator + AllFiles + RustOnly (both match all .rs files)
        let base_len = base_items().len();
        assert_eq!(result.len(), base_len + 3);
    }

    #[test]
    fn build_selection_menu_mixed_types() {
        let mut registry = ExtensionRegistry::new();
        registry.register(make_extension("AllFiles", &["*"], 0));
        registry.register(make_extension("RustOnly", &["*.rs"], 5));

        let result = build_context_menu_for_selection(
            Vec::new(),
            &["main.rs", "readme.txt"],
            &registry,
        );

        // Only AllFiles matches both.
        assert_eq!(result.len(), 1);
        match &result[0] {
            MenuItem::Action { label, .. } => assert_eq!(label, "AllFiles"),
            _ => panic!("Expected AllFiles action"),
        }
    }

    // ─── Extension event tests ──────────────────────────────────────────────

    #[test]
    fn extension_event_activated() {
        let event = ExtensionEvent::Activated {
            extension_id: ExtensionId(42),
            file_paths: vec!["/home/user/file.rs".to_string()],
            action: Some("compile".to_string()),
        };

        match event {
            ExtensionEvent::Activated { extension_id, file_paths, action } => {
                assert_eq!(extension_id, ExtensionId(42));
                assert_eq!(file_paths.len(), 1);
                assert_eq!(action, Some("compile".to_string()));
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn extension_event_load_timeout() {
        let event = ExtensionEvent::LoadTimeout {
            extension_id: ExtensionId(7),
        };

        assert_eq!(
            event,
            ExtensionEvent::LoadTimeout { extension_id: ExtensionId(7) }
        );
    }
}
