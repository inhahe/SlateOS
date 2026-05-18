//! OurOS File Explorer
//!
//! Graphical file manager with:
//! - Directory tree sidebar
//! - File/folder list with icon, name, size, date columns
//! - Address bar with path navigation
//! - Toolbar (back, forward, up, new folder, delete, rename)
//! - Status bar (item count, selected size)
//! - Sort by name/size/date/type
//! - File operations: copy, cut, paste, delete, rename
//! - View modes: list, grid/icon, details
//! - Keyboard navigation
//! - Recycle bin integration
//! - File type associations
//!
//! Uses the guitk library for UI rendering.

#[allow(dead_code)]
mod columns;
#[allow(dead_code)]
mod dropzone;
#[allow(dead_code)]
mod fileops;
#[allow(dead_code)]
mod thumbs;

use guitk::color::Color;
use guitk::render::RenderTree;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ============================================================================
// File entry
// ============================================================================

/// A file or directory entry displayed in the explorer.
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub file_type: FileType,
    pub selected: bool,
    pub icon_id: u32,
}

/// Known file types for icon/association purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Directory,
    Text,
    Image,
    Audio,
    Video,
    Archive,
    Executable,
    Document,
    Code,
    Unknown,
}

impl FileType {
    /// Determine file type from extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" | "log" | "md" | "rst" => Self::Text,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" => Self::Image,
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => Self::Audio,
            "mp4" | "avi" | "mkv" | "webm" | "mov" | "flv" => Self::Video,
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => Self::Archive,
            "exe" | "bin" | "sh" | "cmd" | "bat" => Self::Executable,
            "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx" => Self::Document,
            "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "html" | "css" | "java"
            | "go" | "toml" | "yaml" | "json" | "xml" => Self::Code,
            _ => Self::Unknown,
        }
    }

    /// Icon character for this file type (unicode placeholder).
    pub fn icon_char(self) -> char {
        match self {
            Self::Directory => '\u{1F4C1}', // folder
            Self::Text => '\u{1F4C4}',      // page
            Self::Image => '\u{1F5BC}',     // framed picture
            Self::Audio => '\u{1F3B5}',     // musical note
            Self::Video => '\u{1F3AC}',     // clapper board
            Self::Archive => '\u{1F4E6}',   // package
            Self::Executable => '\u{2699}', // gear
            Self::Document => '\u{1F4D1}',  // bookmark tabs
            Self::Code => '\u{1F4BB}',      // computer
            Self::Unknown => '\u{1F4C3}',   // page with curl
        }
    }
}

// ============================================================================
// View mode
// ============================================================================

/// How files are displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details,  // Table with columns
    List,     // Simple list
    Icons,    // Grid of icons
}

/// Sort criteria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Modified,
    Type,
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Ascending,
    Descending,
}

// ============================================================================
// Clipboard
// ============================================================================

/// File operation pending in clipboard.
#[derive(Clone, Debug)]
pub enum ClipboardOp {
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
}

// ============================================================================
// Explorer state
// ============================================================================

/// File explorer application state.
pub struct ExplorerState {
    /// Current directory.
    pub current_path: PathBuf,
    /// Entries in current directory.
    pub entries: Vec<FileEntry>,
    /// Navigation history (back stack).
    pub history_back: VecDeque<PathBuf>,
    /// Navigation history (forward stack).
    pub history_forward: VecDeque<PathBuf>,
    /// View mode.
    pub view_mode: ViewMode,
    /// Sort criteria.
    pub sort_by: SortBy,
    /// Sort direction.
    pub sort_dir: SortDir,
    /// Show hidden files (names starting with '.').
    pub show_hidden: bool,
    /// Clipboard.
    pub clipboard: Option<ClipboardOp>,
    /// Selected entry indices.
    pub selected_indices: Vec<usize>,
    /// Address bar text (for editing).
    pub address_text: String,
    /// Whether address bar is being edited.
    pub address_editing: bool,
    /// Status bar message.
    pub status_message: String,
    /// Tree sidebar expanded paths.
    pub tree_expanded: Vec<PathBuf>,
    /// Window dimensions.
    pub window_width: u32,
    pub window_height: u32,
    /// Sidebar width.
    pub sidebar_width: f32,
}

