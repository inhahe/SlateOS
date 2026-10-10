//! SlateOS Font Manager: the fonts on this machine, each shown in its own
//! letters; a font file installed; the user's own fonts removed.
//!
//! # What it shows
//!
//! Every family the toolkit's font index finds -- the index every program
//! draws from (`guitk::fontdb`), over the same folders -- through
//! [`library::Library`]: by name, filtered to all of them, the user's own or
//! the fixed-pitch ones, and searched by name. The family chosen is shown in
//! its own letters at three sizes, drawn by the font crate every program
//! draws with ([`preview`]) and handed to the compositor as pictures, since a
//! window's text commands draw in the interface's fonts only; beside them,
//! its styles, whether it is fixed-pitch, whose it is and its files.
//!
//! # What it changes
//!
//! - **Install a font...** copies a font file the user chooses into their
//!   fonts folder (`~/.local/share/fonts`), after reading it as a font --
//!   refused, with why, if it is not one, or if they have it already.
//! - **Remove** deletes the user's own files of the family chosen, after
//!   asking, and says what else a file shared with another family takes with
//!   it. The system's fonts are not this program's to delete, and its Remove
//!   says so.
//!
//! A program reads the fonts once, when it starts, so a change here is seen by
//! programs started afterwards; the window says so after each.
//!
//! # What it no longer pretends
//!
//! Until 2026-10-10 this program listed nineteen fonts written into its
//! source, whatever the machine had; its Install added a line to that list and
//! copied no file, and its Remove took the line away; its preview drew every
//! family in the interface's own font; and its rendering settings -- hinting,
//! antialiasing, subpixel order, a default size -- were kept by nothing and
//! read by nothing. All of that is gone: the list, the files and the preview
//! are the machine's, and the settings wait for something to obey them
//! (known issue `E-the-font-manager-lists-invented-fonts-and-installs-nothing`).

mod library;
mod preview;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use appearance::Palette;
use guitk::color::Color;
use guitk::dialog::{DialogAction, FileDialog};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::textedit;
use guitk::textinput::TextInput;
use oswindow::app::{self, App, ImageChange, Response};
use pathtext::ShowPath;

use library::{FontPlaces, Library};

// ============================================================================
// Layout
// ============================================================================

const DEFAULT_WINDOW_WIDTH: f32 = 1040.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 680.0;
const TOOLBAR_HEIGHT: f32 = 52.0;
const STATUS_HEIGHT: f32 = 30.0;
const SIDEBAR_WIDTH: f32 = 190.0;
const PANEL_WIDTH: f32 = 420.0;
const PADDING: f32 = 16.0;
const ROW_HEIGHT: f32 = 46.0;
const SIDEBAR_ROW_HEIGHT: f32 = 34.0;
const BUTTON_HEIGHT: f32 = 32.0;
const SEARCH_WIDTH: f32 = 280.0;
const TEXT_SIZE: f32 = 13.0;

/// The sizes, in pixels to the em, a family is shown at, and the line each
/// shows: the alphabet's every letter at the small sizes, a few at the large.
const PREVIEW_LINES: [(f32, &str); 3] = [
    (
        14.0,
        "The quick brown fox jumps over the lazy dog. 0123456789",
    ),
    (22.0, "The quick brown fox jumps over the lazy dog"),
    (40.0, "Aa Bb Cc Gg Qq 3 & ?"),
];

/// The most of a family's files the panel lists by name; the rest are
/// counted.
const FILES_LISTED: usize = 6;

/// The longest search typed, in characters.
const SEARCH_CAPACITY: usize = 128;

/// The keys this window answers, as a reader sees them.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("Up / Down", "Move through the fonts"),
    ("Home / End", "The first or last font"),
    ("Ctrl+F", "Search the fonts by name"),
    ("Ctrl+O", "Install a font file"),
    ("Delete", "Remove the font chosen, when it is yours"),
    ("Esc", "Leave the search, or put a question away"),
];

/// Which families the list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Every family.
    All,
    /// Families with a face of the user's own.
    Yours,
    /// Families with a fixed-pitch face: what a terminal can use.
    FixedPitch,
}

impl Filter {
    /// Every filter, in the sidebar's order.
    pub const ALL: [Self; 3] = [Self::All, Self::Yours, Self::FixedPitch];

    /// The sidebar's words for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All fonts",
            Self::Yours => "Yours",
            Self::FixedPitch => "Fixed-pitch",
        }
    }

    /// Whether `family` is one it shows.
    #[must_use]
    pub fn shows(self, family: &library::Family) -> bool {
        match self {
            Self::All => true,
            Self::Yours => family.has_own(),
            Self::FixedPitch => family.is_fixed_pitch(),
        }
    }
}

