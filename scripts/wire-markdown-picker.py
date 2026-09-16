"""Open and Save As reach the file picker, instead of lying and doing nothing."""

import pathlib

p = pathlib.Path("apps/markdowneditor/src/main.rs")
s = p.read_text(encoding="utf-8")

# ---- 1. the field ----------------------------------------------------------
old_field = """pub struct App {
    /// Milliseconds seen since the last whole second was handed to autosave."""
assert s.count(old_field) == 1, "struct"
s = s.replace(old_field, '''pub struct App {
    /// The open/save dialog.
    ///
    /// **Both halves of this door already existed and nothing joined them.**
    /// `App::open_file` reads a file into a new tab and `Document::save_as`
    /// writes atomically and renames the tab; both have tests. What the
    /// toolbar did was `ToolbarAction::OpenFile => self.new_document()` --
    /// click Open, get a blank document -- and `ToolbarAction::SaveAs => {}`,
    /// an empty body under the comment "Would open a save dialog in a real
    /// app".
    ///
    /// The silent Save As was the dangerous one. A person who clicks it, sees
    /// no dialog and no error, and closes the editor has been told nothing and
    /// has every reason to believe the file was written under the new name.
    ///
    /// This is the shape `scripts/find-unpinned-picker-routing.py` was written
    /// for: a control, a writer, and no routing between them. Twenty apps were
    /// pinned against it; this one was not among them because it had no picker
    /// at all.
    pub picker: guitk::dialog::FilePicker,
    /// What the last open or save did, or why it did not happen.
    pub file_status: Option<String>,
    /// Milliseconds seen since the last whole second was handed to autosave.''', 1)

# ---- 2. the initialiser ----------------------------------------------------
old_init = "            tick_ms_carry: 0,"
assert s.count(old_init) >= 1, "init"
s = s.replace(old_init, """            picker: guitk::dialog::FilePicker::new(),
            file_status: None,
            tick_ms_carry: 0,""", 1)

# ---- 3. the two toolbar actions --------------------------------------------
old_open = """            ToolbarAction::OpenFile => {
                // In a real app, this would open a file dialog.
                // For now, we create a new document.
                self.new_document();
            }"""
assert s.count(old_open) == 1, "open action"
s = s.replace(old_open, """            ToolbarAction::OpenFile => {
                self.picker.open_to_read();
            }""", 1)

old_saveas = """            ToolbarAction::SaveAs => {
                // Would open a save dialog in a real app.
            }"""
assert s.count(old_saveas) == 1, "saveas action"
s = s.replace(old_saveas, """            ToolbarAction::SaveAs => {
                let name = self.active_document().name.clone();
                self.picker.open_to_write(name);
            }""", 1)

# ---- 4. what a choice does -------------------------------------------------
anchor = "    pub fn new_document(&mut self) {"
assert s.count(anchor) == 1, "new_document"
s = s.replace(anchor, '''    /// Route an event to the picker, and act on a chosen path.
    ///
    /// Returns `true` when the picker consumed the event, so the caller stops:
    /// an open dialog takes the keyboard, and a window that keeps typing into
    /// the document behind one is a modal that is not modal.
    pub fn picker_took(&mut self, event: &guitk::event::Event) -> bool {
        // Read before `handle`: choosing a path takes the dialog down, and a
        // flag read afterwards would be answering about a picker that is no
        // longer up.
        let saving = self.picker.is_saving();
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            guitk::dialog::Picked::Chose(path) => {
                self.file_status = Some(if saving {
                    match self.active_document_mut().save_as(&path) {
                        Ok(()) => format!("Saved as {}", path.display()),
                        Err(e) => format!("Could not save {}: {e}", path.display()),
                    }
                } else {
                    match self.open_file(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(e) => format!("Could not open {}: {e}", path.display()),
                    }
                });
                true
            }
            guitk::dialog::Picked::Handled | guitk::dialog::Picked::Cancelled => true,
            guitk::dialog::Picked::Ignored => false,
        }
    }

''' + anchor, 1)

p.write_text(s, encoding="utf-8", newline="\n")
print("markdowneditor: picker wired (needs event/render routing)")
