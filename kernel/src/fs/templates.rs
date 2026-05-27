//! File template system for "New → From Template" operations.
//!
//! Provides a registry of file templates that appear in the file
//! explorer's "New" context menu (right-click → New → ...).  Each
//! template defines a name, extension, default content, and icon.
//!
//! ## How It Works
//!
//! 1. Applications register templates during installation
//! 2. The file explorer queries `list()` to build the "New" submenu
//! 3. User selects a template → `create(template_id, dir)` is called
//! 4. A new file is created with the template's default content
//! 5. Name conflicts are resolved with " (2)" suffixes
//!
//! ## Template Sources
//!
//! - **System templates**: built-in defaults (text file, folder)
//! - **Application templates**: registered by installed apps
//! - **User templates**: custom templates in ~/Templates/
//!
//! ## Design Notes
//!
//! - Templates store their default content in memory (small files only;
//!   large templates reference a file path instead).
//! - Maximum template content size: 1 MiB.
//! - Templates are ordered by category and display priority.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered templates.
const MAX_TEMPLATES: usize = 256;

/// Maximum inline template content (1 MiB).
const MAX_CONTENT_SIZE: usize = 1024 * 1024;

/// Maximum template name length.
const MAX_NAME_LEN: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Template category for menu grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Core filesystem items (folder, text file, link).
    Core,
    /// Document templates (word processor, spreadsheet, etc.).
    Document,
    /// Development templates (source code files).
    Development,
    /// Media templates (image, audio, video project files).
    Media,
    /// Application-specific templates.
    Application,
    /// User-defined custom templates.
    User,
}

impl Category {
    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Document => "Documents",
            Self::Development => "Development",
            Self::Media => "Media",
            Self::Application => "Applications",
            Self::User => "User Templates",
        }
    }

    /// Sort order (lower = earlier in menu).
    pub fn sort_order(self) -> u32 {
        match self {
            Self::Core => 0,
            Self::Document => 1,
            Self::Development => 2,
            Self::Media => 3,
            Self::Application => 4,
            Self::User => 5,
        }
    }
}

/// How the template content is stored.
#[derive(Debug, Clone)]
pub enum TemplateContent {
    /// Inline content (small templates, stored in memory).
    Inline(Vec<u8>),
    /// Reference to a file (large templates, path to template file).
    FileRef(String),
    /// Empty file (just create with zero bytes).
    Empty,
    /// Directory (create a directory instead of a file).
    Directory,
}

