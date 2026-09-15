"""One-shot: give apps/jsonviewer a way to open a real file. Deleted after use."""

import pathlib

P = pathlib.Path("apps/jsonviewer/src/main.rs")


def cut(text, needle, repl, n=1):
    assert text.count(needle) == n, f"{text.count(needle)} matches (want {n}): {needle[:70]!r}"
    return text.replace(needle, repl)


s = P.read_text(encoding="utf-8")

# 1. Import.
s = cut(s, "use guitk::Color;", "use guitk::Color;\nuse guitk::dialog::{DialogAction, FileDialog};")

# 2. Field.
s = cut(s, "    width: f32,",
        """    /// The file picker, while one is up.
    ///
    /// The only route a real document has into this viewer. Until 2026-09-15
    /// there was none: `App::new` loaded `SAMPLE_JSON`, a document describing
    /// the viewer itself, and that was the only thing it could ever show.
    file_dialog: Option<FileDialog>,
    /// What the last open attempt did, for the status line.
    last_open: Option<String>,
    width: f32,""")

# 3. Constructor: empty document, no sample, and the new fields.
s = cut(s, """        let mut doc = Document::new(1, String::from("Untitled"));
        // Load sample JSON
        doc.input = SAMPLE_JSON.to_string();
        doc.reparse();
""",
        """        // Opens empty. It used to load `SAMPLE_JSON` -- a document describing
        // this program, its version and its feature list -- which was the only
        // content the viewer could ever hold, because nothing here could read
        // a file. Ctrl+O reads one now.
        let doc = Document::new(1, String::from("Untitled"));
""")
s = cut(s, "            documents: vec![doc],",
        "            documents: vec![doc],\n            file_dialog: None,\n            last_open: Some(String::from(\"Press Ctrl+O to open a JSON file\")),")

# 4. Ctrl+O.
s = cut(s, """                Key::N => {
                    self.new_tab();
                    return;
                }""",
        """                Key::O => {
                    self.open_file_dialog();
                    return;
                }
                Key::N => {
                    self.new_tab();
                    return;
                }""")

# 5. The picker joins the fingerprint.
#
# Without this, opening it would change no watched state, `handle_event` would
# answer `Ignored`, and the window would not redraw -- an invisible dialog
# eating every keystroke, which is the same bug `apps/hexeditor` nearly
# shipped today by a different route.
s = cut(s, """        usize,
        usize,
        ViewMode,
    ) {""",
        """        usize,
        usize,
        ViewMode,
        bool,
    ) {""")
s = cut(s, """            doc.map_or(ViewMode::Tree, |d| d.view_mode),
        )
    }""",
        """            doc.map_or(ViewMode::Tree, |d| d.view_mode),
            // Opening or closing the picker is a redraw. Without this the
            // dialog would be invisible until something else changed state --
            // `handle_key` reports nothing, so this tuple is the only thing
            // that decides whether a frame is drawn.
            self.file_dialog.is_some(),
        )
    }""")