/// What the window last did, said in its status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// The sentence.
    pub text: String,
    /// Whether it says something failed.
    pub failed: bool,
}

/// A removal asked about: the family, the files it would delete, and the
/// other families those files hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// The family.
    pub family: String,
    /// The user's own files of it.
    pub files: Vec<PathBuf>,
    /// The other families with a face in those files.
    pub sharing: Vec<String>,
}

/// The pictures of the family shown: which family they are of, in which
/// colours and how wide, and their ids, so they are made again only when one
/// of those changes.
#[derive(Debug, Default)]
struct Previews {
    /// What the pictures show: the family, the panel's colour, the ink, and
    /// the width they were drawn for.
    key: Option<(String, u32, u32, u32)>,
    /// Each picture's id and size, in [`PREVIEW_LINES`]'s order; `None` for a
    /// line the face could not be drawn at.
    shown: Vec<Option<(u64, u32, u32)>>,
    /// Whether the family's face could not be read at all.
    unreadable: bool,
    /// The next id to give a picture: never reused.
    next_id: u64,
    /// What the compositor is to be told, oldest first.
    changes: Vec<ImageChange>,
}

/// Where each control is, this frame: what drawing and pressing both read,
/// so a control is pressed where it is drawn.
#[derive(Clone, Debug, Default)]
struct Layout {
    search: Rect,
    install: Rect,
    filters: Vec<(Rect, Filter)>,
    list: Rect,
    rows: Vec<(Rect, usize)>,
    remove: Option<Rect>,
    confirm: Option<(Rect, Rect)>,
}

/// The Font Manager.
pub struct FontManager {
    /// The fonts on this machine.
    library: Library,
    /// Which families the list shows.
    filter: Filter,
    /// What is typed in the search box.
    search: TextInput,
    /// Whether the search box has the keyboard.
    search_focused: bool,
    /// The family chosen, by name.
    selected: Option<String>,
    /// How far the list is scrolled down, in pixels.
    list_scroll: f32,
    /// What the window last did.
    message: Option<Message>,
    /// The file picker, while a font file is being chosen.
    dialog: Option<FileDialog>,
    /// A removal waiting for the user's answer.
    confirm: Option<Removal>,
    /// Whether the shortcut card is up.
    show_help: bool,
    /// The window's clipboard, for the search box's copy and paste.
    clipboard: String,
    /// The pictures of the family shown.
    previews: Previews,
    window_width: f32,
    window_height: f32,
    palette: Palette,
}

impl FontManager {
    /// The Font Manager over the fonts in `places`, the first family chosen.
    #[must_use]
    pub fn new(places: FontPlaces) -> Self {
        let library = Library::scan(places);
        let selected = library.families().first().map(|f| f.name.clone());
        Self {
            library,
            filter: Filter::All,
            search: TextInput::new(),
            search_focused: false,
            selected,
            list_scroll: 0.0,
            message: None,
            dialog: None,
            confirm: None,
            show_help: false,
            clipboard: String::new(),
            previews: Previews {
                next_id: 1,
                ..Previews::default()
            },
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
        }
    }

    /// The families the list shows: the filter's, whose name has what is
    /// searched for, in any case.
    #[must_use]
    pub fn visible(&self) -> Vec<&library::Family> {
        let query = self.search.text().trim().to_lowercase();
        self.library
            .families()
            .iter()
            .filter(|family| self.filter.shows(family))
            .filter(|family| query.is_empty() || family.name.to_lowercase().contains(&query))
            .collect()
    }

    /// The family chosen, if it is still installed.
    #[must_use]
    pub fn chosen(&self) -> Option<&library::Family> {
        self.selected
            .as_deref()
            .and_then(|name| self.library.family(name))
    }

    /// Choose the family at `index` among those shown, and scroll it into
    /// view.
    fn choose(&mut self, index: usize) {
        let Some(name) = self.visible().get(index).map(|f| f.name.clone()) else {
            return;
        };
        self.selected = Some(name);
        let top = index_px(index);
        let view = self.list_rect().h;
        if top < self.list_scroll {
            self.list_scroll = top;
        } else if top + ROW_HEIGHT > self.list_scroll + view {
            self.list_scroll = top + ROW_HEIGHT - view;
        }
    }

    /// Where the family chosen is among those shown.
    fn chosen_index(&self) -> Option<usize> {
        let chosen = self.selected.as_deref()?;
        self.visible()
            .iter()
            .position(|family| family.name.eq_ignore_ascii_case(chosen))
    }

