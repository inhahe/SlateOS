//! The Window Rules page's model: a rule as the page edits it -- its numbers
//! still as typed -- the choices each of the editor's rows offers, and a rule
//! said in words for the list.
//!
//! The rules themselves, the file they are kept in and the engine that
//! applies them are lane C's `windowrules` crate
//! (`requests/c-e-a-window-rules-page-in-settings.md`): the page reads the
//! rules with `windowrules::file::load`, writes them with
//! `windowrules::file::store`, and the desktop takes the change from the
//! file. What is here is only what an editor needs on top of the model:
//!
//! * [`RuleDraft`] -- a rule being written. Its name, what it matches and its
//!   numbers stay text until Save, because a half-typed "8" on the way to
//!   "80" is not yet the number meant, and a field that refused every
//!   keystroke that did not leave a valid one could not be typed in.
//!   [`RuleDraft::rule`] turns the draft into a rule, or says in a sentence
//!   why it cannot be one yet -- the reason the page's Save button gives.
//! * The choices the editor's lists offer, each with the words the page
//!   shows: [`MatchKind`], [`Place`], [`Size`], [`Tri`] for each [`Flag`], the
//!   states, the desktops and the opacities.
//! * [`describe_criteria`] and [`describe_actions`]: a rule in a line.
//!
//! # What the editor keeps but does not offer
//!
//! A rule written by hand may say three things the editor has no list for.
//! Each is kept as the file has it, shown, and written back unchanged:
//!
//! * `monitor` and `decorations`, which the desktop does not carry out yet
//!   (`known-issues` `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`):
//!   a list offering them would be one whose every choice did the same
//!   nothing;
//! * `snap`, which the desktop does carry out, but whose zones are named only
//!   in the window protocol's crate, which Settings does not link
//!   (`requests/e-f-settings-could-name-the-snap-zones.md`). The page can
//!   take it off a rule, which needs no names.

use guitk::textinput::TextInput;
use windowrules::{InitialState, MatchCriteria, PositionSpec, RuleActions, SizeSpec, WindowRule};

/// The longest name a rule may have, in characters. The name is what the
/// list shows a rule by and the key it is written under; one longer than a
/// line of the page is a description, not a name.
pub const NAME_CAPACITY: usize = 80;

/// The longest program, title or part of a title a rule matches by.
pub const MATCH_CAPACITY: usize = 256;

/// The longest pair of numbers typed into one of the editor's fields.
pub const NUMBERS_CAPACITY: usize = 24;

/// How many virtual desktops there are.
///
/// A second copy of the desktop shell's `num_desktops`, a literal 4 in
/// `DesktopShell::new`; the shell drops a rule's desktop past it. Asked of
/// lane C as a constant both can read
/// (`requests/e-c-a-window-rule-the-file-cannot-read-is-deleted-by-the-next-save.md`,
/// "one small thing").
pub const DESKTOPS: u32 = 4;

/// The opacities the list offers, solid first. Not below a fifth: a window
/// drawn fainter than that is one a user would take for gone.
const OPACITIES: [f32; 9] = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2];

/// What every list of the editor calls "the rule says nothing about it".
pub const AS_USUAL: &str = "As usual";

// ============================================================================
// What a rule matches
// ============================================================================

/// How a rule picks its windows: [`MatchCriteria`]'s four ways, as the page
/// names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    /// The program the window belongs to: `app`.
    Program,
    /// The window's whole title: `title`.
    Title,
    /// Part of its title, in any case: `title-contains`.
    TitleContains,
    /// Every window: `any`.
    Any,
}

impl MatchKind {
    /// Every way, in the order the list offers them.
    pub const ALL: [Self; 4] = [Self::Program, Self::Title, Self::TitleContains, Self::Any];

    /// The list's words for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Program => "A program's windows",
            Self::Title => "A window by its title",
            Self::TitleContains => "Windows with words in their title",
            Self::Any => "Every window",
        }
    }

    /// The label of the field that says which, or `None` for every window,
    /// which has no field.
    #[must_use]
    pub fn field_label(self) -> Option<&'static str> {
        match self {
            Self::Program => Some("Program"),
            Self::Title => Some("Whole title"),
            Self::TitleContains => Some("Words"),
            Self::Any => None,
        }
    }

    /// What the field shows while it is empty.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Program => "terminal",
            Self::Title => "The title, exactly",
            Self::TitleContains => "Part of the title",
            Self::Any => "",
        }
    }

    /// The way `criteria` matches, and the text it matches.
    fn of(criteria: &MatchCriteria) -> (Self, &str) {
        match criteria {
            MatchCriteria::AppId(text) => (Self::Program, text),
            MatchCriteria::TitleExact(text) => (Self::Title, text),
            MatchCriteria::TitleContains(text) => (Self::TitleContains, text),
            MatchCriteria::Any => (Self::Any, ""),
        }
    }

    /// Why a draft matching this way with no text cannot be saved.
    fn missing(self) -> &'static str {
        match self {
            Self::Program => "Say which program's windows the rule is for.",
            Self::Title => "Say which title the rule is for.",
            Self::TitleContains | Self::Any => "Say which words in the title the rule is for.",
        }
    }
}

