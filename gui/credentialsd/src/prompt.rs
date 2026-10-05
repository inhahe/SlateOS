//! The credential service's prompt: the windows that ask the user whether a
//! program may have a password -- and, when several would do, which.
//!
//! [`WindowPrompt`] is the service's [`Prompt`]. Each question opens a window
//! of its own on the user's display, waits for the answer and closes:
//! [`AskApp`] asks whether, [`ChooseApp`] which. Both are ordinary toolkit
//! applications -- state, events, drawing -- so a test drives them with no
//! display at all.
//!
//! # What the user is shown
//!
//! - **Which program**: its executable's file name, the part a person knows
//!   it by, over its whole path and its process number -- so a program named
//!   like another is told apart by where it lives.
//! - **For what**: the domain asked for, whole and wrapped, never cut. Cut to
//!   fit, `bank.example.attacker.test` would read `bank.example.at…`. Under
//!   it, the address as asked, which may be cut at its end. A domain that is
//!   not plain ASCII is said to be, since a letter from another alphabet can
//!   pass for a Latin one.
//! - **As whom**: the user name, when the program named one.
//! - With the vault locked, a field for its master password.
//!
//! What a program sent is shown as it sent it, except what a person could
//! not see or what would rearrange what they see -- controls, direction marks
//! and overrides, joiners and the other invisible characters -- which is
//! spelled out, `<U+202E>`, so what is shown is what was asked.
//!
//! # Not to be clicked through
//!
//! A window that opens while the user is typing or clicking elsewhere would
//! take the key or the click meant for something else. For [`ARMING`] after
//! it opens its buttons are drawn disabled, and every key and click but
//! Escape is ignored. And nothing is the default: Enter allows only from the
//! password field, with what the user typed there; anywhere else it presses
//! the button that has the keyboard, which starts as Refuse.

use std::fmt::Write as _;
use std::io;
use std::time::Duration;

use appearance::Palette;
use credentials::service::{Asking, Choice, Picked, Prompt, Said, Scope};
use guitk::button;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::field;
use guitk::frame::Rect;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::secretinput::SecretInput;
use guitk::text;
use guitk::textinput::KeyEdit;
use oswindow::app::{App, Response};

/// How long a window ignores everything but Escape after it opens.
pub const ARMING: Duration = Duration::from_millis(600);

/// How often a window not yet armed is woken, to arm itself.
const ARMING_TICK: Duration = Duration::from_millis(50);

/// The windows' width; their height is what their words need.
const WIDTH: f32 = 480.0;
const MARGIN: f32 = 20.0;
const HEADING: f32 = 16.0;
const BODY: f32 = 13.0;
const SMALL: f32 = 12.0;
const GAP: f32 = 12.0;
const FIELD_HEIGHT: f32 = 30.0;
const FIELD_PAD: f32 = 8.0;
const BUTTON_GAP: f32 = 8.0;
/// One login in the choosing window's list.
const ROW_HEIGHT: f32 = 40.0;
/// The most rows the list shows at once; more scroll.
const ROWS_SHOWN: usize = 6;

/// The window's title.
const TITLE: &str = "Password request";

/// What the program calls the windows' program.
const APP_ID: &str = "credentials";

// --- What is shown --------------------------------------------------------

/// Whether a person could not see `c`, or it would rearrange the characters
/// around it: a control, or a default-ignorable code point -- direction marks
/// and overrides, joiners, variation selectors, tags.
fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'..='\u{1160}'
                | '\u{17B4}'..='\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF0}'..='\u{FFF8}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
        )
}

