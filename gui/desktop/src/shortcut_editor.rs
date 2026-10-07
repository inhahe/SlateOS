//! The keyboard-shortcut card's editor: what a keystroke does while the card
//! is open.
//!
//! `design.txt` asks four things of the place a user sets shortcuts:
//!
//! > set a hotkey - capture from keyboard (shows you the function that's
//! > already taken by that if any and lets you modify/delete its hotkey),
//! > select function to apply it to, can search for functions, shows you any
//! > hotkeys already applied to that function and lets you modify/delete them,
//! > can also run an arbitrary command for a hotkey
//!
//! The card could list the bindings, record new keys for one, and delete one.
//! It could not change what a chord *does* — a chord kept its action for life —
//! so every other clause above was out of reach, and
//! [`HotkeyAction::LaunchApp`] could be bound only by editing the file by hand.
//! This module is the rest:
//!
//! | key on the list | does |
//! |---|---|
//! | Enter | record new keys for the row's action |
//! | F2 | choose a different action for the row's keys |
//! | Insert | add a shortcut: choose an action, then press its keys |
//! | Delete | remove the row |
//! | Escape | close the card |
//!
//! Choosing an action opens a searchable list of every action, each with the
//! keys it already has, and "Run a program…" at the top for an arbitrary
//! program. Keys that are already taken are no longer refused outright: the
//! card names what holds them and offers to move them — the "shows you the
//! function that's already taken … and lets you modify" clause.
//!
//! # The pointer, too
//!
//! Every key above is a button along the card's foot as well
//! ([`CardButton`]): New keys, Change action, Remove, Add and Close on the
//! list; Cancel while keys are recorded; Move and Keep for the question;
//! Choose and Cancel over the action list, OK and Cancel under the program
//! field. A button's press *is* its key, handed to
//! [`ShortcutEditor::handle_key`], so the two cannot come to differ. A click
//! chooses a row of the list or an entry of the action list, as Up and Down
//! do, and a double click is that and Enter; the wheel scrolls the action
//! list. Until 2026-10-06 the card answered no press at all, so a user who
//! cannot press F2 or Insert -- with a switch, a pointer, a voice --
//! could read the shortcuts and change none (`design-decisions.md` §1494).
//! Where everything is, for the drawing, a press and tools alike, is one
//! answer: [`ShortcutEditor::layout`].
//!
//! # A program, not a command line
//!
//! `design.txt` says "an arbitrary command", and the entry asks for a
//! *program*. [`HotkeyAction::LaunchApp`] starts its string as one program
//! path, with no arguments, and that is deliberate: splitting a line into
//! words needs a quoting rule, and inventing one here would make `my program`
//! two words to this field and one to the filesystem. The Run box made the
//! same call for the same reason (`DesktopShell::key_on_run_dialog`). So the
//! field says "program", and nothing on screen suggests typing arguments into
//! it; when a quoting rule exists, this is where it would be adopted.
//!
//! # Why a state machine of its own
//!
//! The card's keys were handled in `lib.rs`, with the one state they had —
//! "recording for row *n*" — kept as a field beside a dozen others. Six states,
//! each with its own meaning for Enter and Escape, do not fit that shape, and
//! the transitions between them are what wants testing without a compositor.
//! So the editor is a value, [`ShortcutEditor`]; the shell hands it keystrokes
//! and the registry and gets back what happened.
//!
//! # Addressed by chord, not by row
//!
//! A row number is a position in a sorted map, and every edit can re-sort it:
//! recording `Ctrl+Alt+T` for the row that was `Super+T` moves that binding to
//! another row. So every state that points at a binding holds its [`Hotkey`],
//! and after an edit the selection is found again by chord rather than left on
//! whatever slid into the old position.

use appearance::Palette;
use guitk::button;
use guitk::event::{Key, KeyEvent};
use guitk::frame::Rect;
use guitk::listview::{ListKey, ListViewport};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::textedit::{self, SingleLine};
use guitk::textfind::{Case, contains};
use guitk::textinput::{KeyEdit, TextInput};
use guitk::widget::automation::{Node, Role, Value};
use guitk::widget_style::ButtonStyle;

use crate::hotkeys::{self, Hotkey, HotkeyAction, HotkeyRegistry};

/// Where a chosen action goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Onto these keys, replacing what they did: F2 on a row.
    Keys(Hotkey),
    /// Onto keys not chosen yet: Insert.
    New,
}

/// One entry of the action list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// "Run a program…": ask for a program to start.
    Command,
    /// A fixed action.
    Action(HotkeyAction),
}

impl Choice {
    /// What the list shows for it.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Command => "Run a program\u{2026}".to_string(),
            Self::Action(action) => name_of(action),
        }
    }
}

/// What the editor is in the middle of.
#[derive(Clone, Debug, Default)]
pub enum Mode {
    /// Nothing: the card lists the bindings.
    #[default]
    List,
    /// Recording new keys for the binding on `keys` (Enter on a row).
    Rechord {
        /// The binding's current keys.
        keys: Hotkey,
    },
    /// Choosing an action from the searchable list.
    Pick {
        /// Where the choice goes.
        target: Target,
        /// What has been typed to narrow the list.
        query: TextInput,
        /// Which entry of the *narrowed* list is picked, and which are on
        /// screen, kept in step by the toolkit's viewport.
        view: ListViewport,
    },
    /// Typing the program a "Run a program" shortcut starts.
    Command {
        /// Where the program goes.
        target: Target,
        /// The program, as a path or a name to look up.
        input: TextInput,
    },
    /// Recording the keys for a new shortcut whose action is chosen.
    Record {
        /// The action the keys will start.
        action: HotkeyAction,
    },
    /// The keys pressed already do something else: move them?
    Steal {
        /// The keys.
        keys: Hotkey,
        /// What they would do.
        action: HotkeyAction,
        /// What they do now.
        holder: HotkeyAction,
        /// The keys the action is moving *off*, when this came from
        /// re-recording a row rather than from adding a shortcut.
        replacing: Option<Hotkey>,
    },
}

/// What the shell should do after a keystroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not one of the card's keys: let the global table have it. Only ever
    /// answered on the plain list — every other mode owns the keyboard.
    NotMine,
    /// Handled; nothing to save.
    Handled,
    /// Handled, and the registry changed: save it.
    Changed,
    /// Close the card.
    Close,
}

/// What the editor needs to know about the desktop around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    /// How many virtual desktops exist — one "Switch to Desktop N" each.
    pub desktops: u8,
    /// How many rows of the action list fit on screen; from [`picker_rows`],
    /// given the same height the card is drawn in and the same foot
    /// ([`picker_foot_height`]).
    pub picker_rows: usize,
    /// How many rows each column of the card's own list holds: its page --
    /// from `hotkeys::rows_per_column`, given the same height and foot
    /// ([`list_foot_height`]).
    pub list_rows: usize,
}

/// The card's editor state.
#[derive(Clone, Debug, Default)]
pub struct ShortcutEditor {
    mode: Mode,
    /// The row of the list the keyboard is on.
    selected: usize,
    /// What the last action did, for the card to show. Composed here; the
    /// shell appends a save failure to it, since only the shell can save.
    message: Option<String>,
}