// ============================================================================
// Where and how big
// ============================================================================

/// Where a window opens: one of [`PositionSpec`]'s, or nothing said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Wherever it would have: the rule says nothing.
    AsUsual,
    /// Where it was last.
    Remember,
    /// In the middle of the screen.
    Centre,
    /// At a point, in pixels from the screen's left and top: the field's two
    /// numbers.
    At,
    /// A part of the way across and down: the field's two percentages.
    Part,
}

impl Place {
    /// Every choice, in the order the list offers them.
    pub const ALL: [Self; 5] = [
        Self::AsUsual,
        Self::Remember,
        Self::Centre,
        Self::At,
        Self::Part,
    ];

    /// The list's words for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::AsUsual => AS_USUAL,
            Self::Remember => "Where it was last",
            Self::Centre => "In the middle",
            Self::At => "At a point",
            Self::Part => "A part of the way across",
        }
    }

    /// The label of the field its numbers are typed in, if it has numbers.
    #[must_use]
    pub fn field_label(self) -> Option<&'static str> {
        match self {
            Self::At => Some("From the left and top"),
            Self::Part => Some("Across and down"),
            Self::AsUsual | Self::Remember | Self::Centre => None,
        }
    }

    /// What its field shows while it is empty.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::At => "100 80",
            Self::Part => "50% 25%",
            Self::AsUsual | Self::Remember | Self::Centre => "",
        }
    }
}

/// How big a window opens: one of [`SizeSpec`]'s, or nothing said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// As big as it would have been: the rule says nothing.
    AsUsual,
    /// As big as it was last.
    Remember,
    /// Exactly the field's width and height, in pixels.
    Exactly,
    /// The field's parts of the screen's width and height.
    Part,
}

impl Size {
    /// Every choice, in the order the list offers them.
    pub const ALL: [Self; 4] = [Self::AsUsual, Self::Remember, Self::Exactly, Self::Part];

    /// The list's words for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::AsUsual => AS_USUAL,
            Self::Remember => "As it was last",
            Self::Exactly => "Exactly",
            Self::Part => "A part of the screen",
        }
    }

    /// The label of the field its numbers are typed in, if it has numbers.
    #[must_use]
    pub fn field_label(self) -> Option<&'static str> {
        match self {
            Self::Exactly | Self::Part => Some("Width and height"),
            Self::AsUsual | Self::Remember => None,
        }
    }

    /// What its field shows while it is empty.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Exactly => "800 600",
            Self::Part => "50% 75%",
            Self::AsUsual | Self::Remember => "",
        }
    }
}

// ============================================================================
// The yes-or-no things
// ============================================================================

/// A yes or no that a rule may also say nothing about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    /// The rule says nothing.
    AsUsual,
    /// Yes.
    Yes,
    /// No.
    No,
}

impl Tri {
    /// Every choice, in the order the list offers them.
    pub const ALL: [Self; 3] = [Self::AsUsual, Self::Yes, Self::No];

    /// The list's words for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::AsUsual => AS_USUAL,
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }

    /// The choice that says `value`.
    #[must_use]
    pub fn of(value: Option<bool>) -> Self {
        match value {
            None => Self::AsUsual,
            Some(true) => Self::Yes,
            Some(false) => Self::No,
        }
    }

    /// What it says.
    #[must_use]
    pub fn value(self) -> Option<bool> {
        match self {
            Self::AsUsual => None,
            Self::Yes => Some(true),
            Self::No => Some(false),
        }
    }
}

/// The yes-or-no things a rule can say about a window, each in the sense its
/// row asks it: "Taskbar button -- Yes", where the file says `taskbar: true`
/// and the model `skip_taskbar: Some(false)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// Kept above other windows: `on-top`.
    OnTop,
    /// Kept below other windows: `on-bottom`.
    Below,
    /// Has a taskbar button: `taskbar`.
    Taskbar,
    /// Is offered by Alt+Tab: `alt-tab`.
    AltTab,
    /// Minimised, goes to the system tray: `tray`.
    Tray,
    /// The user can close it: `can-close`.
    CanClose,
    /// The user can move it: `can-move`.
    CanMove,
    /// The user can resize it: `can-resize`.
    CanResize,
}

impl Flag {
    /// Every one, in the order the editor lists them.
    pub const ALL: [Self; 8] = [
        Self::OnTop,
        Self::Below,
        Self::Taskbar,
        Self::AltTab,
        Self::Tray,
        Self::CanClose,
        Self::CanMove,
        Self::CanResize,
    ];