/// `text` as the prompt shows it: each character a person could not see, or
/// that would rearrange the ones around it, spelled out as `<U+202E>`.
#[must_use]
pub fn shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_hidden(c) {
            // Writing to a String cannot fail.
            let _ = write!(out, "<U+{:04X}>", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// What a window says about the ask, worked out once.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Words {
    /// The program's name: its executable's file name.
    name: String,
    /// Its whole path and its process.
    whence: String,
    /// The domain asked for: shown whole.
    domain: String,
    /// The address as asked, where it says more than the domain.
    address: Option<String>,
    /// Whether the domain is not plain ASCII.
    foreign: bool,
    /// The user name the program named.
    username: Option<String>,
}

impl Words {
    fn of(asking: &Asking<'_>) -> Self {
        let exe = &asking.program.exe;
        let name = exe
            .file_name()
            .map_or_else(|| pathcodec::display_path(exe), pathcodec::display_os);
        let domain = match credentials::matching::domain(asking.target) {
            "" => asking.target,
            domain => domain,
        };
        Self {
            name: shown(&name),
            whence: format!(
                "{} \u{b7} process {}",
                shown(&pathcodec::display_path(exe)),
                asking.program.pid
            ),
            domain: shown(domain),
            address: (asking.target != domain).then(|| shown(asking.target)),
            foreign: !domain.is_ascii(),
            username: asking.username.map(shown),
        }
    }
}

/// Which colour a line is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Text,
    Subtle,
    Error,
}

/// One line of words, where it goes.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    text: String,
    y: f32,
    size: f32,
    weight: FontWeightHint,
    ink: Ink,
    /// Whether it may be cut at its end to fit: never so for anything the
    /// user decides by.
    cut: bool,
}

/// Lines laid out from the top, wrapped to a width.
struct Lines {
    lines: Vec<Line>,
    y: f32,
    width: f32,
}

impl Lines {
    fn new(width: f32) -> Self {
        Self {
            lines: Vec::new(),
            y: MARGIN,
            width: (width - 2.0 * MARGIN).max(1.0),
        }
    }

    /// `text`, wrapped -- broken inside a word where one is too long, so
    /// nothing is cut.
    fn wrapped(&mut self, text: &str, size: f32, weight: FontWeightHint, ink: Ink) {
        for piece in text::wrap_hard(text, self.width, size, weight) {
            self.one(piece, size, weight, ink, false);
        }
    }

    /// `text` on one line, cut at its end if it must be.
    fn cut(&mut self, text: &str, size: f32, ink: Ink) {
        self.one(text.to_owned(), size, FontWeightHint::Regular, ink, true);
    }

    fn one(&mut self, text: String, size: f32, weight: FontWeightHint, ink: Ink, cut: bool) {
        self.lines.push(Line {
            text,
            y: self.y,
            size,
            weight,
            ink,
            cut,
        });
        self.y += text::line_height(size, weight);
    }

    fn gap(&mut self, by: f32) {
        self.y += by;
    }

    /// Who is asking, and for what: the head of both windows.
    fn about(&mut self, heading: &str, words: &Words) {
        self.wrapped(heading, HEADING, FontWeightHint::Bold, Ink::Text);
        self.gap(2.0);
        self.wrapped(&words.whence, SMALL, FontWeightHint::Regular, Ink::Subtle);
        self.gap(GAP);
        self.wrapped("for", SMALL, FontWeightHint::Regular, Ink::Subtle);
        self.wrapped(&words.domain, BODY + 2.0, FontWeightHint::Bold, Ink::Text);
        if let Some(address) = &words.address {
            self.cut(address, SMALL, Ink::Subtle);
        }
        if words.foreign {
            self.gap(4.0);
            self.wrapped(
                "Careful: this name has letters that are not plain ASCII, and some \
                 look like others. Check that it is the one you mean.",
                SMALL,
                FontWeightHint::Bold,
                Ink::Text,
            );
        }
        if let Some(username) = &words.username {
            self.gap(4.0);
            self.wrapped(
                &format!("as {username}"),
                BODY,
                FontWeightHint::Regular,
                Ink::Text,
            );
        }
    }
}