/// A registered file template.
#[derive(Debug, Clone)]
pub struct Template {
    /// Unique template ID.
    pub id: u64,
    /// Display name (e.g., "Text Document", "Python Script").
    pub name: String,
    /// File extension including dot (e.g., ".txt", ".py").
    pub extension: String,
    /// Default filename (without number suffix).
    pub default_name: String,
    /// Template content.
    pub content: TemplateContent,
    /// Category for menu grouping.
    pub category: Category,
    /// Icon identifier for the menu item.
    pub icon: String,
    /// Display priority within category (lower = earlier).
    pub priority: u32,
    /// Application that registered this template.
    pub source: String,
    /// Whether this template is visible in menus.
    pub visible: bool,
    /// MIME type of the created file.
    pub mime_type: String,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static TEMPLATE_COUNTER: AtomicU64 = AtomicU64::new(100);
static CREATE_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

static TEMPLATES: spin::Mutex<Vec<Template>> = spin::Mutex::new(Vec::new());
static INITIALIZED: spin::Mutex<bool> = spin::Mutex::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the template system with default templates.
///
/// Call once at boot. Safe to call multiple times (no-op after first).
pub fn init() {
    let mut initialized = INITIALIZED.lock();
    if *initialized {
        return;
    }

    let defaults = [
        // Core.
        ("Folder", "", "New Folder", Category::Core,
         TemplateContent::Directory, "folder", "inode/directory", 0u32),
        ("Text Document", ".txt", "New Text Document", Category::Core,
         TemplateContent::Empty, "text-plain", "text/plain", 1),
        ("Symbolic Link", ".lnk", "New Link", Category::Core,
         TemplateContent::Empty, "emblem-symbolic-link", "inode/symlink", 2),

        // Documents.
        ("Rich Text Document", ".rtf", "New Document", Category::Document,
         TemplateContent::Inline(b"{\\rtf1\\ansi\\pard\\par}".to_vec()),
         "text-rtf", "application/rtf", 0),
        ("HTML Document", ".html", "New Page", Category::Document,
         TemplateContent::Inline(
            b"<!DOCTYPE html>\n<html>\n<head>\n  <meta charset=\"utf-8\">\n  <title>Untitled</title>\n</head>\n<body>\n\n</body>\n</html>\n".to_vec()
         ), "text-html", "text/html", 1),
        ("Markdown Document", ".md", "New Document", Category::Document,
         TemplateContent::Inline(b"# Untitled\n\n".to_vec()),
         "text-x-markdown", "text/markdown", 2),
        ("CSV Spreadsheet", ".csv", "New Spreadsheet", Category::Document,
         TemplateContent::Inline(b"Column A,Column B,Column C\n".to_vec()),
         "text-csv", "text/csv", 3),

        // Development.
        ("Python Script", ".py", "script", Category::Development,
         TemplateContent::Inline(b"#!/usr/bin/env python3\n\ndef main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n".to_vec()),
         "text-x-python", "text/x-python", 0),
        ("Shell Script", ".sh", "script", Category::Development,
         TemplateContent::Inline(b"#!/bin/sh\n\nset -e\n\n".to_vec()),
         "text-x-script", "text/x-shellscript", 1),
        ("Rust Source", ".rs", "main", Category::Development,
         TemplateContent::Inline(b"fn main() {\n    println!(\"Hello, world!\");\n}\n".to_vec()),
         "text-x-rust", "text/x-rust", 2),
        ("C Source", ".c", "main", Category::Development,
         TemplateContent::Inline(b"#include <stdio.h>\n\nint main(void) {\n    return 0;\n}\n".to_vec()),
         "text-x-c", "text/x-c", 3),
        ("JSON File", ".json", "data", Category::Development,
         TemplateContent::Inline(b"{\n}\n".to_vec()),
         "application-json", "application/json", 4),
        ("YAML File", ".yaml", "config", Category::Development,
         TemplateContent::Inline(b"---\n".to_vec()),
         "text-x-yaml", "text/yaml", 5),
    ];

    let mut templates = TEMPLATES.lock();
    for (name, ext, default_name, cat, content, icon, mime, prio) in defaults {
        let id = TEMPLATE_COUNTER.fetch_add(1, Ordering::Relaxed);
        templates.push(Template {
            id,
            name: String::from(name),
            extension: String::from(ext),
            default_name: String::from(default_name),
            content,
            category: cat,
            icon: String::from(icon),
            priority: prio,
            source: String::from("system"),
            visible: true,
            mime_type: String::from(mime),
        });
    }

    *initialized = true;
}

// ---------------------------------------------------------------------------
// Template operations
// ---------------------------------------------------------------------------

/// Register a new template.
pub fn register(
    name: &str,
    extension: &str,
    default_name: &str,
    category: Category,
    content: TemplateContent,
    icon: &str,
    mime_type: &str,
    source: &str,
) -> KernelResult<u64> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    if let TemplateContent::Inline(ref data) = content {
        if data.len() > MAX_CONTENT_SIZE {
            return Err(KernelError::InvalidArgument);
        }
    }

    let mut templates = TEMPLATES.lock();
    if templates.len() >= MAX_TEMPLATES {
        return Err(KernelError::OutOfMemory);
    }

    let id = TEMPLATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    templates.push(Template {
        id,
        name: String::from(name),
        extension: String::from(extension),
        default_name: String::from(default_name),
        content,
        category,
        icon: String::from(icon),
        priority: 50,
        source: String::from(source),
        visible: true,
        mime_type: String::from(mime_type),
    });