    /// Its row's label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OnTop => "Kept on top",
            Self::Below => "Kept below other windows",
            Self::Taskbar => "Taskbar button",
            Self::AltTab => "In Alt+Tab",
            Self::Tray => "Minimises to the tray",
            Self::CanClose => "Can be closed",
            Self::CanMove => "Can be moved",
            Self::CanResize => "Can be resized",
        }
    }

    /// What `actions` say of it, in its row's sense.
    #[must_use]
    pub fn get(self, actions: &RuleActions) -> Option<bool> {
        match self {
            Self::OnTop => actions.always_on_top,
            Self::Below => actions.always_on_bottom,
            Self::Taskbar => actions.skip_taskbar.map(|skip| !skip),
            Self::AltTab => actions.skip_alt_tab.map(|skip| !skip),
            Self::Tray => actions.to_tray,
            Self::CanClose => actions.prevent_close.map(|prevent| !prevent),
            Self::CanMove => actions.prevent_move.map(|prevent| !prevent),
            Self::CanResize => actions.prevent_resize.map(|prevent| !prevent),
        }
    }

    /// Make `actions` say `value` of it, in its row's sense.
    pub fn set(self, actions: &mut RuleActions, value: Option<bool>) {
        let inverted = value.map(|v| !v);
        match self {
            Self::OnTop => actions.always_on_top = value,
            Self::Below => actions.always_on_bottom = value,
            Self::Taskbar => actions.skip_taskbar = inverted,
            Self::AltTab => actions.skip_alt_tab = inverted,
            Self::Tray => actions.to_tray = value,
            Self::CanClose => actions.prevent_close = inverted,
            Self::CanMove => actions.prevent_move = inverted,
            Self::CanResize => actions.prevent_resize = inverted,
        }
    }

    /// It, said of a rule that says `value`: for [`describe_actions`].
    fn said(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::OnTop, true) => "kept on top",
            (Self::OnTop, false) => "not kept on top",
            (Self::Below, true) => "kept below other windows",
            (Self::Below, false) => "not kept below other windows",
            (Self::Taskbar, true) => "a taskbar button",
            (Self::Taskbar, false) => "no taskbar button",
            (Self::AltTab, true) => "in Alt+Tab",
            (Self::AltTab, false) => "not in Alt+Tab",
            (Self::Tray, true) => "minimises to the tray",
            (Self::Tray, false) => "minimises to the taskbar",
            (Self::CanClose, true) => "can be closed",
            (Self::CanClose, false) => "cannot be closed",
            (Self::CanMove, true) => "can be moved",
            (Self::CanMove, false) => "cannot be moved",
            (Self::CanResize, true) => "can be resized",
            (Self::CanResize, false) => "cannot be resized",
        }
    }
}

// ============================================================================
// The lists of values
// ============================================================================

/// How a window opens, in the order the list offers: as it would, or one of
/// [`InitialState`]'s four.
pub const STATES: [Option<InitialState>; 5] = [
    None,
    Some(InitialState::Normal),
    Some(InitialState::Minimized),
    Some(InitialState::Maximized),
    Some(InitialState::Fullscreen),
];

/// The list's words for `state`.
#[must_use]
pub fn state_label(state: Option<InitialState>) -> &'static str {
    match state {
        None => AS_USUAL,
        Some(InitialState::Normal) => "Neither minimised nor maximised",
        Some(InitialState::Minimized) => "Minimised",
        Some(InitialState::Maximized) => "Maximised",
        Some(InitialState::Fullscreen) => "Full screen",
    }
}

/// The desktops the list offers: as it would, each of the [`DESKTOPS`] --
/// and `current`, if a rule names one past them, so that the list shows what
/// the rule says rather than a desktop it does not.
#[must_use]
pub fn desktop_choices(current: Option<u32>) -> Vec<Option<u32>> {
    let mut choices: Vec<Option<u32>> = std::iter::once(None)
        .chain((0..DESKTOPS).map(Some))
        .collect();
    if let Some(desktop) = current
        && !choices.contains(&Some(desktop))
    {
        choices.push(Some(desktop));
    }
    choices
}

/// The list's words for `desktop`, counted from one as people count them.
#[must_use]
pub fn desktop_label(desktop: Option<u32>) -> String {
    match desktop {
        None => AS_USUAL.to_owned(),
        Some(d) if d < DESKTOPS => format!("Desktop {}", d.saturating_add(1)),
        Some(d) => format!("Desktop {} -- there are {DESKTOPS}", d.saturating_add(1)),
    }
}

/// The opacities the list offers: as it would, then solid down to a fifth --
/// and `current` in its place among them, if a rule says one between, so that
/// opening the list never changes what a rule says.
#[must_use]
pub fn opacity_choices(current: Option<f32>) -> Vec<Option<f32>> {
    let mut choices: Vec<Option<f32>> = std::iter::once(None)
        .chain(OPACITIES.iter().copied().map(Some))
        .collect();
    if let Some(opacity) = current
        && !OPACITIES.iter().any(|step| same_opacity(*step, opacity))
    {
        let at = choices
            .iter()
            .position(|choice| choice.is_some_and(|step| step < opacity))
            .unwrap_or(choices.len());
        choices.insert(at, Some(opacity));
    }
    choices
}

/// Whether two opacities are the one the list shows: within a twentieth of a
/// percent, finer than a percentage shows.
#[must_use]
pub fn same_opacity(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.0005
}