/// Draw `line`, `width` wide from the margin.
fn draw_line(tree: &mut RenderTree, p: &Palette, line: &Line, width: f32) {
    let color = match line.ink {
        Ink::Text => p.text,
        Ink::Subtle => p.subtext0,
        Ink::Error => p.red,
    };
    tree.push(RenderCommand::Text {
        x: MARGIN,
        y: line.y,
        text: line.text.clone(),
        color,
        font_size: line.size,
        font_weight: line.weight,
        max_width: line.cut.then(|| (width - 2.0 * MARGIN).max(1.0)),
        overflow: if line.cut {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// `n` -- a handful of rows or buttons -- as a length to multiply.
fn count(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// A window's height in whole pixels, for the size it asks for.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window a few hundred pixels tall, rounded up; never negative"
)]
fn pixels(length: f32) -> u32 {
    length.max(1.0).ceil() as u32
}

/// A row of buttons, right-aligned at `y`: where each went.
fn button_row<B: Copy>(width: f32, y: f32, p: &Palette, buttons: &[(B, &str)]) -> Vec<(B, Rect)> {
    let style = &p.widget_style.button;
    let widths: Vec<f32> = buttons
        .iter()
        .map(|(_, label)| button::width(style, label))
        .collect();
    let gaps = BUTTON_GAP * count(buttons.len().saturating_sub(1));
    let mut x = width - MARGIN - widths.iter().sum::<f32>() - gaps;
    buttons
        .iter()
        .zip(widths)
        .map(|((which, _), w)| {
            let rect = Rect::new(x, y, w, button::HEIGHT);
            x += w + BUTTON_GAP;
            (*which, rect)
        })
        .collect()
}

/// How a window counts down to taking input.
#[derive(Clone, Copy, Debug)]
struct Arming {
    /// Milliseconds left.
    left: u64,
}

impl Arming {
    fn new() -> Self {
        Self {
            left: u64::try_from(ARMING.as_millis()).unwrap_or(u64::MAX),
        }
    }

    const fn armed(self) -> bool {
        self.left == 0
    }

    /// Let `elapsed_ms` pass: whether that armed it.
    fn pass(&mut self, elapsed_ms: u64) -> bool {
        let was = self.armed();
        self.left = self.left.saturating_sub(elapsed_ms);
        !was && self.armed()
    }

    fn interval(self) -> Option<Duration> {
        (!self.armed()).then_some(ARMING_TICK)
    }
}

// --- Asking ---------------------------------------------------------------

/// The ask window's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AskButton {
    Refuse,
    Once,
    UntilLocked,
}

impl AskButton {
    const ALL: [(Self, &'static str); 3] = [
        (Self::Refuse, "Refuse"),
        (Self::Once, "Allow once"),
        (Self::UntilLocked, "Allow until locked"),
    ];
}

/// What has the keyboard in the ask window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AskFocus {
    Field,
    On(AskButton),
}

/// The window that asks whether a program may have a password.
pub struct AskApp {
    words: Words,
    locked: bool,
    wrong: bool,
    field: SecretInput,
    focus: AskFocus,
    arming: Arming,
    hovered: Option<AskButton>,
    pressed: Option<AskButton>,
    said: Option<Said>,
    palette: Palette,
    focus_ring: f32,
    /// Where it last drew each button.
    drawn: Vec<(AskButton, Rect)>,
    /// Where it last drew its field.
    field_at: Option<Rect>,
}

impl std::fmt::Debug for AskApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskApp")
            .field("words", &self.words)
            .field("locked", &self.locked)
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

/// Where the ask window puts things, at a width.
struct AskLayout {
    lines: Vec<Line>,
    field: Option<Rect>,
    buttons_y: f32,
    height: f32,
}

impl AskApp {
    /// The window for `asking`.
    #[must_use]
    pub fn new(asking: &Asking<'_>) -> Self {
        Self {
            words: Words::of(asking),
            locked: asking.locked,
            wrong: asking.wrong,
            field: SecretInput::new(),
            focus: if asking.locked {
                AskFocus::Field
            } else {
                AskFocus::On(AskButton::Refuse)
            },
            arming: Arming::new(),
            hovered: None,
            pressed: None,
            said: None,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            focus_ring: guitk::style::FOCUS_RING_WIDTH,
            drawn: Vec::new(),
            field_at: None,
        }
    }

    /// What the user said, once they have.
    pub fn take_said(&mut self) -> Option<Said> {
        self.said.take()
    }

    fn layout(&self, width: f32) -> AskLayout {
        let mut lines = Lines::new(width);
        lines.about(
            &format!("{} asks for a password", self.words.name),
            &self.words,
        );
        let mut field = None;
        if self.locked {
            lines.gap(GAP);
            lines.wrapped(
                &format!(
                    "The password manager is locked. Type its master password \
                     to let {} have this password.",
                    self.words.name
                ),
                BODY,
                FontWeightHint::Regular,
                Ink::Text,
            );
            lines.gap(6.0);
            field = Some(Rect::new(MARGIN, lines.y, lines.width, FIELD_HEIGHT));
            lines.gap(FIELD_HEIGHT);
            if self.wrong {
                lines.gap(4.0);
                lines.wrapped(
                    "That is not the master password.",
                    SMALL,
                    FontWeightHint::Regular,
                    Ink::Error,
                );
            }
        }
        lines.gap(GAP * 1.5);
        let buttons_y = lines.y;
        AskLayout {
            height: buttons_y + button::HEIGHT + MARGIN,
            lines: lines.lines,
            field,
            buttons_y,
        }
    }

    /// The order the keyboard goes round in.
    fn focus_order(&self) -> Vec<AskFocus> {
        let mut order = Vec::with_capacity(4);
        if self.locked {
            order.push(AskFocus::Field);
        }
        order.extend(AskButton::ALL.iter().map(|(b, _)| AskFocus::On(*b)));
        order
    }

    fn move_focus(&mut self, back: bool) {
        let order = self.focus_order();
        let at = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let len = order.len();
        let next = if back {
            at.checked_sub(1).unwrap_or(len.saturating_sub(1))
        } else {
            at.saturating_add(1).checked_rem(len).unwrap_or(0)
        };
        if let Some(focus) = order.get(next) {
            self.focus = *focus;
        }
    }

    fn answer(&mut self, said: Said) -> Response {
        self.said = Some(said);
        self.field.clear();
        Response::Exit
    }

    fn press(&mut self, which: AskButton) -> Response {
        let scope = match which {
            AskButton::Refuse => return self.answer(Said::Refuse),
            AskButton::Once => Scope::Once,
            AskButton::UntilLocked => Scope::UntilLocked,
        };
        if !self.locked {
            return self.answer(Said::Allow {
                scope,
                master: None,
            });
        }
        if self.field.is_empty() {
            // Nothing to open the vault with: the keyboard goes where it is
            // typed.
            self.focus = AskFocus::Field;
            return Response::Redraw;
        }
        let master = self.field.take();
        self.answer(Said::Allow {
            scope,
            master: Some(master),
        })
    }

    fn key(&mut self, key: &KeyEvent) -> Response {
        if key.key == Key::Escape {
            return self.answer(Said::Refuse);
        }
        if !self.arming.armed() {
            return Response::Idle;
        }
        match (key.key, self.focus) {
            (Key::Tab, _) => {
                self.move_focus(key.modifiers.shift);
                Response::Redraw
            }
            (Key::Enter, AskFocus::Field) => {
                if self.field.is_empty() {
                    Response::Idle
                } else {
                    self.press(AskButton::Once)
                }
            }
            (Key::Enter | Key::Space, AskFocus::On(which)) => self.press(which),
            (Key::Left | Key::Up, AskFocus::On(_)) => {
                self.move_focus(true);
                Response::Redraw
            }
            (Key::Right | Key::Down, AskFocus::On(_)) => {
                self.move_focus(false);
                Response::Redraw
            }
            (_, AskFocus::Field) => match self.field.edit_key(key) {
                KeyEdit::Unhandled => Response::Idle,
                KeyEdit::Handled | KeyEdit::Changed => Response::Redraw,
            },
            _ => Response::Idle,
        }
    }

    fn mouse(&mut self, mouse: &MouseEvent) -> Response {
        let over = self
            .drawn
            .iter()
            .find(|(_, rect)| rect.contains(mouse.x, mouse.y))
            .map(|(which, _)| *which);
        match mouse.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                if over == self.hovered {
                    Response::Idle
                } else {
                    self.hovered = over;
                    Response::Redraw
                }
            }
            MouseEventKind::Leave => {
                self.pressed = None;
                if self.hovered.take().is_some() {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            MouseEventKind::Press(MouseButton::Left) if self.arming.armed() => {
                if over.is_some() {
                    self.pressed = over;
                    return Response::Redraw;
                }
                if self
                    .field_at
                    .is_some_and(|rect| rect.contains(mouse.x, mouse.y))
                {
                    self.focus = AskFocus::Field;
                    return Response::Redraw;
                }
                Response::Idle
            }
            MouseEventKind::Release(MouseButton::Left) => match self.pressed.take() {
                // Released where it was pressed: a click.
                Some(which) if over == Some(which) && self.arming.armed() => self.press(which),
                Some(_) => Response::Redraw,
                None => Response::Idle,
            },
            _ => Response::Idle,
        }
    }
}

impl App for AskApp {
    fn title(&self) -> String {
        TITLE.to_owned()
    }

    fn app_id(&self) -> String {
        APP_ID.to_owned()
    }

    fn initial_size(&self) -> (u32, u32) {
        (pixels(WIDTH), pixels(self.layout(WIDTH).height))
    }

    fn resizable(&self) -> bool {
        false
    }

    fn tick_interval(&self) -> Option<Duration> {
        self.arming.interval()
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::Tick { elapsed_ms } => {
                if self.arming.pass(*elapsed_ms) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            Event::CloseRequested => self.answer(Said::Refuse),
            Event::Key(key) if key.pressed => self.key(key),
            Event::Mouse(mouse) => self.mouse(mouse),
            _ => Response::Idle,
        }
    }

    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn appearance_changed(&mut self, settings: &appearance::AppearanceSettings) {
        self.focus_ring = settings.focus_ring_width();
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        let laid = self.layout(width);
        let p = self.palette;
        let armed = self.arming.armed();
        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, width, height, p.base);
        for line in &laid.lines {
            draw_line(&mut tree, &p, line, width);
        }
        self.field_at = laid.field;
        if let Some(rect) = laid.field {
            let focused = self.focus == AskFocus::Field;
            let state = field::State {
                hovered: false,
                focused,
                disabled: !armed,
                invalid: self.wrong && self.field.is_empty(),
            };
            field::draw(&mut tree, &p, rect, state, self.focus_ring);
            let text_y = rect.y + (rect.h - BODY) / 2.0;
            let dots = self.field.dots();
            if self.field.is_all_selected() {
                let w = text::measure(&dots, BODY, FontWeightHint::Regular);
                tree.fill_rect(rect.x + FIELD_PAD, text_y, w, BODY + 2.0, p.surface2);
            }
            tree.push(RenderCommand::Text {
                x: rect.x + FIELD_PAD,
                y: text_y,
                text: dots,
                color: p.text,
                font_size: BODY,
                font_weight: FontWeightHint::Regular,
                max_width: Some((rect.w - 2.0 * FIELD_PAD).max(1.0)),
                overflow: TextOverflow::Clip,
            });
            if focused && armed {
                let before: String =
                    std::iter::repeat_n(guitk::secretinput::DOT, self.field.caret()).collect();
                let x = rect.x
                    + FIELD_PAD
                    + text::measure(&before, BODY, FontWeightHint::Regular)
                        .min(rect.w - 2.0 * FIELD_PAD);
                tree.fill_rect(
                    x,
                    text_y - 1.0,
                    guitk::textedit::CARET_WIDTH,
                    BODY + 4.0,
                    p.text,
                );
            }
        }
        self.drawn = button_row(width, laid.buttons_y, &p, &AskButton::ALL);
        for ((which, rect), (_, label)) in self.drawn.iter().zip(AskButton::ALL) {
            let state = button::State {
                hovered: armed && self.hovered == Some(*which),
                pressed: armed && self.pressed == Some(*which),
                disabled: !armed,
                focused: self.focus == AskFocus::On(*which),
            };
            button::draw(
                &mut tree,
                &p,
                (rect.x, rect.y, rect.w, rect.h),
                label,
                button::Kind::Plain,
                state,
                p.base,
                self.focus_ring,
            );
        }
        tree
    }
}

// --- Choosing -------------------------------------------------------------

/// The choosing window's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChooseButton {
    Nothing,
    Give,
}