impl ExplorerState {
    pub fn new(start_path: &Path) -> Self {
        let mut state = Self {
            current_path: start_path.to_path_buf(),
            entries: Vec::new(),
            history_back: VecDeque::new(),
            history_forward: VecDeque::new(),
            view_mode: ViewMode::Details,
            sort_by: SortBy::Name,
            sort_dir: SortDir::Ascending,
            show_hidden: false,
            clipboard: None,
            selected_indices: Vec::new(),
            address_text: start_path.to_string_lossy().to_string(),
            address_editing: false,
            status_message: String::new(),
            tree_expanded: vec![PathBuf::from("/")],
            window_width: 900,
            window_height: 600,
            sidebar_width: 200.0,
        };
        state.load_directory();
        state
    }

    // ======================================================================
    // Navigation
    // ======================================================================

    /// Navigate to a new directory.
    pub fn navigate_to(&mut self, path: &Path) {
        if path == self.current_path {
            return;
        }
        self.history_back.push_back(self.current_path.clone());
        if self.history_back.len() > 50 {
            self.history_back.pop_front();
        }
        self.history_forward.clear();
        self.current_path = path.to_path_buf();
        self.address_text = self.current_path.to_string_lossy().to_string();
        self.selected_indices.clear();
        self.load_directory();
    }

    /// Go back in history.
    pub fn go_back(&mut self) {
        if let Some(prev) = self.history_back.pop_back() {
            self.history_forward.push_back(self.current_path.clone());
            self.current_path = prev;
            self.address_text = self.current_path.to_string_lossy().to_string();
            self.selected_indices.clear();
            self.load_directory();
        }
    }

    /// Go forward in history.
    pub fn go_forward(&mut self) {
        if let Some(next) = self.history_forward.pop_back() {
            self.history_back.push_back(self.current_path.clone());
            self.current_path = next;
            self.address_text = self.current_path.to_string_lossy().to_string();
            self.selected_indices.clear();
            self.load_directory();
        }
    }