impl ShortcutEditor {
    /// An editor on the first row, doing nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the editor is in the middle of.
    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Whether keystrokes are currently *data* — keys being recorded — rather
    /// than commands. While they are, nothing else may act on them: a user
    /// recording keys for "close window" must not close a window by pressing
    /// them.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        matches!(self.mode, Mode::Rechord { .. } | Mode::Record { .. })
    }

    /// Whether the editor owns the keyboard: every mode but the plain list.
    #[must_use]
    pub fn owns_keyboard(&self) -> bool {
        !matches!(self.mode, Mode::List)
    }

    /// Whether the card should draw the action list rather than the bindings.
    #[must_use]
    pub fn is_picking(&self) -> bool {
        matches!(self.mode, Mode::Pick { .. } | Mode::Command { .. })
    }

    /// The list row the keyboard is on.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Put the keyboard on `row`.
    pub fn set_selected(&mut self, row: usize) {
        self.selected = row;
    }

    /// What the last action did.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Replace the message — how the shell appends a save failure.
    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
    }

    /// Back to the list with nothing in progress, as closing the card does.
    pub fn reset(&mut self) {
        self.mode = Mode::List;
        self.message = None;
    }

    /// The row whose keys are `keys` chosen by a press, as Up and Down choose
    /// one: the keyboard goes to it, and what the last edit said goes, as it
    /// does for a step. Only on the plain list -- while keys are recorded or
    /// a question is asked, a row is not what the card is waiting for.
    /// Whether it was.
    pub fn choose_row(&mut self, registry: &HotkeyRegistry, keys: Hotkey) -> bool {
        if !matches!(self.mode, Mode::List) {
            return false;
        }
        let Some(row) = registry
            .all_bindings()
            .position(|(h, _)| same_binding(h, &keys))
        else {
            return false;
        };
        self.selected = row;
        self.message = None;
        true
    }

    /// The entry at `index` among those the search leaves chosen by a press,
    /// as Up and Down choose one. Only while an action is chosen. Whether it
    /// was.
    pub fn choose_entry(&mut self, index: usize, cx: Context) -> bool {
        let Mode::Pick { query, view, .. } = &mut self.mode else {
            return false;
        };
        let len = filtered(&choices(cx.desktops), query.text()).len();
        if index >= len {
            return false;
        }
        view.set_height(cx.picker_rows, len);
        view.select(Some(index), len);
        true
    }

    /// The action list scrolled `rows` rows, down for a positive count, as
    /// the wheel scrolls it: what is chosen stays chosen. Whether it moved.
    pub fn scroll_entries(&mut self, rows: isize, cx: Context) -> bool {
        let Mode::Pick { query, view, .. } = &mut self.mode else {
            return false;
        };
        let len = filtered(&choices(cx.desktops), query.text()).len();
        view.set_height(cx.picker_rows, len);
        let before = view.visible_range(len);
        view.scroll_by(rows, len);
        view.visible_range(len) != before
    }

    /// Where entry `id` -- its place among every entry ([`choices`]) -- is
    /// among those the search leaves, and which of those is chosen: `None`
    /// with no action list up, or with the search leaving it out.
    #[must_use]
    pub fn entry_place(&self, id: usize, cx: Context) -> Option<(usize, Option<usize>)> {
        let Mode::Pick { query, view, .. } = &self.mode else {
            return None;
        };
        let all = choices(cx.desktops);
        let wanted = all.get(id)?;
        let index = filtered(&all, query.text())
            .iter()
            .position(|choice| choice == wanted)?;
        Some((index, view.selected()))
    }

    /// The buttons along the card's foot as the editor stands, each with
    /// whether it can be pressed now: the list's five, a row's three greyed
    /// with no row to act on; Cancel while keys are recorded; the question's
    /// two answers; Choose -- greyed while the search leaves nothing to
    /// choose -- and Cancel over the action list; OK and Cancel under the
    /// program field. Each does what its key does, here, now.
    #[must_use]
    pub fn buttons(&self, registry: &HotkeyRegistry, cx: Context) -> Vec<(CardButton, bool)> {
        match &self.mode {
            Mode::List => {
                let rows = !registry.is_empty();
                vec![
                    (CardButton::NewKeys, rows),
                    (CardButton::ChangeAction, rows),
                    (CardButton::Remove, rows),
                    (CardButton::Add, true),
                    (CardButton::Close, true),
                ]
            }
            Mode::Rechord { .. } | Mode::Record { .. } => vec![(CardButton::Cancel, true)],
            Mode::Steal { .. } => vec![(CardButton::Move, true), (CardButton::Keep, true)],
            Mode::Pick { query, .. } => vec![
                (
                    CardButton::Choose,
                    !filtered(&choices(cx.desktops), query.text()).is_empty(),
                ),
                (CardButton::Cancel, true),
            ],
            Mode::Command { .. } => vec![(CardButton::Ok, true), (CardButton::Cancel, true)],
        }
    }

    /// What the foot's line says now: what the last edit did; on the plain
    /// list with nothing to report, the keys the card answers; nothing else.
    #[must_use]
    pub fn foot_line(&self) -> Option<&str> {
        match self.message.as_deref() {
            Some(message) => Some(message),
            None if !self.owns_keyboard() => Some(HINT),
            None => None,
        }
    }

    /// Act on one keystroke.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        registry: &mut HotkeyRegistry,
        cx: Context,
    ) -> Outcome {
        if !key.pressed {
            // Releases are no command, but mid-recording they belong to the
            // recording: one must not reach the global table half-way through
            // a chord.
            return if self.owns_keyboard() {
                Outcome::Handled
            } else {
                Outcome::NotMine
            };
        }
        match std::mem::take(&mut self.mode) {
            Mode::List => self.list_key(key, registry, cx),
            Mode::Rechord { keys } => self.rechord_key(key, keys, registry),
            Mode::Pick {
                target,
                query,
                view,
            } => self.pick_key(key, target, query, view, registry, cx),
            Mode::Command { target, input } => self.command_key(key, target, input, registry),
            Mode::Record { action } => self.record_key(key, action, registry),
            Mode::Steal {
                keys,
                action,
                holder,
                replacing,
            } => self.steal_key(key, keys, action, holder, replacing, registry),
        }
    }

    // ------------------------------------------------------------------
    // The list
    // ------------------------------------------------------------------

    fn list_key(&mut self, key: &KeyEvent, registry: &mut HotkeyRegistry, cx: Context) -> Outcome {
        let last = registry.len().saturating_sub(1);
        self.selected = self.selected.min(last);
        let row_keys = registry.all_bindings().nth(self.selected).map(|(h, _)| *h);
        match key.key {
            Key::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.message = None;
                Outcome::Handled
            }
            Key::Down => {
                // Clamped rather than wrapping, like every list in the shell.
                self.selected = self.selected.saturating_add(1).min(last);
                self.message = None;
                Outcome::Handled
            }
            // A column at a time, or the ends (`design-decisions.md` §1416).
            // A chord these do not read -- Ctrl+Page Down -- is not the
            // card's, so a shortcut on it still works with the card up.
            Key::PageUp | Key::PageDown | Key::Home | Key::End => match ListKey::of(key) {
                Some(nav) => {
                    if let Some(row) = nav.target(Some(self.selected), registry.len(), cx.list_rows)
                    {
                        self.selected = row;
                    }
                    self.message = None;
                    Outcome::Handled
                }
                None => Outcome::NotMine,
            },
            Key::Enter => {
                if let Some(keys) = row_keys {
                    self.mode = Mode::Rechord { keys };
                    self.message = Some("Press the new keys, or Escape to cancel".to_string());
                }
                Outcome::Handled
            }
            Key::F2 => {
                if let Some(keys) = row_keys {
                    self.open_picker(Target::Keys(keys), cx);
                }
                Outcome::Handled
            }
            Key::Insert => {
                self.open_picker(Target::New, cx);
                Outcome::Handled
            }
            Key::Delete => match row_keys {
                Some(keys) => self.delete(keys, registry),
                None => Outcome::Handled,
            },
            Key::Escape => {
                self.message = None;
                Outcome::Close
            }
            _ => Outcome::NotMine,
        }
    }

    fn open_picker(&mut self, target: Target, cx: Context) {
        self.message = Some(match &target {
            Target::Keys(keys) => format!("Choose what {} does", keys.display_name()),
            Target::New => "Choose what the new shortcut does".to_string(),
        });
        // The first entry picked from the start, so Enter straight away is an
        // answer rather than a no-op.
        let mut view = ListViewport::new(cx.picker_rows);
        view.select(Some(0), choices(cx.desktops).len());
        self.mode = Mode::Pick {
            target,
            query: TextInput::new(),
            view,
        };
    }

    /// Unbind `keys`.
    ///
    /// The shell's save is what makes it stay unbound: it writes a `none=`
    /// line for every default the registry no longer holds, so the next
    /// login's merge onto the defaults does not quietly restore it.
    fn delete(&mut self, keys: Hotkey, registry: &mut HotkeyRegistry) -> Outcome {
        let Some(action) = registry.conflicts_with(&keys).cloned() else {
            self.message = Some("That row is gone".to_string());
            return Outcome::Handled;
        };
        registry.unregister(&keys);
        self.message = Some(match chords_of(registry, &action).as_slice() {
            [] => format!("{} is no longer on any keys", name_of(&action)),
            rest => format!(
                "{} is off {}; it still has {}",
                name_of(&action),
                keys.display_name(),
                rest.join(", ")
            ),
        });
        Outcome::Changed
    }

    // ------------------------------------------------------------------
    // Recording keys
    // ------------------------------------------------------------------

    fn rechord_key(
        &mut self,
        key: &KeyEvent,
        old: Hotkey,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        if is_modifier(key.key) {
            self.mode = Mode::Rechord { keys: old };
            return Outcome::Handled;
        }
        if key.key == Key::Escape {
            self.message = Some("Unchanged".to_string());
            return Outcome::Handled;
        }
        let Some(action) = registry.conflicts_with(&old).cloned() else {
            self.message = Some("That row is gone".to_string());
            return Outcome::Handled;
        };
        // Normalised as the registry will store it, so the message names
        // the binding that exists rather than the keys as pressed.
        self.bind(
            Hotkey::normalized(key.key, key.modifiers),
            action,
            Some(old),
            registry,
        )
    }

    fn record_key(
        &mut self,
        key: &KeyEvent,
        action: HotkeyAction,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        if is_modifier(key.key) {
            self.mode = Mode::Record { action };
            return Outcome::Handled;
        }
        if key.key == Key::Escape {
            self.message = Some("Unchanged".to_string());
            return Outcome::Handled;
        }
        self.bind(
            Hotkey::normalized(key.key, key.modifiers),
            action,
            None,
            registry,
        )
    }

    /// Put `action` on `keys`, moving it off `replacing` if given — or, when
    /// the keys already do something else, ask first.
    fn bind(
        &mut self,
        keys: Hotkey,
        action: HotkeyAction,
        replacing: Option<Hotkey>,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        if replacing.is_some_and(|old| same_binding(&old, &keys)) {
            self.message = Some("Unchanged".to_string());
            return Outcome::Handled;
        }
        match registry.conflicts_with(&keys).cloned() {
            Some(holder) if holder == action => {
                // Already true, under another row. Moving this row onto it
                // would be merging two rows the user did not ask to merge.
                self.message = Some(format!(
                    "{} is already {}",
                    keys.display_name(),
                    name_of(&action)
                ));
                Outcome::Handled
            }
            Some(holder) => {
                // Named and offered, not merely refused.
                self.message = Some(format!(
                    "{} is already {}. Enter moves it to {}; Escape keeps it",
                    keys.display_name(),
                    name_of(&holder),
                    name_of(&action)
                ));
                self.mode = Mode::Steal {
                    keys,
                    action,
                    holder,
                    replacing,
                };
                Outcome::Handled
            }
            None => self.commit(keys, action, replacing, None, registry),
        }
    }

    fn steal_key(
        &mut self,
        key: &KeyEvent,
        keys: Hotkey,
        action: HotkeyAction,
        holder: HotkeyAction,
        replacing: Option<Hotkey>,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        match key.key {
            Key::Enter => {
                registry.unregister(&keys);
                self.commit(keys, action, replacing, Some(holder), registry)
            }
            Key::Escape => {
                self.message = Some("Unchanged".to_string());
                Outcome::Handled
            }
            // Anything else is not an answer to the question on screen.
            _ => {
                self.mode = Mode::Steal {
                    keys,
                    action,
                    holder,
                    replacing,
                };
                Outcome::Handled
            }
        }
    }

    /// Make the change: `keys` start `action`, `replacing` is freed, and the
    /// list's selection follows the binding to wherever it now sorts.
    ///
    /// `displaced` is what the keys did before, if anything, so the message
    /// can say when that action has been left with no keys at all.
    fn commit(
        &mut self,
        keys: Hotkey,
        action: HotkeyAction,
        replacing: Option<Hotkey>,
        displaced: Option<HotkeyAction>,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        if let Some(old) = replacing {
            registry.unregister(&old);
        }
        if let Err(e) = registry.register(keys, action.clone()) {
            // Not reachable after the checks above -- the keys were free or
            // were just freed -- but a failure must not lose the old binding.
            if let Some(old) = replacing {
                // Re-registering keys this action just held cannot conflict.
                drop(registry.register(old, action));
            }
            self.message = Some(format!("Could not set {}: {e}", keys.display_name()));
            return Outcome::Handled;
        }
        if let Some(row) = registry
            .all_bindings()
            .position(|(h, _)| same_binding(h, &keys))
        {
            self.selected = row;
        }
        let mut message = format!("{} is now {}", keys.display_name(), name_of(&action));
        if let Some(old) = displaced.filter(|old| *old != action) {
            if chords_of(registry, &old).is_empty() {
                message.push_str("; ");
                message.push_str(&name_of(&old));
                message.push_str(" has no keys now");
            }
        }
        self.message = Some(message);
        Outcome::Changed
    }

    // ------------------------------------------------------------------
    // Choosing an action
    // ------------------------------------------------------------------

    fn pick_key(
        &mut self,
        key: &KeyEvent,
        target: Target,
        mut query: TextInput,
        mut view: ListViewport,
        registry: &mut HotkeyRegistry,
        cx: Context,
    ) -> Outcome {
        let shown = filtered(&choices(cx.desktops), query.text());
        let len = shown.len();
        view.set_height(cx.picker_rows, len);
        match key.key {
            Key::Escape => {
                self.message = Some("Unchanged".to_string());
                return Outcome::Handled;
            }
            Key::Up => view.select_prev(len),
            Key::Down => view.select_next(len),
            Key::PageUp => view.page_up(len),
            Key::PageDown => view.page_down(len),
            // Home and End are the search field's while it has text to move
            // through, and the list's when it is empty or Ctrl is held -- the
            // rule for every field over a list in the shell (§1416).
            Key::Home | Key::End if key.modifiers.ctrl || query.text().is_empty() => {
                if let Some(nav) = ListKey::of(key) {
                    view.go(nav, len);
                }
            }
            Key::Enter => {
                let picked = view.selected().and_then(|i| shown.get(i)).cloned();
                return match picked {
                    Some(Choice::Command) => {
                        let mut input = TextInput::new();
                        // Changing what a chord runs starts from what it runs
                        // now, so fixing a typo is an edit rather than a retype.
                        if let Target::Keys(keys) = &target {
                            if let Some(HotkeyAction::LaunchApp(cmd)) =
                                registry.conflicts_with(keys)
                            {
                                input.set_text(cmd);
                            }
                        }
                        self.message = Some("Type the program to start, then Enter".to_string());
                        self.mode = Mode::Command { target, input };
                        Outcome::Handled
                    }
                    Some(Choice::Action(action)) => self.choose(target, action, registry),
                    None => {
                        // Nothing matches what was typed: stay, so it can be
                        // changed.
                        self.mode = Mode::Pick {
                            target,
                            query,
                            view,
                        };
                        Outcome::Handled
                    }
                };
            }
            _ => {
                if query.edit_key(key, hotkeys::LABEL_FONT_SIZE, FontWeightHint::Regular)
                    == KeyEdit::Changed
                {
                    // A new query is a new list; a position in the old one
                    // names nothing.
                    let len = filtered(&choices(cx.desktops), query.text()).len();
                    view.reset();
                    view.select(Some(0), len);
                }
            }
        }
        self.mode = Mode::Pick {
            target,
            query,
            view,
        };
        Outcome::Handled
    }

    fn command_key(
        &mut self,
        key: &KeyEvent,
        target: Target,
        mut input: TextInput,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        match key.key {
            Key::Escape => {
                self.message = Some("Unchanged".to_string());
                Outcome::Handled
            }
            Key::Enter => {
                let command = input.text().trim().to_string();
                if command.is_empty() {
                    self.message = Some("Type a program first, or Escape".to_string());
                    self.mode = Mode::Command { target, input };
                    return Outcome::Handled;
                }
                self.choose(target, HotkeyAction::LaunchApp(command), registry)
            }
            _ => {
                // What the field does not want is still not the global table's:
                // the command box owns the keyboard.
                input.edit_key(key, hotkeys::LABEL_FONT_SIZE, FontWeightHint::Regular);
                self.mode = Mode::Command { target, input };
                Outcome::Handled
            }
        }
    }

    /// An action was chosen for `target`.
    fn choose(
        &mut self,
        target: Target,
        action: HotkeyAction,
        registry: &mut HotkeyRegistry,
    ) -> Outcome {
        match target {
            Target::Keys(keys) => {
                let before = registry.conflicts_with(&keys).cloned();
                if before.as_ref() == Some(&action) {
                    self.message = Some("Unchanged".to_string());
                    return Outcome::Handled;
                }
                // The keys stay; what they do changes. Freed first so the
                // register inside `commit` cannot collide with the binding it
                // replaces.
                registry.unregister(&keys);
                let outcome = self.commit(keys, action, None, before.clone(), registry);
                if outcome != Outcome::Changed {
                    // Put back what was there rather than leave the keys dead.
                    if let Some(old) = before {
                        drop(registry.register(keys, old));
                    }
                }
                outcome
            }
            Target::New => {
                self.message = Some(format!(
                    "Press the keys for {}, or Escape to cancel",
                    name_of(&action)
                ));
                self.mode = Mode::Record { action };
                Outcome::Handled
            }
        }
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    /// Where everything on the card is as the editor stands -- what its
    /// drawing, a press on it and tools all go by, so none of the three can
    /// put a part somewhere the others do not.
    ///
    /// The card is centred on a `screen` of that size and is at most
    /// `max_height` tall, its buttons in `style`: the list's card while the
    /// bindings are shown, the action list's while an action is chosen or a
    /// program typed. `cx` must have been worked out for the same height and
    /// style ([`Context::picker_rows`] and [`Context::list_rows`]).
    #[must_use]
    pub fn layout(
        &self,
        registry: &HotkeyRegistry,
        cx: Context,
        screen: (f32, f32),
        max_height: f32,
        style: &ButtonStyle,
    ) -> CardLayout {
        let buttons = self.buttons(registry, cx);
        if self.is_picking() {
            let foot = picker_foot_height(style);
            let rows = picker_rows(max_height, foot);
            let card = centred(screen, PICKER_WIDTH, picker_height(rows, foot));
            let field = Rect::new(
                card.x + hotkeys::PADDING,
                card.y + hotkeys::HEADER_HEIGHT,
                card.w - hotkeys::PADDING * 2.0,
                FIELD_HEIGHT,
            );
            let mut entries = Vec::new();
            let mut list = None;
            if let Mode::Pick { query, view, .. } = &self.mode {
                let len = filtered(&choices(cx.desktops), query.text()).len();
                #[expect(clippy::cast_precision_loss, reason = "at most MAX_PICKER_ROWS rows")]
                let tall = rows as f32 * hotkeys::ROW_HEIGHT;
                let area = Rect::new(
                    card.x + hotkeys::PADDING / 2.0,
                    field.y + FIELD_HEIGHT + hotkeys::PADDING / 2.0,
                    card.w - hotkeys::PADDING,
                    tall,
                );
                // A copy, sized to the card being laid out: the stored one
                // was sized at the last keystroke, and the display may have
                // changed since.
                let mut view = *view;
                view.set_height(rows, len);
                let top = view.visible_range(len).start;
                entries = view
                    .visible_range(len)
                    .map(|index| (index, entry_rect(area, index, top)))
                    .collect();
                list = Some(area);
            }
            let (message, buttons) = foot_places(
                card,
                foot,
                &buttons,
                style,
                PICKER_WIDTH - hotkeys::PADDING * 2.0,
            );
            CardLayout {
                card,
                rows: Vec::new(),
                list,
                field: Some(field),
                entries,
                message,
                buttons,
                foot,
            }
        } else {
            let foot = list_foot_height(style);
            let (width, height) = hotkeys::settings_panel_size(registry, max_height, foot);
            let card = centred(screen, width, height);
            let rows = hotkeys::row_rects(registry, (card.x, card.y), max_height, foot);
            let list = Rect::new(
                card.x,
                card.y + hotkeys::HEADER_HEIGHT,
                card.w,
                (card.h - hotkeys::HEADER_HEIGHT - foot).max(0.0),
            );
            let (message, buttons) = foot_places(
                card,
                foot,
                &buttons,
                style,
                hotkeys::PANEL_WIDTH - hotkeys::PADDING * 2.0,
            );
            CardLayout {
                card,
                rows,
                list: Some(list),
                field: None,
                entries: Vec::new(),
                message,
                buttons,
                foot,
            }
        }
    }

    /// Draw the card's foot, where `layout` has it: the line saying what the
    /// last edit did -- or, on the plain list with nothing to report, the keys
    /// the card answers -- `font_size` high, and the buttons, `hovered` lit
    /// where it can be pressed.
    pub fn render_foot(
        &self,
        tree: &mut RenderTree,
        p: &Palette,
        layout: &CardLayout,
        font_size: f32,
        hovered: Option<CardButton>,
        focus_ring: f32,
    ) {
        if let Some(line) = self.foot_line() {
            let tall = guitk::text::line_height(font_size, FontWeightHint::Regular);
            tree.push(RenderCommand::Text {
                x: layout.message.x,
                y: layout.message.y + (layout.message.h - tall) / 2.0,
                text: line.to_string(),
                // `subtext0` and not the accent: an outcome, not an
                // invitation, and the accent is what the card already uses for
                // where the keyboard is.
                color: p.subtext0,
                font_size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(layout.message.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
        for &(which, enabled, at) in &layout.buttons {
            button::draw(
                tree,
                p,
                (at.x, at.y, at.w, at.h),
                which.label(),
                which.kind(),
                button::State {
                    hovered: enabled && hovered == Some(which),
                    disabled: !enabled,
                    ..button::State::default()
                },
                p.panel_bg(),
                focus_ring,
            );
        }
    }

    /// The card as tools see it, where `layout` has it: a dialog named by its
    /// heading. On the list's card, the bindings -- each row named by what it
    /// does, described by its keys (and the program it starts, where it
    /// starts one), chosen where the keyboard is, usable on the plain list
    /// alone. On the action list's, its field holding what is typed, and the
    /// entries the search leaves -- each described by the keys it already
    /// has, one scrolled out of the list told above or below it, where the
    /// user would find it scrolling. Then the foot's line: a label saying
    /// what it says, and, while keys are recorded, the field that takes them.
    /// Then the buttons, each described by its key -- or, greyed, by why.
    #[must_use]
    pub fn automation(
        &self,
        registry: &HotkeyRegistry,
        cx: Context,
        layout: &CardLayout,
    ) -> Node<CardPart> {
        let heading = match &self.mode {
            Mode::Pick { .. } => "Choose an action",
            Mode::Command { .. } => "Run a program",
            _ => "Keyboard Shortcuts",
        };
        let mut card = Node::new(CardPart::Card, Role::Dialog, heading, layout.card);
        match &self.mode {
            Mode::Pick { query, view, .. } => {
                card.children.push(field_node(
                    "Search",
                    query.text(),
                    layout.field.unwrap_or_default(),
                ));
                let area = layout.list.unwrap_or_default();
                let all = choices(cx.desktops);
                let shown = filtered(&all, query.text());
                let mut view = *view;
                view.set_height(cx.picker_rows, shown.len());
                let top = view.visible_range(shown.len()).start;
                let mut list = Node::new(CardPart::Entries, Role::List, "Actions", area);
                for (index, choice) in shown.iter().enumerate() {
                    let Some(id) = all.iter().position(|c| c == choice) else {
                        continue;
                    };
                    let mut entry = Node::new(
                        CardPart::Entry(id),
                        Role::ListItem,
                        choice.label(),
                        entry_rect(area, index, top),
                    );
                    if let Choice::Action(action) = choice {
                        let keys = chords_of(registry, action);
                        if !keys.is_empty() {
                            entry.description = Some(keys.join(", "));
                        }
                    }
                    entry.value = Some(Value::Chosen(view.selected() == Some(index)));
                    list.children.push(entry);
                }
                card.children.push(list);
            }
            Mode::Command { input, .. } => {
                card.children.push(field_node(
                    "Program",
                    input.text(),
                    layout.field.unwrap_or_default(),
                ));
            }
            _ => {
                let on_list = matches!(self.mode, Mode::List);
                let selected = self.selected.min(registry.len().saturating_sub(1));
                let mut list = Node::new(
                    CardPart::Bindings,
                    Role::List,
                    "Shortcuts",
                    layout.list.unwrap_or_default(),
                );
                for (row, ((keys, action), (_, at))) in
                    registry.all_bindings().zip(&layout.rows).enumerate()
                {
                    let mut node = Node::new(
                        CardPart::Binding(*keys),
                        Role::ListItem,
                        action.display_label(),
                        *at,
                    );
                    node.description = Some(match action.launch() {
                        Some(launch) => {
                            format!("{}, {}", keys.display_name(), launch.display_line())
                        }
                        None => keys.display_name(),
                    });
                    node.value = Some(Value::Chosen(row == selected));
                    node.focused = on_list && row == selected;
                    node.focusable = on_list;
                    node.enabled = on_list;
                    list.children.push(node);
                }
                card.children.push(list);
            }
        }
        if self.is_recording() {
            // What the card waits for while it says "Press the new keys": a
            // field the keys go into, as a key capture is to every tool.
            let mut keys = Node::new(
                CardPart::Message,
                Role::TextField,
                self.foot_line().unwrap_or("Press the new keys"),
                layout.message,
            );
            keys.value = Some(Value::Text(String::new()));
            keys.focused = true;
            keys.focusable = true;
            card.children.push(keys);
        } else if let Some(line) = self.foot_line() {
            card.children.push(Node::new(
                CardPart::Message,
                Role::Label,
                line,
                layout.message,
            ));
        }
        for &(which, enabled, at) in &layout.buttons {
            let mut node = Node::new(CardPart::Button(which), Role::Button, which.label(), at);
            node.enabled = enabled;
            node.description = Some(if enabled {
                hotkeys::Hotkey::new(which.key(), guitk::event::Modifiers::NONE).display_name()
            } else {
                which.why_disabled().to_owned()
            });
            card.children.push(node);
        }
        card
    }

    /// Draw the action list, or the program field, as a card where `layout`
    /// has it.
    pub fn render_picker(
        &self,
        tree: &mut RenderTree,
        registry: &HotkeyRegistry,
        p: &Palette,
        layout: &CardLayout,
        cx: Context,
        caret_width: f32,
        focus_ring: f32,
    ) {
        let (x, y, width, height) = (layout.card.x, layout.card.y, layout.card.w, layout.card.h);
        hotkeys::push_card(&mut tree.commands, p, x, y, width, height);
        tree.clip(x, y, width, height);

        let (heading, field, placeholder) = match &self.mode {
            Mode::Pick { query, .. } => ("Choose an action", Some(query), "Type to search"),
            Mode::Command { input, .. } => (
                "Run a program",
                Some(input),
                "A program, such as /usr/bin/terminal",
            ),
            _ => ("Choose an action", None, ""),
        };
        tree.push(RenderCommand::Text {
            x: x + hotkeys::PADDING,
            y: y + hotkeys::PADDING,
            text: heading.to_string(),
            color: p.text,
            font_size: hotkeys::HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - hotkeys::PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The field: the toolkit's (`guitk::field`), in the theme's shape,
        // with the keyboard -- what is typed goes into it.
        let field_rect = layout.field.unwrap_or_default();
        let (field_x, field_y, field_w) = (field_rect.x, field_rect.y, field_rect.w);
        guitk::field::draw(
            tree,
            p,
            guitk::frame::Rect::new(field_x, field_y, field_w, FIELD_HEIGHT),
            guitk::field::State {
                focused: true,
                ..guitk::field::State::default()
            },
            focus_ring,
        );
        let line_h = hotkeys::LABEL_FONT_SIZE + 4.0;
        let text_y = field_y + (FIELD_HEIGHT - line_h) / 2.0;
        if let Some(input) = field {
            if input.text().is_empty() {
                tree.push(RenderCommand::Text {
                    x: field_x + FIELD_INSET,
                    y: text_y,
                    text: placeholder.to_string(),
                    // Secondary text, not `overlay0`: a placeholder is read
                    // -- it is the field's instruction -- and `overlay0` is
                    // the disabled grey, below the contrast floor on
                    // purpose. `check-overlay0-ink.py` refuses exactly this
                    // shape ("emptiness is not a switched-off state").
                    color: p.subtext0,
                    font_size: hotkeys::LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(field_w - FIELD_INSET * 2.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            textedit::draw(
                tree,
                &SingleLine {
                    text: input.text(),
                    cursor: input.cursor(),
                    selection_anchor: input.selection_anchor(),
                    focused: true,
                    x: field_x + FIELD_INSET,
                    y: text_y,
                    width: field_w - FIELD_INSET * 2.0,
                    line_height: line_h,
                    font_size: hotkeys::LABEL_FONT_SIZE,
                    weight: FontWeightHint::Regular,
                    color: p.text,
                    selection_bg: p.accent,
                    selection_fg: p.on_accent(),
                    caret_width,
                },
            );
        }

        // The list, for picking; nothing below the field for a program. Its
        // rows where the layout has them, which sized its copy of the view to
        // the card being drawn.
        if let Mode::Pick { query, view, .. } = &self.mode {
            let shown = filtered(&choices(cx.desktops), query.text());
            let content_w = width - hotkeys::PADDING * 2.0;
            let mut row_y = layout.list.map_or(field_y + FIELD_HEIGHT, |list| list.y);
            for &(index, at) in &layout.entries {
                let Some(choice) = shown.get(index) else {
                    break;
                };
                row_y = at.y;
                let picked = view.selected() == Some(index);
                if picked {
                    tree.push(RenderCommand::FillRect {
                        x: at.x,
                        y: row_y + 2.0,
                        width: at.w,
                        height: hotkeys::ROW_HEIGHT - 4.0,
                        color: p.surface0,
                        corner_radii: CornerRadii::all(6.0),
                    });
                }
                tree.push(RenderCommand::Text {
                    x: x + hotkeys::PADDING,
                    y: row_y + (hotkeys::ROW_HEIGHT - hotkeys::LABEL_FONT_SIZE) / 2.0,
                    text: choice.label(),
                    color: if picked { p.text } else { p.subtext1 },
                    font_size: hotkeys::LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w * 0.55),
                    overflow: TextOverflow::Ellipsis,
                });
                // The keys it already has — "shows you any hotkeys already
                // applied to that function". Right-aligned, like the key
                // column on the list.
                let keys = match choice {
                    Choice::Action(action) => chords_of(registry, action).join(", "),
                    Choice::Command => String::new(),
                };
                if !keys.is_empty() {
                    let max = content_w * 0.4;
                    let w = guitk::text::measure(
                        &keys,
                        hotkeys::KEY_FONT_SIZE,
                        FontWeightHint::Regular,
                    )
                    .min(max);
                    tree.push(RenderCommand::Text {
                        x: x + width - hotkeys::PADDING - w,
                        y: row_y + (hotkeys::ROW_HEIGHT - hotkeys::KEY_FONT_SIZE) / 2.0,
                        text: keys,
                        color: p.subtext0,
                        font_size: hotkeys::KEY_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            if shown.is_empty() {
                tree.push(RenderCommand::Text {
                    x: x + hotkeys::PADDING,
                    y: row_y + (hotkeys::ROW_HEIGHT - hotkeys::LABEL_FONT_SIZE) / 2.0,
                    text: "No action matches".to_string(),
                    color: p.subtext0,
                    font_size: hotkeys::LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
        tree.unclip();
    }
}

/// The action-list card's width: one column of the bindings card, plus room
/// for the keys each action already has.
const PICKER_WIDTH: f32 = hotkeys::PANEL_WIDTH;
/// Height of the search or program field.
const FIELD_HEIGHT: f32 = 32.0;
/// Space between the field's edge and its text.
const FIELD_INSET: f32 = 8.0;
/// Rows the action list never shrinks below, so a short display still shows
/// several candidates rather than one.
const MIN_PICKER_ROWS: usize = 3;
/// Rows it never grows past: a list taller than this is scanned, not read.
const MAX_PICKER_ROWS: usize = 12;

/// The card height for `rows` visible rows over a foot `foot` tall.
fn picker_height(rows: usize, foot: f32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "at most MAX_PICKER_ROWS rows")]
    let rows = rows as f32;
    hotkeys::HEADER_HEIGHT
        + FIELD_HEIGHT
        + hotkeys::PADDING / 2.0
        + rows * hotkeys::ROW_HEIGHT
        + hotkeys::PADDING / 2.0
        // The line saying what the last edit did, and the buttons.
        + foot
}

/// How many rows of the action list fit in `max_height` over a foot `foot`
/// tall ([`picker_foot_height`]).
#[must_use]
pub fn picker_rows(max_height: f32, foot: f32) -> usize {
    (MIN_PICKER_ROWS..=MAX_PICKER_ROWS)
        .rev()
        .find(|&rows| picker_height(rows, foot) <= max_height)
        .unwrap_or(MIN_PICKER_ROWS)
}

/// A bare modifier is the user still assembling a chord: taking Super alone
/// as the answer would bind the shortcut the instant they reached for it.
fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::LeftCtrl
            | Key::RightCtrl
            | Key::LeftAlt
            | Key::RightAlt
            | Key::LeftShift
            | Key::RightShift
            | Key::LeftSuper
            | Key::RightSuper
    )
}

/// Whether two chords are the same binding under the registry's rules.
///
/// Through [`Hotkey::matches`], which applies the registry's normalisation to
/// both sides: a dedicated key is the same binding whatever modifiers are held,
/// so a field-by-field comparison would call `Super+VolumeUp` a change from
/// `VolumeUp` and record it as one.
fn same_binding(a: &Hotkey, b: &Hotkey) -> bool {
    a.matches(b.key, &b.modifiers())
}

/// The name a message or a list row uses for an action.
///
/// The program, for a program: "Launch App" is the same label on every
/// command shortcut, and a message saying `Ctrl+Alt+T is now Launch App` would
/// not say which. A desktop by its number, counted from one as a user counts.
#[must_use]
pub fn name_of(action: &HotkeyAction) -> String {
    match action {
        HotkeyAction::LaunchApp(cmd) => format!("\u{201C}{cmd}\u{201D}"),
        HotkeyAction::SwitchDesktop(n) => {
            format!("Switch to Desktop {}", u16::from(*n).saturating_add(1))
        }
        other => other.display_label().to_string(),
    }
}

/// The keys `action` is on, in display form, in the registry's order.
#[must_use]
pub fn chords_of(registry: &HotkeyRegistry, action: &HotkeyAction) -> Vec<String> {
    registry
        .all_bindings()
        .filter(|(_, a)| *a == action)
        .map(|(h, _)| h.display_name())
        .collect()
}

/// Every entry the action list offers: "Run a command…", each fixed action,
/// and one "Switch to Desktop N" per desktop.
///
/// Built from [`HotkeyAction::ALL`], whose completeness
/// `scripts/check-variant-lists.py` holds the enum to, so an action added to
/// the enum is offered here without anyone remembering to.
#[must_use]
pub fn choices(desktops: u8) -> Vec<Choice> {
    let mut out = vec![Choice::Command];
    for action in HotkeyAction::ALL {
        match action {
            // Offered as its own entry, above.
            HotkeyAction::LaunchApp(_) => {}
            // One entry per desktop that exists.
            HotkeyAction::SwitchDesktop(_) => {
                out.extend((0..desktops).map(|n| Choice::Action(HotkeyAction::SwitchDesktop(n))));
            }
            other => out.push(Choice::Action(other)),
        }
    }
    out
}

/// The entries whose label contains `query`, ignoring case and the spaces
/// around it.
#[must_use]
pub fn filtered(all: &[Choice], query: &str) -> Vec<Choice> {
    let query = query.trim();
    all.iter()
        .filter(|choice| contains(&choice.label(), query, Case::Insensitive))
        .cloned()
        .collect()
}

// ----------------------------------------------------------------------
// The card's foot, and where everything on the card is
// ----------------------------------------------------------------------

/// What the foot's line says on the plain list with nothing to report: the
/// keys the card answers -- each a button below it too. Without it the
/// editor's keys are discoverable only by reading the source, which is the
/// fate of every keyboard feature nobody is told about.
pub(crate) const HINT: &str = "Enter: new keys \u{b7} F2: change action \u{b7} Insert: add \u{b7} Delete: remove \u{b7} Esc: close";

/// A button along the card's foot. Each *is* one of the card's keys: a press
/// on it hands that key to [`ShortcutEditor::handle_key`] as the keyboard
/// does, so a click and a keystroke cannot come to mean different things.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardButton {
    /// Enter on the list: record new keys for the chosen row.
    NewKeys,
    /// F2: choose what the chosen row's keys do.
    ChangeAction,
    /// Delete: take the chosen row off.
    Remove,
    /// Insert: add a shortcut.
    Add,
    /// Escape on the list: close the card.
    Close,
    /// Escape while keys are recorded, an action chosen or a program typed.
    Cancel,
    /// Enter on the question whether to take keys from what holds them:
    /// take them.
    Move,
    /// Escape on that question: leave them where they are.
    Keep,
    /// Enter over the action list: the chosen entry.
    Choose,
    /// Enter under the program field.
    Ok,
}

impl CardButton {
    /// What it says.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NewKeys => "New keys",
            Self::ChangeAction => "Change action",
            Self::Remove => "Remove",
            Self::Add => "Add",
            Self::Close => "Close",
            Self::Cancel => "Cancel",
            Self::Move => "Move",
            Self::Keep => "Keep",
            Self::Choose => "Choose",
            Self::Ok => "OK",
        }
    }

    /// The key it is.
    #[must_use]
    pub const fn key(self) -> Key {
        match self {
            Self::NewKeys | Self::Move | Self::Choose | Self::Ok => Key::Enter,
            Self::ChangeAction => Key::F2,
            Self::Remove => Key::Delete,
            Self::Add => Key::Insert,
            Self::Close | Self::Cancel | Self::Keep => Key::Escape,
        }
    }

    /// How it is drawn: the one Enter presses as what the card is there for,
    /// the one that takes a shortcut off in red, the rest plain.
    #[must_use]
    pub const fn kind(self) -> button::Kind {
        match self {
            Self::NewKeys | Self::Move | Self::Choose | Self::Ok => button::Kind::Primary,
            Self::Remove => button::Kind::Destructive,
            Self::ChangeAction | Self::Add | Self::Close | Self::Cancel | Self::Keep => {
                button::Kind::Plain
            }
        }
    }

    /// Why it cannot be pressed, for the time it cannot (`design-decisions.md`
    /// §1473): a row's buttons with no row to act on, Choose with nothing
    /// left to choose. The others can always be pressed.
    #[must_use]
    pub const fn why_disabled(self) -> &'static str {
        match self {
            Self::NewKeys | Self::ChangeAction | Self::Remove => "There are no shortcuts to change",
            Self::Choose => "No action matches what is typed",
            Self::Add | Self::Close | Self::Cancel | Self::Move | Self::Keep | Self::Ok => "",
        }
    }

    /// Whether it acts on the list's chosen row: laid out at the left of its
    /// line, apart from those acting on the card, at the right.
    const fn on_the_row(self) -> bool {
        matches!(self, Self::NewKeys | Self::ChangeAction | Self::Remove)
    }
}

/// Every button the list's card shows in any of its modes, the most it
/// shows at once: its foot is laid out for these whatever the card is
/// doing, so the card keeps its size while keys are recorded or a question
/// is asked.
const LIST_BUTTONS: [CardButton; 5] = [
    CardButton::NewKeys,
    CardButton::ChangeAction,
    CardButton::Remove,
    CardButton::Add,
    CardButton::Close,
];
/// The same for the action list's card: two buttons in either of its modes.
const PICKER_BUTTONS: [CardButton; 2] = [CardButton::Choose, CardButton::Cancel];

/// Height of the foot's line saying what the last edit did.
const MESSAGE_HEIGHT: f32 = hotkeys::ROW_HEIGHT;
/// Space between two buttons side by side, and between two lines of them.
const BUTTON_GAP: f32 = 8.0;

/// The lines `buttons` take across `room`, as many on each as fit, in order,
/// at their widths in `style`. A button wider than all the room has a line
/// of its own.
fn button_lines(buttons: &[CardButton], style: &ButtonStyle, room: f32) -> Vec<Vec<CardButton>> {
    let mut lines: Vec<Vec<CardButton>> = Vec::new();
    let mut used = 0.0_f32;
    for &which in buttons {
        let wide = button::width(style, which.label());
        match lines.last_mut() {
            Some(line) if used + BUTTON_GAP + wide <= room => {
                line.push(which);
                used += BUTTON_GAP + wide;
            }
            _ => {
                lines.push(vec![which]);
                used = wide;
            }
        }
    }
    lines
}

/// How tall a foot is whose buttons take `lines` lines: the line saying what
/// the last edit did, the buttons' lines and the gaps between them, and the
/// card's padding under the last.
fn foot_height_for(lines: usize) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a line per button at most, and there are five"
    )]
    let lines = lines.max(1) as f32;
    MESSAGE_HEIGHT + lines * button::height() + (lines - 1.0) * BUTTON_GAP + hotkeys::PADDING
}

/// How tall the list's card's foot is, its buttons in `style`: what
/// [`hotkeys::render_settings_panel`] leaves under the rows. Laid out for
/// every button the list shows (`LIST_BUTTONS`) across one column -- the
/// narrowest the card is -- so the room holds them on a card of any width,
/// at any text size.
#[must_use]
pub fn list_foot_height(style: &ButtonStyle) -> f32 {
    foot_height_for(
        button_lines(
            &LIST_BUTTONS,
            style,
            hotkeys::PANEL_WIDTH - hotkeys::PADDING * 2.0,
        )
        .len(),
    )
}

/// The same for the action list's card.
#[must_use]
pub fn picker_foot_height(style: &ButtonStyle) -> f32 {
    foot_height_for(
        button_lines(
            &PICKER_BUTTONS,
            style,
            PICKER_WIDTH - hotkeys::PADDING * 2.0,
        )
        .len(),
    )
}

/// Where the foot of `card` -- the bottom `foot` of it -- puts its line and
/// `buttons`: the line across the top of the foot, inside the card's
/// padding; the buttons in the lines they take across `room`, the last line
/// on the card's bottom padding -- where a dialog's buttons are -- and in
/// each line those acting on the chosen row at its left, the rest at its
/// right. In the order `buttons` gives them.
fn foot_places(
    card: Rect,
    foot: f32,
    buttons: &[(CardButton, bool)],
    style: &ButtonStyle,
    room: f32,
) -> (Rect, Vec<(CardButton, bool, Rect)>) {
    let inner_x = card.x + hotkeys::PADDING;
    let inner_w = (card.w - hotkeys::PADDING * 2.0).max(0.0);
    let message = Rect::new(inner_x, card.y + card.h - foot, inner_w, MESSAGE_HEIGHT);
    let which: Vec<CardButton> = buttons.iter().map(|&(b, _)| b).collect();
    let tall = button::height();
    let mut placed: Vec<(CardButton, Rect)> = Vec::with_capacity(which.len());
    let mut top = card.y + card.h - hotkeys::PADDING - tall;
    for line in button_lines(&which, style, room).iter().rev() {
        let mut left = inner_x;
        for &b in line.iter().filter(|b| b.on_the_row()) {
            let wide = button::width(style, b.label());
            placed.push((b, Rect::new(left, top, wide, tall)));
            left += wide + BUTTON_GAP;
        }
        let mut right = inner_x + inner_w;
        for &b in line.iter().rev().filter(|b| !b.on_the_row()) {
            let wide = button::width(style, b.label());
            right -= wide;
            placed.push((b, Rect::new(right, top, wide, tall)));
            right -= BUTTON_GAP;
        }
        top -= tall + BUTTON_GAP;
    }
    let buttons = buttons
        .iter()
        .filter_map(|&(b, enabled)| {
            placed
                .iter()
                .find(|(p, _)| *p == b)
                .map(|&(_, at)| (b, enabled, at))
        })
        .collect();
    (message, buttons)
}

/// A card `width` by `height` centred on a `screen` of that size -- its
/// top-left corner held on the screen, so a card larger than a small display
/// shows its start rather than losing its heading off the top.
fn centred((screen_w, screen_h): (f32, f32), width: f32, height: f32) -> Rect {
    Rect::new(
        ((screen_w - width) / 2.0).max(0.0),
        ((screen_h - height) / 2.0).max(0.0),
        width,
        height,
    )
}

/// Where entry `index` of the action list is while the list shows from entry
/// `top`, in a list drawn in `area`: its row, or above or below the list for
/// one scrolled out of it -- where the user would find it, scrolling.
fn entry_rect(area: Rect, index: usize, top: usize) -> Rect {
    let rows = |n: usize| f32::from(u16::try_from(n).unwrap_or(u16::MAX)) * hotkeys::ROW_HEIGHT;
    let y = if index >= top {
        area.y + rows(index.saturating_sub(top))
    } else {
        area.y - rows(top.saturating_sub(index))
    };
    Rect::new(area.x, y, area.w, hotkeys::ROW_HEIGHT)
}

/// The card's field as tools see it: named, holding what is typed, with the
/// keyboard -- it has it whenever it is shown.
fn field_node(name: &str, text: &str, at: Rect) -> Node<CardPart> {
    let mut field = Node::new(CardPart::Field, Role::TextField, name, at);
    field.value = Some(Value::Text(text.to_owned()));
    field.focused = true;
    field.focusable = true;
    field
}

/// Where everything on the card is, as [`ShortcutEditor::layout`] works it
/// out.
#[derive(Clone, Debug, PartialEq)]
pub struct CardLayout {
    /// The card.
    pub card: Rect,
    /// Each binding's row, by its keys, on the list's card: as
    /// `hotkeys::row_rects` has them.
    pub rows: Vec<(Hotkey, Rect)>,
    /// The bindings' area on the list's card, the action list's on the
    /// other while an action is chosen.
    pub list: Option<Rect>,
    /// The search or program field, on the action list's card.
    pub field: Option<Rect>,
    /// Each entry of the action list on screen: its place among those the
    /// search leaves, and its row.
    pub entries: Vec<(usize, Rect)>,
    /// The foot's line saying what the last edit did.
    pub message: Rect,
    /// The buttons along the foot, each with whether it can be pressed.
    pub buttons: Vec<(CardButton, bool, Rect)>,
    /// How tall the foot is.
    pub foot: f32,
}

/// What a point on the card is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardSpot {
    /// A binding's row, by its keys.
    Row(Hotkey),
    /// An entry of the action list, by its place among those the search
    /// leaves.
    Entry(usize),
    /// The search or program field.
    Field,
    /// The action list, between or below its entries.
    List,
    /// A button, and whether it can be pressed.
    Button(CardButton, bool),
    /// The card, on none of the above.
    Card,
}

impl CardLayout {
    /// What is at `(x, y)`; `None` off the card.
    #[must_use]
    pub fn at(&self, x: f32, y: f32) -> Option<CardSpot> {
        if !self.card.contains(x, y) {
            return None;
        }
        if let Some(&(which, enabled, _)) = self.buttons.iter().find(|(_, _, at)| at.contains(x, y))
        {
            return Some(CardSpot::Button(which, enabled));
        }
        if let Some(&(keys, _)) = self.rows.iter().find(|(_, at)| at.contains(x, y)) {
            return Some(CardSpot::Row(keys));
        }
        if let Some(&(index, _)) = self.entries.iter().find(|(_, at)| at.contains(x, y)) {
            return Some(CardSpot::Entry(index));
        }
        if self.field.is_some_and(|field| field.contains(x, y)) {
            return Some(CardSpot::Field);
        }
        if self.field.is_some() && self.list.is_some_and(|list| list.contains(x, y)) {
            return Some(CardSpot::List);
        }
        Some(CardSpot::Card)
    }
}

/// A part of the card, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardPart {
    /// The card: the bindings, or the action list while one is chosen.
    Card,
    /// The bindings.
    Bindings,
    /// A binding's row, by its keys -- which, unlike its place, an edit does
    /// not move.
    Binding(Hotkey),
    /// The foot's line saying what the last edit did; while keys are
    /// recorded, the field they go into.
    Message,
    /// The search over the action list, or the program field.
    Field,
    /// The action list.
    Entries,
    /// An entry of the action list, by its place among every entry
    /// ([`choices`]) -- not among those the search leaves, which change as it
    /// is typed.
    Entry(usize),
    /// A button along the foot.
    Button(CardButton),
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::Modifiers;

    const CX: Context = Context {
        desktops: 4,
        picker_rows: 8,
        list_rows: 4,
    };

    /// The built-in theme's buttons.
    const STYLE: ButtonStyle = guitk::widget_style::WidgetStyle::AERO.button;
    /// A display the card is laid out on.
    const SCREEN: (f32, f32) = (1920.0, 1080.0);

    /// The card's list pages a column at a time and reaches its ends with Home
    /// and End, Ctrl or not (§1416); Ctrl+Page Down is not the card's.
    #[test]
    fn the_cards_list_pages_by_a_column_and_reaches_its_ends() {
        let bindings: Vec<(Hotkey, HotkeyAction)> = [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
        ]
        .into_iter()
        .map(|k| (hk(k, true, true, false, false), HotkeyAction::ShowDesktop))
        .collect();
        let mut reg = registry(&[]);
        for (h, a) in bindings {
            // Several chords on one action is allowed; the list is by chord.
            reg.register(h, a).unwrap();
        }
        let mut ed = ShortcutEditor::new();
        assert_eq!(
            ed.handle_key(&press(Key::PageDown), &mut reg, CX),
            Outcome::Handled
        );
        assert_eq!(ed.selected, 4, "one column of four down");
        ed.handle_key(&press(Key::End), &mut reg, CX);
        assert_eq!(ed.selected, 9);
        ed.handle_key(&press(Key::PageUp), &mut reg, CX);
        assert_eq!(ed.selected, 5);
        let ctrl_home = chord(Key::Home, true, false, false, false);
        ed.handle_key(&ctrl_home, &mut reg, CX);
        assert_eq!(ed.selected, 0);
        let ctrl_page_down = chord(Key::PageDown, true, false, false, false);
        assert_eq!(
            ed.handle_key(&ctrl_page_down, &mut reg, CX),
            Outcome::NotMine,
            "Ctrl+Page Down was taken from the desktop"
        );
        assert_eq!(ed.selected, 0);
    }

    /// In the action picker, Home and End are the list's while the search is
    /// empty, the search field's once it has text, and the list's again with
    /// Ctrl held.
    #[test]
    fn home_and_end_in_the_picker_follow_the_field_over_a_list_rule() {
        let mut reg = registry(&[]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        let selected = |ed: &ShortcutEditor| match ed.mode() {
            Mode::Pick { view, .. } => view.selected(),
            other => panic!("not picking: {other:?}"),
        };
        let caret = |ed: &ShortcutEditor| match ed.mode() {
            Mode::Pick { query, .. } => query.cursor().byte(),
            other => panic!("not picking: {other:?}"),
        };
        ed.handle_key(&press(Key::End), &mut reg, CX);
        let last = selected(&ed).expect("End picked a row");
        assert!(last > 0, "End stayed on the first row");
        ed.handle_key(&press(Key::Home), &mut reg, CX);
        assert_eq!(selected(&ed), Some(0));

        feed(&mut ed, &mut reg, &typed("win"));
        let before = selected(&ed);
        ed.handle_key(&press(Key::Home), &mut reg, CX);
        assert_eq!(caret(&ed), 0, "Home did not move the caret");
        assert_eq!(selected(&ed), before, "Home moved the list, not the caret");
        ed.handle_key(&chord(Key::End, true, false, false, false), &mut reg, CX);
        assert_ne!(selected(&ed), before, "Ctrl+End did not move the list");
    }

    fn press(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn chord(k: Key, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                ctrl,
                alt,
                shift,
                super_key,
            },
            text: String::new(),
        }
    }

    fn typed(s: &str) -> Vec<KeyEvent> {
        s.chars()
            .map(|c| KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: c.to_string(),
            })
            .collect()
    }

    fn feed(ed: &mut ShortcutEditor, reg: &mut HotkeyRegistry, keys: &[KeyEvent]) {
        for k in keys {
            ed.handle_key(k, reg, CX);
        }
    }

    /// Put the editor's list selection on the row holding `keys`.
    fn select_row(ed: &mut ShortcutEditor, reg: &HotkeyRegistry, keys: Hotkey) {
        let row = reg
            .all_bindings()
            .position(|(h, _)| same_binding(h, &keys))
            .expect("the chord is bound");
        ed.set_selected(row);
    }

    fn hk(k: Key, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> Hotkey {
        Hotkey::new(
            k,
            Modifiers {
                ctrl,
                alt,
                shift,
                super_key,
            },
        )
    }

    /// A registry holding exactly what a test names, so no shipped default
    /// can turn out to hold the chord a test picked.
    fn registry(bindings: &[(Hotkey, HotkeyAction)]) -> HotkeyRegistry {
        let mut reg = HotkeyRegistry::new();
        for (h, a) in bindings {
            reg.register(*h, a.clone()).unwrap();
        }
        reg
    }

    #[test]
    fn f2_points_a_chord_at_a_different_action() {
        let alt_f4 = hk(Key::F4, false, true, false, false);
        let mut reg = registry(&[(alt_f4, HotkeyAction::CloseWindow)]);
        let mut ed = ShortcutEditor::new();
        select_row(&mut ed, &reg, alt_f4);
        assert_eq!(
            ed.handle_key(&press(Key::F2), &mut reg, CX),
            Outcome::Handled
        );
        assert!(ed.is_picking());
        feed(&mut ed, &mut reg, &typed("lock"));
        assert_eq!(
            ed.handle_key(&press(Key::Enter), &mut reg, CX),
            Outcome::Changed
        );
        assert_eq!(
            reg.lookup(Key::F4, &alt_f4.modifiers()),
            Some(&HotkeyAction::ScreenLock)
        );
        assert_eq!(
            ed.message(),
            Some("Alt+F4 is now Lock Screen; Close Window has no keys now")
        );
        assert!(matches!(ed.mode(), Mode::List));
    }

    #[test]
    fn insert_adds_a_shortcut_found_by_searching() {
        let mut reg = registry(&[]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        // "Window Overview" and nothing else: "overview" alone also finds
        // Cycle Windows in the Overview, listed first.
        feed(&mut ed, &mut reg, &typed("WINDOW OVERVIEW"));
        assert_eq!(
            ed.handle_key(&press(Key::Enter), &mut reg, CX),
            Outcome::Handled
        );
        assert!(ed.is_recording(), "the action is chosen; now the keys");
        // A bare modifier is the chord being assembled, not the answer.
        let ctrl_alt = chord(Key::LeftCtrl, true, true, false, false);
        assert_eq!(ed.handle_key(&ctrl_alt, &mut reg, CX), Outcome::Handled);
        assert!(ed.is_recording());
        let keys = chord(Key::O, true, true, false, false);
        assert_eq!(ed.handle_key(&keys, &mut reg, CX), Outcome::Changed);
        assert_eq!(
            reg.lookup(Key::O, &keys.modifiers),
            Some(&HotkeyAction::ToggleOverview)
        );
        assert_eq!(ed.message(), Some("Ctrl+Alt+O is now Window Overview"));
    }

    #[test]
    fn a_shortcut_can_start_an_arbitrary_program() {
        let mut reg = registry(&[]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        // "Run a program..." is the first entry of the unfiltered list.
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::Command { .. }));
        // An empty program is refused, not bound.
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::Command { .. }));
        feed(&mut ed, &mut reg, &typed("  /usr/bin/terminal "));
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        assert!(ed.is_recording());
        let keys = chord(Key::T, true, true, false, false);
        assert_eq!(ed.handle_key(&keys, &mut reg, CX), Outcome::Changed);
        assert_eq!(
            reg.lookup(Key::T, &keys.modifiers),
            Some(&HotkeyAction::LaunchApp("/usr/bin/terminal".to_string())),
            "trimmed, and bound"
        );
        assert_eq!(
            ed.message(),
            Some("Ctrl+Alt+T is now \u{201C}/usr/bin/terminal\u{201D}")
        );
    }

    #[test]
    fn changing_a_program_starts_from_the_program_it_runs() {
        let keys = hk(Key::T, true, true, false, false);
        let mut reg = registry(&[(keys, HotkeyAction::LaunchApp("/usr/bin/termnial".into()))]);
        let mut ed = ShortcutEditor::new();
        select_row(&mut ed, &reg, keys);
        ed.handle_key(&press(Key::F2), &mut reg, CX);
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let Mode::Command { input, .. } = ed.mode() else {
            panic!("not in the command field");
        };
        assert_eq!(input.text(), "/usr/bin/termnial", "an edit, not a retype");
    }

    #[test]
    fn taken_keys_are_offered_and_moved_only_on_enter() {
        let super_l = hk(Key::L, false, false, false, true);
        let super_e = hk(Key::E, false, false, false, true);
        let mut reg = registry(&[
            (super_l, HotkeyAction::ScreenLock),
            (super_e, HotkeyAction::ShowDesktop),
        ]);
        let mut ed = ShortcutEditor::new();
        // Re-record Show Desktop onto Super+L, which is taken.
        select_row(&mut ed, &reg, super_e);
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let pressed = chord(Key::L, false, false, false, true);
        assert_eq!(ed.handle_key(&pressed, &mut reg, CX), Outcome::Handled);
        assert!(matches!(ed.mode(), Mode::Steal { .. }));
        assert_eq!(
            ed.message(),
            Some("Super+L is already Lock Screen. Enter moves it to Show Desktop; Escape keeps it")
        );
        // Anything but an answer is not an answer.
        ed.handle_key(&press(Key::Down), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::Steal { .. }));
        assert_eq!(
            reg.lookup(Key::L, &super_l.modifiers()),
            Some(&HotkeyAction::ScreenLock)
        );

        assert_eq!(
            ed.handle_key(&press(Key::Enter), &mut reg, CX),
            Outcome::Changed
        );
        assert_eq!(
            reg.lookup(Key::L, &super_l.modifiers()),
            Some(&HotkeyAction::ShowDesktop)
        );
        assert_eq!(
            reg.lookup(Key::E, &super_e.modifiers()),
            None,
            "moved off its old keys"
        );
        assert_eq!(
            ed.message(),
            Some("Super+L is now Show Desktop; Lock Screen has no keys now")
        );
    }

    #[test]
    fn escape_keeps_taken_keys_where_they_were() {
        let super_l = hk(Key::L, false, false, false, true);
        let mut reg = registry(&[(super_l, HotkeyAction::ScreenLock)]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        feed(&mut ed, &mut reg, &typed("show desktop"));
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        ed.handle_key(&chord(Key::L, false, false, false, true), &mut reg, CX);
        assert_eq!(
            ed.handle_key(&press(Key::Escape), &mut reg, CX),
            Outcome::Handled
        );
        assert_eq!(
            reg.lookup(Key::L, &super_l.modifiers()),
            Some(&HotkeyAction::ScreenLock)
        );
        assert_eq!(reg.len(), 1);
        assert_eq!(ed.message(), Some("Unchanged"));
    }

    #[test]
    fn keys_the_action_already_has_are_not_a_conflict_to_resolve() {
        let alt_tab = hk(Key::Tab, false, true, false, false);
        let super_tab = hk(Key::Tab, false, false, false, true);
        let mut reg = registry(&[
            (alt_tab, HotkeyAction::CycleWindows),
            (super_tab, HotkeyAction::CycleWindows),
        ]);
        let mut ed = ShortcutEditor::new();
        select_row(&mut ed, &reg, super_tab);
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let out = ed.handle_key(&chord(Key::Tab, false, true, false, false), &mut reg, CX);
        assert_eq!(out, Outcome::Handled);
        assert_eq!(reg.len(), 2, "nothing merged");
        assert_eq!(ed.message(), Some("Alt+Tab is already Cycle Windows"));
    }

    #[test]
    fn re_recording_the_same_keys_changes_nothing() {
        // VolumeUp is a dedicated key: the registry ignores modifiers held
        // with it, so Super+VolumeUp *is* VolumeUp and must read as unchanged
        // rather than as a new binding.
        let vol = Hotkey::bare(Key::VolumeUp);
        let mut reg = registry(&[(vol, HotkeyAction::VolumeUp)]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let out = ed.handle_key(
            &chord(Key::VolumeUp, false, false, false, true),
            &mut reg,
            CX,
        );
        assert_eq!(out, Outcome::Handled);
        assert_eq!(ed.message(), Some("Unchanged"));
    }

    #[test]
    fn while_editing_no_key_reaches_the_global_table() {
        let mut reg = registry(&[(
            hk(Key::F4, false, true, false, false),
            HotkeyAction::CloseWindow,
        )]);
        let mut ed = ShortcutEditor::new();
        // On the plain list, an unknown key is not the card's.
        assert_eq!(
            ed.handle_key(&press(Key::A), &mut reg, CX),
            Outcome::NotMine
        );
        let everything = [
            press(Key::A),
            press(Key::Tab),
            press(Key::Left),
            chord(Key::F4, false, true, false, false),
            KeyEvent {
                pressed: false,
                ..press(Key::A)
            },
        ];
        for open in [Key::F2, Key::Insert] {
            ed.reset();
            ed.handle_key(&press(open), &mut reg, CX);
            for k in &everything {
                assert_ne!(
                    ed.handle_key(k, &mut reg, CX),
                    Outcome::NotMine,
                    "{open:?} then {k:?}"
                );
            }
        }
        // And recording: Alt+F4 is data here, not "close the window".
        ed.reset();
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        assert!(ed.is_recording());
        let release = KeyEvent {
            pressed: false,
            ..press(Key::LeftAlt)
        };
        assert_eq!(ed.handle_key(&release, &mut reg, CX), Outcome::Handled);
    }

    #[test]
    fn delete_says_which_keys_the_action_still_has() {
        let alt_tab = hk(Key::Tab, false, true, false, false);
        let super_tab = hk(Key::Tab, false, false, false, true);
        let mut reg = registry(&[
            (alt_tab, HotkeyAction::CycleWindows),
            (super_tab, HotkeyAction::CycleWindows),
        ]);
        let mut ed = ShortcutEditor::new();
        select_row(&mut ed, &reg, super_tab);
        assert_eq!(
            ed.handle_key(&press(Key::Delete), &mut reg, CX),
            Outcome::Changed
        );
        assert_eq!(
            ed.message(),
            Some("Cycle Windows is off Super+Tab; it still has Alt+Tab")
        );
        ed.set_selected(0);
        ed.handle_key(&press(Key::Delete), &mut reg, CX);
        assert_eq!(ed.message(), Some("Cycle Windows is no longer on any keys"));
        assert!(reg.is_empty());
    }

    #[test]
    fn the_selection_follows_a_binding_that_re_sorts() {
        let a = hk(Key::A, false, false, false, true);
        let z = hk(Key::Z, false, false, false, true);
        let mut reg = registry(&[
            (a, HotkeyAction::ShowDesktop),
            (z, HotkeyAction::ScreenLock),
        ]);
        let mut ed = ShortcutEditor::new();
        select_row(&mut ed, &reg, a);
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        ed.handle_key(&chord(Key::Y, false, false, false, true), &mut reg, CX);
        let (at, action) = reg.all_bindings().nth(ed.selected()).unwrap();
        assert_eq!(
            *action,
            HotkeyAction::ShowDesktop,
            "still on the binding just edited"
        );
        assert_eq!(at.key, Key::Y);
    }

    #[test]
    fn nothing_matching_the_search_leaves_enter_with_nothing_to_do() {
        let mut reg = registry(&[]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        feed(&mut ed, &mut reg, &typed("zzzz no such action"));
        assert_eq!(
            ed.handle_key(&press(Key::Enter), &mut reg, CX),
            Outcome::Handled
        );
        assert!(ed.is_picking(), "still searching");
        assert!(reg.is_empty());
    }

    #[test]
    fn the_list_offers_every_action_once_and_one_entry_per_desktop() {
        let all = choices(4);
        assert_eq!(all.first(), Some(&Choice::Command));
        assert_eq!(all.iter().filter(|c| **c == Choice::Command).count(), 1);
        for n in 0..4 {
            assert!(all.contains(&Choice::Action(HotkeyAction::SwitchDesktop(n))));
        }
        assert!(!all.contains(&Choice::Action(HotkeyAction::SwitchDesktop(4))));
        assert!(
            !all.iter()
                .any(|c| matches!(c, Choice::Action(HotkeyAction::LaunchApp(_))))
        );
        // Every fixed action exactly once: ALL less LaunchApp and the desktop
        // placeholder, plus four desktops, plus the command entry.
        assert_eq!(all.len(), HotkeyAction::ALL.len() - 2 + 4 + 1);
        let labels: Vec<String> = all.iter().map(Choice::label).collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            labels.len(),
            "two entries read the same: {labels:?}"
        );
    }

    #[test]
    fn searching_ignores_case_and_surrounding_spaces() {
        let all = choices(2);
        let hits: Vec<String> = filtered(&all, "  DESKTOP ")
            .iter()
            .map(Choice::label)
            .collect();
        assert!(hits.contains(&"Show Desktop".to_string()));
        assert!(hits.contains(&"Switch to Desktop 2".to_string()));
        assert!(!hits.contains(&"Lock Screen".to_string()));
        assert_eq!(filtered(&all, "").len(), all.len());
    }

    #[test]
    fn the_picker_lists_the_keys_each_action_already_has() {
        let alt_tab = hk(Key::Tab, false, true, false, false);
        let mut reg = registry(&[(alt_tab, HotkeyAction::CycleWindows)]);
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        feed(&mut ed, &mut reg, &typed("cycle windows"));
        let mut tree = RenderTree::new();
        let layout = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        ed.render_picker(
            &mut tree,
            &reg,
            &Palette::for_mode(false),
            &layout,
            CX,
            2.0,
            2.0,
        );
        let texts: Vec<String> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Cycle Windows".to_string()));
        assert!(
            texts.contains(&"Alt+Tab".to_string()),
            "its keys, beside it: {texts:?}"
        );
    }

    #[test]
    fn the_picker_fits_the_height_it_is_given() {
        let foot = picker_foot_height(&STYLE);
        for h in [0.0, 200.0, 400.0, 800.0, 4000.0] {
            let rows = picker_rows(h, foot);
            assert!((MIN_PICKER_ROWS..=MAX_PICKER_ROWS).contains(&rows));
            if picker_height(MIN_PICKER_ROWS, foot) <= h {
                assert!(picker_height(rows, foot) <= h, "{rows} rows overflow {h}");
            }
        }
        assert_eq!(picker_rows(10_000.0, foot), MAX_PICKER_ROWS);
    }

    // ---- the foot, the layout, and what tools see ----

    /// Two bindings, on Ctrl+F1 and Ctrl+F2.
    fn two() -> HotkeyRegistry {
        registry(&[
            (
                hk(Key::F1, true, false, false, false),
                HotkeyAction::ShowDesktop,
            ),
            (
                hk(Key::F2, true, false, false, false),
                HotkeyAction::ScreenLock,
            ),
        ])
    }

    /// **The foot offers the keys of the mode the card is in, each as a
    /// button that is that key**: the list's five -- a row's three greyed
    /// with no row -- Cancel while keys are recorded, Move and Keep for the
    /// question, Choose (greyed with nothing to choose) and Cancel over the
    /// action list, OK and Cancel under the program field.
    #[test]
    fn the_foot_offers_the_keys_of_the_mode_the_card_is_in() {
        use CardButton::{Add, Cancel, ChangeAction, Choose, Close, Keep, Move, NewKeys, Remove};
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        assert_eq!(
            ed.buttons(&reg, CX),
            [
                (NewKeys, true),
                (ChangeAction, true),
                (Remove, true),
                (Add, true),
                (Close, true)
            ]
        );
        assert_eq!(
            ed.buttons(&HotkeyRegistry::new(), CX),
            [
                (NewKeys, false),
                (ChangeAction, false),
                (Remove, false),
                (Add, true),
                (Close, true)
            ]
        );
        // Each is the key the module's table gives what it does.
        let keys: Vec<Key> = [NewKeys, ChangeAction, Remove, Add, Close]
            .iter()
            .map(|b| b.key())
            .collect();
        assert_eq!(
            keys,
            [Key::Enter, Key::F2, Key::Delete, Key::Insert, Key::Escape]
        );

        ed.handle_key(&press(NewKeys.key()), &mut reg, CX);
        assert!(ed.is_recording());
        assert_eq!(ed.buttons(&reg, CX), [(Cancel, true)]);
        // The second row's keys, pressed for the first: the question.
        ed.handle_key(&chord(Key::F2, true, false, false, false), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::Steal { .. }));
        assert_eq!(ed.buttons(&reg, CX), [(Move, true), (Keep, true)]);
        ed.handle_key(&press(Keep.key()), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::List));

        ed.handle_key(&press(Add.key()), &mut reg, CX);
        assert_eq!(ed.buttons(&reg, CX), [(Choose, true), (Cancel, true)]);
        feed(&mut ed, &mut reg, &typed("no action is called this"));
        assert_eq!(ed.buttons(&reg, CX), [(Choose, false), (Cancel, true)]);
        ed.handle_key(&press(Cancel.key()), &mut reg, CX);

        ed.handle_key(&press(Add.key()), &mut reg, CX);
        // "Run a program..." is first, and chosen.
        ed.handle_key(&press(Choose.key()), &mut reg, CX);
        assert!(matches!(ed.mode(), Mode::Command { .. }));
        assert_eq!(
            ed.buttons(&reg, CX),
            [(CardButton::Ok, true), (Cancel, true)]
        );
        // Greyed buttons say why; the rest can always be pressed.
        for b in [NewKeys, ChangeAction, Remove, Choose] {
            assert!(!b.why_disabled().is_empty(), "{b:?}");
        }
    }

    /// **The list's buttons fit one line of a one-column card at the
    /// built-in size**, and the room under the rows is that line's.
    #[test]
    fn the_lists_buttons_fit_one_line_at_the_built_in_size() {
        let room = hotkeys::PANEL_WIDTH - hotkeys::PADDING * 2.0;
        assert_eq!(button_lines(&LIST_BUTTONS, &STYLE, room).len(), 1);
        assert_eq!(button_lines(&PICKER_BUTTONS, &STYLE, room).len(), 1);
        assert!((list_foot_height(&STYLE) - foot_height_for(1)).abs() < 0.01);
        assert!(foot_height_for(2) > foot_height_for(1));
    }

    /// **Buttons that do not fit wrap onto more lines** rather than off the
    /// card: in order, none lost, no line empty, none wider than the room
    /// unless one button alone is.
    #[test]
    fn buttons_that_do_not_fit_wrap_onto_more_lines() {
        for room in [500.0, 300.0, 150.0, 10.0] {
            let lines = button_lines(&LIST_BUTTONS, &STYLE, room);
            let flat: Vec<CardButton> = lines.iter().flatten().copied().collect();
            assert_eq!(flat, LIST_BUTTONS, "at {room}");
            for line in &lines {
                assert!(!line.is_empty());
                let wide: f32 = line
                    .iter()
                    .map(|b| button::width(&STYLE, b.label()))
                    .sum::<f32>()
                    + BUTTON_GAP * (line.len() - 1) as f32;
                assert!(
                    line.len() == 1 || wide <= room,
                    "{line:?} is {wide} in {room}"
                );
            }
        }
        assert!(button_lines(&LIST_BUTTONS, &STYLE, 150.0).len() > 1);
    }

    /// **The list's foot puts what acts on the chosen row at the left and
    /// what acts on the card at the right**, on the card's bottom padding,
    /// under the line saying what the last edit did -- and every button
    /// stays where it was whatever the card is doing, the card keeping its
    /// size.
    #[test]
    fn the_foot_lays_the_rows_buttons_left_and_the_cards_right() {
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        let laid = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        let at = |which: CardButton| {
            laid.buttons
                .iter()
                .find(|(b, _, _)| *b == which)
                .map(|&(_, _, r)| r)
                .expect("shown")
        };
        let inner_left = laid.card.x + hotkeys::PADDING;
        let inner_right = laid.card.x + laid.card.w - hotkeys::PADDING;
        assert!((at(CardButton::NewKeys).x - inner_left).abs() < 0.01);
        assert!((at(CardButton::Close).x + at(CardButton::Close).w - inner_right).abs() < 0.01);
        assert!(at(CardButton::Remove).x + at(CardButton::Remove).w < at(CardButton::Add).x);
        let bottom = laid.card.y + laid.card.h - hotkeys::PADDING;
        for (which, _, r) in &laid.buttons {
            assert!(
                (r.y + r.h - bottom).abs() < 0.01,
                "{which:?} is not on the foot"
            );
            assert!(
                r.y >= laid.message.y + laid.message.h,
                "{which:?} is over the line"
            );
            for (other, _, s) in &laid.buttons {
                assert!(
                    which == other || r.intersect(*s).is_none(),
                    "{which:?} over {other:?}"
                );
            }
        }
        // Recording, the card keeps its size and Cancel takes Close's place.
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let recording = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        assert_eq!(recording.card, laid.card);
        assert_eq!(recording.buttons.len(), 1);
        assert_eq!(
            recording.buttons[0].2,
            Rect::new(
                inner_right - button::width(&STYLE, "Cancel"),
                at(CardButton::Close).y,
                button::width(&STYLE, "Cancel"),
                button::height(),
            )
        );
    }

    /// **A point on the card is on what is drawn there**: a row by its keys,
    /// a button and whether it can be pressed, the field, an entry of the
    /// action list by its place among those the search leaves -- and off
    /// the card, nothing.
    #[test]
    fn a_point_on_the_card_is_on_what_is_drawn_there() {
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        let laid = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        for (keys, r) in &laid.rows {
            let (x, y) = r.centre();
            assert_eq!(laid.at(x, y), Some(CardSpot::Row(*keys)));
        }
        let (x, y) = laid.buttons[2].2.centre();
        assert_eq!(
            laid.at(x, y),
            Some(CardSpot::Button(CardButton::Remove, true))
        );
        assert_eq!(
            laid.at(laid.card.x + 2.0, laid.card.y + 2.0),
            Some(CardSpot::Card)
        );
        assert_eq!(laid.at(laid.card.x - 2.0, laid.card.y), None);

        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        let picking = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        let field = picking.field.expect("a search field");
        assert_eq!(
            picking.at(field.centre().0, field.centre().1),
            Some(CardSpot::Field)
        );
        let (index, row) = picking.entries[1];
        assert_eq!(
            picking.at(row.centre().0, row.centre().1),
            Some(CardSpot::Entry(index))
        );
        assert!(picking.rows.is_empty());
    }

    /// **A row or an entry is chosen as Up and Down choose one; the wheel
    /// scrolls the action list without moving the choice.** A row only on
    /// the plain list; an entry only while an action is chosen.
    #[test]
    fn rows_and_entries_are_chosen_as_the_keys_choose_them() {
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        let second = hk(Key::F2, true, false, false, false);
        ed.set_message(Some("Unchanged".to_string()));
        assert!(ed.choose_row(&reg, second));
        assert_eq!(ed.selected(), 1);
        assert_eq!(ed.message(), None, "a step clears what the last edit said");
        assert!(!ed.choose_row(&reg, hk(Key::F9, false, false, false, false)));
        assert!(!ed.choose_entry(0, CX), "no action list up");

        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        assert!(!ed.choose_row(&reg, hk(Key::F1, true, false, false, false)));
        assert_eq!(ed.selected(), 1, "a row is not what a recording waits for");
        ed.handle_key(&press(Key::Escape), &mut reg, CX);

        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        assert!(ed.choose_entry(3, CX));
        assert_eq!(ed.entry_place(3, CX), Some((3, Some(3))));
        assert!(!ed.choose_entry(10_000, CX));
        let before = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE).entries[0].0;
        assert!(ed.scroll_entries(4, CX));
        let after = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE).entries[0].0;
        assert!(after > before, "{before} then {after}");
        assert_eq!(
            ed.entry_place(3, CX),
            Some((3, Some(3))),
            "the wheel moved the choice"
        );
        // Searched for, an entry is where the search leaves it.
        feed(&mut ed, &mut reg, &typed("lock"));
        let all = choices(CX.desktops);
        let lock = all
            .iter()
            .position(|c| *c == Choice::Action(HotkeyAction::ScreenLock))
            .expect("offered");
        assert_eq!(ed.entry_place(lock, CX).map(|(at, _)| at), Some(0));
        assert_eq!(ed.entry_place(0, CX), None, "Run a program is searched out");
    }

    /// **The card as tools see it**: a dialog named by its heading; the
    /// rows named by what they do, described by their keys, the one with the
    /// keyboard chosen; the foot's line as a label saying what it says; the
    /// buttons, each described by its key, a greyed one by why.
    #[test]
    fn tools_see_the_list_its_line_and_its_buttons() {
        let reg = two();
        let ed = ShortcutEditor::new();
        let laid = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        let card = ed.automation(&reg, CX, &laid);
        assert_eq!(
            (card.role, card.name.as_str(), card.bounds),
            (Role::Dialog, "Keyboard Shortcuts", laid.card)
        );
        let list = &card.children[0];
        assert_eq!((list.id, list.role), (CardPart::Bindings, Role::List));
        let first = &list.children[0];
        assert_eq!(
            (
                first.id,
                first.role,
                first.name.as_str(),
                first.description.as_deref()
            ),
            (
                CardPart::Binding(hk(Key::F1, true, false, false, false)),
                Role::ListItem,
                HotkeyAction::ShowDesktop.display_label(),
                Some("Ctrl+F1")
            )
        );
        assert_eq!(first.value, Some(Value::Chosen(true)));
        assert!(first.focused && first.enabled);
        assert_eq!(list.children[1].value, Some(Value::Chosen(false)));
        assert_eq!(first.bounds, laid.rows[0].1);

        let line = card
            .walk()
            .find(|n| n.id == CardPart::Message)
            .expect("the line");
        assert_eq!((line.role, line.name.as_str()), (Role::Label, HINT));
        let change = card
            .walk()
            .find(|n| n.id == CardPart::Button(CardButton::ChangeAction))
            .expect("Change action");
        assert_eq!(
            (change.role, change.enabled, change.description.as_deref()),
            (Role::Button, true, Some("F2"))
        );

        let empty = HotkeyRegistry::new();
        let laid = ed.layout(&empty, CX, SCREEN, 900.0, &STYLE);
        let card = ed.automation(&empty, CX, &laid);
        let greyed = card
            .walk()
            .find(|n| n.id == CardPart::Button(CardButton::Remove))
            .expect("Remove");
        assert!(!greyed.enabled);
        assert_eq!(
            greyed.description.as_deref(),
            Some(CardButton::Remove.why_disabled())
        );
    }

    /// **While keys are recorded the line is the field they go into**, named
    /// by what it asks; the rows can be read and not used.
    #[test]
    fn tools_see_a_recording_as_a_field_for_the_keys() {
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Enter), &mut reg, CX);
        let laid = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        let card = ed.automation(&reg, CX, &laid);
        let keys = card
            .walk()
            .find(|n| n.id == CardPart::Message)
            .expect("the line");
        assert_eq!(keys.role, Role::TextField);
        assert_eq!(Some(keys.name.as_str()), ed.message());
        assert!(keys.focused);
        assert_eq!(keys.bounds, laid.message);
        let row = card
            .walk()
            .find(|n| matches!(n.id, CardPart::Binding(_)))
            .expect("a row");
        assert!(
            !row.enabled && !row.focused,
            "a row while keys are recorded"
        );
    }

    /// **The action list as tools see it**: its field holding what is
    /// typed; each entry the search leaves, named by its label, described by
    /// the keys it has, by its place among every entry -- one scrolled out
    /// told above the list, where scrolling finds it.
    #[test]
    fn tools_see_the_action_list_and_its_field() {
        let mut reg = two();
        let mut ed = ShortcutEditor::new();
        ed.handle_key(&press(Key::Insert), &mut reg, CX);
        feed(&mut ed, &mut reg, &typed("o"));
        // Down past the visible rows, so the first entries scroll out.
        for _ in 0..12 {
            ed.handle_key(&press(Key::Down), &mut reg, CX);
        }
        let laid = ed.layout(&reg, CX, SCREEN, 900.0, &STYLE);
        let card = ed.automation(&reg, CX, &laid);
        assert_eq!(card.name, "Choose an action");
        let field = card
            .walk()
            .find(|n| n.id == CardPart::Field)
            .expect("the field");
        assert_eq!(
            (field.role, field.value.clone()),
            (Role::TextField, Some(Value::Text("o".to_string())))
        );
        let list = card
            .walk()
            .find(|n| n.id == CardPart::Entries)
            .expect("the list");
        let all = choices(CX.desktops);
        let shown = filtered(&all, "o");
        assert_eq!(list.children.len(), shown.len());
        let area = laid.list.expect("the list's area");
        assert!(
            list.children[0].bounds.y < area.y,
            "the first entry, scrolled out, is not told above the list"
        );
        let lock = list
            .children
            .iter()
            .find(|n| n.name == "Lock Screen")
            .expect("Lock Screen is searched for");
        let id = all
            .iter()
            .position(|c| *c == Choice::Action(HotkeyAction::ScreenLock))
            .expect("offered");
        assert_eq!(lock.id, CardPart::Entry(id));
        assert_eq!(lock.description.as_deref(), Some("Ctrl+F2"));
        let chosen: Vec<&str> = list
            .children
            .iter()
            .filter(|n| n.value == Some(Value::Chosen(true)))
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(chosen.len(), 1, "{chosen:?}");
    }
}