impl ChooseButton {
    const ALL: [(Self, &'static str); 2] =
        [(Self::Nothing, "Give none"), (Self::Give, "Give this one")];
}

/// The window that asks which of several logins a program may have.
pub struct ChooseApp {
    words: Words,
    /// Each login offered: what it is for, and its user name.
    choices: Vec<(String, String)>,
    /// The row picked, if any yet.
    selected: Option<usize>,
    /// The first row shown.
    scrolled: usize,
    arming: Arming,
    hovered: Option<ChooseButton>,
    pressed: Option<ChooseButton>,
    picked: Option<Picked>,
    palette: Palette,
    focus_ring: f32,
    drawn: Vec<(ChooseButton, Rect)>,
    /// Where it last drew the list.
    list_at: Option<Rect>,
}

impl std::fmt::Debug for ChooseApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChooseApp")
            .field("words", &self.words)
            .field("choices", &self.choices)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

/// Where the choosing window puts things, at a width.
struct ChooseLayout {
    lines: Vec<Line>,
    list: Rect,
    buttons_y: f32,
    height: f32,
}

impl ChooseApp {
    /// The window for choosing among `choices` for `asking`.
    #[must_use]
    pub fn new(asking: &Asking<'_>, choices: &[Choice<'_>]) -> Self {
        Self {
            words: Words::of(asking),
            choices: choices
                .iter()
                .map(|c| (shown(c.target), shown(c.username)))
                .collect(),
            selected: None,
            scrolled: 0,
            arming: Arming::new(),
            hovered: None,
            pressed: None,
            picked: None,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            focus_ring: guitk::style::FOCUS_RING_WIDTH,
            drawn: Vec::new(),
            list_at: None,
        }
    }

    /// What the user picked, once they have.
    pub fn take_picked(&mut self) -> Option<Picked> {
        self.picked.take()
    }

    fn rows_shown(&self) -> usize {
        self.choices.len().clamp(1, ROWS_SHOWN)
    }

    fn layout(&self, width: f32) -> ChooseLayout {
        let mut lines = Lines::new(width);
        lines.about(
            &format!("Which password may {} have?", self.words.name),
            &self.words,
        );
        lines.gap(GAP);
        lines.wrapped(
            "More than one of your saved logins is for this. Pick the one to give it.",
            BODY,
            FontWeightHint::Regular,
            Ink::Text,
        );
        lines.gap(6.0);
        let list_height = ROW_HEIGHT * count(self.rows_shown());
        let list = Rect::new(MARGIN, lines.y, lines.width, list_height);
        lines.gap(list_height + GAP * 1.5);
        let buttons_y = lines.y;
        ChooseLayout {
            height: buttons_y + button::HEIGHT + MARGIN,
            lines: lines.lines,
            list,
            buttons_y,
        }
    }

    fn answer(&mut self, picked: Picked) -> Response {
        self.picked = Some(picked);
        Response::Exit
    }

    fn press(&mut self, which: ChooseButton) -> Response {
        match (which, self.selected) {
            (ChooseButton::Nothing, _) => self.answer(Picked::Nothing),
            (ChooseButton::Give, Some(at)) => self.answer(Picked::Login(at)),
            (ChooseButton::Give, None) => Response::Idle,
        }
    }

    /// Select row `at`, scrolling it into view.
    fn select(&mut self, at: usize) -> Response {
        if at >= self.choices.len() {
            return Response::Idle;
        }
        self.selected = Some(at);
        let shown = self.rows_shown();
        if at < self.scrolled {
            self.scrolled = at;
        } else if at >= self.scrolled.saturating_add(shown) {
            self.scrolled = at.saturating_add(1).saturating_sub(shown);
        }
        Response::Redraw
    }

    fn key(&mut self, key: &KeyEvent) -> Response {
        if key.key == Key::Escape {
            return self.answer(Picked::Nothing);
        }
        if !self.arming.armed() {
            return Response::Idle;
        }
        let last = self.choices.len().saturating_sub(1);
        match key.key {
            Key::Down => self.select(self.selected.map_or(0, |at| at.saturating_add(1).min(last))),
            Key::Up => self.select(self.selected.map_or(0, |at| at.saturating_sub(1))),
            Key::Home => self.select(0),
            Key::End => self.select(last),
            Key::Enter => self.press(ChooseButton::Give),
            _ => Response::Idle,
        }
    }

    /// The row under `(x, y)`, if any.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        let list = self.list_at?;
        if !list.contains(x, y) {
            return None;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative offset inside the list, over a row's height"
        )]
        let row = ((y - list.y) / ROW_HEIGHT) as usize;
        let at = self.scrolled.saturating_add(row);
        (at < self.choices.len()).then_some(at)
    }

    fn mouse(&mut self, mouse: &MouseEvent) -> Response {
        let over = self
            .drawn
            .iter()
            .find(|(_, rect)| rect.contains(mouse.x, mouse.y))
            .map(|(which, _)| *which);
        match mouse.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                if over == self.hovered {
                    Response::Idle
                } else {
                    self.hovered = over;
                    Response::Redraw
                }
            }
            MouseEventKind::Leave => {
                self.pressed = None;
                if self.hovered.take().is_some() {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            MouseEventKind::Press(MouseButton::Left) if self.arming.armed() => {
                if over.is_some() {
                    self.pressed = over;
                    return Response::Redraw;
                }
                match self.row_at(mouse.x, mouse.y) {
                    Some(at) => self.select(at),
                    None => Response::Idle,
                }
            }
            MouseEventKind::Release(MouseButton::Left) => match self.pressed.take() {
                Some(which) if over == Some(which) && self.arming.armed() => self.press(which),
                Some(_) => Response::Redraw,
                None => Response::Idle,
            },
            // Notches, positive away from the user: up the list.
            MouseEventKind::Scroll { dy, .. } => {
                let most = self.choices.len().saturating_sub(self.rows_shown());
                let before = self.scrolled;
                self.scrolled = if dy > 0.0 {
                    self.scrolled.saturating_sub(1)
                } else if dy < 0.0 {
                    self.scrolled.saturating_add(1).min(most)
                } else {
                    self.scrolled
                };
                if self.scrolled == before {
                    Response::Idle
                } else {
                    Response::Redraw
                }
            }
            _ => Response::Idle,
        }
    }
}