/// The list's words for `opacity`.
#[must_use]
pub fn opacity_label(opacity: Option<f32>) -> String {
    match opacity {
        None => AS_USUAL.to_owned(),
        Some(o) if same_opacity(o, 1.0) => "Solid".to_owned(),
        Some(o) => format!("{}%", shown_percent(o)),
    }
}

/// `fraction` as a percentage, as a person would write it: `50`, `33.3`.
///
/// To a tenth of a percent: a part of a screen finer than that is a part of
/// a pixel on any screen there is.
#[must_use]
pub fn shown_percent(fraction: f32) -> String {
    let shown = format!("{:.1}", fraction * 100.0);
    match shown.strip_suffix(".0") {
        Some(whole) => whole.to_owned(),
        None => shown,
    }
}

// ============================================================================
// A rule being written
// ============================================================================

/// A rule as the editor holds it while it is written.
///
/// The fields a person types into are [`TextInput`]s, read only by
/// [`RuleDraft::rule`]; what the lists choose is written straight into
/// [`RuleDraft::actions`], which holds every action -- the ones the editor
/// does not offer included, so that they are written back as they were.
#[derive(Clone, Debug)]
pub struct RuleDraft {
    /// The rule being changed, by its place in the list; `None` for a new
    /// rule.
    pub editing: Option<usize>,
    /// Its name.
    pub name: TextInput,
    /// How it picks its windows.
    pub matching: MatchKind,
    /// The program, title or words it picks them by.
    pub match_text: TextInput,
    /// Where its windows open.
    pub place: Place,
    /// The monitor "In the middle" means, counted from nought: kept from a
    /// rule that names one (`centre 2`), since the list offers only the
    /// first.
    pub centre_monitor: u32,
    /// The numbers [`Place::At`] and [`Place::Part`] are given in.
    pub place_text: TextInput,
    /// How big they open.
    pub size: Size,
    /// The numbers [`Size::Exactly`] and [`Size::Part`] are given in.
    pub size_text: TextInput,
    /// No smaller than this: width and height, or nothing.
    pub min_size: TextInput,
    /// No larger than this.
    pub max_size: TextInput,
    /// Everything else it does, set by the editor's lists as they are
    /// chosen. Its `position`, `size`, `min_size` and `max_size` are the
    /// fields' above, which [`RuleDraft::rule`] writes over these.
    pub actions: RuleActions,
    /// Whether it is applied.
    pub enabled: bool,
    /// Whether it is applied to the first window it matches only.
    pub once: bool,
}

impl Default for RuleDraft {
    fn default() -> Self {
        Self::new()
    }
}

/// A field holding `text`.
fn field(text: &str) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(text);
    input
}

/// Two numbers `text` gives as a pair -- `800 600`, `800, 600`, `800x600` --
/// each read by `read`; `None` unless there are exactly two and both read.
fn two<T>(text: &str, read: impl Fn(&str) -> Option<T>) -> Option<(T, T)> {
    let mut words = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | 'x' | 'X' | '\u{d7}'))
        .filter(|word| !word.is_empty());
    let (first, second) = (words.next()?, words.next()?);
    if words.next().is_some() {
        return None;
    }
    Some((read(first)?, read(second)?))
}

/// A percentage, `%` or not, from nought to a hundred, as a fraction.
fn percent(word: &str) -> Option<f32> {
    let number: f32 = word.strip_suffix('%').unwrap_or(word).parse().ok()?;
    (0.0..=100.0).contains(&number).then_some(number / 100.0)
}

/// A percentage above nought: a part of the screen a window can be.
fn part(word: &str) -> Option<f32> {
    percent(word).filter(|fraction| *fraction > 0.0)
}

/// A whole number of pixels above nought: a size a window can be.
fn pixels(word: &str) -> Option<u32> {
    word.parse().ok().filter(|n| *n > 0)
}

/// A width and height typed into `input`: `None` for an empty field, the pair
/// for one that reads, and `refused` for one that does not.
fn optional_size(input: &TextInput, refused: &str) -> Result<Option<(u32, u32)>, String> {
    let text = input.text().trim();
    if text.is_empty() {
        return Ok(None);
    }
    two(text, pixels)
        .map(Some)
        .ok_or_else(|| refused.to_owned())
}

/// `pair` as the editor shows it: `800 600`, or nothing.
fn shown_pair(pair: Option<(u32, u32)>) -> String {
    pair.map_or_else(String::new, |(w, h)| format!("{w} {h}"))
}