    /// Navigate to parent directory.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.navigate_to(&parent);
        }
    }

    /// Open entry: navigate if directory, launch if file.
    pub fn open_entry(&mut self, index: usize) {
        if let Some(entry) = self.entries.get(index) {
            if entry.is_dir {
                let path = entry.path.clone();
                self.navigate_to(&path);
            } else {
                // In a real implementation, launch the associated application
                self.status_message = format!("Opening: {}", entry.name);
            }
        }
    }

    // ======================================================================
    // Directory loading
    // ======================================================================

    /// Load entries from the current directory.
    pub fn load_directory(&mut self) {
        self.entries.clear();

        match fs::read_dir(&self.current_path) {
            Ok(read_dir) => {
                for entry_result in read_dir {
                    let entry = match entry_result {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    let name = entry.file_name().to_string_lossy().to_string();

                    // Skip hidden files if not showing them
                    if !self.show_hidden && name.starts_with('.') {
                        continue;
                    }

                    let path = entry.path();
                    let meta = fs::metadata(&path).ok();
                    let is_dir = meta.as_ref().map_or(false, |m| m.is_dir());
                    let size = meta.as_ref().map_or(0, |m| m.len());
                    let modified = meta.as_ref().and_then(|m| m.modified().ok());

                    let file_type = if is_dir {
                        FileType::Directory
                    } else {
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        FileType::from_extension(&ext)
                    };

                    self.entries.push(FileEntry {
                        name,
                        path,
                        is_dir,
                        size,
                        modified,
                        file_type,
                        selected: false,
                        icon_id: 0,
                    });
                }
            }
            Err(e) => {
                self.status_message = format!("Error: {e}");
            }
        }

        self.sort_entries();
        self.update_status();
    }

    /// Sort entries according to current sort settings.
    fn sort_entries(&mut self) {
        // Directories always come first
        self.entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            let ord = match self.sort_by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Type => {
                    let ext_a = a.path.extension().map(|e| e.to_string_lossy().to_string());
                    let ext_b = b.path.extension().map(|e| e.to_string_lossy().to_string());
                    ext_a.cmp(&ext_b)
                }
            };

            match self.sort_dir {
                SortDir::Ascending => ord,
                SortDir::Descending => ord.reverse(),
            }
        });
    }

    /// Update the status bar message.
    fn update_status(&mut self) {
        let dir_count = self.entries.iter().filter(|e| e.is_dir).count();
        let file_count = self.entries.len() - dir_count;
        let total_size: u64 = self.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();

        self.status_message = format!(
            "{} folder(s), {} file(s) — {}",
            dir_count,
            file_count,
            format_size(total_size)
        );
    }

    // ======================================================================
    // Selection
    // ======================================================================

    pub fn select_single(&mut self, index: usize) {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            entry.selected = i == index;
        }
        self.selected_indices = vec![index];
    }

    pub fn toggle_selection(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.selected = !entry.selected;
            if entry.selected {
                self.selected_indices.push(index);
            } else {
                self.selected_indices.retain(|&i| i != index);
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selected_indices.clear();
        for (i, entry) in self.entries.iter_mut().enumerate() {
            entry.selected = true;
            self.selected_indices.push(i);
        }
    }

    pub fn deselect_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
        self.selected_indices.clear();
    }

    // ======================================================================
    // File operations
    // ======================================================================

    /// Copy selected files to clipboard.
    pub fn copy_selected(&mut self) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();
        if !paths.is_empty() {
            self.clipboard = Some(ClipboardOp::Copy(paths));
            self.status_message = format!(
                "{} item(s) copied to clipboard",
                self.selected_indices.len()
            );
        }
    }

    /// Cut selected files to clipboard.
    pub fn cut_selected(&mut self) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();
        if !paths.is_empty() {
            self.clipboard = Some(ClipboardOp::Cut(paths));
            self.status_message = format!(
                "{} item(s) cut to clipboard",
                self.selected_indices.len()
            );
        }
    }

    /// Paste clipboard contents into current directory.
    pub fn paste(&mut self) {
        let op = match self.clipboard.take() {
            Some(op) => op,
            None => {
                self.status_message = "Nothing to paste".to_string();
                return;
            }
        };

        match op {
            ClipboardOp::Copy(paths) => {
                for src in &paths {
                    let name = src.file_name().unwrap_or_default();
                    let dst = self.current_path.join(name);
                    if src.is_dir() {
                        let _ = copy_dir_recursive(src, &dst);
                    } else {
                        let _ = fs::copy(src, &dst);
                    }
                }
                self.clipboard = Some(ClipboardOp::Copy(paths));
                self.status_message = "Paste complete".to_string();
            }
            ClipboardOp::Cut(paths) => {
                for src in &paths {
                    let name = src.file_name().unwrap_or_default();
                    let dst = self.current_path.join(name);
                    let _ = fs::rename(src, &dst);
                }
                self.status_message = "Move complete".to_string();
            }
        }

        self.load_directory();
    }

    /// Delete selected files (move to recycle bin or permanent delete).
    pub fn delete_selected(&mut self, permanent: bool) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();

        for path in &paths {
            if permanent {
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            } else {
                // Move to recycle bin (/var/recycle or ~/.local/share/Trash)
                let trash_dir = PathBuf::from("/var/recycle");
                let _ = fs::create_dir_all(&trash_dir);
                if let Some(name) = path.file_name() {
                    let dst = trash_dir.join(name);
                    let _ = fs::rename(path, &dst);
                }
            }
        }

        self.status_message = format!("{} item(s) deleted", paths.len());
        self.load_directory();
    }

    /// Create a new folder.
    pub fn create_folder(&mut self, name: &str) {
        let path = self.current_path.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {
                self.status_message = format!("Created folder: {name}");
                self.load_directory();
            }
            Err(e) => {
                self.status_message = format!("Error creating folder: {e}");
            }
        }
    }

    /// Rename an entry.
    pub fn rename_entry(&mut self, index: usize, new_name: &str) {
        if let Some(entry) = self.entries.get(index) {
            let old_path = &entry.path;
            let new_path = old_path.with_file_name(new_name);
            match fs::rename(old_path, &new_path) {
                Ok(()) => {
                    self.status_message = format!("Renamed to: {new_name}");
                    self.load_directory();
                }
                Err(e) => {
                    self.status_message = format!("Rename failed: {e}");
                }
            }
        }
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the complete file explorer UI.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // Background
        tree.fill_rect(0.0, 0.0, w, h, Color::from_hex(0xF5F5F5));

        // Toolbar (top)
        self.render_toolbar(&mut tree);

        // Address bar
        self.render_address_bar(&mut tree);

        // Sidebar (directory tree)
        self.render_sidebar(&mut tree);

        // File list
        self.render_file_list(&mut tree);

        // Status bar (bottom)
        self.render_status_bar(&mut tree);

        tree
    }

    fn render_toolbar(&self, tree: &mut RenderTree) {
        let toolbar_h = 36.0;
        tree.fill_rect(0.0, 0.0, self.window_width as f32, toolbar_h, Color::from_hex(0xE8E8E8));

        // Navigation buttons
        let buttons = ["\u{2190}", "\u{2192}", "\u{2191}", "|", "\u{1F4C1}+", "\u{2702}", "\u{1F4CB}"];
        let mut x = 8.0;
        for btn_text in &buttons {
            if *btn_text == "|" {
                // Separator
                tree.fill_rect(x, 4.0, 1.0, toolbar_h - 8.0, Color::from_hex(0xC0C0C0));
                x += 12.0;
            } else {
                tree.fill_rect(x, 4.0, 28.0, 28.0, Color::from_hex(0xD0D0D0));
                tree.text(x + 6.0, 10.0, btn_text, Color::from_hex(0x333333), 14.0);
                x += 32.0;
            }
        }
    }

    fn render_address_bar(&self, tree: &mut RenderTree) {
        let bar_y = 36.0;
        let bar_h = 28.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, bar_h, Color::WHITE);
        tree.stroke_rect(4.0, bar_y + 2.0, w - 8.0, bar_h - 4.0, Color::from_hex(0xC0C0C0), 1.0);
        tree.text(12.0, bar_y + 7.0, &self.address_text, Color::BLACK, 13.0);
    }

    fn render_sidebar(&self, tree: &mut RenderTree) {
        let sidebar_y = 64.0;
        let sidebar_h = self.window_height as f32 - 64.0 - 24.0; // minus toolbar and status bar
        let sw = self.sidebar_width;

        tree.fill_rect(0.0, sidebar_y, sw, sidebar_h, Color::from_hex(0xF0F0F0));
        tree.stroke_rect(sw - 1.0, sidebar_y, 1.0, sidebar_h, Color::from_hex(0xD0D0D0), 1.0);

        // Quick access items
        let items = ["/ (Root)", "/home", "/tmp", "/var", "/usr"];
        for (i, item) in items.iter().enumerate() {
            let iy = sidebar_y + 8.0 + i as f32 * 24.0;
            tree.text(16.0, iy + 4.0, item, Color::from_hex(0x333333), 12.0);
        }
    }

    fn render_file_list(&self, tree: &mut RenderTree) {
        let list_x = self.sidebar_width;
        let list_y = 64.0;
        let list_w = self.window_width as f32 - self.sidebar_width;
        let list_h = self.window_height as f32 - 64.0 - 24.0;

        // Column headers (details mode)
        if self.view_mode == ViewMode::Details {
            let header_h = 22.0;
            tree.fill_rect(list_x, list_y, list_w, header_h, Color::from_hex(0xE0E0E0));
            tree.text(list_x + 32.0, list_y + 4.0, "Name", Color::from_hex(0x333333), 11.0);
            tree.text(list_x + list_w - 200.0, list_y + 4.0, "Size", Color::from_hex(0x333333), 11.0);
            tree.text(list_x + list_w - 100.0, list_y + 4.0, "Modified", Color::from_hex(0x333333), 11.0);

            // Entries
            let row_h = 22.0;
            let start_y = list_y + header_h;
            let visible_rows = ((list_h - header_h) / row_h) as usize;

            for (i, entry) in self.entries.iter().take(visible_rows).enumerate() {
                let ey = start_y + i as f32 * row_h;

                // Selection highlight
                if entry.selected {
                    tree.fill_rect(list_x, ey, list_w, row_h, Color::from_hex(0xCCE8FF));
                } else if i % 2 == 1 {
                    tree.fill_rect(list_x, ey, list_w, row_h, Color::from_hex(0xFAFAFA));
                }

                // Icon
                let icon = if entry.is_dir { "\u{1F4C1}" } else { "\u{1F4C4}" };
                tree.text(list_x + 8.0, ey + 3.0, icon, Color::BLACK, 12.0);

                // Name
                let name_color = if entry.is_dir {
                    Color::from_hex(0x0066CC)
                } else {
                    Color::BLACK
                };
                tree.text(list_x + 32.0, ey + 4.0, &entry.name, name_color, 12.0);

                // Size
                if !entry.is_dir {
                    tree.text(
                        list_x + list_w - 200.0,
                        ey + 4.0,
                        &format_size(entry.size),
                        Color::GRAY,
                        11.0,
                    );
                }
            }
        }
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let bar_y = self.window_height as f32 - 24.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, 24.0, Color::from_hex(0xE8E8E8));
        tree.text(8.0, bar_y + 5.0, &self.status_message, Color::from_hex(0x555555), 11.0);
    }

    // ======================================================================
    // Sort
    // ======================================================================

    pub fn set_sort(&mut self, by: SortBy) {
        if self.sort_by == by {
            // Toggle direction
            self.sort_dir = match self.sort_dir {
                SortDir::Ascending => SortDir::Descending,
                SortDir::Descending => SortDir::Ascending,
            };
        } else {
            self.sort_by = by;
            self.sort_dir = SortDir::Ascending;
        }
        self.sort_entries();
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.load_directory();
    }
}

// ============================================================================
// Utility functions
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_dst = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &entry_dst)?;
        } else {
            fs::copy(entry.path(), &entry_dst)?;
        }
    }
    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Start in home directory or root
    let start_path = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"));

    let mut explorer = ExplorerState::new(&start_path);

    // Render initial view
    let render = explorer.render();
    println!("File Explorer initialized at: {}", explorer.current_path.display());
    println!("  {} entries loaded", explorer.entries.len());
    println!("  {} render commands", render.len());
    println!("  Status: {}", explorer.status_message);

    // Demonstrate navigation
    if explorer.entries.iter().any(|e| e.is_dir) {
        let first_dir_idx = explorer.entries.iter().position(|e| e.is_dir).unwrap_or(0);
        explorer.open_entry(first_dir_idx);
        println!(
            "\nNavigated to: {}",
            explorer.current_path.display()
        );
        println!("  {} entries", explorer.entries.len());

        // Go back
        explorer.go_back();
        println!(
            "Back to: {}",
            explorer.current_path.display()
        );
    }

    println!("\nFile Explorer ready.");
}