impl App for ChooseApp {
    fn title(&self) -> String {
        TITLE.to_owned()
    }

    fn app_id(&self) -> String {
        APP_ID.to_owned()
    }

    fn initial_size(&self) -> (u32, u32) {
        (pixels(WIDTH), pixels(self.layout(WIDTH).height))
    }

    fn resizable(&self) -> bool {
        false
    }

    fn tick_interval(&self) -> Option<Duration> {
        self.arming.interval()
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::Tick { elapsed_ms } => {
                if self.arming.pass(*elapsed_ms) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            Event::CloseRequested => self.answer(Picked::Nothing),
            Event::Key(key) if key.pressed => self.key(key),
            Event::Mouse(mouse) => self.mouse(mouse),
            _ => Response::Idle,
        }
    }

    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn appearance_changed(&mut self, settings: &appearance::AppearanceSettings) {
        self.focus_ring = settings.focus_ring_width();
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        let laid = self.layout(width);
        let p = self.palette;
        let armed = self.arming.armed();
        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, width, height, p.base);
        for line in &laid.lines {
            draw_line(&mut tree, &p, line, width);
        }
        let list = laid.list;
        self.list_at = Some(list);
        field::draw(
            &mut tree,
            &p,
            list,
            field::State {
                disabled: !armed,
                ..field::State::default()
            },
            self.focus_ring,
        );
        tree.clip(list.x, list.y, list.w, list.h);
        let rows = self.choices.iter().enumerate().skip(self.scrolled);
        for (shown_at, (at, (target, username))) in rows.take(self.rows_shown()).enumerate() {
            let y = list.y + ROW_HEIGHT * count(shown_at);
            if self.selected == Some(at) {
                tree.fill_rect(list.x, y, list.w, ROW_HEIGHT, p.surface1);
            }
            let room = Some((list.w - 2.0 * FIELD_PAD).max(1.0));
            tree.push(RenderCommand::Text {
                x: list.x + FIELD_PAD,
                y: y + 4.0,
                text: username.clone(),
                color: p.text,
                font_size: BODY,
                font_weight: FontWeightHint::Bold,
                max_width: room,
                overflow: TextOverflow::Ellipsis,
            });
            tree.push(RenderCommand::Text {
                x: list.x + FIELD_PAD,
                y: y + 4.0 + text::line_height(BODY, FontWeightHint::Bold),
                text: target.clone(),
                color: p.subtext0,
                font_size: SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: room,
                overflow: TextOverflow::Ellipsis,
            });
        }
        tree.unclip();
        self.drawn = button_row(width, laid.buttons_y, &p, &ChooseButton::ALL);
        for ((which, rect), (_, label)) in self.drawn.iter().zip(ChooseButton::ALL) {
            let usable = armed && (*which == ChooseButton::Nothing || self.selected.is_some());
            let state = button::State {
                hovered: usable && self.hovered == Some(*which),
                pressed: usable && self.pressed == Some(*which),
                disabled: !usable,
                focused: false,
            };
            button::draw(
                &mut tree,
                &p,
                (rect.x, rect.y, rect.w, rect.h),
                label,
                button::Kind::Plain,
                state,
                p.base,
                self.focus_ring,
            );
        }
        tree
    }
}

