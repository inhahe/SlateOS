//! Run Dialog — desktop shell component.
//!
//! A Windows-style "Run" dialog (typically invoked via Ctrl+R or Super+R)
//! that lets users type a command to execute. Supports text editing, command
//! history, fuzzy autocomplete, and path resolution.
//!
//! The history is kept across sessions by the shell, not here: this module
//! holds it and does no filesystem I/O. `DesktopShell::load_run_history`
//! hands it what `runbox.yaml` holds at start, and the session writes it back
//! (`DesktopShell::save_run_history`) whenever the box has run something --
//! each entry's bytes, percent-encoded (design-decisions §426), so a command
//! naming a file whose name is not text comes back as it ran.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut run_dialog = RunDialog::new();
//!
//! // When Ctrl+R or Super+R is pressed:
//! run_dialog.show();
//!
//! // Forward key/mouse events while visible:
//! run_dialog.handle_key_event(&key_event);
//! run_dialog.handle_mouse_event(&mouse_event);
//!
//! // Each frame, if visible:
//! let commands = run_dialog.render(&palette);
//!
//! // Drain events to act on:
//! for event in run_dialog.drain_events() {
//!     match event {
//!         RunDialogEvent::Execute(request) => { /* open or run it */ }
//!         RunDialogEvent::Browse => { /* open file picker */ }
//!         RunDialogEvent::Cancel => { /* dismiss */ }
//!         RunDialogEvent::Closed => { /* cleanup */ }
//!     }
//! }
//! ```

use crate::dialog_frame::{DialogFrame, FrameLayout};
use appearance::Palette;
use appearance::Surface;
use guitk::event::{EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
// The field's state, and no longer a copy of it. This type lived here, private,
// until 2026-09-17; twenty-two files in the tree hand-roll typing because they
// could not reach it, and the bidirectional caret it carries is the reason a
// second implementation would have been a worse one. See design-decisions 541.
use guitk::textinput::TextInput;
// The candidate ranking is shared with both launchers. It used to be a third
// copy of the same routine here, under a comment saying it "uses the same
// algorithm as the application launcher for consistency" — a promise with no
// mechanism behind it.
use guitk::textfind::fuzzy_score;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

// ============================================================================
// Colour
// ============================================================================
//
// This module used to declare sixteen `const … : Color` of its own, all
// Catppuccin Mocha, which is why a Light desktop drew a dark Run box. They are
// gone; `render` takes the `&Palette` the shell resolved and reads roles out
// of it. See known-issues.md
// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//
// Two of the sixteen needed a decision rather than a lookup, both of them
// blue:
//
// - The **focus border, the selection highlight and the selected suggestion**
//   were `BLUE`. They are all "this is where you are typing", which is the
//   thing a user who picked a Red desktop expects to be red — so they read
//   `p.accent`, not `p.blue`.
// - The **OK button** was `BUTTON_PRIMARY` = the same blue, with
//   `BUTTON_PRIMARY_TEXT` = Mocha `base` on top. It is the default action, so
//   it too follows the accent, and its label becomes `p.on_accent()` rather
//   than a fixed dark value — a pale Latte accent needs dark text and a deep
//   Mocha one needs light, which is exactly what `on_accent` answers.
//
// The error message stays `p.red`: "this command does not exist" means the
// same thing on every desktop, and a Red-themed user must not get an error
// that is indistinguishable from the OK button.

// ============================================================================
// Constants
// ============================================================================

// The box is the theme's window frame (`dialog_frame`) around this content,
// so the frame's title bar and border are added to these, not part of them:
// under the built-in frame -- a 30-pixel bar, a 1-pixel border -- the whole
// box is 450 by 180, as it was before it wore one.
/// The content's width, inside the frame.
const CONTENT_WIDTH: f32 = 448.0;
/// The content's height, under the frame's title bar.
const CONTENT_HEIGHT: f32 = 148.0;
/// The title on the frame's bar.
const TITLE: &str = "Run";
const PADDING: f32 = 16.0;
const INPUT_HEIGHT: f32 = 28.0;
/// The instruction's top, from the content's.
const INSTRUCTION_Y: f32 = 21.0;
/// The command field's top, from the content's.
const INPUT_Y: f32 = 69.0;
const BUTTON_HEIGHT: f32 = 28.0;
const BUTTON_WIDTH: f32 = 75.0;
const BUTTON_SPACING: f32 = 8.0;
const BODY_FONT_SIZE: f32 = 12.0;
const INPUT_FONT_SIZE: f32 = 13.0;
const AUTOCOMPLETE_ROW_HEIGHT: f32 = 26.0;
const MAX_AUTOCOMPLETE: usize = 8;
const MAX_HISTORY: usize = 50;

// ============================================================================
// Events emitted by the dialog
// ============================================================================

/// Events produced by the Run dialog for the shell to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunDialogEvent {
    /// User pressed OK or Enter — run or open what was asked for. See
    /// [`RunRequest`] for what it carries and why both halves.
    Execute(RunRequest),
    /// User clicked Browse — open a file picker.
    Browse,
    /// User pressed Cancel or Escape.
    Cancel,
    /// Dialog was dismissed (after Cancel or Execute).
    Closed,
}

/// What the Run box was asked for: the whole line, and the same line as a
/// program and its arguments.
///
/// Both, because a line means one of two things and only the shell can tell
/// which: `/home/u/My Documents` is a folder to open, whole, spaces and all;
/// `editor /home/u/notes.txt` is a program to run with an argument. Windows'
/// Run box -- which `design.txt` asks this one to be like -- takes both, and
/// tries the whole line as a path first. So does the shell
/// (`DesktopShell::run_request`), and that order is what lets a path with a
/// space in it through without quotes.
///
/// Until 2026-09-25 the event carried the whole line as one program path, so
/// `editor notes.txt` asked for a program called "editor notes.txt" and a
/// folder asked to be executed. `design-decisions.md` §870 records the
/// quoting rule and why it is the shell's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    /// Everything that was asked for, exactly: the typed line, or the bytes
    /// **Browse** chose while the field still shows them. An `OsString`
    /// because a chosen file may have no UTF-8 spelling -- the field can only
    /// *show* a lossy rendering of such a name, and carrying the bytes keeps
    /// what opens the file the user pointed at.
    pub whole: OsString,
    /// The line as words: a program, then its arguments. A path chosen
    /// through Browse is one word, never split.
    pub words: Vec<OsString>,
}