    /// Keep the choice among the families shown -- the first, if the filter
    /// or the search took it away -- and the list within its rows.
    fn settle(&mut self) {
        if self.chosen_index().is_none() {
            self.selected = self.visible().first().map(|f| f.name.clone());
        }
        let rows = f32_count(self.visible().len());
        let most = (rows * ROW_HEIGHT - self.list_rect().h).max(0.0);
        self.list_scroll = self.list_scroll.clamp(0.0, most);
    }

    // ========================================================================
    // Changing the fonts
    // ========================================================================

    /// Open the file picker on the folder a font is most likely in: the
    /// user's Downloads, else their home.
    fn open_install_dialog(&mut self) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let start = home
            .as_ref()
            .map(|home| home.join("Downloads"))
            .filter(|downloads| downloads.is_dir())
            .or(home)
            .unwrap_or_else(std::env::temp_dir);
        let mut dialog = FileDialog::open()
            .with_initial_path(start)
            .with_filter("Fonts", &["*.ttf", "*.otf", "*.ttc", "*.otc"]);
        dialog.set_entries(guitk::dialog::list_directory(dialog.current_path()));
        self.dialog = Some(dialog);
    }

    /// Apply the file picker's answer: install the file chosen, or put the
    /// picker away.
    fn apply_dialog_answer(&mut self, action: DialogAction) {
        match action {
            DialogAction::Selected(path) => {
                self.dialog = None;
                self.install(&path);
            }
            DialogAction::Cancelled => self.dialog = None,
            DialogAction::NavigatedTo(path) => {
                if let Some(dialog) = self.dialog.as_mut() {
                    dialog.set_entries(guitk::dialog::list_directory(&path));
                }
            }
            DialogAction::None => {}
        }
    }

    /// Install the font file at `path`, choose its family, and say what
    /// happened.
    fn install(&mut self, path: &Path) {
        self.message = Some(match self.library.install(path) {
            Ok(installed) => {
                self.filter = Filter::All;
                self.search.clear();
                self.selected = Some(installed.family.clone());
                self.settle();
                if let Some(index) = self.chosen_index() {
                    self.choose(index);
                }
                Message {
                    text: format!(
                        "Installed {} {}. Programs started from now on can use it.",
                        installed.family, installed.style
                    ),
                    failed: false,
                }
            }
            Err(e) => Message {
                text: format!("{} was not installed: {e}.", file_name(path)),
                failed: true,
            },
        });
    }

    /// Ask about removing the family chosen -- if any of it is the user's.
    fn ask_to_remove(&mut self) {
        let Some(family) = self.chosen() else {
            return;
        };
        let files = family.own_files();
        if files.is_empty() {
            return;
        }
        let sharing = self.library.sharing(&family.name, &files);
        self.confirm = Some(Removal {
            family: family.name.clone(),
            files,
            sharing,
        });
    }

    /// Remove the family asked about, and say what happened.
    fn remove_confirmed(&mut self) {
        let Some(removal) = self.confirm.take() else {
            return;
        };
        self.message = Some(match self.library.remove(&removal.family) {
            Ok(deleted) => Message {
                text: format!(
                    "Removed {}: {deleted} file{} deleted. Programs started from now on will not have it.",
                    removal.family,
                    if deleted == 1 { "" } else { "s" }
                ),
                failed: false,
            },
            Err(e) => Message {
                text: format!("{} was not removed: {e}.", removal.family),
                failed: true,
            },
        });
        self.settle();
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// Handle an input event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if let Event::Resize { width, height } = event {
            self.window_width = *width as f32;
            self.window_height = *height as f32;
            self.settle();
            return EventResult::Consumed;
        }
        // The picker is modal: what is typed is a file's name, and a press
        // is in its list.
        if self.dialog.is_some() {
            let (width, height) = (self.window_width, self.window_height);
            let action = match (event, self.dialog.as_mut()) {
                (Event::Key(key), Some(dialog)) if key.pressed => dialog.handle_event(key, height),
                (Event::Mouse(mouse), Some(dialog)) => dialog.handle_mouse(mouse, width, height),
                _ => DialogAction::None,
            };
            self.apply_dialog_answer(action);
            return EventResult::Consumed;
        }
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        let plain = textline::is_plain(key.modifiers);
        if key.key == Key::F1 && plain {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            if plain && matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        // A question about a removal is answered before anything else hears
        // a key: Enter removes, Escape keeps.
        if self.confirm.is_some() {
            match key.key {
                Key::Enter if plain => self.remove_confirmed(),
                Key::Escape if plain => self.confirm = None,
                _ => {}
            }
            return EventResult::Consumed;
        }
        if textline::is_ctrl_chord(key.modifiers) {
            return match key.key {
                Key::F => {
                    self.search_focused = true;
                    EventResult::Consumed
                }
                Key::O => {
                    self.open_install_dialog();
                    EventResult::Consumed
                }
                _ if self.search_focused => self.search_key(key),
                _ => EventResult::Ignored,
            };
        }
        if self.search_focused {
            if plain && matches!(key.key, Key::Escape | Key::Enter) {
                self.search_focused = false;
                return EventResult::Consumed;
            }
            if matches!(key.key, Key::Up | Key::Down) {
                self.search_focused = false;
            } else {
                return self.search_key(key);
            }
        }
        if !plain {
            return EventResult::Ignored;
        }
        let count = self.visible().len();
        let at = self.chosen_index();
        let to = match key.key {
            Key::Down => at.map_or(Some(0), |at| at.checked_add(1).filter(|n| *n < count)),
            Key::Up => at.and_then(|at| at.checked_sub(1)).or(at),
            Key::Home => (count > 0).then_some(0),
            Key::End => count.checked_sub(1),
            Key::Delete => {
                self.ask_to_remove();
                return EventResult::Consumed;
            }
            Key::Escape if !self.search.text().is_empty() => {
                self.search.clear();
                self.settle();
                return EventResult::Consumed;
            }
            _ => return EventResult::Ignored,
        };
        if let Some(index) = to {
            self.choose(index);
        }
        EventResult::Consumed
    }

    /// A key for the search box: `textline`'s keys, the list following what
    /// is typed.
    fn search_key(&mut self, key: &KeyEvent) -> EventResult {
        let edit = textline::apply_key(
            &mut self.search,
            key,
            SEARCH_CAPACITY,
            &self.clipboard,
            TEXT_SIZE,
        );
        if let Some(copied) = edit.copied {
            self.clipboard = copied;
        }
        if edit.handled {
            self.list_scroll = 0.0;
            self.settle();
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        if self.show_help {
            if matches!(mouse.kind, MouseEventKind::Press(_)) {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        let layout = self.layout();
        if let MouseEventKind::Scroll { dy, .. } = mouse.kind {
            if self.confirm.is_none() && layout.list.contains(mouse.x, mouse.y) {
                let before = self.list_scroll;
                self.list_scroll += guitk::wheel::pixels(dy, ROW_HEIGHT);
                self.settle();
                return if (self.list_scroll - before).abs() > f32::EPSILON {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                };
            }
            return EventResult::Ignored;
        }
        if !matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (x, y) = (mouse.x, mouse.y);
        // The question is modal: its two buttons, and nothing under it.
        if self.confirm.is_some() {
            if let Some((remove, keep)) = layout.confirm {
                if remove.contains(x, y) {
                    self.remove_confirmed();
                } else if keep.contains(x, y) {
                    self.confirm = None;
                }
            }
            return EventResult::Consumed;
        }
        self.search_focused = layout.search.contains(x, y);
        if self.search_focused {
            return EventResult::Consumed;
        }
        if layout.install.contains(x, y) {
            self.open_install_dialog();
            return EventResult::Consumed;
        }
        if let Some((_, filter)) = layout.filters.iter().find(|(r, _)| r.contains(x, y)) {
            self.filter = *filter;
            self.list_scroll = 0.0;
            self.settle();
            return EventResult::Consumed;
        }
        if let Some((_, index)) = layout.rows.iter().find(|(r, _)| r.contains(x, y)) {
            self.choose(*index);
            return EventResult::Consumed;
        }
        if layout.remove.is_some_and(|r| r.contains(x, y)) {
            self.ask_to_remove();
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    // ========================================================================
    // Layout
    // ========================================================================

    /// The list's area: between the sidebar and the panel, under the
    /// toolbar, over the status line.
    fn list_rect(&self) -> Rect {
        Rect::new(
            SIDEBAR_WIDTH,
            TOOLBAR_HEIGHT,
            (self.window_width - SIDEBAR_WIDTH - PANEL_WIDTH).max(0.0),
            (self.window_height - TOOLBAR_HEIGHT - STATUS_HEIGHT).max(0.0),
        )
    }

    /// The panel's area, at the window's right.
    fn panel_rect(&self) -> Rect {
        let x = (self.window_width - PANEL_WIDTH).max(SIDEBAR_WIDTH);
        Rect::new(
            x,
            TOOLBAR_HEIGHT,
            (self.window_width - x).max(0.0),
            (self.window_height - TOOLBAR_HEIGHT - STATUS_HEIGHT).max(0.0),
        )
    }

    /// Where every control is now.
    fn layout(&self) -> Layout {
        let search = Rect::new(
            SIDEBAR_WIDTH + PADDING,
            (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0,
            SEARCH_WIDTH,
            BUTTON_HEIGHT,
        );
        let install_width = button_width(INSTALL_LABEL);
        let install = Rect::new(
            self.window_width - PADDING - install_width,
            (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0,
            install_width,
            BUTTON_HEIGHT,
        );
        let filters = Filter::ALL
            .iter()
            .enumerate()
            .map(|(n, filter)| {
                (
                    Rect::new(
                        8.0,
                        TOOLBAR_HEIGHT + PADDING + f32_count(n) * SIDEBAR_ROW_HEIGHT,
                        SIDEBAR_WIDTH - 16.0,
                        SIDEBAR_ROW_HEIGHT,
                    ),
                    *filter,
                )
            })
            .collect();
        let list = self.list_rect();
        let rows = (0..self.visible().len())
            .map(|index| {
                (
                    Rect::new(
                        list.x,
                        list.y + index_px(index) - self.list_scroll,
                        list.w,
                        ROW_HEIGHT,
                    ),
                    index,
                )
            })
            .filter(|(r, _)| r.bottom() > list.y && r.y < list.bottom())
            .collect();
        let remove = self
            .chosen()
            .filter(|family| family.has_own())
            .map(|_| self.remove_button());
        let confirm = self.confirm.as_ref().map(|_| self.confirm_buttons());
        Layout {
            search,
            install,
            filters,
            list,
            rows,
            remove,
            confirm,
        }
    }

    /// The panel's Remove button, at its foot.
    fn remove_button(&self) -> Rect {
        let panel = self.panel_rect();
        Rect::new(
            panel.x + PADDING,
            panel.bottom() - PADDING - BUTTON_HEIGHT - 22.0,
            button_width(REMOVE_LABEL),
            BUTTON_HEIGHT,
        )
    }

    /// The question's box, in the window's middle.
    fn confirm_box(&self) -> Rect {
        let (w, h) = (460.0, 190.0);
        Rect::new(
            (self.window_width - w) / 2.0,
            (self.window_height - h) / 2.0,
            w,
            h,
        )
    }

    /// The question's Remove and Keep buttons.
    fn confirm_buttons(&self) -> (Rect, Rect) {
        let b = self.confirm_box();
        let keep_w = button_width(KEEP_LABEL);
        let remove_w = button_width(REMOVE_LABEL);
        let y = b.bottom() - PADDING - BUTTON_HEIGHT;
        let keep = Rect::new(b.right() - PADDING - keep_w, y, keep_w, BUTTON_HEIGHT);
        let remove = Rect::new(keep.x - 8.0 - remove_w, y, remove_w, BUTTON_HEIGHT);
        (remove, keep)
    }

    // ========================================================================
    // Pictures
    // ========================================================================

    /// Make the family chosen's pictures again if what they show has
    /// changed -- another family, other colours, another width -- giving
    /// the old ones back first.
    fn refresh_previews(&mut self) {
        let width = preview_width(self.panel_rect().w);
        let background = argb(self.palette.mantle);
        let ink = argb(self.palette.text);
        let key = self
            .chosen()
            .map(|family| (family.name.clone(), background, ink, width));
        if key == self.previews.key {
            return;
        }
        for (id, _, _) in self.previews.shown.drain(..).flatten() {
            self.previews.changes.push(ImageChange::Drop(id));
        }
        self.previews.unreadable = false;
        self.previews.key.clone_from(&key);
        let Some((name, ..)) = key else {
            return;
        };
        // Parsed once and shared by every line: the parse is the costly part,
        // and the same for every size (`ScaledFont::shared`).
        let face = self.library.load(&name).map(std::sync::Arc::new);
        for (size, text) in PREVIEW_LINES {
            let picture = face
                .as_ref()
                .and_then(|face| preview::line(face, text, size, width, background, ink));
            let Some(picture) = picture else {
                self.previews.unreadable = true;
                self.previews.shown.push(None);
                continue;
            };
            let id = self.previews.next_id;
            self.previews.next_id = id.saturating_add(1);
            self.previews.changes.push(ImageChange::Upload {
                id,
                width: picture.width,
                height: picture.height,
                stride: picture.width.saturating_mul(4),
                format: oswindow::PixelFormat::Argb8888,
                bytes: guitk::canvas::WireBytes::from_le_argb(&picture.pixels),
            });
            self.previews
                .shown
                .push(Some((id, picture.width, picture.height)));
        }
    }

    // ========================================================================
    // Drawing
    // ========================================================================

    /// The whole window.
    pub fn render_tree(&self) -> RenderTree {
        let pal = &self.palette;
        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, pal.base);
        let layout = self.layout();
        self.render_toolbar(&mut tree, &layout);
        self.render_sidebar(&mut tree, &layout);
        self.render_list(&mut tree, &layout);
        self.render_panel(&mut tree, &layout);
        self.render_status(&mut tree);
        if let Some(removal) = &self.confirm {
            self.render_confirm(&mut tree, removal);
        }
        if let Some(dialog) = &self.dialog {
            tree.commands
                .extend(dialog.render(pal, self.window_width, self.window_height));
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut tree,
                pal,
                (self.window_width, self.window_height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
        }
        tree
    }

    fn render_toolbar(&self, tree: &mut RenderTree, layout: &Layout) {
        let pal = &self.palette;
        tree.fill_rect(0.0, 0.0, self.window_width, TOOLBAR_HEIGHT, pal.mantle);
        bold(tree, PADDING, 16.0, "Fonts", pal.text, 18.0);
        let field = layout.search;
        guitk::field::draw(
            tree,
            pal,
            field,
            guitk::field::State {
                hovered: false,
                focused: self.search_focused,
                disabled: false,
                invalid: false,
            },
            guitk::style::FOCUS_RING_WIDTH,
        );
        if self.search.text().is_empty() && !self.search_focused {
            clipped(
                tree,
                field.x + 8.0,
                field.y + 8.0,
                "Search by name",
                pal.subtext0,
                TEXT_SIZE,
                field.w - 16.0,
            );
        } else {
            textedit::draw(
                tree,
                &textedit::SingleLine {
                    text: self.search.text(),
                    cursor: self.search.cursor(),
                    selection_anchor: self.search.selection_anchor(),
                    focused: self.search_focused,
                    x: field.x + 8.0,
                    y: field.y + 8.0,
                    width: field.w - 16.0,
                    line_height: 18.0,
                    font_size: TEXT_SIZE,
                    weight: FontWeightHint::Regular,
                    color: pal.text,
                    selection_bg: pal.accent,
                    selection_fg: pal.crust,
                    caret_width: 1.5,
                },
            );
        }
        button(tree, pal, layout.install, INSTALL_LABEL, true, true);
    }

    fn render_sidebar(&self, tree: &mut RenderTree, layout: &Layout) {
        let pal = &self.palette;
        tree.fill_rect(
            0.0,
            TOOLBAR_HEIGHT,
            SIDEBAR_WIDTH,
            self.window_height - TOOLBAR_HEIGHT - STATUS_HEIGHT,
            pal.mantle,
        );
        for (rect, filter) in &layout.filters {
            let count = self
                .library
                .families()
                .iter()
                .filter(|family| filter.shows(family))
                .count();
            let chosen = *filter == self.filter;
            if chosen {
                fill_rounded(tree, *rect, pal.surface1, 6.0);
            }
            tree.text(
                rect.x + 10.0,
                rect.y + 9.0,
                filter.label(),
                pal.text,
                TEXT_SIZE,
            );
            let shown = count.to_string();
            tree.text(
                rect.right() - 10.0 - guitk::text::width(&shown, TEXT_SIZE),
                rect.y + 9.0,
                &shown,
                pal.subtext0,
                TEXT_SIZE,
            );
        }
    }

    fn render_list(&self, tree: &mut RenderTree, layout: &Layout) {
        let pal = &self.palette;
        let list = layout.list;
        tree.clip(list.x, list.y, list.w, list.h);
        let visible = self.visible();
        if visible.is_empty() {
            let words = if self.library.families().is_empty() {
                "No fonts are installed here: text is drawn in the built-in face."
            } else if self.search.text().trim().is_empty() {
                "No fonts are of this kind."
            } else {
                "No font's name has that in it."
            };
            clipped(
                tree,
                list.x + PADDING,
                list.y + PADDING,
                words,
                pal.subtext0,
                TEXT_SIZE,
                list.w - 2.0 * PADDING,
            );
        }
        let chosen = self.chosen_index();
        for (rect, index) in &layout.rows {
            let Some(family) = visible.get(*index) else {
                continue;
            };
            if chosen == Some(*index) {
                fill_rounded(
                    tree,
                    Rect::new(rect.x + 6.0, rect.y + 2.0, rect.w - 12.0, rect.h - 4.0),
                    pal.surface1,
                    6.0,
                );
            }
            clipped(
                tree,
                rect.x + PADDING,
                rect.y + 7.0,
                &family.name,
                pal.text,
                14.0,
                rect.w - 2.0 * PADDING,
            );
            clipped(
                tree,
                rect.x + PADDING,
                rect.y + 26.0,
                &family_summary(family),
                pal.subtext0,
                12.0,
                rect.w - 2.0 * PADDING,
            );
        }
        tree.unclip();
    }

    fn render_panel(&self, tree: &mut RenderTree, layout: &Layout) {
        let pal = &self.palette;
        let panel = self.panel_rect();
        tree.fill_rect(panel.x, panel.y, panel.w, panel.h, pal.mantle);
        tree.clip(panel.x, panel.y, panel.w, panel.h);
        let x = panel.x + PADDING;
        let wide = panel.w - 2.0 * PADDING;
        let Some(family) = self.chosen() else {
            clipped(
                tree,
                x,
                panel.y + PADDING,
                "Choose a font to see it.",
                pal.subtext0,
                TEXT_SIZE,
                wide,
            );
            tree.unclip();
            return;
        };
        let mut y = panel.y + PADDING;
        bold(tree, x, y, &family.name, pal.text, 20.0);
        y += 34.0;
        if self.previews.unreadable && self.previews.shown.iter().all(Option::is_none) {
            clipped(
                tree,
                x,
                y,
                "This font cannot be drawn: its file could not be read as a font.",
                pal.ink(pal.red),
                TEXT_SIZE,
                wide,
            );
            y += 24.0;
        }
        for (id, w, h) in self.previews.shown.iter().flatten() {
            tree.push(RenderCommand::Image {
                x: x - 4.0,
                y,
                width: f32_u32(*w),
                height: f32_u32(*h),
                image_id: *id,
            });
            y += f32_u32(*h) + 4.0;
        }
        y += 8.0;
        let whose = if family.all_own() {
            "Yours"
        } else if family.has_own() {
            "Partly yours, partly the system's"
        } else {
            "The system's"
        };
        let rows = [
            ("Styles", family.style_names().join(", ")),
            (
                "Fixed-pitch",
                if family.is_fixed_pitch() { "Yes" } else { "No" }.to_owned(),
            ),
            ("Whose", whose.to_owned()),
        ];
        for (label, value) in rows {
            tree.text(x, y, label, pal.subtext0, 12.0);
            for line in guitk::text::wrap(&value, wide - 100.0, TEXT_SIZE, FontWeightHint::Regular)
            {
                tree.text(x + 100.0, y, &line, pal.text, TEXT_SIZE);
                y += 18.0;
            }
            y += 4.0;
        }
        tree.text(x, y, "Files", pal.subtext0, 12.0);
        let mut files: Vec<PathBuf> = Vec::new();
        for face in &family.faces {
            if !files.contains(&face.path) {
                files.push(face.path.clone());
            }
        }
        for path in files.iter().take(FILES_LISTED) {
            clipped(
                tree,
                x + 100.0,
                y,
                &path.as_path().shown().to_string(),
                pal.text,
                12.0,
                wide - 100.0,
            );
            y += 17.0;
        }
        if files.len() > FILES_LISTED {
            let more = files.len().saturating_sub(FILES_LISTED);
            tree.text(
                x + 100.0,
                y,
                &format!("and {more} more"),
                pal.subtext0,
                12.0,
            );
        }
        match layout.remove {
            Some(rect) => button(tree, pal, rect, REMOVE_LABEL, false, true),
            None => {
                let rect = self.remove_button();
                button(tree, pal, rect, REMOVE_LABEL, false, false);
                clipped(
                    tree,
                    rect.right() + 10.0,
                    rect.y + 8.0,
                    "The system's fonts are shared by every account.",
                    pal.subtext0,
                    12.0,
                    panel.right() - rect.right() - 10.0 - PADDING,
                );
            }
        }
        clipped(
            tree,
            x,
            panel.bottom() - PADDING - 14.0,
            "Programs started after a change see it.",
            pal.subtext0,
            12.0,
            wide,
        );
        tree.unclip();
    }

    fn render_status(&self, tree: &mut RenderTree) {
        let pal = &self.palette;
        let y = self.window_height - STATUS_HEIGHT;
        tree.fill_rect(0.0, y, self.window_width, STATUS_HEIGHT, pal.crust);
        let (text, color) = match &self.message {
            Some(Message { text, failed: true }) => (text.clone(), pal.ink(pal.red)),
            Some(Message {
                text,
                failed: false,
            }) => (text.clone(), pal.text),
            None => {
                let count = self.library.families().len();
                (
                    format!(
                        "{count} font famil{} installed.",
                        if count == 1 { "y" } else { "ies" }
                    ),
                    pal.subtext0,
                )
            }
        };
        clipped(
            tree,
            PADDING,
            y + 8.0,
            &text,
            color,
            12.0,
            self.window_width - 2.0 * PADDING,
        );
    }

    fn render_confirm(&self, tree: &mut RenderTree, removal: &Removal) {
        let pal = &self.palette;
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            Color::rgba(0, 0, 0, 120),
        );
        let b = self.confirm_box();
        fill_rounded(tree, b, pal.surface0, 10.0);
        let wide = b.w - 2.0 * PADDING;
        bold(
            tree,
            b.x + PADDING,
            b.y + PADDING,
            &format!("Remove {}?", removal.family),
            pal.text,
            16.0,
        );
        let count = removal.files.len();
        let mut said = format!(
            "{count} file{} of yours {} deleted from your fonts folder.",
            if count == 1 { "" } else { "s" },
            if count == 1 { "is" } else { "are" }
        );
        if !removal.sharing.is_empty() {
            said.push_str(&format!(
                " The same files hold {}, which go{} with them.",
                removal.sharing.join(", "),
                if removal.sharing.len() == 1 { "es" } else { "" }
            ));
        }
        let mut y = b.y + PADDING + 30.0;
        for line in guitk::text::wrap(&said, wide, TEXT_SIZE, FontWeightHint::Regular) {
            tree.text(b.x + PADDING, y, &line, pal.text, TEXT_SIZE);
            y += 18.0;
        }
        let (remove, keep) = self.confirm_buttons();
        button(tree, pal, remove, REMOVE_LABEL, true, true);
        button(tree, pal, keep, KEEP_LABEL, false, true);
    }
}

// ============================================================================
// Drawing helpers
// ============================================================================

const INSTALL_LABEL: &str = "Install a font...";
const REMOVE_LABEL: &str = "Remove";
const KEEP_LABEL: &str = "Keep it";

/// How wide a button with `label` is.
fn button_width(label: &str) -> f32 {
    guitk::text::padded_width(label, 14.0, TEXT_SIZE, FontWeightHint::Regular)
}

/// A push button: the accent's colour if `primary`, dimmed if not `live`.
fn button(
    tree: &mut RenderTree,
    pal: &Palette,
    rect: Rect,
    label: &str,
    primary: bool,
    live: bool,
) {
    let (fill, ink) = match (live, primary) {
        (false, _) => (pal.surface0, pal.overlay0),
        (true, true) => (pal.accent, appearance::readable_on(pal.accent)),
        (true, false) => (pal.surface1, pal.text),
    };
    fill_rounded(tree, rect, fill, 6.0);
    tree.text(rect.x + 14.0, rect.y + 8.0, label, ink, TEXT_SIZE);
}

fn fill_rounded(tree: &mut RenderTree, rect: Rect, color: Color, radius: f32) {
    tree.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn bold(tree: &mut RenderTree, x: f32, y: f32, content: &str, color: Color, size: f32) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Text cut at `max_width` with an ellipsis, so a long name says it is
/// longer than shown.
fn clipped(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    content: &str,
    color: Color,
    size: f32,
    max_width: f32,
) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_width.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A family's second line in the list: how many styles, and what else is
/// worth knowing at a glance.
fn family_summary(family: &library::Family) -> String {
    let styles = family.style_names().len();
    let mut said = format!("{styles} style{}", if styles == 1 { "" } else { "s" });
    if family.has_own() {
        said.push_str(" - yours");
    }
    if family.is_fixed_pitch() {
        said.push_str(" - fixed-pitch");
    }
    said
}

/// A file's name, for a sentence about it.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.shown().to_string(),
        |name| Path::new(name).shown().to_string(),
    )
}

/// `color` as the rasterizer's `0xAARRGGBB`.
fn argb(color: Color) -> u32 {
    (u32::from(color.a) << 24)
        | (u32::from(color.r) << 16)
        | (u32::from(color.g) << 8)
        | u32::from(color.b)
}

/// How wide the preview pictures are drawn for a panel `panel_w` wide.
fn preview_width(panel_w: f32) -> u32 {
    let wide = (panel_w - 2.0 * PADDING + 8.0).clamp(1.0, 4096.0);
    // Clamped to a range `u32` holds exactly.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        wide as u32
    }
}

/// The top of the `index`-th row, from the list's top.
fn index_px(index: usize) -> f32 {
    f32_count(index) * ROW_HEIGHT
}

/// A count as a coordinate. A list of fonts is far inside the range `f32`
/// counts exactly.
#[allow(clippy::cast_precision_loss)]
fn f32_count(n: usize) -> f32 {
    n as f32
}

/// A picture's size as a coordinate; a picture is far smaller than `f32`'s
/// exact range.
#[allow(clippy::cast_precision_loss)]
fn f32_u32(n: u32) -> f32 {
    n as f32
}

impl App for FontManager {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Font Manager".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (DEFAULT_WINDOW_WIDTH as u32, DEFAULT_WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        // Made while drawing, so the pictures go up before the frame that
        // names them (`App::take_images`).
        self.refresh_previews();
        self.render_tree()
    }

    fn take_images(&mut self) -> Vec<ImageChange> {
        std::mem::take(&mut self.previews.changes)
    }

    // No `tick_interval`: nothing here ages.
}

fn main() -> ExitCode {
    app::launch("fontmanager", &mut FontManager::new(FontPlaces::standard()))
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