impl RuleDraft {
    /// A new rule: for a program's windows, doing nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            editing: None,
            name: field(""),
            matching: MatchKind::Program,
            match_text: field(""),
            place: Place::AsUsual,
            centre_monitor: 0,
            place_text: field(""),
            size: Size::AsUsual,
            size_text: field(""),
            min_size: field(""),
            max_size: field(""),
            actions: RuleActions::new(),
            enabled: true,
            once: false,
        }
    }

    /// `rule`, the `index`-th, as the editor shows it to be changed.
    #[must_use]
    pub fn of(rule: &WindowRule, index: usize) -> Self {
        let (matching, text) = MatchKind::of(&rule.criteria);
        let a = &rule.actions;
        let (place, centre_monitor, place_text) = match a.position {
            None => (Place::AsUsual, 0, String::new()),
            Some(PositionSpec::RememberLast) => (Place::Remember, 0, String::new()),
            Some(PositionSpec::CenterOnMonitor(monitor)) => (Place::Centre, monitor, String::new()),
            Some(PositionSpec::Absolute { x, y }) => (Place::At, 0, format!("{x} {y}")),
            Some(PositionSpec::Percentage { x_pct, y_pct }) => (
                Place::Part,
                0,
                format!("{}% {}%", shown_percent(x_pct), shown_percent(y_pct)),
            ),
        };
        let (size, size_text) = match a.size {
            None => (Size::AsUsual, String::new()),
            Some(SizeSpec::RememberLast) => (Size::Remember, String::new()),
            Some(SizeSpec::Exact { width, height }) => (Size::Exactly, format!("{width} {height}")),
            Some(SizeSpec::Percentage { w_pct, h_pct }) => (
                Size::Part,
                format!("{}% {}%", shown_percent(w_pct), shown_percent(h_pct)),
            ),
        };
        Self {
            editing: Some(index),
            name: field(&rule.name),
            matching,
            match_text: field(text),
            place,
            centre_monitor,
            place_text: field(&place_text),
            size,
            size_text: field(&size_text),
            min_size: field(&shown_pair(a.min_size)),
            max_size: field(&shown_pair(a.max_size)),
            actions: a.clone(),
            enabled: rule.enabled,
            once: rule.one_shot,
        }
    }

    /// The rule this draft says, among `rules` -- the list it is going into,
    /// the rule it changes included.
    ///
    /// # Errors
    ///
    /// Why it cannot be a rule yet, in a sentence the page shows: no name, a
    /// name another rule has, nothing to match, or numbers that do not read.
    pub fn rule(&self, rules: &[WindowRule]) -> Result<WindowRule, String> {
        let name = self.name.text().trim();
        if name.is_empty() {
            return Err("Give the rule a name: the list shows it by its name.".to_owned());
        }
        if rules
            .iter()
            .enumerate()
            .any(|(at, other)| Some(at) != self.editing && other.name == name)
        {
            return Err(format!(
                "Another rule is called \u{201c}{name}\u{201d} already."
            ));
        }
        let text = self.match_text.text().trim();
        let criteria = match self.matching {
            MatchKind::Any => MatchCriteria::Any,
            kind if text.is_empty() => return Err(kind.missing().to_owned()),
            MatchKind::Program => MatchCriteria::AppId(text.to_owned()),
            MatchKind::Title => MatchCriteria::TitleExact(text.to_owned()),
            MatchKind::TitleContains => MatchCriteria::TitleContains(text.to_owned()),
        };
        let mut actions = self.actions.clone();
        actions.position = self.position()?;
        actions.size = self.window_size()?;
        actions.min_size = optional_size(
            &self.min_size,
            "The smallest size is two numbers, its width and height: 400 300.",
        )?;
        actions.max_size = optional_size(
            &self.max_size,
            "The largest size is two numbers, its width and height: 1600 1200.",
        )?;
        if let (Some((min_w, min_h)), Some((max_w, max_h))) = (actions.min_size, actions.max_size)
            && (min_w > max_w || min_h > max_h)
        {
            return Err("The smallest size is larger than the largest.".to_owned());
        }
        let mut rule = WindowRule::new(0, name, criteria);
        rule.actions = actions;
        rule.enabled = self.enabled;
        rule.one_shot = self.once;
        Ok(rule)
    }

    /// Where the draft says its windows open.
    fn position(&self) -> Result<Option<PositionSpec>, String> {
        let text = self.place_text.text();
        Ok(match self.place {
            Place::AsUsual => None,
            Place::Remember => Some(PositionSpec::RememberLast),
            Place::Centre => Some(PositionSpec::CenterOnMonitor(self.centre_monitor)),
            Place::At => {
                let (x, y) = two(text, |word| word.parse::<i32>().ok()).ok_or(
                    "Where it opens is two numbers, from the left and from the top: 100 80.",
                )?;
                Some(PositionSpec::Absolute { x, y })
            }
            Place::Part => {
                let (x_pct, y_pct) = two(text, percent).ok_or(
                    "Where it opens is two parts of the screen, across and down: 50% 25%.",
                )?;
                Some(PositionSpec::Percentage { x_pct, y_pct })
            }
        })
    }

    /// How big the draft says its windows open.
    fn window_size(&self) -> Result<Option<SizeSpec>, String> {
        let text = self.size_text.text();
        Ok(match self.size {
            Size::AsUsual => None,
            Size::Remember => Some(SizeSpec::RememberLast),
            Size::Exactly => {
                let (width, height) = two(text, pixels).ok_or(
                    "Its size is two numbers above nought, its width and height: 800 600.",
                )?;
                Some(SizeSpec::Exact { width, height })
            }
            Size::Part => {
                let (w_pct, h_pct) = two(text, part)
                    .ok_or("Its size is two parts of the screen, its width and height: 50% 75%.")?;
                Some(SizeSpec::Percentage { w_pct, h_pct })
            }
        })
    }
}

