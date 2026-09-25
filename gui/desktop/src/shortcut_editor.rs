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
use guitk::event::{Key, KeyEvent};
use guitk::listview::ListViewport;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::textedit::{self, SingleLine};
use guitk::textfind::{Case, contains};
use guitk::textinput::{KeyEdit, TextInput};

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
    /// given the same height the card is drawn in.
    pub picker_rows: usize,
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

    /// Width and height of the action-list card, for centring it.
    ///
    /// The same function [`render_picker`](Self::render_picker) sizes itself
    /// with, given the same `max_height`, so the card is centred on the size it
    /// is actually drawn at.
    #[must_use]
    pub fn picker_size(&self, max_height: f32) -> (f32, f32) {
        let rows = picker_rows(max_height);
        (PICKER_WIDTH, picker_height(rows))
    }

    /// Draw the action list, or the program field, as a card at `(x, y)`.
    pub fn render_picker(
        &self,
        tree: &mut RenderTree,
        registry: &HotkeyRegistry,
        p: &Palette,
        x: f32,
        y: f32,
        max_height: f32,
        cx: Context,
        caret_width: f32,
    ) {
        let (width, height) = self.picker_size(max_height);
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

        // The field: a well with the text in it.
        let field_x = x + hotkeys::PADDING;
        let field_y = y + hotkeys::HEADER_HEIGHT;
        let field_w = width - hotkeys::PADDING * 2.0;
        tree.push(RenderCommand::FillRect {
            x: field_x,
            y: field_y,
            width: field_w,
            height: FIELD_HEIGHT,
            color: p.crust,
            corner_radii: CornerRadii::all(6.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: field_x,
            y: field_y,
            width: field_w,
            height: FIELD_HEIGHT,
            // The field has the keyboard, and the accent says where it is.
            color: p.accent,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });
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

        // The list, for picking; nothing below the field for a program.
        if let Mode::Pick { query, view, .. } = &self.mode {
            let shown = filtered(&choices(cx.desktops), query.text());
            let list_top = field_y + FIELD_HEIGHT + hotkeys::PADDING / 2.0;
            let content_w = width - hotkeys::PADDING * 2.0;
            let mut row_y = list_top;
            // A copy, sized to the card being drawn: the stored one was sized at
            // the last keystroke, and the display may have changed since.
            let mut view = *view;
            view.set_height(picker_rows(max_height), shown.len());
            for index in view.visible_range(shown.len()) {
                let Some(choice) = shown.get(index) else {
                    break;
                };
                let picked = view.selected() == Some(index);
                if picked {
                    tree.push(RenderCommand::FillRect {
                        x: x + hotkeys::PADDING / 2.0,
                        y: row_y + 2.0,
                        width: content_w + hotkeys::PADDING,
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
                row_y += hotkeys::ROW_HEIGHT;
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

/// The card height for `rows` visible rows.
fn picker_height(rows: usize) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "at most MAX_PICKER_ROWS rows")]
    let rows = rows as f32;
    hotkeys::HEADER_HEIGHT
        + FIELD_HEIGHT
        + hotkeys::PADDING / 2.0
        + rows * hotkeys::ROW_HEIGHT
        + hotkeys::PADDING
        // The message line the shell draws along the bottom.
        + hotkeys::ROW_HEIGHT
}

/// How many rows of the action list fit in `max_height`.
#[must_use]
pub fn picker_rows(max_height: f32) -> usize {
    (MIN_PICKER_ROWS..=MAX_PICKER_ROWS)
        .rev()
        .find(|&rows| picker_height(rows) <= max_height)
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
    };

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
        feed(&mut ed, &mut reg, &typed("OVERVIEW"));
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
        ed.render_picker(
            &mut tree,
            &reg,
            &Palette::for_mode(false),
            0.0,
            0.0,
            900.0,
            CX,
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
        for h in [0.0, 200.0, 400.0, 800.0, 4000.0] {
            let rows = picker_rows(h);
            assert!((MIN_PICKER_ROWS..=MAX_PICKER_ROWS).contains(&rows));
            if picker_height(MIN_PICKER_ROWS) <= h {
                assert!(picker_height(rows) <= h, "{rows} rows overflow {h}");
            }
        }
        assert_eq!(picker_rows(10_000.0), MAX_PICKER_ROWS);
    }
}
