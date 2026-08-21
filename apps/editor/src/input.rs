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
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

/// The visual width of one tab in the tab bar, and the gap after it.
///
/// Read by both the renderer and the hit test, so that clicking a tab selects
/// the one that was drawn there. Two numbers that must agree are one number.
pub const TAB_WIDTH: f32 = 160.0;
/// Horizontal gap between tabs, in pixels.
pub const TAB_GAP: f32 = 1.0;
/// Width of the close box at the right end of a tab.
pub const TAB_CLOSE_WIDTH: f32 = 24.0;

/// What the caller should do about an event.
///
/// Deliberately not `bool`: "the editor wants to close" and "the editor wants to
/// be redrawn" are different answers, and an event loop that could only be told
/// one of them would have to guess the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorResponse {
    /// Nothing changed; do not redraw.
    Idle,
    /// Something visible changed; draw a new frame.
    Redraw,
    /// The user asked to quit.
    Exit,
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
    pub fn handle_event(&mut self, event: &Event) -> EditorResponse {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                if self.window_width == *width && self.window_height == *height {
                    return EditorResponse::Idle;
                }
                self.window_width = *width;
                self.window_height = *height;
                // The viewport just changed size, so the caret may now be
                // outside it in either direction.
                let visible = self.visible_lines();
                self.active_document_mut().ensure_cursor_visible(visible);
                self.ensure_caret_visible_horizontally();
                EditorResponse::Redraw
            }
            // Coming back to the window is exactly when another program is
            // likely to have written the file — a build, a formatter, a `git
            // checkout` in the terminal the user just came from.
            Event::FocusIn => {
                if self.check_external_change() {
                    EditorResponse::Redraw
                } else {
                    EditorResponse::Idle
                }
            }
            // A drag that ends outside the window never delivers its release,
            // so losing focus has to end it too. Otherwise the next mouse move
            // over the window — with no button held — would go on extending the
            // selection.
            Event::FocusOut => {
                self.dragging = false;
                EditorResponse::Idle
            }
            Event::CloseRequested => EditorResponse::Exit,
            _ => EditorResponse::Idle,
        }
    }

    // ======================================================================
    // Keyboard
    // ======================================================================

    fn handle_key(&mut self, key: &KeyEvent) -> EditorResponse {
        // Modifier state is kept here because mouse events do not carry it:
        // `MouseEvent` has a position and a kind and nothing else, so shift-click
        // can only be recognised from what the keyboard last reported.
        self.modifiers = key.modifiers;
        if !key.pressed {
            return EditorResponse::Idle;
        }

        // A status message describes the last thing that happened. The next
        // keystroke is the next thing, so it goes — and clearing it is itself a
        // visible change, which is why the response is upgraded below rather
        // than being left as whatever the binding returned.
        let had_status = self.status.take().is_some();
        let response = self.dispatch_key(key);
        if had_status && response == EditorResponse::Idle {
            EditorResponse::Redraw
        } else {
            response
        }
    }

    fn dispatch_key(&mut self, key: &KeyEvent) -> EditorResponse {
        if self.external_prompt.is_some() {
            return self.prompt_key(key);
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
    fn prompt_key(&mut self, key: &KeyEvent) -> EditorResponse {
        let reviewing = self
            .external_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.review.is_some());
        if reviewing {
            return match key.key {
                Key::Enter => {
                    self.review_accept();
                    EditorResponse::Redraw
                }
                Key::Escape => {
                    self.review_cancel();
                    EditorResponse::Redraw
                }
                _ => EditorResponse::Idle,
            };
        }
        let choice = match key.key {
            Key::K => ExternalChoice::KeepCurrent,
            Key::R => ExternalChoice::Reload,
            Key::M => ExternalChoice::Merge,
            Key::V => ExternalChoice::Review,
            Key::Escape => {
                self.dismiss_external();
                return EditorResponse::Redraw;
            }
            _ => return EditorResponse::Idle,
        };
        self.resolve_external(choice);
        EditorResponse::Redraw
    }

    /// Keys while the find bar is open.
    ///
    /// Returns `None` for anything the bar does not claim, so it falls through
    /// to the document's bindings — Ctrl+S must still save while searching.
    fn find_key(&mut self, key: &KeyEvent) -> Option<EditorResponse> {
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
                    Some(EditorResponse::Redraw)
                }
                Key::I => {
                    self.find.case_sensitive = !self.find.case_sensitive;
                    self.refresh_matches();
                    Some(EditorResponse::Redraw)
                }
                Key::E => {
                    self.find.use_regex = !self.find.use_regex;
                    self.refresh_matches();
                    Some(EditorResponse::Redraw)
                }
                _ => None,
            };
        }
        match key.key {
            Key::Escape => {
                self.find_visible = false;
                Some(EditorResponse::Redraw)
            }
            Key::Tab => {
                self.find_field = match self.find_field {
                    FindField::Query => FindField::Replace,
                    FindField::Replace => FindField::Query,
                };
                Some(EditorResponse::Redraw)
            }
            Key::Enter => {
                if key.modifiers.shift {
                    self.goto_match(false);
                } else {
                    self.goto_match(true);
                }
                self.after_cursor_move();
                Some(EditorResponse::Redraw)
            }
            Key::Backspace => {
                let field = self.find_field;
                let changed = self.find_field_mut(field).pop().is_some();
                if field == FindField::Query {
                    self.refresh_matches();
                }
                Some(if changed {
                    EditorResponse::Redraw
                } else {
                    EditorResponse::Idle
                })
            }
            _ => {
                // Control characters would otherwise be appended literally: the
                // Enter and Tab cases above are handled, but a key that reports
                // text of '\u{8}' or '\u{1b}' must not become part of the query.
                let ch = key.text.filter(|c| !c.is_control())?;
                let field = self.find_field;
                self.find_field_mut(field).push(ch);
                if field == FindField::Query {
                    self.refresh_matches();
                }
                Some(EditorResponse::Redraw)
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

    /// Keys with Ctrl held, in the document.
    #[allow(clippy::too_many_lines)]
    fn control_key(&mut self, key: &KeyEvent) -> EditorResponse {
        let shift = key.modifiers.shift;
        match key.key {
            Key::S => {
                self.save_active();
                EditorResponse::Redraw
            }
            Key::Z => {
                if shift {
                    self.active_document_mut().redo();
                } else {
                    self.active_document_mut().undo();
                }
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::Y => {
                self.active_document_mut().redo();
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::F => {
                self.find_visible = true;
                self.find_field = FindField::Query;
                // Searching for what is selected is what the user almost always
                // wants, and typing over it costs one keystroke if not. Only a
                // single-line selection: a search term with a newline in it
                // cannot match anything, since matching is per line.
                let selected = self.active_document().selected_text();
                if !selected.is_empty() && !selected.contains('\n') {
                    self.find.query = selected;
                }
                self.refresh_matches();
                EditorResponse::Redraw
            }
            Key::H => {
                self.find_visible = true;
                self.find_field = FindField::Replace;
                EditorResponse::Redraw
            }
            Key::A => {
                self.active_document_mut().select_all();
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::C => {
                self.copy_selection();
                EditorResponse::Idle
            }
            Key::X => {
                if self.copy_selection() {
                    self.active_document_mut().delete_selection();
                    self.after_cursor_move();
                    return EditorResponse::Redraw;
                }
                EditorResponse::Idle
            }
            Key::V => {
                if self.clipboard.is_empty() {
                    return EditorResponse::Idle;
                }
                let text = self.clipboard.clone();
                let doc = self.active_document_mut();
                doc.delete_selection();
                doc.insert_text(&text);
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::D => {
                self.active_document_mut().select_word_at_cursor();
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::W => {
                if self.close_tab() {
                    EditorResponse::Redraw
                } else {
                    self.status = Some(
                        "Unsaved changes — save with Ctrl+S, or Ctrl+Shift+W to discard"
                            .to_string(),
                    );
                    EditorResponse::Redraw
                }
            }
            Key::Home => {
                self.moving(shift, Document::move_to_start);
                EditorResponse::Redraw
            }
            Key::End => {
                self.moving(shift, Document::move_to_end);
                EditorResponse::Redraw
            }
            Key::Left => {
                self.moving(shift, Document::move_word_left);
                EditorResponse::Redraw
            }
            Key::Right => {
                self.moving(shift, Document::move_word_right);
                EditorResponse::Redraw
            }
            Key::Tab | Key::PageDown => {
                self.cycle_tab(!shift);
                EditorResponse::Redraw
            }
            Key::PageUp => {
                self.cycle_tab(false);
                EditorResponse::Redraw
            }
            _ => EditorResponse::Idle,
        }
    }

    /// Keys with no Ctrl, in the document: motion and text.
    fn editing_key(&mut self, key: &KeyEvent) -> EditorResponse {
        let shift = key.modifiers.shift;
        match key.key {
            Key::Left => {
                self.moving(shift, Document::move_left);
                EditorResponse::Redraw
            }
            Key::Right => {
                self.moving(shift, Document::move_right);
                EditorResponse::Redraw
            }
            Key::Up => {
                self.moving(shift, Document::move_up);
                EditorResponse::Redraw
            }
            Key::Down => {
                self.moving(shift, Document::move_down);
                EditorResponse::Redraw
            }
            Key::Home => {
                self.moving(shift, Document::move_home);
                EditorResponse::Redraw
            }
            Key::End => {
                self.moving(shift, Document::move_end);
                EditorResponse::Redraw
            }
            Key::PageUp => {
                let page = self.visible_lines();
                self.moving(shift, |doc| {
                    doc.cursor_line = doc.cursor_line.saturating_sub(page);
                    doc.clamp_cursor();
                });
                EditorResponse::Redraw
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
                EditorResponse::Redraw
            }
            Key::Escape => {
                // Collapse the selection to the caret. Somewhere for Escape to
                // go when there is no panel open, and the counterpart of
                // clicking in the text.
                if self.active_document().selection_anchor.is_none() {
                    return EditorResponse::Idle;
                }
                self.active_document_mut().selection_anchor = None;
                EditorResponse::Redraw
            }
            Key::Backspace => {
                let doc = self.active_document_mut();
                if !doc.delete_selection() {
                    doc.backspace();
                }
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::Delete => {
                let doc = self.active_document_mut();
                if !doc.delete_selection() {
                    doc.delete_forward();
                }
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            Key::Enter => self.type_char('\n'),
            Key::Tab => self.type_char('\t'),
            _ => {
                // Everything else is text or nothing. Control characters are
                // excluded because the three that have bindings are handled
                // above, and the rest would be inserted as unprintable bytes.
                match key.text.filter(|c| !c.is_control()) {
                    Some(ch) => self.type_char(ch),
                    None => EditorResponse::Idle,
                }
            }
        }
    }

    /// Insert one character, replacing the selection if there is one.
    fn type_char(&mut self, ch: char) -> EditorResponse {
        let doc = self.active_document_mut();
        doc.delete_selection();
        doc.insert_char(ch);
        self.after_cursor_move();
        EditorResponse::Redraw
    }

    // ======================================================================
    // Mouse
    // ======================================================================

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EditorResponse {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.mouse_press(mouse.x, mouse.y),
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                if self.caret_position_at(mouse.x, mouse.y).is_none() {
                    return EditorResponse::Idle;
                }
                self.mouse_press(mouse.x, mouse.y);
                self.active_document_mut().select_word_at_cursor();
                self.dragging = false;
                self.after_cursor_move();
                EditorResponse::Redraw
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if !self.dragging {
                    return EditorResponse::Idle;
                }
                self.dragging = false;
                EditorResponse::Idle
            }
            MouseEventKind::Move => {
                if !self.dragging {
                    return EditorResponse::Idle;
                }
                // A drag that leaves the text area does not stop extending: the
                // caret is clamped to the nearest position rather than the
                // selection freezing, which is what makes selecting past the
                // bottom of the screen possible at all.
                let Some((line, col)) = self.caret_position_at(mouse.x, mouse.y) else {
                    return EditorResponse::Idle;
                };
                let doc = self.active_document_mut();
                doc.cursor_line = line;
                doc.cursor_col = col;
                self.after_cursor_move();
                EditorResponse::Redraw
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
                    return EditorResponse::Idle;
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
                    return EditorResponse::Idle;
                }
                doc.scroll_line = scrolled;
                // Deliberately *not* followed by `ensure_cursor_visible`: the
                // wheel moves the view, not the caret. Scrolling that dragged
                // the caret along would lose the user's place the moment they
                // looked somewhere else in the file.
                EditorResponse::Redraw
            }
            _ => EditorResponse::Idle,
        }
    }

    /// A left press: put the caret where the pointer is, or act on the tab bar.
    fn mouse_press(&mut self, x: f32, y: f32) -> EditorResponse {
        if let Some((index, on_close)) = self.tab_at(x, y) {
            self.tabs.set_active(index);
            if on_close && !self.close_tab() {
                self.status = Some("Unsaved changes — save with Ctrl+S first".to_string());
            }
            return EditorResponse::Redraw;
        }
        let Some((line, col)) = self.caret_position_at(x, y) else {
            return EditorResponse::Idle;
        };
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
        doc.cursor_line = line;
        doc.cursor_col = col;
        self.dragging = true;
        self.after_cursor_move();
        EditorResponse::Redraw
    }

    /// Which tab the point is over, and whether it is over that tab's close box.
    ///
    /// `None` for anything below the tab bar or past the last tab.
    #[must_use]
    pub fn tab_at(&self, x: f32, y: f32) -> Option<(usize, bool)> {
        if y < 0.0 || y >= crate::TAB_BAR_HEIGHT || x < 0.0 {
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
            self.status = Some("No file name — Save As needs a file dialog".to_string());
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
            text: None,
        })
    }

    /// A key press that produces a character, as typing does.
    fn typed(ch: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some(ch),
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

    #[test]
    fn typing_replaces_the_selection_rather_than_inserting_beside_it() {
        let mut editor = editor_with("hello world");
        let doc = editor.active_document_mut();
        doc.selection_anchor = Some((0, 0));
        doc.cursor_col = 5;

        assert_eq!(editor.handle_event(&typed('x')), EditorResponse::Redraw);
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
        editor.handle_event(&ctrl(Key::S));
        assert!(
            editor
                .status
                .as_deref()
                .is_some_and(|s| s.contains("Save As")),
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
            text: Some('\u{8}'),
        }));
        assert_eq!(editor.active_document().lines[0], "");

        editor.handle_event(&ctrl(Key::F));
        editor.handle_event(&Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('\u{1b}'),
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
            text: Some('a'),
        });
        assert_eq!(editor.handle_event(&event), EditorResponse::Idle);
        assert_eq!(editor.active_document().lines[0], "abc");
        assert!(editor.modifiers.shift, "shift-click needs this");
    }

    #[test]
    fn a_click_places_the_caret_and_a_drag_selects() {
        let mut editor = editor_with("hello world");
        let x = editor.window_width as f32 / 2.0;
        let y = crate::TAB_BAR_HEIGHT + 1.0;

        let press_at = Event::Mouse(MouseEvent {
            x: editor.text_x() + 1.0,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert_eq!(editor.handle_event(&press_at), EditorResponse::Redraw);
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
            y: crate::TAB_BAR_HEIGHT + 1.0,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        });
        assert_eq!(editor.handle_event(&event), EditorResponse::Redraw);
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
        assert_eq!(editor.handle_event(&wheel(-4.0)), EditorResponse::Redraw);
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
        assert_eq!(editor.handle_event(&wheel(-1.0)), EditorResponse::Redraw);
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

        assert_eq!(editor.tab_at(10.0, 10.0), Some((0, false)));
        editor.handle_event(&Event::Mouse(MouseEvent {
            x: 10.0,
            y: 10.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(editor.tabs.active_index(), 0);

        let close_x = TAB_WIDTH - 4.0;
        assert_eq!(editor.tab_at(close_x, 10.0), Some((0, true)));
        editor.handle_event(&Event::Mouse(MouseEvent {
            x: close_x,
            y: 10.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(editor.tabs.count(), 1);
    }

    #[test]
    fn the_gap_between_tabs_belongs_to_neither() {
        let editor = editor_with("only");
        assert_eq!(editor.tab_at(TAB_WIDTH + 0.5, 10.0), None);
        assert_eq!(editor.tab_at(10.0, crate::TAB_BAR_HEIGHT + 1.0), None);
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
        assert_eq!(response, EditorResponse::Redraw);
        assert!(editor.status.is_none());
        assert_eq!(editor.handle_event(&plain(Key::F5)), EditorResponse::Idle);
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
        assert_eq!(editor.handle_event(&event), EditorResponse::Redraw);
        assert_eq!(editor.window_height, 200);
        let page = editor.visible_lines();
        let doc = editor.active_document();
        assert!(doc.cursor_line < doc.scroll_line + page);

        assert_eq!(
            editor.handle_event(&event),
            EditorResponse::Idle,
            "a resize to the size it already is changes nothing"
        );
    }

    #[test]
    fn close_requested_asks_the_caller_to_exit() {
        let mut editor = editor_with("text");
        assert_eq!(
            editor.handle_event(&Event::CloseRequested),
            EditorResponse::Exit
        );
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