# 6. Dialog handling + open.
s = cut(s, "    fn handle_event(&mut self, event: &Event) -> EventResult {",
        '''    /// Put the file picker up, listing the directory it starts in.
    pub fn open_file_dialog(&mut self) {
        let start = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let mut dialog = FileDialog::open().with_initial_path(start);
        dialog.set_entries(guitk::dialog::list_directory(dialog.current_path()));
        self.file_dialog = Some(dialog);
    }

    fn apply_dialog_action(&mut self, action: DialogAction) -> EventResult {
        match action {
            DialogAction::None => EventResult::Consumed,
            DialogAction::Cancelled => {
                self.file_dialog = None;
                EventResult::Consumed
            }
            DialogAction::NavigatedTo(path) => {
                if let Some(dialog) = self.file_dialog.as_mut() {
                    dialog.set_entries(guitk::dialog::list_directory(&path));
                }
                EventResult::Consumed
            }
            DialogAction::Selected(path) => {
                self.file_dialog = None;
                self.last_open = Some(self.open_path(&path));
                EventResult::Consumed
            }
        }
    }

    /// Read `path` into the active document. Returns what to say about it.
    ///
    /// A file that is not JSON is still *opened* -- the text goes in and the
    /// parse error is what the viewer is for. What is reported here is the
    /// read, not the parse: those are different failures and conflating them
    /// would tell someone their disk was unreadable when their braces were
    /// unbalanced.
    ///
    /// Bounded at [`MAX_OPEN_BYTES`] and says so when it cuts. A JSON document
    /// truncated in the middle is not valid JSON, so a silent cut would show a
    /// parse error about the file's contents that is really about ours.
    pub fn open_path(&mut self, path: &std::path::Path) -> String {
        let shown = path.display().to_string();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => return format!("Could not read {shown}: {err}"),
        };
        let whole = text.len();
        let truncated = whole > MAX_OPEN_BYTES;
        let input = if truncated {
            // On a char boundary, or the `String` would not be valid UTF-8.
            let mut cut = MAX_OPEN_BYTES;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut = cut.saturating_sub(1);
            }
            text.get(..cut).unwrap_or("").to_string()
        } else {
            text
        };

        let name = path
            .file_name()
            .map_or_else(|| shown.clone(), |n| n.to_string_lossy().into_owned());
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        let mut doc = Document::new(id, name);
        doc.input = input;
        doc.reparse();
        self.documents.push(doc);
        self.active_tab = self.documents.len().saturating_sub(1);

        if truncated {
            format!("Opened the first {MAX_OPEN_BYTES} bytes of {shown} -- it is {whole} bytes, so what is shown is not the whole document")
        } else {
            format!("Opened {shown}")
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {''')

# 7. Intercept events while the picker is up.
s = cut(s, """        match event {
            Event::Key(key_ev) => {
                if !key_ev.pressed {
                    return EventResult::Ignored;
                }""",
        """        // The picker takes the event first while it is up, or a click meant
        // for a filename lands on the tree behind it.
        if self.file_dialog.is_some() {
            let (w, h) = (self.width, self.height);
            let action = match (event, self.file_dialog.as_mut()) {
                (Event::Key(key_ev), Some(dialog)) if key_ev.pressed => {
                    dialog.handle_event(key_ev, h)
                }
                (Event::Mouse(mouse), Some(dialog)) => dialog.handle_mouse(mouse, w, h),
                _ => return EventResult::Ignored,
            };
            return self.apply_dialog_action(action);
        }
        match event {
            Event::Key(key_ev) => {
                if !key_ev.pressed {
                    return EventResult::Ignored;
                }""")

# 8. Render the picker last.
s = cut(s, """        self.width = width;
        self.height = height;
        RenderTree {
            commands: self.render_commands(),
        }
    }""",
        """        self.width = width;
        self.height = height;
        let mut commands = self.render_commands();
        // Last, so it is above everything -- the same order in which
        // `handle_event` gives it the click.
        if let Some(dialog) = &self.file_dialog {
            commands.extend(dialog.render(&self.palette, width, height));
        }
        RenderTree { commands }
    }""")

# 9. The sample becomes a fixture, and the cap gets a home.
s = cut(s, "const SAMPLE_JSON: &str = r#\"{",
        """/// The most of a file one open will read.
///
/// Reported when it bites: a JSON document cut in half is not valid JSON, so a
/// silent truncation would show a parse error that is about this cap rather
/// than about the file.
pub const MAX_OPEN_BYTES: usize = 8 * 1024 * 1024;

/// A sample document, for tests.
///
/// `#[cfg(test)]` since 2026-09-15. `App::new` loaded it, so a viewer that
/// could not read a file showed a description of itself instead -- and a
/// fixture production can reach is a fixture that ships.
#[cfg(test)]
const SAMPLE_JSON: &str = r#"{""")

P.write_text(s, encoding="utf-8", newline="\n")
print("jsonviewer has a door")