/// Split a typed line into words the way a POSIX shell splits them, which is
/// the rule the rest of this system's command lines follow.
///
/// - Whitespace separates words; any run of it is one separator.
/// - `'...'` keeps everything inside literally.
/// - `"..."` keeps everything inside, except that `\"` is a quote and `\\`
///   a backslash.
/// - Outside quotes, a backslash makes the next character ordinary.
/// - Quotes join to what touches them: `a"b c"d` is the one word `ab cd`, and
///   `""` is an empty word.
///
/// An unclosed quote is an error rather than a guess: running `editor "my
/// file` as though the quote were closed would open a file the user may not
/// have finished naming.
pub fn split_words(line: &str) -> Result<Vec<String>, UnclosedQuote> {
    let mut words = Vec::new();
    let mut word = String::new();
    // Whether a word has started, which is not the same as being non-empty:
    // `""` is a word with nothing in it.
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(core::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(UnclosedQuote('\'')),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(UnclosedQuote('"')),
                        },
                        Some(c) => word.push(c),
                        None => return Err(UnclosedQuote('"')),
                    }
                }
            }
            '\\' => {
                in_word = true;
                // A trailing backslash escapes nothing and is kept, rather than
                // silently dropped.
                word.push(chars.next().unwrap_or('\\'));
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// A line whose quote was opened and never closed -- which quote it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnclosedQuote(pub char);

// ============================================================================
// Button identifiers
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonId {
    Ok,
    Cancel,
    Browse,
}

// ============================================================================
// Text input state
// ============================================================================

// ============================================================================
// Autocomplete
// ============================================================================

/// An autocomplete suggestion.
#[derive(Clone, Debug)]
struct Suggestion {
    /// Display text.
    text: String,
    /// The bytes the suggestion actually names, which `text` may only
    /// approximate.
    ///
    /// A history entry can be a path the user *pointed at* rather than typed,
    /// and our filenames admit every byte but `/` and NUL — so the entry may
    /// have no UTF-8 spelling and `text` is then a lossy rendering of it.
    /// Accepting the suggestion has to put the real bytes back in the field, or
    /// the completion silently names a different file from the history entry it
    /// offered. For a known-app suggestion, which comes from a text config,
    /// this is just `text` again.
    exact: OsString,
    /// Score for sorting (higher is better).
    score: u32,
}

// ============================================================================
// RunDialog
// ============================================================================

/// The Run dialog state and logic.
pub struct RunDialog {
    /// How wide to draw the caret, in pixels.
    ///
    /// The user's `caret_width_scale` already applied, because this struct is
    /// drawn from a `Palette` and a palette is colours. Carried rather than
    /// looked up per frame, and set from `DesktopShell::set_appearance` --
    /// which is the only place that knows the settings changed.
    caret_width: f32,
    /// How wide the field's focus mark is drawn: the user's focus width,
    /// pushed in with the caret width and for the same reason.
    focus_ring: f32,
    /// Whether the dialog is currently visible.
    visible: bool,
    /// Text input state.
    input: TextInput,
    /// Command history (most recent last).
    ///
    /// `OsString` rather than `String` because an entry can be a path the user
    /// chose with Browse, and such a path may have no UTF-8 spelling. Held as
    /// text, re-running it from history would ask for its rendering — the
    /// octal escapes spelled out as characters, a file that does not exist —
    /// and start nothing, without saying why.
    history: Vec<OsString>,
    /// Current position in history when cycling (-1 = not browsing history).
    history_index: Option<usize>,
    /// Text saved before entering history browse mode.
    pre_history_text: String,
    /// Known application names for autocomplete.
    known_apps: Vec<String>,
    /// Known PATH directories for resolution.
    path_dirs: Vec<String>,
    /// Autocomplete suggestions currently shown.
    suggestions: Vec<Suggestion>,
    /// Selected suggestion index.
    suggestion_index: Option<usize>,
    /// Whether to show autocomplete dropdown.
    show_autocomplete: bool,
    /// Error message to display (e.g., "not found").
    error_message: Option<String>,
    /// Pending events to drain.
    events: Vec<RunDialogEvent>,
    /// Which button is hovered.
    hovered_button: Option<ButtonId>,
    /// Whether the pointer is on the frame's close button.
    close_hovered: bool,
    /// The frame the box wears: the theme's window frame
    /// ([`DialogFrame`]), set by the shell from the appearance settings.
    frame: DialogFrame,
    /// Dialog X position (centered on screen, set by caller or default).
    dialog_x: f32,
    /// Dialog Y position.
    dialog_y: f32,
    /// The exact path Browse put in the field, if the field still shows it.
    ///
    /// The text field has to be a `String` — the user types into it a
    /// character at a time — so a chosen path with no UTF-8 spelling can only
    /// be *displayed* lossily. This holds the real bytes beside the text.
    /// [`execute_current`](Self::execute_current) uses them only while the
    /// text still matches their rendering, which is what makes an edit
    /// discard them without every text-mutating site having to remember to.
    command_exact: Option<PathBuf>,
}

impl RunDialog {
    /// Adopt the user's caret width, in pixels.
    ///
    /// Called by `DesktopShell::set_appearance`; `appearance::AppearanceSettings::caret_width`
    /// is what turns the stored scale into this number.
    pub fn set_caret_width(&mut self, width: f32) {
        self.caret_width = width;
    }

    /// Adopt the user's focus width, in pixels
    /// (`AppearanceSettings::focus_ring_width`), for the field's focus mark.
    pub fn set_focus_ring_width(&mut self, width: f32) {
        self.focus_ring = width;
    }

    /// Wear `frame`: the theme's window frame, from the appearance settings.
    ///
    /// The box keeps its top-left corner; its size is the frame around its
    /// content, so a theme with a taller title bar makes a taller box.
    pub fn set_frame(&mut self, frame: DialogFrame) {
        self.frame = frame;
    }

    /// Where the box and its parts are: the frame around the content, with
    /// its top-left corner at the dialog's position.
    fn layout(&self) -> FrameLayout {
        let (width, height) = self.frame.outer_size(CONTENT_WIDTH, CONTENT_HEIGHT);
        self.frame.layout(guitk::frame::Rect::new(
            self.dialog_x,
            self.dialog_y,
            width,
            height,
        ))
    }

    /// Where the button `id` is: OK at the content's bottom right, Cancel and
    /// Browse to its left. Shared by drawing and clicking, so the two cannot
    /// disagree.
    fn button_rect(content: guitk::frame::Rect, id: ButtonId) -> guitk::frame::Rect {
        let from_right = match id {
            ButtonId::Ok => 1.0,
            ButtonId::Cancel => 2.0,
            ButtonId::Browse => 3.0,
        };
        guitk::frame::Rect::new(
            content.x + content.w
                - PADDING
                - BUTTON_WIDTH * from_right
                - BUTTON_SPACING * (from_right - 1.0),
            content.y + content.h - PADDING - BUTTON_HEIGHT,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        )
    }

    /// Create a new Run dialog (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            caret_width: guitk::textedit::CARET_WIDTH,
            focus_ring: guitk::style::FOCUS_RING_WIDTH,
            input: TextInput::new(),
            history: Vec::new(),
            history_index: None,
            pre_history_text: String::new(),
            known_apps: default_known_apps(),
            path_dirs: default_path_dirs(),
            suggestions: Vec::new(),
            suggestion_index: None,
            show_autocomplete: false,
            error_message: None,
            events: Vec::new(),
            hovered_button: None,
            close_hovered: false,
            frame: DialogFrame::default(),
            // Default to centered-ish position; caller should reposition.
            dialog_x: 200.0,
            dialog_y: 150.0,
            command_exact: None,
        }
    }

    /// Put a path the user *chose* — rather than typed — in the command field.
    ///
    /// The field shows what a `String` can show of it; the exact bytes are
    /// kept beside the text so that pressing Enter starts the file that was
    /// pointed at, even when that file's name has no UTF-8 spelling.
    pub fn set_command_path(&mut self, path: &Path) {
        self.fill_exact(path.as_os_str());
        // The field's contents changed under the autocomplete list, and a
        // stale dropdown over a name the user did not type is worse than none.
        self.suggestions.clear();
        self.suggestion_index = None;
        self.show_autocomplete = false;
        self.error_message = None;
        self.history_index = None;
    }

    /// Put an exact byte string in the command field, showing what a `String`
    /// can show of it and keeping the bytes themselves beside the text.
    ///
    /// Every route by which the field is filled from something the user did not
    /// *type* goes through here — Browse, a history recall, an accepted
    /// autocomplete — because each of them can be carrying a name with no UTF-8
    /// spelling, and each of them was a separate opportunity to drop it.
    fn fill_exact(&mut self, exact: &OsStr) {
        self.input.set_text(&pathcodec::display_os(exact));
        self.command_exact = Some(PathBuf::from(exact));
    }

    /// The directory a file chooser opened from this box should start in.
    ///
    /// Asked of the dialog rather than computed by whoever puts the chooser up,
    /// because the useful answer depends on `command_exact` — which is private,
    /// and has to stay private for the reason
    /// [`set_command_path`](Self::set_command_path) exists at all. A second
    /// Browse should re-open where the first one left off, and "where it left
    /// off" is only spelled exactly by the bytes Browse itself chose; deriving
    /// it from the field's text would send the chooser to a directory whose
    /// name merely *looks* like the one the user picked.
    ///
    /// The field is validated against the text on the same terms
    /// `execute_current` validates it, so a user who has since typed over the
    /// chosen path gets the directory of what they typed.
    ///
    /// Falls back to the root, which is the only directory that certainly
    /// exists: a bare command name like `terminal` is not a path and has no
    /// directory to open, and starting the chooser in "the directory named
    /// `terminal`" would show an empty list.
    #[must_use]
    pub fn browse_start(&self) -> PathBuf {
        let text = self.input.text().trim();
        if !text.starts_with('/') {
            return PathBuf::from("/");
        }
        let shown = self
            .command_exact
            .as_ref()
            .filter(|p| pathcodec::display_path(p).trim() == text)
            .map_or_else(|| PathBuf::from(text), Clone::clone);
        guitk::dialog::parent_of(&shown)
    }

    /// Create a Run dialog with custom known apps and PATH dirs.
    ///
    /// Took a third `history_path: Option<String>` argument until 2026-09-03,
    /// which was stored in a field nothing ever read — the history has never
    /// been written anywhere. A parameter that only *looks* like it turns
    /// persistence on is worse than no parameter, so it is gone rather than
    /// deprecated.
    pub fn with_config(known_apps: Vec<String>, path_dirs: Vec<String>) -> Self {
        let mut dialog = Self::new();
        dialog.known_apps = known_apps;
        dialog.path_dirs = path_dirs;
        dialog
    }

    /// Set the dialog position (e.g., center on screen).
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.dialog_x = x;
        self.dialog_y = y;
    }

    /// Put the dialog in the middle of a screen of this size.
    ///
    /// Here rather than at the caller so that the box's size -- its content's
    /// and its frame's -- can stay private: a caller that had to be told the
    /// box's size in order to centre it would be a caller that could be told
    /// a stale one, and the layout is this module's business.
    ///
    /// Clamped at zero on both axes. A screen narrower than the box is not a
    /// real display, but it is a plausible *test* and a plausible transient
    /// during a mode change, and the arithmetic would otherwise place the box's
    /// left edge off-screen — where the title bar cannot be reached and the
    /// buttons are the half that gets cut.
    pub fn centre_on(&mut self, screen_width: f32, screen_height: f32) {
        let (width, height) = self.frame.outer_size(CONTENT_WIDTH, CONTENT_HEIGHT);
        self.dialog_x = ((screen_width - width) / 2.0).max(0.0);
        self.dialog_y = ((screen_height - height) / 2.0).max(0.0);
    }

    /// Where the dialog's top-left corner currently is.
    ///
    /// The read half of [`set_position`](Self::set_position). Exists because
    /// [`centre_on`](Self::centre_on) is a *calculation* the caller does not do
    /// and therefore cannot check: without this, the only way to tell a centred
    /// box from one left at the constructor's placeholder is to render both and
    /// compare pictures.
    #[must_use]
    pub fn position(&self) -> (f32, f32) {
        (self.dialog_x, self.dialog_y)
    }

    /// Whether the dialog is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the dialog, resetting input state.
    pub fn show(&mut self) {
        self.visible = true;
        self.input.clear();
        self.history_index = None;
        self.pre_history_text.clear();
        self.suggestions.clear();
        self.suggestion_index = None;
        self.show_autocomplete = false;
        self.error_message = None;
        self.hovered_button = None;
        // Harmless to leave — `command_exact` is only believed while the field
        // still renders to it, and the field has just been emptied — but a
        // freshly-shown box holding a path from the last time it was open is a
        // thing a reader has to reason about rather than read.
        self.command_exact = None;
    }

    /// Show the dialog again on a line that could not be started: the line in
    /// the field, exactly as it was typed, and `message` under it -- so a
    /// typo is corrected rather than typed again.
    pub fn show_failed(&mut self, line: &OsStr, message: String) {
        self.show();
        self.fill_exact(line);
        self.error_message = Some(message);
    }

    /// Hide the dialog.
    pub fn hide(&mut self) {
        self.visible = false;
        self.events.push(RunDialogEvent::Closed);
    }

    /// Drain pending events.
    pub fn drain_events(&mut self) -> Vec<RunDialogEvent> {
        core::mem::take(&mut self.events)
    }

    /// Add a command to history (called after successful execution).
    ///
    /// Takes the *exact* bytes, not the text on screen: a command that came
    /// from Browse may name a file with no UTF-8 spelling, and an entry
    /// remembered in its lossy rendering names a different file — in practice
    /// none at all, so recalling it would start nothing and say nothing.
    pub fn add_to_history(&mut self, command: &OsStr) {
        // Remove duplicate if present, so re-running a command moves it to the
        // front rather than filling the list with one entry.
        self.history.retain(|h| h.as_os_str() != command);
        self.history.push(command.to_os_string());
        self.trim_history();
    }

    /// Load a history read from somewhere else -- oldest first, as bytes,
    /// for the reason [`add_to_history`](Self::add_to_history) takes them.
    ///
    /// A file edited by hand may say a command twice: it is kept once, where
    /// it was run last, as running it again would have left it. Only the
    /// newest few hundred entries are looked at, so a file of millions costs
    /// no more than one of fifty.
    pub fn load_history(&mut self, commands: Vec<OsString>) {
        let newest = commands.len().saturating_sub(MAX_HISTORY.saturating_mul(4));
        let mut history: Vec<OsString> = Vec::with_capacity(MAX_HISTORY);
        for command in commands.into_iter().skip(newest) {
            history.retain(|h| *h != command);
            history.push(command);
        }
        self.history = history;
        self.trim_history();
    }

    /// Drop the oldest entries until at most `MAX_HISTORY` remain.
    ///
    /// One rule in one place: `add_to_history` used to cap by removing a
    /// single entry (correct only because it is called after a single push)
    /// and `load_history` by draining a difference it computed itself.
    fn trim_history(&mut self) {
        let excess = self.history.len().saturating_sub(MAX_HISTORY);
        self.history.drain(0..excess);
    }

    /// Get current history for persistence.
    pub fn history(&self) -> &[OsString] {
        &self.history
    }

    // ========================================================================
    // Input handling
    // ========================================================================

    /// Handle a key event. Returns `EventResult::Consumed` if the dialog handled it.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !self.visible || !event.pressed {
            return EventResult::Ignored;
        }

        let ctrl = event.modifiers.ctrl;
        let shift = event.modifiers.shift;

        match event.key {
            // Escape → cancel
            Key::Escape => {
                self.events.push(RunDialogEvent::Cancel);
                self.hide();
            }

            // Enter → execute
            Key::Enter => {
                self.execute_current();
            }

            // Tab → accept autocomplete suggestion
            Key::Tab => {
                self.accept_suggestion();
            }

            // Ctrl+A → select all
            Key::A if ctrl => {
                self.input.select_all();
            }

            // Ctrl+X → cut
            Key::X if ctrl => {
                self.input.cut();
                self.update_suggestions();
            }

            // Ctrl+C → copy
            Key::C if ctrl => {
                self.input.copy();
            }

            // Ctrl+V → paste
            Key::V if ctrl => {
                self.input.paste();
                self.update_suggestions();
            }

            // Arrow keys steer whichever list is open: the autocomplete
            // popup if one is showing, otherwise the command history.
            Key::Up => {
                if self.browsing_suggestions() {
                    self.select_prev_suggestion();
                } else {
                    self.history_prev();
                }
            }

            Key::Down => {
                if self.browsing_suggestions() {
                    self.select_next_suggestion();
                } else {
                    self.history_next();
                }
            }

            // Cursor movement
            Key::Left => {
                self.input
                    .move_cursor_left(shift, INPUT_FONT_SIZE, FontWeightHint::Regular);
            }

            Key::Right => {
                self.input
                    .move_cursor_right(shift, INPUT_FONT_SIZE, FontWeightHint::Regular);
            }

            // The page keys are the list's: the suggestions while they show,
            // else the history. Ctrl+Home and Ctrl+End are the suggestions'
            // while they show; plain Home and End, the text's -- the rule for
            // every field over a list in the shell (`design-decisions.md`
            // §1416).
            Key::PageUp | Key::PageDown if !ctrl => {
                self.page_list(event.key == Key::PageDown);
            }

            Key::Home | Key::End if ctrl && self.browsing_suggestions() => {
                self.page_list(event.key == Key::End);
            }

            Key::Home => {
                self.input.move_home(shift);
            }

            Key::End => {
                self.input.move_end(shift);
            }

            // Editing
            Key::Backspace => {
                self.input.backspace();
                self.update_suggestions();
            }

            Key::Delete => {
                self.input.delete();
                self.update_suggestions();
            }

            // Text input (character typed)
            _ => {
                // One test, not two. A keystroke that typed only a control
                // character is as much "not for this dialog" as one that typed
                // nothing at all, and both must reach `Ignored` — falling
                // through to the `Consumed` below would swallow a keystroke the
                // desktop behind us still wants to see.
                if !event.types_text() {
                    return EventResult::Ignored;
                }
                for ch in event.typed() {
                    self.input.insert_char(ch);
                }
                self.update_suggestions();
                self.error_message = None;
            }
        }

        EventResult::Consumed
    }

    /// The command line as it stands, for the shell's tests.
    #[cfg(test)]
    pub(crate) fn line(&self) -> &str {
        self.input.text()
    }

    /// Where the command field is on screen: where it is drawn, and where a
    /// right-click offers the field's menu (`guitk::editmenu`).
    #[must_use]
    pub fn field_rect(&self) -> guitk::frame::Rect {
        let content = self.layout().content;
        guitk::frame::Rect::new(
            content.x + PADDING + 40.0,
            content.y + INPUT_Y,
            CONTENT_WIDTH - PADDING * 2.0 - 40.0,
            INPUT_HEIGHT,
        )
    }

    /// The command field's right-click menu: Cut, Copy, Paste, Delete and
    /// Select all, each dimmed when it would do nothing and saying why while
    /// the pointer rests on it.
    #[must_use]
    pub fn edit_menu(&self) -> guitk::menu::ContextMenu {
        self.input.edit_menu()
    }

    /// Do what a row of [`edit_menu`](Self::edit_menu) says, as the key that
    /// does the same would: a change brings new suggestions and takes away
    /// the last complaint, as typing does. Answers whether the row was one of
    /// the field's.
    pub fn edit_command(&mut self, id: guitk::menu::MenuItemId) -> bool {
        match self.input.edit_command(id) {
            guitk::textinput::KeyEdit::Unhandled => false,
            guitk::textinput::KeyEdit::Handled => true,
            guitk::textinput::KeyEdit::Changed => {
                self.update_suggestions();
                self.error_message = None;
                true
            }
        }
    }

    /// Handle a mouse event. Returns `EventResult::Consumed` if the dialog handled it.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        if !self.visible {
            return EventResult::Ignored;
        }

        let layout = self.layout();
        let (x, y) = (event.x, event.y);

        // Check if click is outside dialog bounds — dismiss.
        if !layout.outer.contains(x, y) {
            if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
                self.events.push(RunDialogEvent::Cancel);
                self.hide();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // The frame's close button does what Cancel and Escape do.
        let on_close = layout.is_close(x, y);

        // Button hit detection.
        let hit_button = [ButtonId::Ok, ButtonId::Cancel, ButtonId::Browse]
            .into_iter()
            .find(|&id| Self::button_rect(layout.content, id).contains(x, y));

        match &event.kind {
            MouseEventKind::Move => {
                self.hovered_button = hit_button;
                self.close_hovered = on_close;
            }
            MouseEventKind::Press(MouseButton::Left) if on_close => {
                self.events.push(RunDialogEvent::Cancel);
                self.hide();
            }
            MouseEventKind::Press(MouseButton::Left) => {
                match hit_button {
                    Some(ButtonId::Ok) => self.execute_current(),
                    Some(ButtonId::Cancel) => {
                        self.events.push(RunDialogEvent::Cancel);
                        self.hide();
                    }
                    Some(ButtonId::Browse) => {
                        self.events.push(RunDialogEvent::Browse);
                    }
                    None => {
                        // Check autocomplete dropdown clicks.
                        if self.show_autocomplete {
                            let field = self.field_rect();
                            let dropdown_y = field.y + field.h + 2.0;
                            let rel_y = y - dropdown_y;
                            if rel_y >= 0.0 && x >= field.x {
                                let idx = (rel_y / AUTOCOMPLETE_ROW_HEIGHT) as usize;
                                if idx < self.suggestions.len() {
                                    self.suggestion_index = Some(idx);
                                    self.accept_suggestion();
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the dialog to a list of render commands.
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        // The frame: the theme's window frame round the content -- its
        // shadow, the body, the title bar with the title and the close
        // button, the border.
        let layout = self.layout();
        let mut cmds = self.frame.render(&layout, TITLE, p, self.close_hovered);
        let content = layout.content;
        let (x, y) = (content.x, content.y);

        // Instruction text.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + INSTRUCTION_Y,
            text: "Type the name of a program, folder, or document, and the \
                   OS will open it for you."
                .to_string(),
            color: p.subtext0,
            font_size: BODY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CONTENT_WIDTH - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // "Open:" label.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + INPUT_Y + 6.0,
            text: "Open:".to_string(),
            color: p.text,
            font_size: BODY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Input field background: where a right-click offers the field's
        // menu, from the one answer to where the field is.
        let field = self.field_rect();
        let input_x = field.x;
        let input_w = field.w;

        // The toolkit's field (`guitk::field`), in the theme's shape: it has
        // the keyboard whenever the box is up, and a command that does not
        // exist marks it wrong as well as saying so below it.
        guitk::field::draw(
            &mut cmds,
            p,
            field,
            guitk::field::State {
                focused: true,
                invalid: self.error_message.is_some(),
                ..guitk::field::State::default()
            },
            self.focus_ring,
        );

        // Selection highlight (if any).
        if self.input.has_selection() {
            let (start, _) = self.input.selection_range();
            // These are byte offsets that `floor_boundary` keeps on character
            // boundaries — but a render pass is the wrong place to find out
            // that one of them isn't, so it tolerates a bad offset the same
            // way `selected_text` already does rather than panicking mid-frame.
            let text_before_start = self.input.text().get(..start).unwrap_or("");
            let start_px = text::width(text_before_start, INPUT_FONT_SIZE);
            let sel_width = text::width(self.input.selected_text(), INPUT_FONT_SIZE);
            cmds.push(RenderCommand::FillRect {
                x: input_x + 4.0 + start_px,
                y: y + INPUT_Y + 3.0,
                width: sel_width,
                height: INPUT_HEIGHT - 6.0,
                color: p.accent,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Input text.
        cmds.push(RenderCommand::Text {
            x: input_x + 4.0,
            y: y + INPUT_Y + 7.0,
            text: self.input.text().to_string(),
            color: p.text,
            font_size: INPUT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Cursor, placed by the shaper rather than by measuring the text before
        // it. The prefix's width is only where the caret belongs while the line
        // runs in one direction; where a right-to-left run meets a
        // left-to-right one it is not, and which of the two candidate positions
        // is right depends on the affinity the cursor carries. This is what
        // makes the visual arrows draw where they move.
        //
        // It also retires a real panic: `&self.input.text()[..self.input.cursor()]`
        // sliced a `str` at a raw byte offset, so a caret that had drifted
        // inside a character took the whole desktop shell down while merely
        // *drawing* the dialog. `caret_x` is handed the cursor, not a slice.
        let cursor_px = text::caret_x(
            self.input.text(),
            self.input.cursor(),
            INPUT_FONT_SIZE,
            FontWeightHint::Regular,
        );
        let caret_top = y + INPUT_Y + 4.0;
        let mut caret = guitk::render::RenderTree::new();
        guitk::textedit::push_caret(
            &mut caret,
            input_x + 4.0 + cursor_px,
            caret_top,
            INPUT_HEIGHT - 8.0,
            p.text,
            self.caret_width,
        );
        cmds.extend(caret.commands);

        // Error message.
        if let Some(ref err) = self.error_message {
            cmds.push(RenderCommand::Text {
                x: input_x,
                y: y + INPUT_Y + INPUT_HEIGHT + 2.0,
                text: err.clone(),
                color: p.ink(p.red),
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Autocomplete dropdown.
        if self.show_autocomplete && !self.suggestions.is_empty() {
            let dropdown_x = input_x;
            let dropdown_y = y + INPUT_Y + INPUT_HEIGHT + 2.0;
            let dropdown_h = self.suggestions.len() as f32 * AUTOCOMPLETE_ROW_HEIGHT;

            let mut paint = p.surface_paint(Surface::Panel);
            paint.border = Some(paint.border.unwrap_or(p.surface1));
            p.push_paint_radii(
                &mut cmds,
                dropdown_x,
                dropdown_y,
                input_w,
                dropdown_h,
                CornerRadii::all(4.0),
                paint,
            );

            for (i, suggestion) in self.suggestions.iter().enumerate() {
                let row_y = dropdown_y + i as f32 * AUTOCOMPLETE_ROW_HEIGHT;
                let is_selected = self.suggestion_index == Some(i);

                if is_selected {
                    p.push_surface(
                        &mut cmds,
                        dropdown_x + 1.0,
                        row_y,
                        input_w - 2.0,
                        AUTOCOMPLETE_ROW_HEIGHT,
                        0.0,
                        Surface::Selected,
                    );
                }

                cmds.push(RenderCommand::Text {
                    x: dropdown_x + 8.0,
                    y: row_y + 6.0,
                    text: suggestion.text.clone(),
                    color: if is_selected { p.ink(p.accent) } else { p.text },
                    font_size: INPUT_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(input_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Buttons row.
        for (label, id, primary) in [
            ("OK", ButtonId::Ok, true),
            ("Cancel", ButtonId::Cancel, false),
            ("Browse...", ButtonId::Browse, false),
        ] {
            self.render_button(
                p,
                &mut cmds,
                label,
                Self::button_rect(content, id),
                id,
                primary,
            );
        }

        cmds
    }

    // ========================================================================
    // Private methods
    // ========================================================================

    fn render_button(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        label: &str,
        rect: guitk::frame::Rect,
        id: ButtonId,
        primary: bool,
    ) {
        // The toolkit's button, the reference's Aero button: OK the box's own
        // action, tinted with the accent. It used to be the only button the
        // pointer did not light.
        let kind = if primary {
            guitk::button::Kind::Primary
        } else {
            guitk::button::Kind::Plain
        };
        guitk::button::draw(
            cmds,
            p,
            (rect.x, rect.y, rect.w, rect.h),
            label,
            kind,
            guitk::button::State {
                hovered: self.hovered_button == Some(id),
                ..guitk::button::State::default()
            },
            p.base,
            0.0,
        );
    }

    fn execute_current(&mut self) {
        let command = self.input.text().trim().to_string();
        if command.is_empty() {
            return;
        }

        // The bytes Browse chose, but only while the field still *shows* them.
        //
        // Checking the rendering rather than clearing the field on every edit
        // is deliberate: this dialog mutates its text from a dozen places —
        // insert, backspace, delete, cut, paste, history recall, autocomplete
        // accept — and an invalidation that has to be remembered in each of
        // them is one that will eventually be forgotten in a new one. Deriving
        // the answer from the text cannot go stale. A user who types back the
        // exact glyphs on screen gets the file those glyphs came from, which
        // is the only file they could have meant.
        let exact = self
            .command_exact
            .as_ref()
            .filter(|p| pathcodec::display_path(p).trim() == command)
            .map_or_else(
                || OsString::from(&command),
                |p| p.as_os_str().to_os_string(),
            );

        // The line as a program and its arguments. A path Browse chose is one
        // word whatever is in it: it is a name, not a command line, and a
        // name with a space in it split into two would be two things that do
        // not exist.
        let chose_browse = self
            .command_exact
            .as_ref()
            .is_some_and(|p| p.as_os_str() == exact.as_os_str());
        let words: Vec<OsString> = if chose_browse {
            vec![exact.clone()]
        } else {
            match split_words(&command) {
                Ok(words) => words.into_iter().map(OsString::from).collect(),
                Err(UnclosedQuote(quote)) => {
                    self.error_message = Some(format!(
                        "A {} quote is not closed.",
                        if quote == '"' { "double" } else { "single" }
                    ));
                    return;
                }
            }
        };

        // Resolved by the program -- the first word -- unless the whole line
        // is an absolute path, which the shell will try as a thing to open
        // before it splits anything. `is_absolute` rather than a leading `/`:
        // the same question, asked the way the platform spells it.
        let program = words
            .first()
            .and_then(|w| w.to_str())
            .unwrap_or(command.as_str());
        if Path::new(&command).is_absolute() || self.resolve_command(program) {
            // The history gets the bytes, not the rendering, so that pressing
            // Up and Enter re-runs the file that ran — see `add_to_history`.
            self.add_to_history(&exact);
            self.events.push(RunDialogEvent::Execute(RunRequest {
                whole: exact,
                words,
            }));
            self.hide();
        } else {
            self.error_message = Some(format!(
                "\"{}\" is not recognized as an application or command.",
                command
            ));
        }
    }

    /// Resolve a command: check if it is an absolute path, a known app, or on PATH.
    fn resolve_command(&self, command: &str) -> bool {
        // Absolute paths pass through directly.
        if command.starts_with('/') {
            return true;
        }

        // Extract the program name (first word).
        let program = command.split_whitespace().next().unwrap_or(command);

        // Check known apps (case-insensitive).
        let program_lower = program.to_ascii_lowercase();
        for app in &self.known_apps {
            if app.to_ascii_lowercase() == program_lower {
                return true;
            }
        }

        // Check PATH directories (simulate: just check if program name is non-empty
        // and doesn't contain invalid chars — real resolution would stat files).
        if !program.is_empty() && !program.contains('\0') {
            for _dir in &self.path_dirs {
                // In a real implementation, we would check if dir/program exists.
                // For now, accept anything that looks like a valid command name.
                if program
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
                {
                    return true;
                }
            }
        }

        false
    }

    /// Step one entry towards the *older* end of the history, entering browse
    /// mode from the newest entry if not already in it.
    ///
    /// Both directions read the entry through `get` and step with checked
    /// arithmetic. They used to index `self.history[idx]` after deciding the
    /// index was in range in the statement before — which held, but only
    /// because `history_index` was maintained correctly by the two methods
    /// that write it, at a distance of eighty lines from the index expression.
    fn history_prev(&mut self) {
        let entering = self.history_index.is_none();
        let target = match self.history_index {
            // Not browsing yet: start at the newest entry. `checked_sub` is
            // also the empty-history test — there is no newest entry.
            None => self.history.len().checked_sub(1),
            // Already at the oldest: stay there rather than wrapping.
            Some(idx) => Some(idx.saturating_sub(1)),
        };
        let Some(entry) = target.and_then(|idx| self.history.get(idx).cloned()) else {
            return;
        };
        if entering {
            self.pre_history_text = self.input.text().to_string();
        }
        self.history_index = target;
        self.fill_exact(&entry);
        self.hide_suggestions();
    }

    /// Step one entry towards the *newer* end, leaving browse mode and
    /// restoring the user's own text when stepping past the newest.
    fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            match idx
                .checked_add(1)
                .filter(|&newer| newer < self.history.len())
                .and_then(|newer| Some((newer, self.history.get(newer)?.clone())))
            {
                Some((newer, entry)) => {
                    self.history_index = Some(newer);
                    self.fill_exact(&entry);
                }
                None => {
                    // Past the newest entry: back to whatever was typed before
                    // browsing started. Deliberately *not* through
                    // `fill_exact` — this text is the user's own, typed a
                    // character at a time, so there are no exact bytes behind
                    // it. Any left over from the entry just stepped off stop
                    // being believed the moment the field stops rendering to
                    // them, which is now.
                    self.history_index = None;
                    let saved = core::mem::take(&mut self.pre_history_text);
                    self.input.set_text(&saved);
                    self.pre_history_text = saved;
                }
            }
        }
        if self.history_index.is_some() {
            self.hide_suggestions();
        } else {
            self.update_suggestions();
        }
    }

    /// Page Up or Page Down: to the end of whichever list the arrows steer.
    ///
    /// The suggestions all fit on the screen (there are at most
    /// `MAX_AUTOCOMPLETE`), so a page of them is all of them. The history is
    /// not drawn at all, so a page of it is its end: Page Up the oldest entry,
    /// Page Down back past the newest to what was typed before browsing --
    /// where the arrows would take it one step at a time.
    fn page_list(&mut self, down: bool) {
        if self.browsing_suggestions() {
            let last = self.suggestions.len().saturating_sub(1);
            self.suggestion_index = Some(if down { last } else { 0 });
        } else if down {
            // Bounded by the history's length: each step moves one entry
            // newer, and the last leaves browsing.
            for _ in 0..=self.history.len() {
                if self.history_index.is_none() {
                    break;
                }
                self.history_next();
            }
        } else {
            self.history_oldest();
        }
    }

    /// Go straight to the oldest history entry, entering browse mode as
    /// [`history_prev`](Self::history_prev) does.
    fn history_oldest(&mut self) {
        let Some(entry) = self.history.first().cloned() else {
            return;
        };
        if self.history_index.is_none() {
            self.pre_history_text = self.input.text().to_string();
        }
        self.history_index = Some(0);
        self.fill_exact(&entry);
        self.hide_suggestions();
    }

    /// Put the suggestions away while the history is being browsed.
    ///
    /// A recalled entry matches itself, so refreshing the suggestions for it
    /// opened the popup with its first row picked -- and from then on the
    /// arrows steered the popup, not the history: Up recalled one entry and
    /// then stopped. The popup comes back when the user types, or steps past
    /// the newest entry to their own text again.
    fn hide_suggestions(&mut self) {
        self.suggestions.clear();
        self.show_autocomplete = false;
        self.suggestion_index = None;
    }

    /// Whether the arrow keys are steering the autocomplete popup rather than
    /// the history.
    fn browsing_suggestions(&self) -> bool {
        self.show_autocomplete && self.suggestion_index.is_some()
    }

    /// Highlight the previous suggestion, stopping at the first.
    fn select_prev_suggestion(&mut self) {
        if let Some(idx) = self.suggestion_index {
            self.suggestion_index = Some(idx.saturating_sub(1));
        }
    }

    /// Highlight the next suggestion, stopping at the last.
    fn select_next_suggestion(&mut self) {
        if let Some(idx) = self.suggestion_index
            && let Some(next) = idx
                .checked_add(1)
                .filter(|&next| next < self.suggestions.len())
        {
            self.suggestion_index = Some(next);
        }
    }

    fn accept_suggestion(&mut self) {
        if !self.show_autocomplete || self.suggestions.is_empty() {
            return;
        }
        let idx = self.suggestion_index.unwrap_or(0);
        if let Some(exact) = self.suggestions.get(idx).map(|s| s.exact.clone()) {
            // The bytes, not `text`: a history suggestion can be a path with no
            // UTF-8 spelling, and completing to its rendering would silently
            // offer one file and fill in another.
            self.fill_exact(&exact);
            self.show_autocomplete = false;
            self.suggestions.clear();
            self.suggestion_index = None;
        }
    }

    fn update_suggestions(&mut self) {
        let query = self.input.text().trim();
        if query.is_empty() {
            self.suggestions.clear();
            self.show_autocomplete = false;
            self.suggestion_index = None;
            return;
        }

        let mut results: Vec<Suggestion> = Vec::new();

        // Match against known apps.
        for app in &self.known_apps {
            if let Some(score) = fuzzy_score(query, app) {
                results.push(Suggestion {
                    text: app.clone(),
                    exact: OsString::from(app),
                    score,
                });
            }
        }

        // Match against history. The *rendering* is what gets matched and shown
        // — a fuzzy score over bytes the user cannot see would be a score over
        // nothing they could have typed — but the entry's own bytes travel with
        // it so that accepting the suggestion fills in the file it named.
        for cmd in &self.history {
            let shown = pathcodec::display_os(cmd);
            if let Some(score) = fuzzy_score(query, &shown) {
                // Avoid duplicates.
                if !results.iter().any(|s| s.exact == *cmd) {
                    results.push(Suggestion {
                        text: shown,
                        exact: cmd.clone(),
                        score: score.saturating_add(5), // slight history bonus
                    });
                }
            }
        }

        // Sort by score descending.
        results.sort_by_key(|r| std::cmp::Reverse(r.score));
        results.truncate(MAX_AUTOCOMPLETE);

        self.show_autocomplete = !results.is_empty();
        self.suggestions = results;
        // Reset selection to first item if we have suggestions.
        self.suggestion_index = if self.show_autocomplete {
            Some(0)
        } else {
            None
        };
    }
}

impl Default for RunDialog {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Default data
// ============================================================================

fn default_known_apps() -> Vec<String> {
    vec![
        "terminal".to_string(),
        "file-explorer".to_string(),
        "text-editor".to_string(),
        "settings".to_string(),
        "process-explorer".to_string(),
        "calculator".to_string(),
        "browser".to_string(),
        "image-viewer".to_string(),
        "music-player".to_string(),
        "video-player".to_string(),
        "package-manager".to_string(),
        "system-monitor".to_string(),
        "disk-utility".to_string(),
        "network-settings".to_string(),
        "display-settings".to_string(),
    ]
}

fn default_path_dirs() -> Vec<String> {
    vec![
        "/usr/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/bin".to_string(),
        "/sbin".to_string(),
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    // Only the tests place a caret by hand now; the field owns it.
    use appearance::palette_check;
    use guitk::text::TextCursor;

    fn make_key(key: Key, ctrl: bool, shift: bool, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers {
                shift,
                ctrl,
                alt: false,
                super_key: false,
            },
            text: text.map_or_else(String::new, |c| c.to_string()),
        }
    }

    // ====================================================================
    // Text input tests
    // ====================================================================

    // ------------------------------------------------------------------
    // Multi-byte text, which is where the byte offsets are load-bearing
    // ------------------------------------------------------------------

    #[test]
    fn a_cursor_step_crosses_a_whole_character_not_a_byte() {
        // "é" is two bytes, "→" three, "😀" four: one of each, so a step that
        // moved by a fixed amount would land inside a character and the next
        // edit would panic.
        let mut input = TextInput::new();
        input.set_text("aé→😀b");
        input.move_home(false);
        let mut offsets = vec![input.cursor().byte()];
        for _ in 0..5 {
            input.move_cursor_right(false, INPUT_FONT_SIZE, FontWeightHint::Regular);
            offsets.push(input.cursor().byte());
        }
        assert_eq!(offsets, vec![0, 1, 3, 6, 10, 11]);

        // And back, landing on the same boundaries in reverse.
        let mut back = vec![input.cursor().byte()];
        for _ in 0..5 {
            input.move_cursor_left(false, INPUT_FONT_SIZE, FontWeightHint::Regular);
            back.push(input.cursor().byte());
        }
        back.reverse();
        assert_eq!(back, offsets);
    }

    #[test]
    fn an_offset_left_inside_a_character_shortens_the_edit_rather_than_panicking() {
        // Nothing in the type is supposed to produce a mid-character offset,
        // but "supposed to" is what a panic in `String::replace_range` is made
        // of. Every entry point clamps to a boundary instead.
        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.set_cursor(TextCursor::from(3)); // inside the four-byte character, which spans 1..5
        input.set_selection_anchor(None);
        input.backspace();
        // The offset floors to the start of the character it was inside, so
        // backspace takes the `a` before that. *Which* character goes is not
        // the claim — the claim is that an offset the type cannot legitimately
        // hold produces a smaller edit and a cursor still on a boundary,
        // rather than a panic inside `String::replace_range`.
        assert_eq!(input.text(), "😀b");
        assert!(input.text().is_char_boundary(input.cursor().byte()));

        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.set_cursor(TextCursor::from(99)); // past the end
        input.delete();
        assert_eq!(input.text(), "a😀b");
        assert!(input.text().is_char_boundary(input.cursor().byte()));
    }

    /// Where the dialog *draws* its caret, in the order it drew it.
    fn drawn_caret_x(dialog: &RunDialog, p: &Palette) -> f32 {
        dialog
            .render(p)
            .into_iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Line { x1, x2, y1, y2, .. }
                    if (x1 - x2).abs() < f32::EPSILON && y2 > y1 =>
                {
                    Some(x1)
                }
                _ => None,
            })
            .expect("the dialog draws a caret")
    }

    /// Moving the caret correctly is only half of §541 — it must also be
    /// *drawn* where it moved to, and no test of `cursor` can see that half.
    ///
    /// The dialog used to place it at the width of `text[..cursor]`. A prefix
    /// width is where the caret belongs only while the line runs in one
    /// direction: with the visual arrows above walking `2, 4, 2` through the
    /// Hebrew, a prefix measurement sends the drawn caret *backwards* on the
    /// screen twice while the user is pressing Right. Asking the shaper
    /// (`text::caret_x`) instead is what keeps the two halves agreeing.
    ///
    /// **A failure here is that disagreement**: six Rights, six strictly
    /// increasing positions, because the arrow is walking the screen left to
    /// right and nothing it does may go the other way.
    /// Every caret in the shell is drawn at the toolkit's one width.
    ///
    /// Before 2026-09-09 each field picked its own: this dialog drew 1.0, the
    /// launcher 2.0, the path bar 2.0, the toolkit's own fields 1.0. Nothing
    /// had decided that; they were written at different times. They now all go
    /// through `guitk::textedit::push_caret`, which is what gives an
    /// accessibility scale a single place to apply -- see `known-issues.md`
    /// `TD-C-THE-ACCESSIBILITY-CONFIG-IS-A-DEAD-PARALLEL-COPY`.
    ///
    /// The assertion is against the toolkit's constant rather than a literal,
    /// so changing the shared width stays a one-line change; what it forbids is
    /// a field going back to a width of its own.
    #[test]
    fn the_run_dialog_draws_its_caret_at_the_shared_width() {
        let p = Palette::for_mode(false);
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("abc");
        let widths: Vec<f32> = dialog
            .render(&p)
            .into_iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Line { x1, x2, width, .. } if (x1 - x2).abs() < f32::EPSILON => {
                    Some(width)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            widths,
            [guitk::textedit::CARET_WIDTH],
            "the dialog drew a caret at a width of its own"
        );
    }

    #[test]
    fn the_run_dialogs_drawn_caret_only_ever_moves_rightwards_under_the_right_arrow() {
        let p = Palette::for_mode(false);
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("ab\u{05D0}\u{05D1}cd");
        dialog.input.move_home(false);

        let mut xs = vec![drawn_caret_x(&dialog, &p)];
        for _ in 0..6 {
            dialog
                .input
                .move_cursor_right(false, INPUT_FONT_SIZE, FontWeightHint::Regular);
            xs.push(drawn_caret_x(&dialog, &p));
        }
        for pair in xs.windows(2) {
            let (before, after) = (pair[0], pair[1]);
            assert!(
                after > before,
                "the caret went from {before} to {after} on a Right press: {xs:?}"
            );
        }
    }

    /// A caret byte offset off a character boundary must not abort the process.
    ///
    /// This is not hypothetical tidiness. `&self.input.text()[..self.input.cursor()]`
    /// panics on such an offset, and it sat inside `render` — so a cursor that
    /// had drifted took the whole desktop shell down while merely *painting*
    /// the dialog, with no user action involved beyond it being on screen.
    #[test]
    fn a_run_dialog_caret_off_a_character_boundary_draws_rather_than_panicking() {
        let p = Palette::for_mode(false);
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("é");
        dialog.input.set_cursor(TextCursor::from(1)); // inside the two-byte letter
        assert!(!dialog.render(&p).is_empty());
    }

    // ====================================================================
    // History cycling tests
    // ====================================================================

    /// **A history read back keeps each command once, where it ran last**,
    /// as running it again would have left it -- and of a file of thousands,
    /// the newest fifty.
    #[test]
    fn a_loaded_history_keeps_each_command_once_where_it_ran_last() {
        let mut dialog = RunDialog::new();
        dialog.load_history(["ls", "pwd", "ls", "cat"].map(OsString::from).to_vec());
        assert_eq!(dialog.history(), ["pwd", "ls", "cat"].map(OsString::from));
        let many: Vec<OsString> = (0..10_000)
            .map(|i| OsString::from(format!("cmd{i}")))
            .collect();
        dialog.load_history(many);
        assert_eq!(dialog.history().len(), MAX_HISTORY);
        assert_eq!(dialog.history().first(), Some(&OsString::from("cmd9950")));
        assert_eq!(dialog.history().last(), Some(&OsString::from("cmd9999")));
    }

    #[test]
    fn test_history_cycling() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history(OsStr::new("ls"));
        dialog.add_to_history(OsStr::new("pwd"));
        dialog.add_to_history(OsStr::new("cat file.txt"));

        // Navigate up through history.
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "cat file.txt");
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "pwd");
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "ls");

        // Navigate back down.
        dialog.history_next();
        assert_eq!(dialog.input.text(), "pwd");
        dialog.history_next();
        assert_eq!(dialog.input.text(), "cat file.txt");

        // Past the end returns to original.
        dialog.history_next();
        assert_eq!(dialog.input.text(), "");
    }

    /// Up recalls one entry after another through the keys, and Down steps
    /// back. Through the keys is the point: the methods alone always worked,
    /// but a recalled entry opened the suggestions popup, which then took the
    /// arrows, so Up recalled the newest entry and nothing further.
    #[test]
    fn the_arrows_walk_the_whole_history_through_the_keys() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history(OsStr::new("ls"));
        dialog.add_to_history(OsStr::new("pwd"));
        dialog.add_to_history(OsStr::new("cat file.txt"));
        let key = |k| KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        };
        dialog.handle_key_event(&key(Key::Up));
        assert_eq!(dialog.input.text(), "cat file.txt");
        dialog.handle_key_event(&key(Key::Up));
        assert_eq!(
            dialog.input.text(),
            "pwd",
            "the second Up went to the popup"
        );
        dialog.handle_key_event(&key(Key::Up));
        assert_eq!(dialog.input.text(), "ls");
        dialog.handle_key_event(&key(Key::Down));
        assert_eq!(dialog.input.text(), "pwd");
    }

    /// Page Up goes to the oldest entry, and Page Down back past the newest
    /// to what was typed -- the ends of the history the arrows step through.
    #[test]
    fn the_page_keys_go_to_the_ends_of_the_history() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history(OsStr::new("ls"));
        dialog.add_to_history(OsStr::new("pwd"));
        dialog.add_to_history(OsStr::new("cat file.txt"));
        dialog.input.set_text("draft");
        let key = |k| KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        };
        dialog.handle_key_event(&key(Key::PageUp));
        assert_eq!(dialog.input.text(), "ls", "not the oldest entry");
        dialog.handle_key_event(&key(Key::PageDown));
        assert_eq!(dialog.input.text(), "draft", "not back to what was typed");
        assert!(dialog.history_index.is_none());
    }

    #[test]
    fn test_history_preserves_current_text() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history(OsStr::new("old-command"));

        // Type something.
        dialog.input.set_text("partial");

        // Go up into history.
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "old-command");

        // Come back down — original text restored.
        dialog.history_next();
        assert_eq!(dialog.input.text(), "partial");
    }

    #[test]
    fn test_history_max_entries() {
        let mut dialog = RunDialog::new();
        for i in 0..60 {
            dialog.add_to_history(OsStr::new(&format!("cmd{i}")));
        }
        assert_eq!(dialog.history.len(), MAX_HISTORY);
        // Oldest entries removed.
        assert_eq!(dialog.history[0], "cmd10");
    }

    #[test]
    fn test_history_dedup() {
        let mut dialog = RunDialog::new();
        dialog.add_to_history(OsStr::new("ls"));
        dialog.add_to_history(OsStr::new("pwd"));
        dialog.add_to_history(OsStr::new("ls")); // duplicate
        assert_eq!(dialog.history.len(), 2);
        // "ls" should be at the end (most recent).
        assert_eq!(dialog.history[0], "pwd");
        assert_eq!(dialog.history[1], "ls");
    }

    #[test]
    fn stepping_past_either_end_of_the_history_stops_rather_than_wrapping() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history(OsStr::new("one"));
        dialog.add_to_history(OsStr::new("two"));

        // Older, older, and once more past the oldest.
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "two");
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "one");
        dialog.history_prev();
        assert_eq!(dialog.input.text(), "one");

        // Back down, and one step past the newest returns the typed text.
        dialog.history_next();
        assert_eq!(dialog.input.text(), "two");
        dialog.history_next();
        assert_eq!(dialog.input.text(), "");
        assert!(dialog.history_index.is_none());
        // Already out of browse mode: another step changes nothing.
        dialog.history_next();
        assert_eq!(dialog.input.text(), "");
    }

    #[test]
    fn browsing_an_empty_history_does_nothing() {
        let mut dialog = RunDialog::new();
        dialog.show();
        for ch in "typed".chars() {
            dialog.input.insert_char(ch);
        }
        dialog.history_prev();
        dialog.history_next();
        assert_eq!(dialog.input.text(), "typed");
        assert!(dialog.history_index.is_none());
    }

    // ====================================================================
    // Autocomplete / fuzzy matching tests
    // ====================================================================

    #[test]
    fn test_fuzzy_score_exact() {
        let score = fuzzy_score("terminal", "terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 50); // High score for exact match.
    }

    #[test]
    fn test_fuzzy_score_prefix() {
        let score = fuzzy_score("term", "terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 30); // Prefix matches score well.
    }

    #[test]
    fn test_fuzzy_score_no_match() {
        let score = fuzzy_score("xyz", "terminal");
        assert!(score.is_none());
    }

    #[test]
    fn test_fuzzy_score_boundary() {
        // "fe" should match "file-explorer" at word boundaries.
        let score = fuzzy_score("fe", "file-explorer");
        assert!(score.is_some());
    }

    #[test]
    fn test_fuzzy_score_case_insensitive() {
        let score = fuzzy_score("TERM", "terminal");
        assert!(score.is_some());
    }

    #[test]
    fn test_update_suggestions() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("term");
        dialog.update_suggestions();
        assert!(!dialog.suggestions.is_empty());
        // "terminal" should be in the suggestions.
        assert!(dialog.suggestions.iter().any(|s| s.text == "terminal"));
    }

    #[test]
    fn test_accept_suggestion() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("term");
        dialog.update_suggestions();
        assert!(dialog.show_autocomplete);

        dialog.accept_suggestion();
        assert_eq!(dialog.input.text(), "terminal");
        assert!(!dialog.show_autocomplete);
    }

    /// A name with no UTF-8 spelling, built through the platform's own safe API
    /// rather than by asserting bytes into an `OsStr`: a lone high surrogate is
    /// a legal Windows filename with no UTF-8 form, and `0xFF` is the same
    /// everywhere else.
    fn unmappable_name() -> OsString {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt as _;
            OsString::from_wide(&[u16::from(b'z'), 0xD800])
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::ffi::OsStringExt as _;
            OsString::from_vec(vec![b'z', 0xFF])
        }
    }

    /// Completing to a history entry has to fill in the entry's own bytes. The
    /// dropdown can only *show* a lossy rendering, and a completion that put
    /// the rendering in the field would offer one file and enter another —
    /// which is the same defect as the history holding text, reached by a
    /// different route.
    #[test]
    fn accepting_a_history_suggestion_fills_in_its_exact_bytes() {
        let mut dialog = RunDialog::new();
        dialog.show();

        let mut chosen = OsString::from("/usr/bin/");
        chosen.push(unmappable_name());
        dialog.add_to_history(&chosen);

        // "usr" matches the entry and none of the known apps.
        dialog.input.set_text("usr");
        dialog.update_suggestions();
        assert!(
            dialog.suggestions.iter().any(|s| s.exact == chosen),
            "the history entry was not offered at all"
        );
        dialog.suggestion_index = dialog
            .suggestions
            .iter()
            .position(|s| s.exact == chosen)
            .map(Some)
            .unwrap();

        dialog.accept_suggestion();
        assert_eq!(
            dialog.command_exact.as_deref(),
            Some(Path::new(&chosen)),
            "the completion entered the rendering rather than the file"
        );

        // And it survives all the way out as a launch -- as one word, since a
        // chosen file is a name and not a command line.
        dialog.execute_current();
        assert_eq!(
            dialog.drain_events().first(),
            Some(&RunDialogEvent::Execute(RunRequest {
                whole: chosen.clone(),
                words: vec![chosen.clone()],
            }))
        );
    }

    // ====================================================================
    // Event generation tests
    // ====================================================================

    #[test]
    fn test_enter_executes() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("terminal");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Execute(RunRequest {
            whole: OsString::from("terminal"),
            words: vec![OsString::from("terminal")],
        })));
        assert!(events.contains(&RunDialogEvent::Closed));
    }

    #[test]
    fn test_escape_cancels() {
        let mut dialog = RunDialog::new();
        dialog.show();

        let event = make_key(Key::Escape, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Cancel));
        assert!(events.contains(&RunDialogEvent::Closed));
    }

    #[test]
    fn test_empty_enter_does_nothing() {
        let mut dialog = RunDialog::new();
        dialog.show();

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.is_empty());
        assert!(dialog.is_visible()); // Still visible.
    }

    #[test]
    fn test_not_found_error() {
        let mut dialog = RunDialog::new();
        dialog.known_apps.clear();
        dialog.path_dirs.clear();
        dialog.show();
        dialog.input.set_text("nonexistent!@#");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        // Should show error, not execute.
        assert!(dialog.error_message.is_some());
        assert!(dialog.is_visible());
        let events = dialog.drain_events();
        assert!(events.is_empty());

        // And the field says so: its edge and its focus mark are red, the
        // toolkit field's "what is in it is wrong".
        let p = Palette::for_mode(false);
        let reds = dialog
            .render(&p)
            .iter()
            .filter(|cmd| {
                matches!(cmd, RenderCommand::StrokeRect { color, .. }
                    if (color.r, color.g, color.b) == (p.red.r, p.red.g, p.red.b))
            })
            .count();
        assert!(
            reds >= 2,
            "the field does not mark the error: {reds} red strokes"
        );
    }

    #[test]
    fn test_absolute_path_resolves() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("/usr/bin/something");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Execute(RunRequest {
            whole: OsString::from("/usr/bin/something"),
            words: vec![OsString::from("/usr/bin/something")],
        })));
    }

    // ====================================================================
    // A program and its arguments
    // ====================================================================

    fn words(line: &str) -> Vec<String> {
        split_words(line).expect("the line splits")
    }

    #[test]
    fn words_are_split_on_whitespace_of_any_length() {
        assert_eq!(
            words("editor  notes.txt\t--new "),
            ["editor", "notes.txt", "--new"]
        );
        assert!(words("   ").is_empty());
        assert!(words("").is_empty());
    }

    /// Quotes keep a space inside one word, which is how a path with a space
    /// in it is written when it is an argument rather than the whole line.
    #[test]
    fn quotes_keep_a_word_together() {
        assert_eq!(
            words("editor \"/home/u/My Notes/a b.txt\""),
            ["editor", "/home/u/My Notes/a b.txt"]
        );
        assert_eq!(words("echo 'it''s'"), ["echo", "its"]);
        assert_eq!(
            words("a\"b c\"d"),
            ["ab cd"],
            "quotes join what touches them"
        );
        assert_eq!(
            words("run \"\""),
            ["run", ""],
            "an empty quote is an empty word"
        );
    }

    /// Inside double quotes only `\"` and `\\` are escapes; inside single
    /// quotes nothing is; outside quotes a backslash makes the next character
    /// ordinary.
    #[test]
    fn backslashes_follow_the_shell() {
        assert_eq!(words("\"say \\\"hi\\\"\""), ["say \"hi\""]);
        assert_eq!(words("\"a\\\\b\""), ["a\\b"]);
        assert_eq!(words("\"keep \\n\""), ["keep \\n"], "not an escape here");
        assert_eq!(words("'\\n'"), ["\\n"]);
        assert_eq!(words("My\\ Documents"), ["My Documents"]);
        assert_eq!(words("trailing\\"), ["trailing\\"], "kept, not dropped");
    }

    /// An unclosed quote is refused, with a message, and runs nothing.
    #[test]
    fn an_unclosed_quote_is_refused() {
        assert_eq!(split_words("editor \"my file"), Err(UnclosedQuote('"')));
        assert_eq!(split_words("editor 'my file"), Err(UnclosedQuote('\'')));

        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("editor \"my file");
        dialog.execute_current();
        assert!(
            !dialog
                .drain_events()
                .iter()
                .any(|e| matches!(e, RunDialogEvent::Execute(_))),
            "a half-quoted line ran"
        );
        assert!(dialog.is_visible(), "the box stays up to be corrected");
        assert!(
            dialog
                .error_message
                .as_deref()
                .is_some_and(|m| m.contains("double quote")),
            "{:?}",
            dialog.error_message
        );
    }

    /// **A program with arguments reaches the shell as a program and its
    /// arguments** -- it used to arrive as one program named by the whole
    /// line.
    #[test]
    fn a_command_with_arguments_is_a_program_and_its_arguments() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog
            .input
            .set_text("terminal --working-directory \"/home/u/My Stuff\"");
        dialog.execute_current();
        assert_eq!(
            dialog.drain_events().first(),
            Some(&RunDialogEvent::Execute(RunRequest {
                whole: OsString::from("terminal --working-directory \"/home/u/My Stuff\""),
                words: ["terminal", "--working-directory", "/home/u/My Stuff"]
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
            }))
        );
    }

    #[test]
    fn test_show_hide_visibility() {
        let mut dialog = RunDialog::new();
        assert!(!dialog.is_visible());
        dialog.show();
        assert!(dialog.is_visible());
        dialog.hide();
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_render_empty_when_hidden() {
        let dialog = RunDialog::new();
        for light in [false, true] {
            assert!(dialog.render(&Palette::for_mode(light)).is_empty());
        }
    }

    #[test]
    fn test_render_nonempty_when_visible() {
        let mut dialog = RunDialog::new();
        dialog.show();
        for light in [false, true] {
            assert!(!dialog.render(&Palette::for_mode(light)).is_empty());
        }
    }

    /// Every colour this dialog draws comes from the palette it was handed.
    ///
    /// See `security_dialog`'s equivalent for why the light arm is the arm
    /// that does the work: the sixteen constants deleted from this module were
    /// all Catppuccin Mocha, so one left behind is a value the Latte palette
    /// does not contain, and
    /// [`palette_check::assert_drawn_from`](appearance::palette_check::assert_drawn_from)
    /// names it.
    ///
    /// The states below are chosen because each one *selects a colour* no
    /// other state reaches — an error message is the only red, a drawn
    /// selection the only accent fill in the input, a highlighted suggestion
    /// the only `surface0`, and the OK button's `on_accent` label is drawn
    /// whatever the pointer is doing while Cancel's and Browse's hover
    /// treatment is not. Geometry-only variation is deliberately absent; it
    /// would multiply the run time without reaching a single new line.
    #[test]
    fn every_colour_the_dialog_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for error in [false, true] {
                for autocomplete in [false, true] {
                    for selected_suggestion in [false, true] {
                        for text_selection in [false, true] {
                            for hovered in [
                                None,
                                Some(ButtonId::Ok),
                                Some(ButtonId::Cancel),
                                Some(ButtonId::Browse),
                            ] {
                                let mut dialog = RunDialog::new();
                                dialog.show();
                                dialog.input.set_text("fi");
                                if text_selection {
                                    dialog.input.select_all();
                                }
                                if error {
                                    dialog.error_message = Some("not found".into());
                                }
                                if autocomplete {
                                    dialog.suggestions = vec![
                                        Suggestion {
                                            text: "firefox".into(),
                                            exact: "firefox".into(),
                                            score: 10,
                                        },
                                        Suggestion {
                                            text: "files".into(),
                                            exact: "files".into(),
                                            score: 5,
                                        },
                                    ];
                                    dialog.show_autocomplete = true;
                                    dialog.suggestion_index =
                                        if selected_suggestion { Some(0) } else { None };
                                }
                                dialog.hovered_button = hovered;
                                let cmds = dialog.render(&p);
                                assert!(!cmds.is_empty());
                                // The buttons' faces are blended from the
                                // palette by the toolkit's button, against the
                                // box's ground: declared as what they are.
                                let mut derived = Vec::new();
                                for kind in
                                    [guitk::button::Kind::Plain, guitk::button::Kind::Primary]
                                {
                                    for hovered in [false, true] {
                                        let c = guitk::button::paint(
                                            &p,
                                            kind,
                                            guitk::button::State {
                                                hovered,
                                                ..guitk::button::State::default()
                                            },
                                            p.base,
                                        );
                                        derived.extend([c.upper, c.lower, c.edge, c.ink]);
                                    }
                                }
                                palette_check::assert_drawn_from(&p, &cmds, &derived, "run_dialog");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_key_event_ignored_when_hidden() {
        let mut dialog = RunDialog::new();
        let event = make_key(Key::A, false, false, Some('a'));
        let result = dialog.handle_key_event(&event);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn test_key_event_consumed_when_visible() {
        let mut dialog = RunDialog::new();
        dialog.show();
        let event = make_key(Key::A, false, false, Some('a'));
        let result = dialog.handle_key_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(dialog.input.text(), "a");
    }

    #[test]
    fn test_ctrl_a_selects_all() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("hello world");
        let event = make_key(Key::A, true, false, None);
        dialog.handle_key_event(&event);
        assert!(dialog.input.has_selection());
        assert_eq!(dialog.input.selected_text(), "hello world");
    }

    #[test]
    fn test_tab_accepts_autocomplete() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("calc");
        dialog.update_suggestions();
        assert!(dialog.show_autocomplete);

        let event = make_key(Key::Tab, false, false, None);
        dialog.handle_key_event(&event);
        assert_eq!(dialog.input.text(), "calculator");
    }

    // ====================================================================
    // The field's right-click menu
    // ====================================================================

    /// **The field's menu does what its keys do, and a change made from it
    /// is a change as a typed one is**: Paste brings suggestions for what it
    /// put in and takes the last complaint away; Copy changes nothing and
    /// keeps the complaint; a row that is none of the field's is not taken.
    #[test]
    fn the_fields_menu_edits_as_typing_does() {
        use guitk::editmenu::EditCommand;
        let mut dialog = RunDialog::new();
        dialog.show_failed(OsStr::new("nonexistent"), "not found".to_owned());
        assert!(dialog.error_message.is_some());
        guitk::clipboard::set_text("term");
        dialog.input.select_all();

        assert!(dialog.edit_command(EditCommand::Paste.id()));
        assert_eq!(dialog.input.text(), "term");
        assert!(
            dialog.error_message.is_none(),
            "a paste left the complaint about the old line up"
        );
        assert!(
            dialog.suggestions.iter().any(|s| s.text == "terminal"),
            "a paste brought no suggestions for what it put in"
        );

        dialog.error_message = Some("not found".to_owned());
        dialog.input.select_all();
        assert!(dialog.edit_command(EditCommand::Copy.id()));
        assert_eq!(guitk::clipboard::text(), "term");
        assert!(
            dialog.error_message.is_some(),
            "a copy changed nothing, and took the complaint away"
        );

        assert!(!dialog.edit_command(1));
        assert_eq!(dialog.input.text(), "term");
    }

    /// **The field is drawn where `field_rect` says**, which is where a
    /// right-click offers its menu: the line typed is drawn inside it, and
    /// the "Open:" label beside it and the buttons below are not.
    #[test]
    fn the_field_is_drawn_where_its_menu_is_offered() {
        let mut dialog = RunDialog::new();
        dialog.set_position(100.0, 50.0);
        dialog.show();
        dialog.input.set_text("terminal");
        let field = dialog.field_rect();
        let drawn = dialog.render(&Palette::for_mode(false));
        let at = |wanted: &str| {
            drawn
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, x, y, .. } if text == wanted => Some((*x, *y)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted:?} is not drawn"))
        };
        let (tx, ty) = at("terminal");
        assert!(
            field.contains(tx + 1.0, ty + 1.0),
            "the line is drawn outside the field"
        );
        let (lx, ly) = at("Open:");
        assert!(
            !field.contains(lx + 1.0, ly + 1.0),
            "the label is inside the field"
        );
        let (bx, by) = at("OK");
        assert!(
            !field.contains(bx + 1.0, by + 1.0),
            "a button is inside the field"
        );
        // Inside the box, and as tall as the field is drawn.
        let content = dialog.layout().content;
        assert!(field.x > content.x && field.y > content.y);
        assert!(field.x + field.w < content.x + content.w);
        assert!((field.h - INPUT_HEIGHT).abs() < f32::EPSILON);
    }

    /// **The box wears the theme's window frame**: under the built-in frame
    /// it is the size it always was, its title is on the frame's bar, and the
    /// frame's close button lights under the pointer and cancels as Cancel
    /// does.
    #[test]
    fn the_box_wears_the_themes_frame() {
        let mut dialog = RunDialog::new();
        dialog.set_position(100.0, 50.0);
        dialog.show();
        let layout = dialog.layout();
        assert_eq!((layout.outer.w, layout.outer.h), (450.0, 180.0));
        let drawn = dialog.render(&Palette::for_mode(false));
        assert!(
            drawn.iter().any(|c| matches!(c,
                RenderCommand::Text { text, y, .. } if text == TITLE && *y < layout.content.y)),
            "the title is not on the bar"
        );
        let close = layout.close().expect("the box can be closed");
        let at = |kind| MouseEvent {
            x: close.x + 2.0,
            y: close.y + 2.0,
            kind,
        };
        let palette = Palette::for_mode(false);
        let unlit = dialog.render(&palette);
        dialog.handle_mouse_event(&at(MouseEventKind::Move));
        assert!(dialog.close_hovered, "the close button is not lit");
        // And drawn lit: the frame draws it otherwise.
        assert_ne!(
            dialog.render(&palette),
            unlit,
            "the lit close button is drawn as the unlit one"
        );
        assert_eq!(
            dialog.handle_mouse_event(&at(MouseEventKind::Press(MouseButton::Left))),
            EventResult::Consumed
        );
        assert!(!dialog.is_visible());
        // As Cancel does: cancelled, and closed.
        assert_eq!(
            dialog.drain_events(),
            [RunDialogEvent::Cancel, RunDialogEvent::Closed]
        );
    }

    /// **A theme's taller title bar makes a taller box**, centred as a whole,
    /// its content -- the field with it -- under the bar.
    #[test]
    fn a_taller_title_bar_makes_a_taller_box() {
        let mut settings = appearance::AppearanceSettings::default();
        settings.decoration_theme = appearance::themes::DecorationTheme::from_style(
            "tall",
            appearance::decorations::DecorationStyle {
                title_height: 50,
                ..appearance::decorations::DecorationStyle::AERO
            },
        );
        let mut dialog = RunDialog::new();
        dialog.set_frame(DialogFrame::from_settings(&settings, 1.0));
        dialog.centre_on(1000.0, 800.0);
        let layout = dialog.layout();
        assert_eq!((layout.outer.w, layout.outer.h), (450.0, 200.0));
        assert_eq!(dialog.position(), (275.0, 300.0));
        assert!((layout.content.y - (300.0 + 1.0 + 50.0)).abs() < f32::EPSILON);
        assert!((dialog.field_rect().y - (layout.content.y + INPUT_Y)).abs() < f32::EPSILON);
    }
}