    Ok(id)
}

/// Unregister a template by ID.
pub fn unregister(id: u64) -> bool {
    let mut templates = TEMPLATES.lock();
    let before = templates.len();
    templates.retain(|t| t.id != id);
    templates.len() < before
}

/// Unregister all templates from a specific source.
pub fn unregister_source(source: &str) -> usize {
    let mut templates = TEMPLATES.lock();
    let before = templates.len();
    templates.retain(|t| t.source != source);
    before.saturating_sub(templates.len())
}

/// List all visible templates, sorted by category then priority.
pub fn list() -> Vec<Template> {
    let templates = TEMPLATES.lock();
    let mut result: Vec<Template> = templates.iter()
        .filter(|t| t.visible)
        .cloned()
        .collect();

    result.sort_by(|a, b| {
        a.category.sort_order().cmp(&b.category.sort_order())
            .then(a.priority.cmp(&b.priority))
    });

    result
}

/// List templates in a specific category.
pub fn list_category(category: Category) -> Vec<Template> {
    let templates = TEMPLATES.lock();
    let mut result: Vec<Template> = templates.iter()
        .filter(|t| t.visible && t.category == category)
        .cloned()
        .collect();

    result.sort_by_key(|a| a.priority);
    result
}

/// Get a template by ID.
pub fn get(id: u64) -> Option<Template> {
    TEMPLATES.lock().iter().find(|t| t.id == id).cloned()
}

/// Get a template by name (case-insensitive).
pub fn get_by_name(name: &str) -> Option<Template> {
    let lower = name.to_ascii_lowercase();
    TEMPLATES.lock().iter()
        .find(|t| t.name.to_ascii_lowercase() == lower)
        .cloned()
}

