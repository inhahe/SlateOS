//! Turning events into edits.
//!
//! [`EditorState`] models a text editor and draws one; until this module it had
//! no way to be *used*. Every operation on it — `insert_char`, `move_down`,
//! `undo`, `find` — was public and reachable only from a test. The demo `main`
//! typed "Hello" by calling `insert_char` five times.
//!
//! This is the layer that connects a keystroke to one of those operations, and
//! it is deliberately the *only* one. `main` hands events here and draws what
//! comes back; it does not itself know that Ctrl+S saves.
//!
//! ## The rule this module exists to enforce
//!
//! Two things must happen after the caret moves, and they are easy to forget
//! because nothing breaks visibly when only one is done:
//!
//! * [`Document::ensure_cursor_visible`] scrolls the *lines* so the caret's row
//!   is on screen.
//! * [`EditorState::ensure_caret_visible_horizontally`] scrolls the *pixels* so
//!   the caret's column is on screen.
//!
//! Miss the first and the caret vanishes off the bottom of a long file; miss the
//! second and it vanishes off the right of a long line. So no code here moves
//! the caret directly. Every motion goes through [`EditorState::moving`], which
//! handles the selection anchor and then calls both. There is one place to get
//! this right rather than the thirty-odd key bindings below.
//!
//! ## Modes
//!
//! Three, checked in this order, because each one owns the keyboard while it is
//! up:
//!
//! | When | Keys go to |
//! |---|---|
//! | `external_prompt` is set | the file-changed-on-disk prompt |
//! | `find_visible` | the find/replace bar |
//! | otherwise | the document |
//!
//! The find bar takes the keyboard because that is where the user is typing —
//! but Ctrl chords it does not use fall through to the document's, so Ctrl+S
//! still saves with the bar open.

use crate::{Document, EditorState, ExternalChoice};
use guitk::dialog::{DialogAction, FileDialog};
use guitk::event::EventResult;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::menu::MenuItemId;
use guitk::menubar::{MenuBarEntry, MenuBarEvent, MenuBarItem};
// The verdict every handler below returns. Until the editor was moved onto the
// shared harness this was `EditorResponse`, declared in this file, with the same
// three variants and the same meanings -- a second name for a concept the loop
// that consumes it already had a name for. Two enums for one concept is this
// tree's most-repeated defect, and it bites hardest at a seam like this one:
// `main` translated one into the other on every single event, so a variant added
// to one and not to the other would have been a translation that silently picked
// a wrong arm.
use oswindow::app::Response;

/// The visual width of one tab in the tab bar, and the gap after it.
///
/// Read by both the renderer and the hit test, so that clicking a tab selects
/// the one that was drawn there. Two numbers that must agree are one number.
pub const TAB_WIDTH: f32 = 160.0;
/// Horizontal gap between tabs, in pixels.
pub const TAB_GAP: f32 = 1.0;
/// Width of the close box at the right end of a tab.
pub const TAB_CLOSE_WIDTH: f32 = 24.0;

/// One thing the editor can be asked to do.
///
/// The editor had a keyboard and nothing else, so every command lived inside
/// the `match` arm of the key that ran it: "what Ctrl+F does" and "what Find
/// does" were the same thing only by being the same six lines, written once.
/// A second way to ask -- a menu -- is exactly the point where that stops
/// holding, so the arms moved out into named methods and this enum names them.
/// The keyboard and the menu are two *editors* of one command model rather
/// than two models of one command. That is the distinction the comment at the
/// top of this file is already about.
///
/// Only commands a pointer can reach are here. Caret motion (Ctrl+Left, Home)
/// stays in the key tables: there is no menu row for "move one word left", and
/// inventing one would be a menu written for this enum's benefit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Read a file chosen from a dialog into a new tab.
    Open,
    /// Write the active document back to its file.
    Save,
    /// Write the active document to a path chosen from a dialog.
    SaveAs,
    /// Close the active tab.
    CloseTab,
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
    /// Copy the selection, then delete it.
    Cut,
    /// Copy the selection.
    Copy,
    /// Insert the clipboard at the caret.
    Paste,
    /// Select the whole document.
    SelectAll,
    /// Select the word under the caret.
    SelectWord,
    /// Open the find bar.
    Find,
    /// Open the find bar with the caret in the replacement field.
    Replace,
}

impl Command {
    /// Every command, in the order the menus present them.
    ///
    /// The one table. A variant left out of it is absent from the menu *and*
    /// undispatchable -- consistently missing rather than silently running
    /// something else, which is why [`Command::id`] is the discriminant rather
    /// than a position in this list.
    pub const ALL: [Self; 13] = [
        Self::Open,
        Self::Save,
        Self::SaveAs,
        Self::CloseTab,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::SelectWord,
        Self::Find,
        Self::Replace,
    ];

    /// The identifier a menu row carries.
    #[must_use]
    pub fn id(self) -> MenuItemId {
        self as MenuItemId
    }

    /// The command a menu row's identifier names.
    #[must_use]
    pub fn from_id(id: MenuItemId) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// The row's text.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open...",
            Self::Save => "Save",
            Self::SaveAs => "Save As...",
            Self::CloseTab => "Close Tab",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select All",
            Self::SelectWord => "Select Word",
            Self::Find => "Find...",
            Self::Replace => "Replace...",
        }
    }

    /// The keystroke shown alongside the row.
    ///
    /// Spelled out rather than derived from the key tables, and that is a real
    /// duplication -- the lesser of two. Deriving it would need the tables to
    /// answer "which key runs Save", and they are `match` arms *on the key*,
    /// which can only answer the question the other way round. The test
    /// `every_shortcut_a_menu_advertises_is_really_bound` closes the gap by
    /// pressing each one and checking the command ran.
    #[must_use]
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Open => "Ctrl+O",
            Self::Save => "Ctrl+S",
            Self::SaveAs => "Ctrl+Shift+S",
            Self::CloseTab => "Ctrl+W",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Y",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::SelectAll => "Ctrl+A",
            Self::SelectWord => "Ctrl+D",
            Self::Find => "Ctrl+F",
            Self::Replace => "Ctrl+H",
        }
    }
}

/// Which of the find bar's two text fields the keyboard is typing into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindField {
    /// The search term.
    #[default]
    Query,
    /// The replacement text.
    Replace,
}

impl EditorState {
    // ======================================================================
    // Entry point
    // ======================================================================