// --- On the display -------------------------------------------------------

/// The service's prompt on the user's display.
#[derive(Debug, Default)]
pub struct WindowPrompt {
    /// Why the last window could not be shown, for the service's log.
    failure: Option<io::Error>,
}

impl WindowPrompt {
    /// A prompt on the display the environment names.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Why the last question could not be put, if it could not: taken.
    pub fn take_failure(&mut self) -> Option<io::Error> {
        self.failure.take()
    }
}

/// Open `app`'s window on the user's display and run it until it is done.
fn show(app: &mut impl App) -> io::Result<()> {
    let link = oswindow::connect()?;
    let mut events = oswindow::EventLoop::new(link);
    let window =
        oswindow::app::open(&mut events, app).map_err(|e| io::Error::other(e.to_string()))?;
    oswindow::app::drive(&mut events, window, app).map_err(|e| io::Error::other(e.to_string()))
}

impl Prompt for WindowPrompt {
    fn ask(&mut self, asking: &Asking<'_>) -> Said {
        let mut app = AskApp::new(asking);
        match show(&mut app) {
            Ok(()) => app.take_said().unwrap_or(Said::CouldNotAsk),
            Err(e) => {
                self.failure = Some(e);
                Said::CouldNotAsk
            }
        }
    }

    fn choose(&mut self, asking: &Asking<'_>, choices: &[Choice<'_>]) -> Picked {
        let mut app = ChooseApp::new(asking, choices);
        match show(&mut app) {
            Ok(()) => app.take_picked().unwrap_or(Picked::CouldNotAsk),
            Err(e) => {
                self.failure = Some(e);
                Picked::CouldNotAsk
            }
        }
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