/// Set visibility of a template.
pub fn set_visible(id: u64, visible: bool) -> bool {
    let mut templates = TEMPLATES.lock();
    if let Some(t) = templates.iter_mut().find(|t| t.id == id) {
        t.visible = visible;
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// File creation
// ---------------------------------------------------------------------------

/// Create a new file from a template in the specified directory.
///
/// Returns the full path of the created file.  If a file with the
/// default name already exists, appends " (2)", " (3)", etc.
pub fn create(template_id: u64, directory: &str) -> KernelResult<String> {
    let template = get(template_id)
        .ok_or(KernelError::NotFound)?;

    match template.content {
        TemplateContent::Directory => {
            create_directory_from_template(&template, directory)
        }
        _ => {
            create_file_from_template(&template, directory)
        }
    }
}

/// Create a file from a template.
fn create_file_from_template(template: &Template, directory: &str) -> KernelResult<String> {
    // Build the base filename.
    let base_name = alloc::format!("{}{}", template.default_name, template.extension);

    // Find a non-conflicting name.
    let full_path = find_unique_name(directory, &base_name)?;

    // Get content.
    let content = match &template.content {
        TemplateContent::Inline(data) => data.clone(),
        TemplateContent::FileRef(path) => {
            crate::fs::vfs::Vfs::read_file(path)?
        }
        TemplateContent::Empty => Vec::new(),
        TemplateContent::Directory => {
            // Shouldn't reach here.
            return Err(KernelError::InternalError);
        }
    };

    // Write the file.
    crate::fs::vfs::Vfs::write_file(&full_path, &content)?;

    CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_BYTES.fetch_add(content.len() as u64, Ordering::Relaxed);

    Ok(full_path)
}

/// Create a directory from a template.
fn create_directory_from_template(template: &Template, directory: &str) -> KernelResult<String> {
    let full_path = find_unique_name(directory, &template.default_name)?;

    crate::fs::vfs::Vfs::mkdir(&full_path)?;

    CREATE_COUNT.fetch_add(1, Ordering::Relaxed);

    Ok(full_path)
}

/// Find a unique filename in a directory, appending " (2)", " (3)", etc.
fn find_unique_name(directory: &str, base_name: &str) -> KernelResult<String> {
    let base_path = if directory == "/" {
        alloc::format!("/{}", base_name)
    } else {
        alloc::format!("{}/{}", directory, base_name)
    };

    // Check if the base name is available.
    if crate::fs::vfs::Vfs::metadata(&base_path).is_err() {
        return Ok(base_path);
    }

    // Split name and extension for suffix insertion.
    let (stem, ext) = match base_name.rfind('.') {
        Some(dot) if dot > 0 => {
            let s = base_name.get(..dot).unwrap_or("");
            let e = base_name.get(dot..).unwrap_or("");
            (s, e)
        }
        _ => (base_name, ""),
    };

    for n in 2..=9999u32 {
        let candidate_name = alloc::format!("{} ({}){}", stem, n, ext);
        let candidate_path = if directory == "/" {
            alloc::format!("/{}", candidate_name)
        } else {
            alloc::format!("{}/{}", directory, candidate_name)
        };

        if crate::fs::vfs::Vfs::metadata(&candidate_path).is_err() {
            return Ok(candidate_path);
        }
    }

    // All names taken (extremely unlikely).
    Err(KernelError::AlreadyExists)
}

// ---------------------------------------------------------------------------
// Convenience wrappers
// ---------------------------------------------------------------------------

/// Create a new empty text file.
pub fn create_text_file(directory: &str) -> KernelResult<String> {
    init();
    let template = get_by_name("Text Document")
        .ok_or(KernelError::NotFound)?;
    create(template.id, directory)
}

/// Create a new folder.
pub fn create_folder(directory: &str) -> KernelResult<String> {
    init();
    let template = get_by_name("Folder")
        .ok_or(KernelError::NotFound)?;
    create(template.id, directory)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (template_count, create_count, total_bytes).
pub fn stats() -> (usize, u64, u64) {
    let count = TEMPLATES.lock().len();
    (
        count,
        CREATE_COUNT.load(Ordering::Relaxed),
        TOTAL_BYTES.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    CREATE_COUNT.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the template system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: initialization.
    {
        init();
        let (count, _, _) = stats();
        assert!(count > 0);
        serial_println!("[templates] test 1 passed: init ({} templates)", count);
    }

    // Test 2: list templates.
    {
        let templates = list();
        assert!(!templates.is_empty());
        // Should be sorted by category.
        let mut prev_order = 0u32;
        for t in &templates {
            assert!(t.category.sort_order() >= prev_order);
            prev_order = t.category.sort_order();
        }
        serial_println!("[templates] test 2 passed: list + sort");
    }

    // Test 3: get by name.
    {
        let t = get_by_name("Text Document");
        assert!(t.is_some());
        let t = t.unwrap();
        assert_eq!(t.extension, ".txt");
        assert_eq!(t.category, Category::Core);
        serial_println!("[templates] test 3 passed: get by name");
    }

    // Test 4: category listing.
    {
        let core = list_category(Category::Core);
        assert!(!core.is_empty());
        assert!(core.iter().all(|t| t.category == Category::Core));
        serial_println!("[templates] test 4 passed: category listing");
    }

    // Test 5: register custom template.
    {
        let id = register(
            "Test Template", ".test", "test-file",
            Category::User,
            TemplateContent::Inline(b"test content".to_vec()),
            "text-plain", "text/plain", "test-app",
        )?;
        assert!(id > 0);
        let t = get(id);
        assert!(t.is_some());
        assert!(unregister(id));
        let t = get(id);
        assert!(t.is_none());
        serial_println!("[templates] test 5 passed: register + unregister");
    }

    // Test 6: unique name generation.
    {
        // Test with a known path that probably doesn't exist.
        let name = find_unique_name("/tmp", "selftest_template.txt")?;
        assert!(name.contains("selftest_template"));
        serial_println!("[templates] test 6 passed: unique name");
    }

    serial_println!("[templates] all 6 self-tests passed");
    Ok(())
}