// ============================================================================
// A rule in words
// ============================================================================

/// What `criteria` matches, in words.
#[must_use]
pub fn describe_criteria(criteria: &MatchCriteria) -> String {
    match criteria {
        MatchCriteria::AppId(program) => format!("The program {program}'s windows"),
        MatchCriteria::TitleExact(title) => format!("A window titled \u{201c}{title}\u{201d}"),
        MatchCriteria::TitleContains(words) => {
            format!("Windows whose title has \u{201c}{words}\u{201d}")
        }
        MatchCriteria::Any => "Every window".to_owned(),
    }
}

/// What a rule does, in a sentence: every action it sets, and whether it is
/// for the first window only. "Does nothing yet." for a rule that sets none.
#[must_use]
pub fn describe_actions(actions: &RuleActions, once: bool) -> String {
    let mut said: Vec<String> = Vec::new();
    if let Some(position) = actions.position {
        said.push(match position {
            PositionSpec::RememberLast => "opens where it was last".to_owned(),
            PositionSpec::CenterOnMonitor(0) => "opens in the middle of the screen".to_owned(),
            PositionSpec::CenterOnMonitor(monitor) => format!(
                "opens in the middle of monitor {}",
                monitor.saturating_add(1)
            ),
            PositionSpec::Absolute { x, y } => format!("opens at {x}, {y}"),
            PositionSpec::Percentage { x_pct, y_pct } => format!(
                "opens {}% across and {}% down",
                shown_percent(x_pct),
                shown_percent(y_pct)
            ),
        });
    }
    if let Some(size) = actions.size {
        said.push(match size {
            SizeSpec::RememberLast => "as big as it was last".to_owned(),
            SizeSpec::Exact { width, height } => format!("{width} by {height}"),
            SizeSpec::Percentage { w_pct, h_pct } => format!(
                "{}% by {}% of the screen",
                shown_percent(w_pct),
                shown_percent(h_pct)
            ),
        });
    }
    if let Some((w, h)) = actions.min_size {
        said.push(format!("no smaller than {w} by {h}"));
    }
    if let Some((w, h)) = actions.max_size {
        said.push(format!("no larger than {w} by {h}"));
    }
    if let Some(desktop) = actions.desktop {
        said.push(format!("on desktop {}", desktop.saturating_add(1)));
    }
    if let Some(state) = actions.initial_state {
        said.push(
            match state {
                InitialState::Normal => "neither minimised nor maximised",
                InitialState::Minimized => "starts minimised",
                InitialState::Maximized => "starts maximised",
                InitialState::Fullscreen => "starts full screen",
            }
            .to_owned(),
        );
    }
    if let Some(opacity) = actions.opacity {
        said.push(format!("{}% opaque", shown_percent(opacity)));
    }
    for flag in Flag::ALL {
        if let Some(value) = flag.get(actions) {
            said.push(flag.said(value).to_owned());
        }
    }
    if let Some(zone) = actions.snap_zone {
        said.push(format!("snaps into zone {zone}"));
    }
    if let Some(monitor) = actions.target_monitor {
        said.push(format!(
            "on monitor {} (not carried out yet)",
            monitor.saturating_add(1)
        ));
    }
    if let Some(none) = actions.no_decorations {
        said.push(
            if none {
                "without a title bar (not carried out yet)"
            } else {
                "with a title bar (not carried out yet)"
            }
            .to_owned(),
        );
    }
    if once {
        said.push("for the first window it matches only".to_owned());
    }
    if said.is_empty() {
        return "Does nothing yet.".to_owned();
    }
    let sentence = said.join(", ");
    let mut chars = sentence.chars();
    let mut out: String = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    out.push('.');
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    /// A rule saying every thing a rule can, with numbers the editor shows
    /// exactly.
    fn everything() -> WindowRule {
        let mut rule = WindowRule::new(0, "Chat", MatchCriteria::TitleContains("chat".to_owned()));
        let a = &mut rule.actions;
        a.position = Some(PositionSpec::Percentage {
            x_pct: 0.5,
            y_pct: 0.25,
        });
        a.size = Some(SizeSpec::Exact {
            width: 800,
            height: 600,
        });
        a.desktop = Some(1);
        a.always_on_top = Some(true);
        a.always_on_bottom = Some(false);
        a.initial_state = Some(InitialState::Maximized);
        a.opacity = Some(0.8);
        a.skip_taskbar = Some(true);
        a.skip_alt_tab = Some(false);
        a.target_monitor = Some(1);
        a.no_decorations = Some(true);
        a.min_size = Some((400, 300));
        a.max_size = Some((1600, 1200));
        a.prevent_close = Some(true);
        a.prevent_move = Some(false);
        a.prevent_resize = Some(true);
        a.snap_zone = Some(3);
        a.to_tray = Some(true);
        rule.enabled = false;
        rule.one_shot = true;
        rule
    }

    /// **A rule opened in the editor and saved untouched is the rule it
    /// was** -- every action, the ones the editor does not offer included,
    /// and whether it is on and for the first window only.
    #[test]
    fn a_rule_saved_untouched_is_the_rule_it_was() {
        let rule = everything();
        let again = RuleDraft::of(&rule, 0)
            .rule(std::slice::from_ref(&rule))
            .unwrap();
        assert_eq!(again.name, rule.name);
        assert_eq!(again.criteria, rule.criteria);
        assert_eq!(again.actions, rule.actions);
        assert_eq!(again.enabled, rule.enabled);
        assert_eq!(again.one_shot, rule.one_shot);

        for (criteria, position, size) in [
            (
                MatchCriteria::AppId("terminal".to_owned()),
                PositionSpec::RememberLast,
                SizeSpec::RememberLast,
            ),
            (
                MatchCriteria::TitleExact("Notes".to_owned()),
                PositionSpec::CenterOnMonitor(2),
                SizeSpec::Percentage {
                    w_pct: 0.5,
                    h_pct: 0.75,
                },
            ),
            (
                MatchCriteria::Any,
                PositionSpec::Absolute { x: -20, y: 80 },
                SizeSpec::Exact {
                    width: 1,
                    height: 2,
                },
            ),
        ] {
            let mut rule = WindowRule::new(0, "R", criteria);
            rule.actions.position = Some(position);
            rule.actions.size = Some(size);
            let again = RuleDraft::of(&rule, 0).rule(&[]).unwrap();
            assert_eq!(again.criteria, rule.criteria);
            assert_eq!(again.actions, rule.actions, "{position:?} {size:?}");
        }
    }

    /// **A new rule says nothing until it is told**: a draft named and given
    /// a program, saved, is a rule for that program's windows doing nothing.
    #[test]
    fn a_new_rule_does_nothing_until_it_is_told() {
        let mut draft = RuleDraft::new();
        draft.name.set_text("  Mine  ");
        draft.match_text.set_text(" terminal ");
        let rule = draft.rule(&[]).unwrap();
        assert_eq!(rule.name, "Mine", "the name was not trimmed");
        assert_eq!(rule.criteria, MatchCriteria::AppId("terminal".to_owned()));
        assert_eq!(rule.actions, RuleActions::new());
        assert!(rule.enabled && !rule.one_shot);
    }

    /// **A draft that cannot be a rule says why**, one reason for each way.
    #[test]
    fn a_draft_that_cannot_be_a_rule_says_why() {
        let named = |name: &str| {
            let mut draft = RuleDraft::new();
            draft.name.set_text(name);
            draft.match_text.set_text("terminal");
            draft
        };
        let refusal = |draft: &RuleDraft, rules: &[WindowRule]| draft.rule(rules).unwrap_err();

        assert_eq!(
            refusal(&named(" "), &[]),
            "Give the rule a name: the list shows it by its name."
        );
        let taken = WindowRule::new(0, "Mine", MatchCriteria::Any);
        assert_eq!(
            refusal(&named("Mine"), std::slice::from_ref(&taken)),
            "Another rule is called \u{201c}Mine\u{201d} already."
        );
        // Its own name is no other rule's.
        let mut own = named("Mine");
        own.editing = Some(0);
        assert!(own.rule(std::slice::from_ref(&taken)).is_ok());

        for (kind, why) in [
            (
                MatchKind::Program,
                "Say which program's windows the rule is for.",
            ),
            (MatchKind::Title, "Say which title the rule is for."),
            (
                MatchKind::TitleContains,
                "Say which words in the title the rule is for.",
            ),
        ] {
            let mut draft = named("R");
            draft.matching = kind;
            draft.match_text.set_text("  ");
            assert_eq!(refusal(&draft, &[]), why);
        }
        let mut any = named("R");
        any.matching = MatchKind::Any;
        any.match_text.set_text("");
        assert!(any.rule(&[]).is_ok(), "every window needs no text");

        let with = |edit: &dyn Fn(&mut RuleDraft)| {
            let mut draft = named("R");
            edit(&mut draft);
            draft.rule(&[])
        };
        for (edit, starts) in [
            (
                (|d: &mut RuleDraft| {
                    d.place = Place::At;
                    d.place_text.set_text("100");
                }) as fn(&mut RuleDraft),
                "Where it opens is two numbers",
            ),
            (
                |d: &mut RuleDraft| {
                    d.place = Place::Part;
                    d.place_text.set_text("50% 120%");
                },
                "Where it opens is two parts",
            ),
            (
                |d: &mut RuleDraft| {
                    d.size = Size::Exactly;
                    d.size_text.set_text("0 600");
                },
                "Its size is two numbers above nought",
            ),
            (
                |d: &mut RuleDraft| {
                    d.size = Size::Part;
                    d.size_text.set_text("0% 50%");
                },
                "Its size is two parts",
            ),
            (
                |d: &mut RuleDraft| d.min_size.set_text("400"),
                "The smallest size is two numbers",
            ),
            (
                |d: &mut RuleDraft| d.max_size.set_text("a b"),
                "The largest size is two numbers",
            ),
            (
                |d: &mut RuleDraft| {
                    d.min_size.set_text("800 600");
                    d.max_size.set_text("400 300");
                },
                "The smallest size is larger than the largest.",
            ),
        ] {
            let why = with(&edit).unwrap_err();
            assert!(why.starts_with(starts), "{why:?} for {starts:?}");
        }
    }

    /// **Numbers are read as people type them**: spaces, a comma or an `x`
    /// between, a percent sign or not.
    #[test]
    fn numbers_are_read_as_people_type_them() {
        for text in [
            "800 600",
            "800, 600",
            "800x600",
            "800 \u{d7} 600",
            " 800  600 ",
        ] {
            assert_eq!(two(text, pixels), Some((800, 600)), "{text:?}");
        }
        assert_eq!(two("800 600 1", pixels), None);
        assert_eq!(two("-20 80", |w| w.parse::<i32>().ok()), Some((-20, 80)));
        assert_eq!(two("50% 25", percent), Some((0.5, 0.25)));
        assert_eq!(two("100% 0%", percent), Some((1.0, 0.0)));
        assert_eq!(two("100.5% 1%", percent), None);
        assert_eq!(part("0%"), None, "a window cannot be no part of the screen");
    }

    /// **The yes-or-no rows mean what their labels say**, whichever way the
    /// model stores them.
    #[test]
    fn each_flag_reads_and_writes_its_own_action_in_its_rows_sense() {
        for flag in Flag::ALL {
            for value in [Some(true), Some(false), None] {
                let mut actions = RuleActions::new();
                flag.set(&mut actions, value);
                assert_eq!(flag.get(&actions), value, "{flag:?}");
                // One action, and no other.
                assert_eq!(
                    actions.active_count(),
                    usize::from(value.is_some()),
                    "{flag:?}"
                );
            }
        }
        let mut actions = RuleActions::new();
        Flag::Taskbar.set(&mut actions, Some(false));
        assert_eq!(
            actions.skip_taskbar,
            Some(true),
            "no taskbar button is a skipped one"
        );
        Flag::CanClose.set(&mut actions, Some(false));
        assert_eq!(actions.prevent_close, Some(true));
        for tri in Tri::ALL {
            assert_eq!(Tri::of(tri.value()), tri);
        }
    }

    /// **A rule is said in a sentence**, every action of it.
    #[test]
    fn a_rule_is_said_in_a_sentence() {
        let rule = everything();
        assert_eq!(
            describe_actions(&rule.actions, rule.one_shot),
            "Opens 50% across and 25% down, 800 by 600, no smaller than 400 by 300, \
             no larger than 1600 by 1200, on desktop 2, starts maximised, 80% opaque, \
             kept on top, not kept below other windows, no taskbar button, in Alt+Tab, \
             minimises to the tray, cannot be closed, can be moved, cannot be resized, \
             snaps into zone 3, on monitor 2 (not carried out yet), without a title bar \
             (not carried out yet), for the first window it matches only."
        );
        assert_eq!(
            describe_actions(&RuleActions::new(), false),
            "Does nothing yet."
        );
        let mut remember = RuleActions::new();
        remember.position = Some(PositionSpec::RememberLast);
        remember.size = Some(SizeSpec::RememberLast);
        assert_eq!(
            describe_actions(&remember, false),
            "Opens where it was last, as big as it was last."
        );
        assert_eq!(
            describe_criteria(&MatchCriteria::AppId("terminal".to_owned())),
            "The program terminal's windows"
        );
        assert_eq!(describe_criteria(&MatchCriteria::Any), "Every window");
    }

    /// **A list shows what a rule says, even off the list**: a desktop past
    /// the desktops there are and an opacity between the steps are offered
    /// as themselves, in their place.
    #[test]
    fn a_list_shows_what_a_rule_says_even_off_the_list() {
        assert_eq!(
            desktop_choices(None).len(),
            1 + usize::try_from(DESKTOPS).unwrap()
        );
        let far = desktop_choices(Some(6));
        assert_eq!(far.last(), Some(&Some(6)));
        assert_eq!(desktop_label(Some(6)), "Desktop 7 -- there are 4");
        assert_eq!(desktop_label(Some(0)), "Desktop 1");

        let between = opacity_choices(Some(0.85));
        let at = between.iter().position(|o| *o == Some(0.85)).unwrap();
        assert_eq!(between[at - 1], Some(0.9));
        assert_eq!(between[at + 1], Some(0.8));
        assert_eq!(
            opacity_choices(Some(0.8)).len(),
            1 + OPACITIES.len(),
            "a step is not added twice"
        );
        assert_eq!(opacity_label(Some(0.85)), "85%");
        assert_eq!(opacity_label(Some(1.0)), "Solid");
        assert_eq!(shown_percent(1.0 / 3.0), "33.3");
    }
}