    /// Apply one event and say what the caller should do about it.
    pub fn handle_event(&mut self, event: &Event) -> Response {
        // Before anything else except a resize, which every surface needs.
        if !matches!(event, Event::Resize { .. })
            && let Some(response) = self.dialog_event(event)
        {
            return response;
        }
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                if self.resize(*width, *height) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            // Coming back to the window is exactly when another program is
            // likely to have written the file — a build, a formatter, a `git
            // checkout` in the terminal the user just came from.
            Event::FocusIn => {
                if self.check_external_change() {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            // A drag that ends outside the window never delivers its release,
            // so losing focus has to end it too. Otherwise the next mouse move
            // over the window — with no button held — would go on extending the
            // selection.
            Event::FocusOut => {
                self.dragging = false;
                Response::Idle
            }
            Event::CloseRequested => Response::Exit,
            _ => Response::Idle,
        }
    }

    /// Adopt a new window size; `true` if it was not the size already held.
    ///
    /// Two callers, which is the whole reason it is a method rather than the
    /// body of the `Event::Resize` arm above: that arm, and the editor's
    /// `App::render`, which is handed the size the compositor actually granted
    /// and may be the first thing to learn of it. A compositor is free to give a
    /// window a different size from the one it was asked for without ever
    /// calling that a resize, and the first frame is drawn before any event has
    /// arrived at all — so if the two fix-ups below lived only in the event arm,
    /// the opening frame of a window the compositor sized down would lay out its
    /// gutter and status bar for a viewport that is not the one on screen.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        if self.window_width == width && self.window_height == height {
            return false;
        }
        self.window_width = width;
        self.window_height = height;
        // The viewport just changed size, so the caret may now be outside it in
        // either direction.
        let visible = self.visible_lines();
        self.active_document_mut().ensure_cursor_visible(visible);
        self.ensure_caret_visible_horizontally();
        true
    }

    // ======================================================================
    // Keyboard
    // ======================================================================

    fn handle_key(&mut self, key: &KeyEvent) -> Response {
        // Modifier state is kept here because mouse events do not carry it:
        // `MouseEvent` has a position and a kind and nothing else, so shift-click
        // can only be recognised from what the keyboard last reported.
        self.modifiers = key.modifiers;
        if !key.pressed {
            return Response::Idle;
        }

        // A status message describes the last thing that happened. The next
        // keystroke is the next thing, so it goes — and clearing it is itself a
        // visible change, which is why the response is upgraded below rather
        // than being left as whatever the binding returned.
        let had_status = self.status.take().is_some();
        let response = self.dispatch_key(key);
        if had_status && response == Response::Idle {
            Response::Redraw
        } else {
            response
        }
    }

    fn dispatch_key(&mut self, key: &KeyEvent) -> Response {
        if self.external_prompt.is_some() {
            return self.prompt_key(key);
        }
        // The bar sees the key after the modal prompt, which is asking a
        // question that has to be answered first, and before the typing tables,
        // which it cannot steal from: a closed bar claims Alt+mnemonic only.
        if let Some(response) = self.menu_bar_key(key) {
            return response;
        }
        if self.find_visible
            && let Some(response) = self.find_key(key)
        {
            return response;
        }
        if key.modifiers.ctrl {
            return self.control_key(key);
        }
        self.editing_key(key)
    }

    /// Keys while the file-changed-on-disk prompt is up.
    ///
    /// The prompt is modal on purpose: it is asking which of two versions of the
    /// file the buffer should hold, and every editing key would be applied to an
    /// answer that has not been given yet.
    fn prompt_key(&mut self, key: &KeyEvent) -> Response {
        let reviewing = self
            .external_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.review.is_some());
        if reviewing {
            return match key.key {
                Key::Enter => {
                    self.review_accept();
                    Response::Redraw
                }
                Key::Escape => {
                    self.review_cancel();
                    Response::Redraw
                }
                _ => Response::Idle,
            };
        }
        let choice = match key.key {
            Key::K => ExternalChoice::KeepCurrent,
            Key::R => ExternalChoice::Reload,
            Key::M => ExternalChoice::Merge,
            Key::V => ExternalChoice::Review,
            Key::Escape => {
                self.dismiss_external();
                return Response::Redraw;
            }
            _ => return Response::Idle,
        };
        self.resolve_external(choice);
        Response::Redraw
    }

    /// Keys while the find bar is open.
    ///
    /// Returns `None` for anything the bar does not claim, so it falls through
    /// to the document's bindings — Ctrl+S must still save while searching.
    fn find_key(&mut self, key: &KeyEvent) -> Option<Response> {
        if key.modifiers.ctrl {
            return match key.key {
                Key::R => {
                    if key.modifiers.shift {
                        let n = self.replace_all_matches();
                        self.status = Some(format!("Replaced {n} occurrence(s)"));
                    } else {
                        self.replace_current_match();
                    }
                    self.after_cursor_move();
                    Some(Response::Redraw)
                }
                Key::I => {
                    self.find.case_sensitive = !self.find.case_sensitive;
                    self.refresh_matches();
                    Some(Response::Redraw)
                }
                Key::E => {
                    self.find.use_regex = !self.find.use_regex;
                    self.refresh_matches();
                    Some(Response::Redraw)
                }
                _ => None,
            };
        }
        match key.key {
            Key::Escape => {
                self.find_visible = false;
                Some(Response::Redraw)
            }
            Key::Tab => {
                self.find_field = match self.find_field {
                    FindField::Query => FindField::Replace,
                    FindField::Replace => FindField::Query,
                };
                Some(Response::Redraw)
            }
            Key::Enter => {
                if key.modifiers.shift {
                    self.goto_match(false);
                } else {
                    self.goto_match(true);
                }
                self.after_cursor_move();
                Some(Response::Redraw)
            }
            Key::Backspace => {
                let field = self.find_field;
                let changed = self.find_field_mut(field).pop().is_some();
                if field == FindField::Query {
                    self.refresh_matches();
                }
                Some(if changed {
                    Response::Redraw
                } else {
                    Response::Idle
                })
            }
            _ => {
                // Control characters would otherwise be appended literally: the
                // Enter and Tab cases above are handled, but a key that reports
                // text of '\u{8}' or '\u{1b}' must not become part of the query.
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return None;
                }
                let field = self.find_field;
                self.find_field_mut(field).push_str(&typed);
                if field == FindField::Query {
                    self.refresh_matches();
                }
                Some(Response::Redraw)
            }
        }
    }

    fn find_field_mut(&mut self, field: FindField) -> &mut String {
        match field {
            FindField::Query => &mut self.find.query,
            FindField::Replace => &mut self.find.replace_text,
        }
    }

    // The four wrappers below exist for one reason: `FindState`'s operations
    // take the document as an argument, so calling them as
    // `self.find.find_all(self.active_document())` borrows `self` twice.
    // Destructuring names the two fields separately, which is what tells the
    // borrow checker they are disjoint — a `&mut self` method that reached for
    // both through `self` could not be proved so.

    /// Re-run the search after the query or its options changed.
    fn refresh_matches(&mut self) {
        let Self { find, tabs, .. } = self;
        find.find_all(tabs.active());
    }

    /// Move the caret to the next (or previous) match.
    fn goto_match(&mut self, forward: bool) {
        let Self { find, tabs, .. } = self;
        if forward {
            find.next_match(tabs.active_mut());
        } else {
            find.prev_match(tabs.active_mut());
        }
    }

    fn replace_current_match(&mut self) {
        let Self { find, tabs, .. } = self;
        find.replace_current(tabs.active_mut());
    }

    fn replace_all_matches(&mut self) -> usize {
        let Self { find, tabs, .. } = self;
        find.replace_all(tabs.active_mut())
    }

    /// Whether a command has anything to do right now.
    ///
    /// A row greyed by this is greyed because running it would do nothing, and
    /// each condition is the one the command itself already checks --
    /// `Document::undo` pops an empty stack and returns, so "the stack is
    /// empty" is a reading of its precondition rather than a guess at it.
    #[must_use]
    pub fn command_enabled(&self, command: Command) -> bool {
        let doc = self.active_document();
        match command {
            Command::Undo => !doc.undo_stack.is_empty(),
            Command::Redo => !doc.redo_stack.is_empty(),
            Command::Cut | Command::Copy => doc.has_selection(),
            Command::Paste => !self.clipboard.is_empty(),
            Command::Open
            | Command::Save
            | Command::SaveAs
            | Command::CloseTab
            | Command::SelectAll
            | Command::SelectWord
            | Command::Find
            | Command::Replace => true,
        }
    }

    /// Run a command, whoever asked for it.
    ///
    /// The single body of every command the menu and the keyboard share. A
    /// command that is not enabled does nothing and says so, which is what
    /// makes the greying above advisory rather than the only guard: a menu can
    /// be opened, the document changed under it by a reload, and the row
    /// clicked afterwards.
    pub fn run(&mut self, command: Command) -> Response {
        if !self.command_enabled(command) {
            return Response::Idle;
        }
        match command {
            Command::Open => {
                self.open_dialog(crate::DialogPurpose::Open);
                Response::Redraw
            }
            Command::Save => {
                self.save_active();
                Response::Redraw
            }
            Command::SaveAs => {
                self.open_dialog(crate::DialogPurpose::SaveAs);
                Response::Redraw
            }
            Command::CloseTab => {
                self.close_active_tab();
                Response::Redraw
            }
            Command::Undo => {
                self.active_document_mut().undo();
                self.after_cursor_move();
                Response::Redraw
            }
            Command::Redo => {
                self.active_document_mut().redo();
                self.after_cursor_move();
                Response::Redraw
            }
            Command::Cut => {
                self.cut_selection();
                Response::Redraw
            }
            // Copying changes nothing on screen -- the selection it read is
            // still there and still looks the same -- so it asks for no frame.
            Command::Copy => {
                self.copy_selection();
                Response::Idle
            }
            Command::Paste => {
                self.paste_clipboard();
                Response::Redraw
            }
            Command::SelectAll => {
                self.active_document_mut().select_all();
                self.after_cursor_move();
                Response::Redraw
            }
            Command::SelectWord => {
                self.active_document_mut().select_word_at_cursor();
                self.after_cursor_move();
                Response::Redraw
            }
            Command::Find => {
                self.open_find(FindField::Query);
                Response::Redraw
            }
            Command::Replace => {
                self.open_find(FindField::Replace);
                Response::Redraw
            }
        }
    }

    /// Put up the open-or-save dialog.
    ///
    /// Starts in the active document's own directory, which is where a user
    /// looking for a neighbouring file will look first; failing that, the home
    /// directory. A Save As is seeded with the current name so the common case
    /// -- same name, different folder -- is one click.
    fn open_dialog(&mut self, purpose: crate::DialogPurpose) {
        let doc = self.active_document();
        let start = doc
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map_or_else(std::env::temp_dir, std::path::Path::to_path_buf);
        let dialog = match purpose {
            crate::DialogPurpose::Open => FileDialog::open().with_initial_path(start),
            crate::DialogPurpose::SaveAs => FileDialog::save()
                .with_initial_path(start)
                .with_filename(doc.name.clone()),
        };
        self.dialog_purpose = purpose;
        self.dialog = Some(Self::listed(dialog));
    }

    /// Fill a dialog's listing from the directory it is showing.
    ///
    /// `FileDialog` does not read the filesystem -- the host answers with
    /// `set_entries`, the same division `guitk::pathbar` uses for its
    /// completions, so the toolkit stays testable without a disk. Forgetting
    /// this does not fail: the dialog opens, draws its chrome, and lists
    /// nothing at all, for ever.
    fn listed(mut dialog: FileDialog) -> FileDialog {
        let entries = guitk::dialog::list_directory(dialog.current_path());
        dialog.set_entries(entries);
        dialog
    }

    /// Act on a path the user chose, and say what to tell them.
    fn dialog_chose(&mut self, path: &std::path::Path) -> String {
        match self.dialog_purpose {
            crate::DialogPurpose::Open => match self.open_file(path) {
                Ok(()) => format!("Opened {}", path.display()),
                Err(e) => format!("Could not open {}: {e}", path.display()),
            },
            crate::DialogPurpose::SaveAs => match self.active_document_mut().save_as(path) {
                Ok(()) => format!("Saved as {}", path.display()),
                Err(e) => format!("Could not save to {}: {e}", path.display()),
            },
        }
    }

    /// Give the dialog an event, `None` if there is no dialog up.
    ///
    /// Modal: it answers everything while it is open, because it is asking
    /// which file and every editing key would be applied to an answer that has
    /// not been given yet. The same rule the external-change prompt follows.
    fn dialog_event(&mut self, event: &Event) -> Option<Response> {
        let height = self.window_height as f32;
        let width = self.window_width as f32;
        let action = {
            let dialog = self.dialog.as_mut()?;
            match event {
                Event::Key(key) if key.pressed => dialog.handle_event(key, height),
                Event::Mouse(mouse) => dialog.handle_mouse(mouse, width, height),
                // A resize or a focus change is not the dialog's to answer, but
                // it must not fall through to the document either: the dialog
                // is modal and the keystroke that dismisses it has not arrived.
                _ => DialogAction::None,
            }
        };
        match action {
            DialogAction::Selected(path) => {
                self.dialog = None;
                self.status = Some(self.dialog_chose(&path));
            }
            DialogAction::Cancelled => {
                self.dialog = None;
            }
            // The dialog has moved to another directory and is showing
            // whatever the last one held until it is told what is here.
            DialogAction::NavigatedTo(path) => {
                if let Some(dialog) = self.dialog.as_mut() {
                    dialog.set_entries(guitk::dialog::list_directory(&path));
                }
            }
            DialogAction::None => {}
        }
        Some(Response::Redraw)
    }

    /// Open the find bar with one of its fields focused.
    ///
    /// Searching for what is selected is what the user almost always wants, and
    /// typing over it costs one keystroke if not. Only a single-line selection:
    /// a search term with a newline in it cannot match anything, since matching
    /// is per line.
    fn open_find(&mut self, field: FindField) {
        self.find_visible = true;
        self.find_field = field;
        let selected = self.active_document().selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            self.find.query = selected;
        }
        self.refresh_matches();
    }

    /// Copy the selection, then delete it.
    fn cut_selection(&mut self) {
        if self.copy_selection() {
            self.active_document_mut().delete_selection();
            self.after_cursor_move();
        }
    }

    /// Insert the clipboard at the caret, replacing any selection.
    fn paste_clipboard(&mut self) {
        let text = self.clipboard.clone();
        let doc = self.active_document_mut();
        doc.delete_selection();
        doc.insert_text(&text);
        self.after_cursor_move();
    }

    /// Close the active tab, or say why it will not close.
    fn close_active_tab(&mut self) {
        if !self.close_tab() {
            self.status =
                Some("Unsaved changes — save with Ctrl+S, or Ctrl+Shift+W to discard".to_string());
        }
    }

    // ======================================================================
    // Menu bar
    // ======================================================================

    /// Rebuild the menu rows from the editor's current state.
    ///
    /// Called on the event that is about to *open* a menu rather than on every
    /// change, and **only while the bar is shut**: `MenuBar::set_items` closes
    /// any open dropdown, because the open index and the hovered row are
    /// offsets into the rows being replaced. Refreshing mid-navigation would
    /// shut the menu under the user on their first arrow key -- which is what
    /// it did, until `the_search_menu_opens_the_find_bar` said so.
    ///
    /// The top-level labels never change, so a shut bar is never stale, and
    /// the rows cannot go stale while open because every event reaches the bar
    /// before it reaches the document.
    pub fn refresh_menu_items(&mut self) {
        let row = |command: Command| MenuBarEntry::Action {
            label: command.label().to_string(),
            shortcut: Some(command.shortcut().to_string()),
            enabled: self.command_enabled(command),
            id: command.id(),
        };
        let items = vec![
            MenuBarItem {
                label: "&File".to_string(),
                children: vec![
                    row(Command::Open),
                    MenuBarEntry::Separator,
                    row(Command::Save),
                    row(Command::SaveAs),
                    MenuBarEntry::Separator,
                    row(Command::CloseTab),
                ],
            },
            MenuBarItem {
                label: "&Edit".to_string(),
                children: vec![
                    row(Command::Undo),
                    row(Command::Redo),
                    MenuBarEntry::Separator,
                    row(Command::Cut),
                    row(Command::Copy),
                    row(Command::Paste),
                    MenuBarEntry::Separator,
                    row(Command::SelectAll),
                    row(Command::SelectWord),
                ],
            },
            MenuBarItem {
                label: "&Search".to_string(),
                children: vec![row(Command::Find), row(Command::Replace)],
            },
        ];
        self.menu_bar.set_items(items);
    }

    /// The window size the bar lays its dropdowns out inside.
    fn viewport(&self) -> (f32, f32) {
        (self.window_width as f32, self.window_height as f32)
    }

    /// Run whatever the bar reported.
    ///
    /// Always a redraw: the bar consumed the event, which means it opened,
    /// closed or moved its highlight, and all three are visible whatever the
    /// command underneath did or did not change.
    fn drain_menu_bar(&mut self) -> Response {
        for event in self.menu_bar.drain_events() {
            if let MenuBarEvent::ItemClicked(id) = event
                && let Some(command) = Command::from_id(id)
            {
                // Discarded deliberately: see the note above on why the answer
                // is a redraw regardless of what the command reports.
                let _ = self.run(command);
            }
        }
        Response::Redraw
    }

    /// Offer a mouse event to the bar, `None` if it did not want it.
    fn menu_bar_mouse(&mut self, mouse: &MouseEvent) -> Option<Response> {
        if !self.menu_bar.is_open() {
            // A shut bar can only be reached by a pointer inside its own strip,
            // and a mouse *move* arrives on every pixel of travel, which is not
            // a rate to be allocating eleven strings at.
            if mouse.y >= crate::TAB_BAR_TOP {
                return None;
            }
            self.refresh_menu_items();
        }
        let viewport = self.viewport();
        if self.menu_bar.handle_mouse_event(mouse, viewport) == EventResult::Ignored {
            return None;
        }
        Some(self.drain_menu_bar())
    }

    /// Offer a key event to the bar, `None` if it did not want it.
    ///
    /// A closed bar claims only Alt+mnemonic, so this can sit ahead of the
    /// typing tables without swallowing anything typed into the document.
    fn menu_bar_key(&mut self, key: &KeyEvent) -> Option<Response> {
        if !self.menu_bar.is_open() {
            if !key.modifiers.alt {
                return None;
            }
            self.refresh_menu_items();
        }
        let viewport = self.viewport();
        if self.menu_bar.handle_key_event(key, viewport) == EventResult::Ignored {
            return None;
        }
        Some(self.drain_menu_bar())
    }

    /// Keys with Ctrl held, in the document.
    fn control_key(&mut self, key: &KeyEvent) -> Response {
        let shift = key.modifiers.shift;
        match key.key {
            Key::O => self.run(Command::Open),
            Key::S => self.run(if shift {
                Command::SaveAs
            } else {
                Command::Save
            }),
            Key::Z => self.run(if shift { Command::Redo } else { Command::Undo }),
            Key::Y => self.run(Command::Redo),
            Key::F => self.run(Command::Find),
            Key::H => self.run(Command::Replace),
            Key::A => self.run(Command::SelectAll),
            Key::C => self.run(Command::Copy),
            Key::X => self.run(Command::Cut),
            Key::V => self.run(Command::Paste),
            Key::D => self.run(Command::SelectWord),
            Key::W => self.run(Command::CloseTab),
            Key::Home => {
                self.moving(shift, Document::move_to_start);
                Response::Redraw
            }
            Key::End => {
                self.moving(shift, Document::move_to_end);
                Response::Redraw
            }
            Key::Left => {
                self.moving(shift, Document::move_word_left);
                Response::Redraw
            }
            Key::Right => {
                self.moving(shift, Document::move_word_right);
                Response::Redraw
            }
            Key::Tab | Key::PageDown => {
                self.cycle_tab(!shift);
                Response::Redraw
            }
            Key::PageUp => {
                self.cycle_tab(false);
                Response::Redraw
            }
            _ => Response::Idle,
        }
    }

    /// Keys with no Ctrl, in the document: motion and text.
    fn editing_key(&mut self, key: &KeyEvent) -> Response {
        let shift = key.modifiers.shift;
        match key.key {
            Key::Left => {
                self.moving(shift, Document::move_left);
                Response::Redraw
            }
            Key::Right => {
                self.moving(shift, Document::move_right);
                Response::Redraw
            }
            Key::Up => {
                self.moving(shift, Document::move_up);
                Response::Redraw
            }
            Key::Down => {
                self.moving(shift, Document::move_down);
                Response::Redraw
            }
            Key::Home => {
                self.moving(shift, Document::move_home);
                Response::Redraw
            }
            Key::End => {
                self.moving(shift, Document::move_end);
                Response::Redraw
            }
            Key::PageUp => {
                let page = self.visible_lines();
                self.moving(shift, |doc| {
                    doc.cursor_line = doc.cursor_line.saturating_sub(page);
                    doc.clamp_cursor();
                });
                Response::Redraw
            }
            Key::PageDown => {
                let page = self.visible_lines();
                self.moving(shift, |doc| {
                    doc.cursor_line = doc
                        .cursor_line
                        .saturating_add(page)
                        .min(doc.lines.len().saturating_sub(1));
                    doc.clamp_cursor();
                });
                Response::Redraw
            }
            Key::Escape => {
                // Collapse the selection to the caret. Somewhere for Escape to
                // go when there is no panel open, and the counterpart of
                // clicking in the text.
                if self.active_document().selection_anchor.is_none() {
                    return Response::Idle;
                }
                self.active_document_mut().selection_anchor = None;
                Response::Redraw
            }
            Key::Backspace => {
                let doc = self.active_document_mut();
                if !doc.delete_selection() {
                    doc.backspace();
                }
                self.after_cursor_move();
                Response::Redraw
            }
            Key::Delete => {
                let doc = self.active_document_mut();
                if !doc.delete_selection() {
                    doc.delete_forward();
                }
                self.after_cursor_move();
                Response::Redraw
            }
            Key::Enter => self.type_char('\n'),
            Key::Tab => self.type_char('\t'),
            _ => {
                // Everything else is text or nothing. Control characters are
                // excluded because the three that have bindings are handled
                // above, and the rest would be inserted as unprintable bytes.
                let mut response = Response::Idle;
                for ch in key.typed() {
                    response = self.type_char(ch);
                }
                response
            }
        }
    }

    /// Insert one character, replacing the selection if there is one.
    fn type_char(&mut self, ch: char) -> Response {
        let doc = self.active_document_mut();
        doc.delete_selection();
        doc.insert_char(ch);
        self.after_cursor_move();
        Response::Redraw
    }

    // ======================================================================
    // Mouse
    // ======================================================================

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> Response {
        // The bar gets first refusal -- except during a drag that began in the
        // text, which owns the pointer until it is released: a selection being
        // extended past the top of the window must not be taken over by a menu
        // the pointer merely crossed on the way. And not while the modal prompt
        // is up, for the reason the keyboard does not reach it either.
        if !self.dragging
            && self.external_prompt.is_none()
            && let Some(response) = self.menu_bar_mouse(mouse)
        {
            return response;
        }
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.mouse_press(mouse.x, mouse.y),
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                if self.caret_cursor_at(mouse.x, mouse.y).is_none() {
                    return Response::Idle;
                }
                self.mouse_press(mouse.x, mouse.y);
                self.active_document_mut().select_word_at_cursor();
                self.dragging = false;
                self.after_cursor_move();
                Response::Redraw
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if !self.dragging {
                    return Response::Idle;
                }
                self.dragging = false;
                Response::Idle
            }
            MouseEventKind::Move => {
                if !self.dragging {
                    return Response::Idle;
                }
                // A drag that leaves the text area does not stop extending: the
                // caret is clamped to the nearest position rather than the
                // selection freezing, which is what makes selecting past the
                // bottom of the screen possible at all.
                let Some((line, cursor)) = self.caret_cursor_at(mouse.x, mouse.y) else {
                    return Response::Idle;
                };
                // Through `set_cursor` so the affinity travels with the offset:
                // a drag that ends inside a right-to-left run has a side, and
                // dropping it would snap the caret to the far end of the line
                // on the next repaint.
                self.active_document_mut().set_cursor(line, cursor);
                self.after_cursor_move();
                Response::Redraw
            }
            MouseEventKind::Scroll { dy, .. } => {
                let doc = self.active_document_mut();
                // `dy` is a notch count, not a distance. This used to divide it
                // by the line height, which is what a pixel distance would
                // want — and one notch over a 21px line is 0.14 lines,
                // truncated to zero, so the wheel did nothing at all at any
                // speed. The accumulator also keeps the fractions a trackpad
                // sends, which the old truncation discarded outright.
                let lines = doc.wheel.rows(dy);
                if lines == 0 {
                    return Response::Idle;
                }
                let last = doc.lines.len().saturating_sub(1);
                let scrolled = if lines > 0 {
                    doc.scroll_line
                        .saturating_add(lines.unsigned_abs())
                        .min(last)
                } else {
                    doc.scroll_line.saturating_sub(lines.unsigned_abs())
                };
                if scrolled == doc.scroll_line {
                    return Response::Idle;
                }
                doc.scroll_line = scrolled;
                // Deliberately *not* followed by `ensure_cursor_visible`: the
                // wheel moves the view, not the caret. Scrolling that dragged
                // the caret along would lose the user's place the moment they
                // looked somewhere else in the file.
                Response::Redraw
            }
            _ => Response::Idle,
        }
    }

    /// A left press: put the caret where the pointer is, or act on the tab bar.
    fn mouse_press(&mut self, x: f32, y: f32) -> Response {
        if let Some((index, on_close)) = self.tab_at(x, y) {
            self.tabs.set_active(index);
            if on_close && !self.close_tab() {
                self.status = Some("Unsaved changes — save with Ctrl+S first".to_string());
            }
            return Response::Redraw;
        }
        let Some((line, cursor)) = self.caret_cursor_at(x, y) else {
            return Response::Idle;
        };
        let col = cursor.byte;
        // Shift-click extends from wherever the selection already starts, so
        // shift-clicking twice grows one selection rather than starting two.
        let extend = self.modifiers.shift;
        let doc = self.active_document_mut();
        if extend {
            if doc.selection_anchor.is_none() {
                doc.selection_anchor = Some((doc.cursor_line, doc.cursor_col));
            }
        } else {
            // The anchor is set to the press point even though nothing is
            // selected yet: the drag that may follow needs somewhere to extend
            // from. `has_selection` treats an anchor equal to the caret as no
            // selection, which is what keeps a plain click from arming a delete.
            doc.selection_anchor = Some((line, col));
        }
        // `set_cursor` rather than the two fields, so the affinity the click
        // carried is the one the caret is drawn with. A press at the seam
        // between a Latin and a Hebrew run has the same byte offset from
        // either side, and only the side says where the caret goes.
        doc.set_cursor(line, cursor);
        self.dragging = true;
        self.after_cursor_move();
        Response::Redraw
    }

    /// Which tab the point is over, and whether it is over that tab's close box.
    ///
    /// `None` for anything below the tab bar or past the last tab.
    #[must_use]
    pub fn tab_at(&self, x: f32, y: f32) -> Option<(usize, bool)> {
        if y < crate::TAB_BAR_TOP || y >= crate::TEXT_TOP || x < 0.0 {
            return None;
        }
        let pitch = TAB_WIDTH + TAB_GAP;
        let index = (x / pitch) as usize;
        if index >= self.tabs.count() {
            return None;
        }
        let within = x - (index as f32) * pitch;
        if within > TAB_WIDTH {
            // In the gap between two tabs.
            return None;
        }
        Some((index, within >= TAB_WIDTH - TAB_CLOSE_WIDTH))
    }

    // ======================================================================
    // Shared helpers
    // ======================================================================

    /// Run a caret motion with the selection and the scroll handled around it.
    ///
    /// `extend` is Shift: it keeps the anchor (creating one at the old caret if
    /// there was none) so the selection grows; without it the selection is
    /// dropped, which is what makes a bare arrow key collapse one.
    fn moving(&mut self, extend: bool, motion: impl FnOnce(&mut Document)) {
        let doc = self.active_document_mut();
        if extend {
            if doc.selection_anchor.is_none() {
                doc.selection_anchor = Some((doc.cursor_line, doc.cursor_col));
            }
        } else {
            doc.selection_anchor = None;
        }
        motion(doc);
        self.after_cursor_move();
    }

    /// Bring the caret back on screen, vertically and horizontally.
    ///
    /// Called after *every* caret movement. See the module docs: the two calls
    /// are separate and forgetting either is invisible until the caret is gone.
    fn after_cursor_move(&mut self) {
        let visible = self.visible_lines();
        self.active_document_mut().ensure_cursor_visible(visible);
        self.ensure_caret_visible_horizontally();
    }

    /// Copy the selection to the editor's clipboard. Returns whether there was
    /// anything to copy — an empty selection must not clear what was copied
    /// before, or Ctrl+C on nothing would silently discard the clipboard.
    fn copy_selection(&mut self) -> bool {
        let text = self.active_document().selected_text();
        if text.is_empty() {
            return false;
        }
        self.clipboard = text;
        true
    }

    /// Save the active document, reporting failure in the status bar.
    ///
    /// An editor driven by keystrokes has no return value to hand a refusal
    /// back through, so a failed save must land somewhere the user is looking.
    fn save_active(&mut self) {
        if self.active_document().path.is_none() {
            // Was "No file name — Save As needs a file dialog", which stopped
            // being true on 2026-09-14: there is a dialog, and this is the one
            // moment the user certainly wants it. Asking beats refusing.
            self.open_dialog(crate::DialogPurpose::SaveAs);
            self.status = Some("Choose where to save it".to_string());
            return;
        }
        match self.active_document_mut().save() {
            Ok(()) => {
                let name = self.active_document().name.clone();
                self.status = Some(format!("Saved {name}"));
            }
            Err(e) => self.status = Some(format!("Save failed: {e}")),
        }
    }

    /// Move to the next (or previous) tab, wrapping at both ends.
    fn cycle_tab(&mut self, forward: bool) {
        let count = self.tabs.count();
        if count < 2 {
            return;
        }
        let at = self.tabs.active_index();
        let next = if forward {
            // Written as a comparison rather than `% count` because a remainder
            // is a division, and the compiler cannot see that `count >= 2` here.
            let ahead = at.saturating_add(1);
            if ahead >= count { 0 } else { ahead }
        } else {
            at.checked_sub(1).unwrap_or(count.saturating_sub(1))
        };
        self.tabs.set_active(next);
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::Language;
    use guitk::event::Modifiers;

    fn editor_with(text: &str) -> EditorState {
        let mut editor = EditorState::new();
        let doc = editor.active_document_mut();
        doc.lines = text.lines().map(str::to_string).collect();
        if doc.lines.is_empty() {
            doc.lines.push(String::new());
        }
        doc.language = Language::Rust;
        editor
    }

    /// A key press with no character attached — a chord or a named key.
    fn press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// A key press that produces a character, as typing does.
    fn typed(ch: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        })
    }

    fn ctrl(key: Key) -> Event {
        press(key, Modifiers::ctrl())
    }

    fn shift(key: Key) -> Event {
        press(key, Modifiers::shift())
    }

    fn plain(key: Key) -> Event {
        press(key, Modifiers::NONE)
    }

    // ---- the menu bar and the commands under it ------------------------

    /// Turn a menu's advertised spelling ("Ctrl+S") into the keystroke it names.
    ///
    /// The test reads the same string the user does, so a shortcut advertised
    /// wrongly fails here rather than being quietly restated in the test.
    fn keystroke(spelling: &str) -> Event {
        let mut modifiers = Modifiers::NONE;
        let mut named: Option<Key> = None;
        for part in spelling.split('+') {
            match part {
                "Ctrl" => modifiers.ctrl = true,
                "Shift" => modifiers.shift = true,
                "Alt" => modifiers.alt = true,
                other => named = Some(letter_key(other)),
            }
        }
        let key = named.unwrap_or_else(|| panic!("{spelling} names no key"));
        press(key, modifiers)
    }

    fn letter_key(name: &str) -> Key {
        match name {
            "A" => Key::A,
            "C" => Key::C,
            "D" => Key::D,
            "F" => Key::F,
            "H" => Key::H,
            "O" => Key::O,
            "S" => Key::S,
            "V" => Key::V,
            "W" => Key::W,
            "X" => Key::X,
            "Y" => Key::Y,
            "Z" => Key::Z,
            other => panic!("no key is named {other}"),
        }
    }

    /// **Every shortcut a menu advertises really runs that command.**
    ///
    /// `Command::shortcut` spells the keystroke out by hand -- it cannot be
    /// derived, because the key tables are `match` arms *on the key* and can
    /// only answer "what does Ctrl+S do", never "what runs Save". This is the
    /// test that doc comment promises: it presses what the menu advertises and
    /// checks the command actually happened, so a row labelled with a
    /// keystroke nobody bound fails here.
    ///
    /// The inner `match` is exhaustive on purpose. A command added to the enum
    /// stops this file compiling until someone says what pressing its
    /// advertised key should do -- the one moment when whoever is adding it
    /// holds both halves of the answer at once.
    #[test]
    fn every_shortcut_a_menu_advertises_is_really_bound() {
        for command in Command::ALL {
            let event = keystroke(command.shortcut());
            match command {
                Command::Open => {
                    let mut editor = editor_with("ab");
                    assert!(editor.dialog.is_none());
                    editor.handle_event(&event);
                    assert!(
                        editor.dialog.is_some(),
                        "Ctrl+O did not put up a file dialog"
                    );
                }
                Command::SaveAs => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&event);
                    assert!(
                        editor.dialog.is_some(),
                        "Ctrl+Shift+S did not put up a file dialog"
                    );
                    assert_eq!(
                        editor.dialog_purpose,
                        crate::DialogPurpose::SaveAs,
                        "the dialog is up but asking the wrong question"
                    );
                }
                Command::Save => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&event);
                    // A document with no path cannot be saved without asking
                    // where, so Ctrl+S puts up Save As. It used to answer "No
                    // file name -- Save As needs a file dialog", which was true
                    // until 2026-09-14. The property is unchanged and only its
                    // evidence moved: the assertion is still "Ctrl+S reached
                    // Save", now witnessed by the dialog rather than by a
                    // refusal.
                    assert!(
                        editor.dialog.is_some(),
                        "{} did not reach Save: {:?}",
                        command.shortcut(),
                        editor.status
                    );
                }
                Command::CloseTab => {
                    let mut editor = editor_with("ab");
                    editor.tabs.open(Document::new());
                    assert_eq!(editor.tabs.count(), 2);
                    editor.handle_event(&event);
                    assert_eq!(editor.tabs.count(), 1, "Ctrl+W did not reach Close Tab");
                }
                Command::Undo => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&typed('c'));
                    editor.handle_event(&event);
                    assert_eq!(
                        editor.active_document().lines[0],
                        "ab",
                        "Ctrl+Z did not reach Undo"
                    );
                }
                Command::Redo => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&typed('c'));
                    editor.handle_event(&ctrl(Key::Z));
                    editor.handle_event(&event);
                    assert_eq!(
                        editor.active_document().lines[0],
                        "cab",
                        "Ctrl+Y did not reach Redo"
                    );
                }
                Command::Cut => {
                    let mut editor = editor_with("ab");
                    editor.active_document_mut().select_all();
                    editor.handle_event(&event);
                    assert_eq!(editor.clipboard, "ab", "Ctrl+X did not copy");
                    assert_eq!(
                        editor.active_document().lines[0],
                        "",
                        "Ctrl+X copied but did not cut"
                    );
                }
                Command::Copy => {
                    let mut editor = editor_with("ab");
                    editor.active_document_mut().select_all();
                    editor.handle_event(&event);
                    assert_eq!(editor.clipboard, "ab", "Ctrl+C did not reach Copy");
                    assert_eq!(
                        editor.active_document().lines[0],
                        "ab",
                        "Ctrl+C deleted what it copied"
                    );
                }
                Command::Paste => {
                    let mut editor = editor_with("ab");
                    editor.clipboard = "zz".to_string();
                    editor.handle_event(&event);
                    assert_eq!(
                        editor.active_document().lines[0],
                        "zzab",
                        "Ctrl+V did not reach Paste"
                    );
                }
                Command::SelectAll => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&event);
                    assert!(
                        editor.active_document().has_selection(),
                        "Ctrl+A did not reach Select All"
                    );
                }
                Command::SelectWord => {
                    let mut editor = editor_with("hello world");
                    editor.handle_event(&event);
                    assert_eq!(
                        editor.active_document().selected_text(),
                        "hello",
                        "Ctrl+D did not reach Select Word"
                    );
                }
                Command::Find => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&event);
                    assert!(editor.find_visible, "Ctrl+F did not open the find bar");
                    assert_eq!(editor.find_field, FindField::Query);
                }
                Command::Replace => {
                    let mut editor = editor_with("ab");
                    editor.handle_event(&event);
                    assert!(editor.find_visible, "Ctrl+H did not open the find bar");
                    assert_eq!(
                        editor.find_field,
                        FindField::Replace,
                        "Ctrl+H opened the find bar on the wrong field"
                    );
                }
            }
        }
    }

    /// A file on disk, in a directory this test owns.
    fn scratch_file(
        tag: &str,
        name: &str,
        body: &str,
    ) -> (scratchdir::ScratchDir, std::path::PathBuf) {
        let dir = scratchdir::ScratchDir::new(&format!("editor_dialog_{tag}"));
        let path = dir.dir().join(name);
        std::fs::write(&path, body).expect("write the fixture");
        (dir, path)
    }

    /// **Open reaches a real file and its text arrives in the editor.**
    ///
    /// Through `handle_event` and the dialog's own keys, not by calling the
    /// loader: the halves were never the problem. `apps/editor` had no Open at
    /// all, and `known-issues.md` recorded the cause as "there is no file
    /// picker anywhere in gui/" -- a claim made by looking for a *crate* of
    /// that name, while `guitk::dialog::FileDialog` sat in the toolkit this
    /// program already depends on, with four other programs driving it.
    #[test]
    fn open_reads_the_file_the_dialog_returned() {
        let (dir, _path) = scratch_file(
            "open",
            "greeting.txt",
            "hello from disk
",
        );
        // A document already in that directory, so Ctrl+O starts there and the
        // listing comes from `open_dialog` -- the code under test. An earlier
        // version of this test called `set_entries` itself and went on passing
        // with that wiring deleted: a test performing the production step it
        // was written to check.
        let anchor = dir.dir().join("anchor.txt");
        std::fs::write(&anchor, "x").expect("write the anchor");
        let mut editor = editor_with("");
        editor.open_file(&anchor).expect("open the anchor");
        editor.handle_event(&ctrl(Key::O));

        let dialog = editor.dialog.as_mut().expect("Ctrl+O put up no dialog");
        let index = dialog
            .entries()
            .iter()
            .position(|e| e.name == "greeting.txt")
            .expect("the fixture is not in the listing");
        dialog.select_entry(index);
        editor.handle_event(&plain(Key::Enter));

        assert!(editor.dialog.is_none(), "the dialog stayed up");
        assert_eq!(
            editor.active_document().lines[0],
            "hello from disk",
            "status: {:?}",
            editor.status
        );
    }

    /// **Save As writes the buffer to the chosen name.**
    #[test]
    fn save_as_writes_to_the_chosen_path() {
        let dir = scratchdir::ScratchDir::new("editor_dialog_saveas");
        // Anchored in the directory, so the dialog opens there without this
        // test navigating it -- the same reason as the Open test above.
        let anchor = dir.dir().join("anchor.txt");
        std::fs::write(&anchor, "x").expect("write the anchor");
        let mut editor = editor_with("");
        editor.open_file(&anchor).expect("open the anchor");
        editor.active_document_mut().lines = vec!["written by the editor".to_string()];
        let mut mods = Modifiers::ctrl();
        mods.shift = true;
        editor.handle_event(&press(Key::S, mods));

        let dialog = editor.dialog.as_mut().expect("no dialog");
        dialog.set_filename("out.txt");
        editor.handle_event(&plain(Key::Enter));

        assert!(editor.dialog.is_none(), "the dialog stayed up");
        let written = std::fs::read_to_string(dir.dir().join("out.txt"))
            .unwrap_or_else(|e| panic!("nothing was written: {e}; status {:?}", editor.status));
        assert!(written.contains("written by the editor"), "{written:?}");
    }

    /// Escape puts the dialog away and changes nothing.
    #[test]
    fn cancelling_the_dialog_leaves_the_document_alone() {
        let mut editor = editor_with("untouched");
        editor.handle_event(&ctrl(Key::O));
        assert!(editor.dialog.is_some());

        editor.handle_event(&plain(Key::Escape));

        assert!(editor.dialog.is_none(), "Escape did not dismiss it");
        assert_eq!(editor.active_document().lines[0], "untouched");
    }

    /// **The dialog is modal: typing does not reach the document behind it.**
    #[test]
    fn the_document_does_not_see_keys_while_the_dialog_is_up() {
        let mut editor = editor_with("abc");
        editor.handle_event(&ctrl(Key::O));

        editor.handle_event(&typed('z'));

        assert_eq!(
            editor.active_document().lines[0],
            "abc",
            "a keystroke meant for the dialog was typed into the document"
        );
    }

    /// Every command a menu row can carry maps back to the command itself.
    #[test]
    fn a_menu_rows_identifier_names_the_command_that_made_it() {
        for command in Command::ALL {
            assert_eq!(
                Command::from_id(command.id()),
                Some(command),
                "{command:?} did not survive the trip through its own id"
            );
        }
    }

    /// **The menu reaches a command, through the same body the keyboard uses.**
    #[test]
    fn the_search_menu_opens_the_find_bar() {
        let mut editor = editor_with("hello");
        assert!(!editor.find_visible);

        editor.handle_event(&press(Key::S, Modifiers::alt()));
        assert!(
            editor.menu_bar.is_open(),
            "Alt+S did not open the Search menu"
        );
        // Opening highlights nothing, so the first Down lands on the first row.
        editor.handle_event(&plain(Key::Down));
        editor.handle_event(&plain(Key::Enter));

        assert!(
            editor.find_visible,
            "the menu's Find row did not open the find bar"
        );
        assert!(
            !editor.menu_bar.is_open(),
            "the menu stayed open after running a row"
        );
    }

    /// The bar sits at the top of the window, where a click can reach it.
    #[test]
    fn clicking_the_top_of_the_window_opens_a_menu() {
        let mut editor = editor_with("hello");
        editor.resize(900, 600);

        editor.handle_event(&Event::Mouse(MouseEvent {
            x: 20.0,
            y: guitk::menubar::BAR_HEIGHT / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));

        assert!(
            editor.menu_bar.is_open(),
            "a click on the bar opened nothing"
        );
    }

    /// **Typing still reaches the document.**
    ///
    /// The bar is offered every key before the typing tables see them, which is
    /// only safe because a closed bar claims Alt+mnemonic and nothing else.
    #[test]
    fn the_menu_bar_does_not_swallow_typing() {
        let mut editor = editor_with("");
        editor.handle_event(&typed('x'));

        assert_eq!(editor.active_document().lines[0], "x");
        assert!(!editor.menu_bar.is_open(), "typing opened a menu");
    }

    /// **A drag that crosses the bar is not stolen by it.**
    ///
    /// Selecting upwards past the first line puts the pointer in the bar, and a
    /// menu opening mid-drag would both lose the selection and leave a menu
    /// open that nobody asked for.
    #[test]
    fn a_drag_that_crosses_the_menu_bar_is_not_stolen_by_it() {
        let mut editor = editor_with("zero\none\ntwo");
        editor.resize(900, 600);
        let x = editor.text_x() + 1.0;

        editor.handle_event(&Event::Mouse(MouseEvent {
            x,
            y: crate::TEXT_TOP + editor.line_height * 1.5,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert!(editor.dragging, "the press did not start a drag");

        editor.handle_event(&Event::Mouse(MouseEvent {
            x,
            y: guitk::menubar::BAR_HEIGHT / 2.0,
            kind: MouseEventKind::Move,
        }));

        assert!(
            !editor.menu_bar.is_open(),
            "a drag across the bar opened a menu"
        );
        assert!(editor.dragging, "the drag was dropped at the bar");
    }

    /// A command with nothing to do does nothing, however it is asked.
    ///
    /// The greying on a menu row is advisory: a menu can be opened, the
    /// document changed under it by a reload, and the row clicked afterwards,
    /// so the command checks for itself as well.
    #[test]
    fn a_command_with_nothing_to_do_does_nothing() {
        let mut editor = editor_with("ab");

        assert!(
            !editor.command_enabled(Command::Undo),
            "a document nobody has edited has an edit to undo"
        );
        assert_eq!(editor.run(Command::Undo), Response::Idle);
        assert_eq!(editor.active_document().lines[0], "ab");

        assert!(
            !editor.command_enabled(Command::Paste),
            "the clipboard starts empty"
        );
        assert_eq!(editor.run(Command::Paste), Response::Idle);
        assert_eq!(editor.active_document().lines[0], "ab");
    }

    /// An edit makes Undo available, and undoing it makes Redo available.
    #[test]
    fn what_a_menu_row_offers_follows_the_document() {
        let mut editor = editor_with("ab");
        editor.handle_event(&typed('c'));

        assert!(
            editor.command_enabled(Command::Undo),
            "an edit is not undoable"
        );
        assert!(
            !editor.command_enabled(Command::Redo),
            "nothing was undone yet"
        );

        editor.handle_event(&ctrl(Key::Z));
        assert!(
            editor.command_enabled(Command::Redo),
            "an undo is not redoable"
        );
    }

    /// **A click lands on the line it looks like it landed on.**
    ///
    /// The renderer and the hit test have to read the same top edge. They did
    /// not, once: `visible_lines` subtracted a hardcoded 64 for a strip 32
    /// pixels tall, and the viewport came out two lines short. The menu bar
    /// moved that edge again, and one copy of it left behind looks, from the
    /// outside, exactly like a click landing on the wrong line.
    #[test]
    fn a_click_lands_on_the_line_it_looks_like_it_landed_on() {
        let mut editor = editor_with("zero\none\ntwo\nthree");
        editor.resize(900, 600);
        let x = editor.text_x() + 1.0;

        for row in 0..4 {
            let y = crate::TEXT_TOP + (row as f32 + 0.5) * editor.line_height;
            let (line, _) = editor
                .caret_cursor_at(x, y)
                .unwrap_or_else(|| panic!("nothing to click at row {row}"));
            assert_eq!(line, row, "row {row} at y={y} reported line {line}");
        }

        // And the chrome above it is chrome, not line zero.
        assert!(
            editor.caret_cursor_at(x, crate::TEXT_TOP - 1.0).is_none(),
            "a click in the tab strip reached the text"
        );
        assert!(
            editor
                .caret_cursor_at(x, guitk::menubar::BAR_HEIGHT / 2.0)
                .is_none(),
            "a click on the menu bar reached the text"
        );
    }

    /// The text area is what is left between the bars, and never negative.
    #[test]
    fn the_text_area_is_what_the_bars_leave() {
        let mut editor = editor_with("x");
        editor.resize(900, 600);
        let expected = 600.0 - crate::TEXT_TOP - crate::STATUS_BAR_HEIGHT;
        assert!((editor.editor_height() - expected).abs() < f32::EPSILON);

        // A window shorter than its own chrome asks for an empty text area
        // rather than a negative one.
        editor.resize(900, 10);
        assert!(
            editor.editor_height().abs() < f32::EPSILON,
            "a window shorter than its chrome asked for {} pixels of text",
            editor.editor_height()
        );
        assert_eq!(editor.visible_lines(), 0);
    }

    #[test]
    fn typing_replaces_the_selection_rather_than_inserting_beside_it() {
        let mut editor = editor_with("hello world");
        let doc = editor.active_document_mut();
        doc.selection_anchor = Some((0, 0));
        doc.cursor_col = 5;

        assert_eq!(editor.handle_event(&typed('x')), Response::Redraw);
        assert_eq!(editor.active_document().lines[0], "x world");
        assert!(!editor.active_document().has_selection());
    }

    #[test]
    fn backspace_with_a_selection_deletes_the_selection_in_one_undo_step() {
        let mut editor = editor_with("abcdef");
        let doc = editor.active_document_mut();
        doc.selection_anchor = Some((0, 1));
        doc.cursor_col = 5;

        editor.handle_event(&plain(Key::Backspace));
        assert_eq!(editor.active_document().lines[0], "af");

        // One step, not four: the whole deletion comes back at once.
        editor.active_document_mut().undo();
        assert_eq!(editor.active_document().lines[0], "abcdef");
    }

    #[test]
    fn shift_arrow_extends_and_a_bare_arrow_collapses() {
        let mut editor = editor_with("abcdef");
        editor.handle_event(&shift(Key::Right));
        editor.handle_event(&shift(Key::Right));
        assert_eq!(editor.active_document().selected_text(), "ab");

        editor.handle_event(&plain(Key::Right));
        assert!(!editor.active_document().has_selection());
        assert_eq!(editor.active_document().cursor_col, 3);
    }

    #[test]
    fn a_caret_moved_off_screen_scrolls_back_into_view() {
        let mut editor = editor_with(&"line\n".repeat(200));
        let page = editor.visible_lines();
        assert!(page > 1, "the default window must show more than one line");

        for _ in 0..=page {
            editor.handle_event(&plain(Key::Down));
        }
        let doc = editor.active_document();
        assert!(
            doc.cursor_line >= doc.scroll_line && doc.cursor_line < doc.scroll_line + page,
            "caret at {} outside view {}..{}",
            doc.cursor_line,
            doc.scroll_line,
            doc.scroll_line + page
        );
    }

    #[test]
    fn a_caret_moved_past_the_right_edge_scrolls_horizontally() {
        let mut editor = editor_with(&"x".repeat(4000));
        editor.handle_event(&ctrl(Key::End));
        assert!(
            editor.active_document().scroll_px > 0.0,
            "the end of a 4000-column line must not be off screen"
        );
    }

    #[test]
    fn cut_and_paste_move_text_through_the_clipboard() {
        let mut editor = editor_with("alpha beta");
        let doc = editor.active_document_mut();
        doc.selection_anchor = Some((0, 0));
        doc.cursor_col = 5;

        editor.handle_event(&ctrl(Key::X));
        assert_eq!(editor.clipboard, "alpha");
        assert_eq!(editor.active_document().lines[0], " beta");

        editor.handle_event(&ctrl(Key::End));
        editor.handle_event(&ctrl(Key::V));
        assert_eq!(editor.active_document().lines[0], " betaalpha");
    }

    #[test]
    fn copying_nothing_leaves_the_clipboard_alone() {
        let mut editor = editor_with("abc");
        editor.clipboard = "kept".to_string();
        editor.handle_event(&ctrl(Key::C));
        assert_eq!(editor.clipboard, "kept");
    }

    #[test]
    fn pasting_multiple_lines_splits_the_buffer() {
        let mut editor = editor_with("start|end");
        editor.clipboard = "one\ntwo".to_string();
        editor.active_document_mut().cursor_col = 5;

        editor.handle_event(&ctrl(Key::V));
        let doc = editor.active_document();
        assert_eq!(doc.lines, vec!["startone", "two|end"]);
        assert_eq!((doc.cursor_line, doc.cursor_col), (1, 3));
    }

    #[test]
    fn a_pasted_crlf_does_not_leave_a_carriage_return_inside_a_line() {
        let mut editor = editor_with("");
        editor.clipboard = "one\r\ntwo\r".to_string();
        editor.handle_event(&ctrl(Key::V));
        let doc = editor.active_document();
        assert_eq!(doc.lines, vec!["one", "two", ""]);
        assert!(doc.lines.iter().all(|l| !l.contains('\r')));
    }

    #[test]
    fn ctrl_arrow_moves_by_words() {
        let mut editor = editor_with("alpha  beta_two, gamma");
        editor.handle_event(&ctrl(Key::Right));
        assert_eq!(
            editor.active_document().cursor_col,
            7,
            "start of `beta_two`"
        );
        editor.handle_event(&ctrl(Key::Right));
        assert_eq!(editor.active_document().cursor_col, 15, "the comma");
        editor.handle_event(&ctrl(Key::Left));
        assert_eq!(editor.active_document().cursor_col, 7);
    }

    #[test]
    fn ctrl_a_selects_the_whole_buffer() {
        let mut editor = editor_with("one\ntwo\nthree");
        editor.handle_event(&ctrl(Key::A));
        assert_eq!(editor.active_document().selected_text(), "one\ntwo\nthree");
    }

    #[test]
    fn undo_and_redo_are_reachable_from_the_keyboard() {
        let mut editor = editor_with("");
        editor.handle_event(&typed('a'));
        editor.handle_event(&typed('b'));
        assert_eq!(editor.active_document().lines[0], "ab");

        editor.handle_event(&ctrl(Key::Z));
        assert_eq!(editor.active_document().lines[0], "a");
        editor.handle_event(&ctrl(Key::Y));
        assert_eq!(editor.active_document().lines[0], "ab");
        editor.handle_event(&press(
            Key::Z,
            Modifiers {
                shift: true,
                ctrl: true,
                ..Modifiers::NONE
            },
        ));
        assert_eq!(editor.active_document().lines[0], "ab", "already redone");
    }

    #[test]
    fn the_find_bar_takes_typing_but_not_ctrl_s() {
        let mut editor = editor_with("needle in a haystack");
        editor.handle_event(&ctrl(Key::F));
        assert!(editor.find_visible);

        for ch in "needle".chars() {
            editor.handle_event(&typed(ch));
        }
        assert_eq!(editor.find.query, "needle");
        assert_eq!(
            editor.active_document().lines[0],
            "needle in a haystack",
            "typing in the find bar must not reach the document"
        );
        assert_eq!(editor.find.matches.len(), 1);

        // A chord the bar does not claim still reaches the document's bindings.
        // Witnessed by the Save As dialog now rather than by the refusal that
        // stood in for it before there was one; the claim is the same.
        editor.handle_event(&ctrl(Key::S));
        assert!(
            editor.dialog.is_some(),
            "Ctrl+S while searching should still try to save: {:?}",
            editor.status
        );
        assert!(editor.find_visible, "and must not close the bar");
    }

    #[test]
    fn escape_closes_the_find_bar_and_then_collapses_the_selection() {
        let mut editor = editor_with("abc");
        editor.handle_event(&ctrl(Key::F));
        editor.handle_event(&plain(Key::Escape));
        assert!(!editor.find_visible);

        editor.handle_event(&ctrl(Key::A));
        assert!(editor.active_document().has_selection());
        editor.handle_event(&plain(Key::Escape));
        assert!(!editor.active_document().has_selection());
    }

    #[test]
    fn ctrl_f_seeds_the_query_from_a_single_line_selection() {
        let mut editor = editor_with("alpha beta");
        editor.handle_event(&ctrl(Key::D));
        assert_eq!(editor.active_document().selected_text(), "alpha");
        editor.handle_event(&ctrl(Key::F));
        assert_eq!(editor.find.query, "alpha");
    }

    #[test]
    fn a_control_character_never_becomes_text() {
        let mut editor = editor_with("");
        // A backspace that also reports its control character must delete, and
        // must not additionally insert '\u{8}'.
        editor.handle_event(&Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "\u{8}".to_string(),
        }));
        assert_eq!(editor.active_document().lines[0], "");

        editor.handle_event(&ctrl(Key::F));
        editor.handle_event(&Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "\u{1b}".to_string(),
        }));
        assert_eq!(editor.find.query, "");
    }

    #[test]
    fn key_releases_change_nothing_but_do_record_modifiers() {
        let mut editor = editor_with("abc");
        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: false,
            modifiers: Modifiers::shift(),
            text: "a".to_string(),
        });
        assert_eq!(editor.handle_event(&event), Response::Idle);
        assert_eq!(editor.active_document().lines[0], "abc");
        assert!(editor.modifiers.shift, "shift-click needs this");
    }

    #[test]
    fn a_click_places_the_caret_and_a_drag_selects() {
        let mut editor = editor_with("hello world");
        let x = editor.window_width as f32 / 2.0;
        let y = crate::TEXT_TOP + 1.0;

        let press_at = Event::Mouse(MouseEvent {
            x: editor.text_x() + 1.0,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert_eq!(editor.handle_event(&press_at), Response::Redraw);
        assert_eq!(editor.active_document().cursor_col, 0);
        assert!(editor.dragging);
        assert!(
            !editor.active_document().has_selection(),
            "a plain click selects nothing"
        );

        let drag_to = Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        });
        editor.handle_event(&drag_to);
        assert!(editor.active_document().has_selection());

        let release = Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        editor.handle_event(&release);
        assert!(!editor.dragging);

        // Movement after the release must not extend any further.
        let selected = editor.active_document().selected_text();
        editor.handle_event(&Event::Mouse(MouseEvent {
            x: editor.text_x() + 1.0,
            y,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(editor.active_document().selected_text(), selected);
    }

    #[test]
    fn losing_focus_ends_a_drag() {
        let mut editor = editor_with("hello world");
        editor.dragging = true;
        editor.handle_event(&Event::FocusOut);
        assert!(!editor.dragging);
    }

    #[test]
    fn a_double_click_selects_the_word_under_the_pointer() {
        let mut editor = editor_with("alpha beta");
        let event = Event::Mouse(MouseEvent {
            x: editor.text_x() + 1.0,
            y: crate::TEXT_TOP + 1.0,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        });
        assert_eq!(editor.handle_event(&event), Response::Redraw);
        assert_eq!(editor.active_document().selected_text(), "alpha");
        assert!(!editor.dragging, "a double click does not start a drag");
    }

    /// One notch of the wheel, as the compositor actually sends it.
    ///
    /// `dy` is a *notch count*: 1.0 per detent, fractional for a trackpad.
    /// This helper exists mainly so no test can quietly reintroduce the old
    /// habit of writing a pixel distance here — which is what let the dead
    /// wheel below survive having a test.
    fn wheel(dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: 100.0,
            y: 100.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    #[test]
    fn the_wheel_moves_the_view_and_leaves_the_caret_alone() {
        let mut editor = editor_with(&"line\n".repeat(200));
        assert_eq!(editor.handle_event(&wheel(-4.0)), Response::Redraw);
        assert!(editor.active_document().scroll_line > 0);
        assert_eq!(
            editor.active_document().cursor_line,
            0,
            "scrolling must not drag the caret"
        );
    }

    /// The regression test for a wheel that did nothing at any speed.
    ///
    /// The handler used to compute `dy / line_height * 3.0`, treating the
    /// notch count as a pixel distance: one notch came to 0.14 lines, `as i64`
    /// truncated that to 0, and the handler returned `Idle`. The old version of
    /// the test above passed anyway, because it sent `dy = -line_height * 4.0`
    /// — a pixel distance, the same wrong dialect the handler spoke. A single
    /// ordinary notch is the case that matters and the case that was broken.
    #[test]
    fn a_single_notch_scrolls_the_view() {
        let mut editor = editor_with(&"line\n".repeat(200));
        assert_eq!(editor.handle_event(&wheel(-1.0)), Response::Redraw);
        assert_eq!(
            editor.active_document().scroll_line,
            3,
            "one notch is three lines"
        );
    }

    /// Away from the user goes down the file, towards the user comes back.
    #[test]
    fn the_wheel_scrolls_both_ways() {
        let mut editor = editor_with(&"line\n".repeat(200));
        editor.handle_event(&wheel(-5.0));
        let down = editor.active_document().scroll_line;
        assert!(down > 0, "scrolling away from the user moves down the file");
        editor.handle_event(&wheel(5.0));
        assert_eq!(
            editor.active_document().scroll_line,
            0,
            "and the same distance back returns to the top"
        );
    }

    /// A precision trackpad sends fractions of a notch. Truncating each event
    /// on its own would return zero every time and never scroll at all.
    #[test]
    fn a_trackpads_fractions_of_a_notch_eventually_scroll() {
        let mut editor = editor_with(&"line\n".repeat(200));
        for _ in 0..10 {
            editor.handle_event(&wheel(-0.1));
        }
        assert_eq!(
            editor.active_document().scroll_line,
            3,
            "ten tenths of a notch is one notch, which is three lines"
        );
    }

    /// The remainder belongs to the document, not the editor: a fraction
    /// earned in one tab must not deliver a line in another.
    #[test]
    fn each_tab_keeps_its_own_wheel_remainder() {
        let mut editor = editor_with(&"line\n".repeat(200));
        editor.handle_event(&wheel(-0.2));
        assert_eq!(editor.active_document().scroll_line, 0, "not yet a line");

        let mut other = Document::new();
        other.lines = vec!["line".to_string(); 200];
        editor.tabs.open(other);
        editor.handle_event(&wheel(-0.2));
        assert_eq!(
            editor.active_document().scroll_line,
            0,
            "the first tab's fraction must not scroll the second"
        );
    }

    /// Scrolling stops at the last line rather than running off the end.
    #[test]
    fn the_wheel_stops_at_the_end_of_the_file() {
        let mut editor = editor_with(&"line\n".repeat(20));
        for _ in 0..50 {
            editor.handle_event(&wheel(-1.0));
        }
        let last = editor.active_document().lines.len().saturating_sub(1);
        assert_eq!(editor.active_document().scroll_line, last);
    }

    #[test]
    fn clicking_a_tab_selects_it_and_the_close_box_closes_it() {
        let mut editor = editor_with("first");
        editor.tabs.open(Document::new());
        assert_eq!(editor.tabs.count(), 2);
        assert_eq!(editor.tabs.active_index(), 1);

        // Ten pixels into the strip, wherever the strip begins -- a bare
        // `10.0` meant "inside the tabs" only while they were at the very top
        // of the window, and stopped meaning it when the menu bar went above.
        let in_strip = crate::TAB_BAR_TOP + 10.0;
        assert_eq!(editor.tab_at(10.0, in_strip), Some((0, false)));
        editor.handle_event(&Event::Mouse(MouseEvent {
            x: 10.0,
            y: in_strip,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(editor.tabs.active_index(), 0);

        let close_x = TAB_WIDTH - 4.0;
        assert_eq!(editor.tab_at(close_x, in_strip), Some((0, true)));
        editor.handle_event(&Event::Mouse(MouseEvent {
            x: close_x,
            y: in_strip,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(editor.tabs.count(), 1);
    }

    #[test]
    fn the_gap_between_tabs_belongs_to_neither() {
        let editor = editor_with("only");
        assert_eq!(
            editor.tab_at(TAB_WIDTH + 0.5, crate::TAB_BAR_TOP + 10.0),
            None
        );
        // Below the strip is the text, and above it is the menu bar.
        assert_eq!(editor.tab_at(10.0, crate::TEXT_TOP + 1.0), None);
        assert_eq!(editor.tab_at(10.0, crate::TAB_BAR_TOP - 1.0), None);
    }

    #[test]
    fn ctrl_tab_cycles_and_wraps() {
        let mut editor = editor_with("first");
        editor.tabs.open(Document::new());
        editor.tabs.open(Document::new());
        editor.tabs.set_active(0);

        editor.handle_event(&ctrl(Key::Tab));
        assert_eq!(editor.tabs.active_index(), 1);
        editor.handle_event(&ctrl(Key::Tab));
        editor.handle_event(&ctrl(Key::Tab));
        assert_eq!(editor.tabs.active_index(), 0, "wraps at the end");

        editor.handle_event(&press(
            Key::Tab,
            Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            },
        ));
        assert_eq!(editor.tabs.active_index(), 2, "and at the beginning");
    }

    #[test]
    fn closing_a_modified_tab_is_refused_with_a_message() {
        let mut editor = editor_with("text");
        editor.active_document_mut().modified = true;
        editor.handle_event(&ctrl(Key::W));
        assert_eq!(editor.tabs.count(), 1);
        assert!(
            editor
                .status
                .as_deref()
                .is_some_and(|s| s.contains("Unsaved")),
            "{:?}",
            editor.status
        );
    }

    #[test]
    fn a_status_message_is_cleared_by_the_next_keystroke() {
        let mut editor = editor_with("text");
        editor.active_document_mut().modified = true;
        editor.handle_event(&ctrl(Key::W));
        assert!(editor.status.is_some());

        // A key with no binding at all still clears it, and says so, because the
        // message vanishing is itself a visible change.
        let response = editor.handle_event(&plain(Key::F5));
        assert_eq!(response, Response::Redraw);
        assert!(editor.status.is_none());
        assert_eq!(editor.handle_event(&plain(Key::F5)), Response::Idle);
    }

    #[test]
    fn resizing_updates_the_viewport_and_pulls_the_caret_back() {
        let mut editor = editor_with(&"line\n".repeat(200));
        editor.active_document_mut().cursor_line = 30;
        editor.handle_event(&plain(Key::Down));

        let event = Event::Resize {
            width: 400,
            height: 200,
        };
        assert_eq!(editor.handle_event(&event), Response::Redraw);
        assert_eq!(editor.window_height, 200);
        let page = editor.visible_lines();
        let doc = editor.active_document();
        assert!(doc.cursor_line < doc.scroll_line + page);

        assert_eq!(
            editor.handle_event(&event),
            Response::Idle,
            "a resize to the size it already is changes nothing"
        );
    }

    #[test]
    fn close_requested_asks_the_caller_to_exit() {
        let mut editor = editor_with("text");
        assert_eq!(editor.handle_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn the_disk_prompt_owns_the_keyboard_while_it_is_up() {
        let mut editor = editor_with("buffer");
        editor.external_prompt = Some(crate::ExternalChangePrompt {
            tab: 0,
            change: diffcore::DiskChange::Modified {
                disk: "elsewhere".to_string(),
            },
            review: None,
        });

        // A letter that would otherwise be typed answers the prompt instead.
        editor.handle_event(&typed('x'));
        assert_eq!(editor.active_document().lines[0], "buffer");

        editor.handle_event(&plain(Key::K));
        assert!(
            editor.external_prompt.is_none(),
            "K keeps the current buffer"
        );
        assert_eq!(editor.active_document().lines[0], "buffer");
    }
}
