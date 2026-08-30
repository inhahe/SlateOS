//! Hotkey Manager — global keyboard shortcut management for the desktop shell.
//!
//! Provides a registry-based system for mapping key combinations (key + modifiers)
//! to desktop actions. The registry supports conflict detection, configuration
//! persistence (key=value text format), and a default binding set that mirrors
//! common desktop OS conventions.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut registry = HotkeyRegistry::defaults();
//!
//! // Register a custom binding:
//! registry.register(
//!     Hotkey::new(Key::T, Modifiers::ctrl()),
//!     HotkeyAction::LaunchApp("terminal".into()),
//! )?;
//!
//! // Look up a key event:
//! if let Some(action) = registry.lookup(Key::F4, &mods) {
//!     match action {
//!         HotkeyAction::CloseWindow => { /* close focused window */ }
//!         _ => {}
//!     }
//! }
//!
//! // Persist to/from text:
//! let config = HotkeyConfig::from_registry(&registry);
//! let text = config.save();
//! let loaded = HotkeyConfig::load(&text)?;
//! let restored = loaded.into_registry();
//! ```
//!
//! # Colour
//!
//! Every colour is read from the [`Palette`] the caller supplies; this module
//! holds none of its own. Four judgements decide which role goes where, and
//! each of them is a test rather than a comment.
//!
//! 1. **The panel is as transparent as the user asked, not as transparent as
//!    this file guessed.** The background used to be `Color::rgba(30, 30, 46,
//!    240)` — Mocha `base` with an alpha soldered onto it. That is a
//!    *setting* frozen into a constant: a user who turned transparency off
//!    still saw the wallpaper through their hotkey list, and a user who turned
//!    it up to Full got a panel noticeably more solid than every other popup
//!    on the same screen. [`Palette::panel_bg`] is `base` at the palette's own
//!    `panel_alpha`, so the panel now answers the setting.
//! 2. **Nothing here is accented, and that is a claim rather than an
//!    oversight.** This panel is a *reference card* — the user is reading it,
//!    not operating it, and the selected row marks where they are looking
//!    rather than what is in force. So the selection moves a rung
//!    ([`surface0`](Palette::surface0) over the panel, ink from
//!    [`subtext1`](Palette::subtext1) up to [`text`](Palette::text)) instead
//!    of changing hue. That also matches the shell's other keyboard-driven
//!    list, the launcher, which fills its selected row with a surface and
//!    spends its accent on a separate marker bar. The count is asserted at
//!    zero, so an accent appearing here later has to be a decision rather than
//!    a slip.
//! 3. **The selection is said twice, and both sayings are tested.** A fill
//!    appears *and* the label brightens. Either alone is a one-bit signal that
//!    a low-contrast display or a user with poor colour discrimination can
//!    lose; together they survive either failure. A test that checked only the
//!    fill would let the ink branch collapse silently, which is the exact
//!    shape of bug this conversion exists to catch.
//! 4. **A key badge is three rungs of one stack, and the stack is what is
//!    checked.** [`mantle`](Palette::mantle) behind the panel it sits on,
//!    [`surface1`](Palette::surface1) as its edge, [`subtext0`](Palette::subtext0)
//!    as its ink. The tests assert the *relations* — a badge is a different
//!    colour from the panel in both modes, and its ink is dimmer than the
//!    header's — because those hold in Mocha and Latte alike, whereas "the
//!    badge is `#181825`" holds in neither once the mode can change.

use appearance::Palette;
use guitk::event::{Key, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use std::collections::BTreeMap;
use std::fmt;

// ============================================================================
// Rendering constants
// ============================================================================

const PANEL_WIDTH: f32 = 560.0;
const PANEL_RADIUS: f32 = 10.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 38.0;
const KEY_BADGE_HEIGHT: f32 = 24.0;
const KEY_BADGE_RADIUS: f32 = 4.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 13.0;
const KEY_FONT_SIZE: f32 = 12.0;

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during hotkey registration or configuration parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyError {
    /// The hotkey is already bound to another action.
    Conflict {
        hotkey: Hotkey,
        existing: HotkeyAction,
    },
    /// The configuration text contains an invalid line.
    ParseError { line_number: usize, message: String },
    /// An unrecognized key name was encountered.
    UnknownKey(String),
    /// An unrecognized action name was encountered.
    UnknownAction(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { hotkey, existing } => {
                write!(
                    f,
                    "hotkey {} is already bound to {:?}",
                    hotkey.display_name(),
                    existing
                )
            }
            Self::ParseError {
                line_number,
                message,
            } => {
                write!(f, "parse error on line {}: {}", line_number, message)
            }
            Self::UnknownKey(name) => write!(f, "unknown key: {}", name),
            Self::UnknownAction(name) => write!(f, "unknown action: {}", name),
        }
    }
}

// ============================================================================
// Hotkey — a key + modifier combination
// ============================================================================

/// Keys that mean the same thing whatever modifiers are held.
///
/// A dedicated key is not a chord. `VolumeUp` has one meaning and no other job,
/// so requiring a bare press would mean a volume key that stopped working
/// because Shift happened to be down — a bug the user could feel but never
/// describe. Keyboards that put these behind an Fn layer are free to report
/// whatever the layer left set, and several do.
///
/// This is enforced by [`Hotkey::normalized`] clearing the modifiers outright,
/// so the registry holds *one* binding per key rather than sixteen, and a
/// `Ctrl+VolumeUp` bound to something else is reported as the conflict it is
/// instead of becoming a binding no press can reach.
const MODIFIER_AGNOSTIC_KEYS: &[Key] = &[Key::VolumeUp, Key::VolumeDown, Key::VolumeMute];

/// Every distinct [`Modifiers`] value there is.
///
/// Sixteen, because there are four flags. Written out rather than counted up in
/// a loop so that this is a list a reader can check against `Modifiers` by eye,
/// and so a fifth flag added to that struct leaves this obviously — rather than
/// silently — short.
pub(crate) const ALL_MODIFIER_SETS: [Modifiers; 16] = {
    const fn m(shift: bool, ctrl: bool, alt: bool, super_key: bool) -> Modifiers {
        Modifiers {
            shift,
            ctrl,
            alt,
            super_key,
        }
    }
    [
        m(false, false, false, false),
        m(true, false, false, false),
        m(false, true, false, false),
        m(true, true, false, false),
        m(false, false, true, false),
        m(true, false, true, false),
        m(false, true, true, false),
        m(true, true, true, false),
        m(false, false, false, true),
        m(true, false, false, true),
        m(false, true, false, true),
        m(true, true, false, true),
        m(false, false, true, true),
        m(true, false, true, true),
        m(false, true, true, true),
        m(true, true, true, true),
    ]
};

/// A keyboard shortcut: one principal key combined with zero or more modifiers.
///
/// Two presses that differ only in a modifier the binding does not name are
/// *different* hotkeys — matching the whole modifier set is what makes a loose
/// chord unable to swallow a tighter one. The table used to be a chain of `if`s
/// each testing only the modifiers it cared about, so `Super+Left` (snap the
/// window) was tested before `Ctrl+Super+Left` (previous desktop) and matched
/// with Ctrl held as well: the virtual-desktop shortcuts were unreachable and
/// had never once fired. A map keyed on the exact set cannot reproduce that.
///
/// The two exceptions are in [`normalized`](Self::normalized), which is applied
/// on the way *in* as well as on the way out, so they are properties of the
/// stored binding rather than of the lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl Hotkey {
    /// Create a hotkey with the given key and modifier set.
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            super_key: modifiers.super_key,
        }
    }

    /// Create a hotkey with no modifiers.
    pub fn bare(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
            super_key: false,
        }
    }

    /// Return the modifier state as a `Modifiers` value.
    pub fn modifiers(&self) -> Modifiers {
        Modifiers {
            ctrl: self.ctrl,
            alt: self.alt,
            shift: self.shift,
            super_key: self.super_key,
        }
    }

    /// The binding a press of `key` with `modifiers` actually names.
    ///
    /// Two rules, both applied on registration as well as on lookup so that the
    /// registry never holds a binding no keystroke can reach:
    ///
    /// 1. **A dedicated key ignores modifiers entirely.** See
    ///    [`MODIFIER_AGNOSTIC_KEYS`].
    /// 2. **A chord never includes the modifier the key *is*.** Whether the
    ///    Super bit is already set on the press of the Super key itself is the
    ///    keyboard driver's business, and neither answer should change what the
    ///    key does. Without this the start menu would open on one kind of driver
    ///    and not the other, which is precisely the bug the old table avoided by
    ///    writing `super_key: _` in that one arm.
    ///
    /// Everything else matches exactly.
    fn normalized(key: Key, modifiers: Modifiers) -> Self {
        if MODIFIER_AGNOSTIC_KEYS.contains(&key) {
            return Self::bare(key);
        }
        let Modifiers {
            mut shift,
            mut ctrl,
            mut alt,
            mut super_key,
        } = modifiers;
        match key {
            Key::LeftShift | Key::RightShift => shift = false,
            Key::LeftCtrl | Key::RightCtrl => ctrl = false,
            Key::LeftAlt | Key::RightAlt => alt = false,
            Key::LeftSuper | Key::RightSuper => super_key = false,
            _ => {}
        }
        Self {
            key,
            ctrl,
            alt,
            shift,
            super_key,
        }
    }

    /// Every exact chord a compositor must deliver for this binding to fire.
    ///
    /// One entry for an ordinary chord; sixteen for a
    /// [modifier-agnostic](MODIFIER_AGNOSTIC_KEYS) key; two for a key that is
    /// itself a modifier, because [`normalized`](Self::normalized) accepts the
    /// press with and without its own bit and a grab names one exact chord —
    /// there is no "any modifier" spelling in the protocol, and inventing one
    /// for three keys would put a special case in the compositor's keystroke
    /// path to save forty-eight entries in a hash map.
    fn chords(&self) -> Vec<(Key, Modifiers)> {
        if MODIFIER_AGNOSTIC_KEYS.contains(&self.key) {
            return ALL_MODIFIER_SETS
                .iter()
                .map(|&modifiers| (self.key, modifiers))
                .collect();
        }
        let mut with_own_bit = self.modifiers();
        match self.key {
            Key::LeftShift | Key::RightShift => with_own_bit.shift = true,
            Key::LeftCtrl | Key::RightCtrl => with_own_bit.ctrl = true,
            Key::LeftAlt | Key::RightAlt => with_own_bit.alt = true,
            Key::LeftSuper | Key::RightSuper => with_own_bit.super_key = true,
            _ => return vec![(self.key, self.modifiers())],
        }
        vec![(self.key, self.modifiers()), (self.key, with_own_bit)]
    }

    /// Test whether a key event matches this hotkey.
    ///
    /// Both sides go through [`normalized`](Self::normalized), so this answers
    /// the same question the registry does. Comparing the raw fields instead
    /// would let `matches` disagree with [`HotkeyRegistry::lookup`] about the
    /// two keys that have a rule — which is a difference nobody would look for.
    pub fn matches(&self, key: Key, modifiers: &Modifiers) -> bool {
        Self::normalized(self.key, self.modifiers()) == Self::normalized(key, *modifiers)
    }

    /// Human-readable name for display (e.g., "Ctrl+Alt+Delete").
    pub fn display_name(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.super_key {
            parts.push("Super");
        }
        parts.push(key_display_name(self.key));
        parts.join("+")
    }
}

impl PartialOrd for Hotkey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hotkey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Sort by modifiers first (ctrl, alt, shift, super), then by key name.
        let self_mods = (self.ctrl, self.alt, self.shift, self.super_key);
        let other_mods = (other.ctrl, other.alt, other.shift, other.super_key);
        self_mods
            .cmp(&other_mods)
            .then_with(|| key_sort_name(self.key).cmp(key_sort_name(other.key)))
    }
}

// ============================================================================
// HotkeyAction — what a hotkey triggers
// ============================================================================

/// Action performed when a hotkey is triggered.
///
/// This is the desktop's *whole* shortcut vocabulary — there is no second list
/// anywhere. It used to have a twin: a private `DesktopAction` in `lib.rs` held
/// the bindings the shell actually ran, hardcoded in one exhaustive match, while
/// this enum sat beside it with a registry, conflict detection and a config file
/// and was reachable from nothing. Neither was a superset of the other, so
/// "which shortcuts does this desktop have?" had two different answers depending
/// on which file you opened, and every shortcut named here was a shortcut a user
/// could read about and never press. See `design-decisions.md` §571.
///
/// Not every action can be carried out on a keystroke *from here*: the ones that
/// start a program report the command through
/// [`HotkeyOutcome::launches`](crate::HotkeyOutcome::launches) — see
/// [`command`](Self::command) — because the shell has no connection to the
/// process server, and the two brightness actions can be carried out at all (see
/// `known-issues.md` → `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    // ---- the focused window --------------------------------------------
    /// Close the focused window.
    CloseWindow,
    /// Minimize the focused window.
    MinimizeWindow,
    /// Maximize the focused window.
    MaximizeWindow,
    /// Un-maximize the focused window, or minimize it if it was not maximized.
    ///
    /// One key that walks a window down a step at a time, which is what makes it
    /// worth having beside [`MinimizeWindow`](Self::MinimizeWindow): which of
    /// the two it does depends on the state the *compositor* last reported, not
    /// on anything the shell decided.
    RestoreOrMinimize,
    /// Snap the focused window to the left half.
    SnapLeft,
    /// Snap the focused window to the right half.
    SnapRight,
    /// Open (or close) the multi-zone tiling chooser for the focused window.
    ///
    /// Distinct from [`SnapLeft`](Self::SnapLeft) and its neighbour, which are
    /// one keystroke each and place the window immediately. This one opens a
    /// chooser, because there are twenty-two zones across the six layouts and no
    /// plausible set of chords for them.
    ToggleZoneOverlay,
    /// Minimize every window on the current desktop.
    ShowDesktop,

    // ---- moving between windows ----------------------------------------
    /// Step the Alt+Tab switcher forwards, opening it if it is closed.
    CycleWindows,
    /// Step the Alt+Tab switcher backwards, opening it if it is closed.
    CycleWindowsBackwards,
    /// Open (or close) the Exposé overlay — every window on every desktop.
    ///
    /// Distinct from [`CycleWindows`](Self::CycleWindows), which is the same job
    /// for the common case: Alt+Tab is fast and blind, showing a strip of titles
    /// you step through without looking. This shows all of them at once, to
    /// scale, and is what you reach for when you do not remember how many
    /// presses away the window is — or which desktop it is on, which Alt+Tab
    /// cannot answer at all.
    ToggleOverview,

    // ---- virtual desktops ----------------------------------------------
    /// Show the desktop before the current one.
    PreviousDesktop,
    /// Show the desktop after the current one.
    NextDesktop,
    /// Show one particular virtual desktop, counting from zero.
    ///
    /// No default chord: numbering the desktops onto `Super+1`…`Super+9` would
    /// take nine keys from every application on the machine, and the two
    /// relative bindings above already reach every desktop.
    SwitchDesktop(u8),

    // ---- the shell's own surfaces --------------------------------------
    /// Open (or close) the start menu.
    ToggleStartMenu,
    /// Open (or close) the Run box.
    ///
    /// Distinct from [`ToggleStartMenu`](Self::ToggleStartMenu), which also has
    /// a search field: the start menu searches a list of *installed* programs
    /// and can only offer what is in it. The Run box takes a path and arguments,
    /// which is what you reach for when the thing you want to start is not on
    /// any menu.
    ToggleRunDialog,
    /// Open (or close) the notification pane.
    ///
    /// The pane is the only surface that holds a message the user did not ask
    /// for, so it needs a way in that does not depend on having noticed a tray
    /// icon change — a notification posted while the user was away is one they
    /// will never see a transient hint for.
    ToggleNotifications,
    /// Open (or close) the card that lists every shortcut.
    ///
    /// The card is the only place the bindings are written down at runtime, so
    /// without a chord of its own it is unreachable — and it is exactly the
    /// thing a user reaches for when they have forgotten the chords. It is
    /// listed on itself, which is the point: the card is how you learn the key
    /// that opens the card.
    ToggleShortcutCard,
    /// Close whatever popup is open.
    ///
    /// The one action that can decline the press: with nothing open it is not
    /// consumed and reaches the focused window, whose own dialog may be what the
    /// user meant to dismiss. That is what [`is_conditional`](Self::is_conditional)
    /// reports, and why its chord is grabbed only while a popup is up.
    DismissPopup,

    // ---- sound ----------------------------------------------------------
    /// Turn the volume up one step.
    VolumeUp,
    /// Turn the volume down one step.
    VolumeDown,
    /// Silence output, or let it back.
    VolumeMute,

    // ---- display --------------------------------------------------------
    /// Increase display brightness.
    ///
    /// Nothing carries this out yet; there is no backlight channel out of the
    /// shell. Kept bindable so the chord survives the channel arriving. See
    /// `known-issues.md` → `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.
    BrightnessUp,
    /// Decrease display brightness. See [`BrightnessUp`](Self::BrightnessUp).
    BrightnessDown,

    // ---- starting a program ---------------------------------------------
    /// Start the program named by this command line.
    LaunchApp(String),
    /// Start the process explorer.
    ShowTaskManager,
    /// Start the settings application.
    SystemSettings,
    /// Lock the screen.
    ScreenLock,
    /// Capture the whole screen.
    Screenshot,
    /// Capture a region the user draws.
    ScreenshotRegion,
}

/// The command each fixed launching action starts.
///
/// Written here rather than at the six call sites so that the paths appear once,
/// and pinned against [`crate::launcher::builtin_app_database`] by
/// `every_command_a_shortcut_names_is_a_program_the_shell_knows_about`: a
/// shortcut that starts `/usr/bin/procexploder` is a shortcut that silently does
/// nothing, and the typo is invisible to every other test in this file.
const TASK_MANAGER_COMMAND: &str = "/usr/bin/procexplorer";
const SETTINGS_COMMAND: &str = "/usr/bin/settings";
const LOCK_COMMAND: &str = "/usr/bin/lockscreen";
/// The flags are the screenshot tool's own; see `apps/screenshot/src/main.rs`,
/// which parses `--fullscreen`/`-f` and `--region`/`-r` as its first argument.
/// A flag it does not know would leave it sitting in its interactive menu, which
/// is not what either shortcut promises.
const SCREENSHOT_COMMAND: &str = "/usr/bin/screenshot --fullscreen";
const SCREENSHOT_REGION_COMMAND: &str = "/usr/bin/screenshot --region";

impl HotkeyAction {
    /// Whether the press is claimed only when the shell has something to do.
    ///
    /// True for [`DismissPopup`](Self::DismissPopup) and nothing else. A key the
    /// shell claims unconditionally is a key no window can ever see, and closing
    /// a dialog is what Escape does far more often than closing the start menu —
    /// so the chord for a conditional action is grabbed and released as popups
    /// open and close rather than held for the session.
    #[must_use]
    pub const fn is_conditional(&self) -> bool {
        matches!(self, Self::DismissPopup)
    }

    /// The program this action starts, if starting a program is what it does.
    ///
    /// `None` for every action that acts on a window or on the shell itself. The
    /// shell has no connection to the process server, so even the actions that
    /// *do* answer here are reported rather than performed — see
    /// [`HotkeyOutcome::launches`](crate::HotkeyOutcome::launches).
    #[must_use]
    pub fn command(&self) -> Option<&str> {
        match self {
            Self::LaunchApp(command) => Some(command),
            Self::ShowTaskManager => Some(TASK_MANAGER_COMMAND),
            Self::SystemSettings => Some(SETTINGS_COMMAND),
            Self::ScreenLock => Some(LOCK_COMMAND),
            Self::Screenshot => Some(SCREENSHOT_COMMAND),
            Self::ScreenshotRegion => Some(SCREENSHOT_REGION_COMMAND),
            _ => None,
        }
    }

    /// Serialize the action to a string for configuration persistence.
    fn to_config_value(&self) -> String {
        match self {
            Self::CloseWindow => "close_window".to_string(),
            Self::MinimizeWindow => "minimize_window".to_string(),
            Self::MaximizeWindow => "maximize_window".to_string(),
            Self::RestoreOrMinimize => "restore_or_minimize".to_string(),
            Self::SnapLeft => "snap_left".to_string(),
            Self::SnapRight => "snap_right".to_string(),
            Self::ToggleZoneOverlay => "toggle_zone_overlay".to_string(),
            Self::ShowDesktop => "show_desktop".to_string(),
            Self::CycleWindows => "cycle_windows".to_string(),
            Self::CycleWindowsBackwards => "cycle_windows_backwards".to_string(),
            Self::ToggleOverview => "toggle_overview".to_string(),
            Self::PreviousDesktop => "previous_desktop".to_string(),
            Self::NextDesktop => "next_desktop".to_string(),
            Self::SwitchDesktop(n) => format!("switch_desktop:{n}"),
            Self::ToggleStartMenu => "toggle_start_menu".to_string(),
            Self::ToggleRunDialog => "toggle_run_dialog".to_string(),
            Self::ToggleNotifications => "toggle_notifications".to_string(),
            Self::ToggleShortcutCard => "toggle_shortcut_card".to_string(),
            Self::DismissPopup => "dismiss_popup".to_string(),
            Self::VolumeUp => "volume_up".to_string(),
            Self::VolumeDown => "volume_down".to_string(),
            Self::VolumeMute => "volume_mute".to_string(),
            Self::BrightnessUp => "brightness_up".to_string(),
            Self::BrightnessDown => "brightness_down".to_string(),
            Self::LaunchApp(app) => format!("launch:{app}"),
            Self::ShowTaskManager => "show_task_manager".to_string(),
            Self::SystemSettings => "system_settings".to_string(),
            Self::ScreenLock => "screen_lock".to_string(),
            Self::Screenshot => "screenshot".to_string(),
            Self::ScreenshotRegion => "screenshot_region".to_string(),
        }
    }

    /// Parse an action from a configuration value string.
    fn from_config_value(value: &str) -> Result<Self, HotkeyError> {
        if let Some(app) = value.strip_prefix("launch:") {
            return Ok(Self::LaunchApp(app.to_string()));
        }
        if let Some(n_str) = value.strip_prefix("switch_desktop:") {
            let n = n_str
                .parse::<u8>()
                .map_err(|_| HotkeyError::UnknownAction(value.to_string()))?;
            return Ok(Self::SwitchDesktop(n));
        }
        match value {
            "close_window" => Ok(Self::CloseWindow),
            "minimize_window" => Ok(Self::MinimizeWindow),
            "maximize_window" => Ok(Self::MaximizeWindow),
            "restore_or_minimize" => Ok(Self::RestoreOrMinimize),
            "snap_left" => Ok(Self::SnapLeft),
            "snap_right" => Ok(Self::SnapRight),
            "toggle_zone_overlay" => Ok(Self::ToggleZoneOverlay),
            "show_desktop" => Ok(Self::ShowDesktop),
            "cycle_windows" => Ok(Self::CycleWindows),
            "cycle_windows_backwards" => Ok(Self::CycleWindowsBackwards),
            "toggle_overview" => Ok(Self::ToggleOverview),
            "previous_desktop" => Ok(Self::PreviousDesktop),
            "next_desktop" => Ok(Self::NextDesktop),
            "toggle_start_menu" => Ok(Self::ToggleStartMenu),
            "toggle_run_dialog" => Ok(Self::ToggleRunDialog),
            "toggle_notifications" => Ok(Self::ToggleNotifications),
            "toggle_shortcut_card" => Ok(Self::ToggleShortcutCard),
            "dismiss_popup" => Ok(Self::DismissPopup),
            "volume_up" => Ok(Self::VolumeUp),
            "volume_down" => Ok(Self::VolumeDown),
            "volume_mute" => Ok(Self::VolumeMute),
            "brightness_up" => Ok(Self::BrightnessUp),
            "brightness_down" => Ok(Self::BrightnessDown),
            "show_task_manager" => Ok(Self::ShowTaskManager),
            "system_settings" => Ok(Self::SystemSettings),
            "screen_lock" => Ok(Self::ScreenLock),
            "screenshot" => Ok(Self::Screenshot),
            "screenshot_region" => Ok(Self::ScreenshotRegion),
            _ => Err(HotkeyError::UnknownAction(value.to_string())),
        }
    }

    /// Short human-readable label for display in a settings panel.
    #[must_use]
    pub fn display_label(&self) -> &str {
        match self {
            Self::CloseWindow => "Close Window",
            Self::MinimizeWindow => "Minimize Window",
            Self::MaximizeWindow => "Maximize Window",
            Self::RestoreOrMinimize => "Restore or Minimize",
            Self::SnapLeft => "Snap Left",
            Self::SnapRight => "Snap Right",
            Self::ToggleZoneOverlay => "Tiling Zones",
            Self::ShowDesktop => "Show Desktop",
            Self::CycleWindows => "Cycle Windows",
            Self::CycleWindowsBackwards => "Cycle Windows Backwards",
            Self::ToggleOverview => "Window Overview",
            Self::PreviousDesktop => "Previous Desktop",
            Self::NextDesktop => "Next Desktop",
            Self::SwitchDesktop(_) => "Switch Desktop",
            Self::ToggleStartMenu => "Start Menu",
            Self::ToggleRunDialog => "Run Dialog",
            Self::ToggleNotifications => "Notifications",
            // Deliberately not "Keyboard Shortcuts", which is what the card's
            // own header reads: the label appears in a row *on* that card, and
            // a row that repeats the title verbatim reads as though the heading
            // had been drawn twice. Phrased as an action, like every other
            // label here.
            Self::ToggleShortcutCard => "Show Shortcuts",
            Self::DismissPopup => "Dismiss Popup",
            Self::VolumeUp => "Volume Up",
            Self::VolumeDown => "Volume Down",
            Self::VolumeMute => "Volume Mute",
            Self::BrightnessUp => "Brightness Up",
            Self::BrightnessDown => "Brightness Down",
            Self::LaunchApp(_) => "Launch App",
            Self::ShowTaskManager => "Task Manager",
            Self::SystemSettings => "Settings",
            Self::ScreenLock => "Lock Screen",
            Self::Screenshot => "Screenshot",
            Self::ScreenshotRegion => "Screenshot Region",
        }
    }
}

// ============================================================================
// HotkeyRegistry — the core binding store
// ============================================================================

/// Registry of keyboard shortcuts mapped to actions.
///
/// Uses a `BTreeMap` for deterministic iteration order, which makes
/// configuration serialization stable across runs.
pub struct HotkeyRegistry {
    bindings: BTreeMap<Hotkey, HotkeyAction>,
}

impl HotkeyRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
        }
    }

    /// Create a registry pre-populated with the standard default bindings.
    pub fn defaults() -> Self {
        let mut reg = Self::new();
        register_defaults(&mut reg);
        reg
    }

    /// Register a hotkey binding. Returns an error if the hotkey is already
    /// bound to a different action.
    pub fn register(&mut self, hotkey: Hotkey, action: HotkeyAction) -> Result<(), HotkeyError> {
        let hotkey = Hotkey::normalized(hotkey.key, hotkey.modifiers());
        if let Some(existing) = self.bindings.get(&hotkey) {
            // Allow re-registering the same action (idempotent).
            if *existing == action {
                return Ok(());
            }
            return Err(HotkeyError::Conflict {
                hotkey,
                existing: existing.clone(),
            });
        }
        self.bindings.insert(hotkey, action);
        Ok(())
    }

    /// Remove a hotkey binding. Returns `true` if something was removed.
    pub fn unregister(&mut self, hotkey: &Hotkey) -> bool {
        self.bindings
            .remove(&Hotkey::normalized(hotkey.key, hotkey.modifiers()))
            .is_some()
    }

    /// Look up the action for a given key + modifiers combination.
    pub fn lookup(&self, key: Key, modifiers: &Modifiers) -> Option<&HotkeyAction> {
        self.bindings.get(&Hotkey::normalized(key, *modifiers))
    }

    /// Every exact chord the shell must hold for the whole session.
    ///
    /// The table says what a chord *means*; this says which chords the shell has
    /// to be able to hear at all. They are different questions, because the
    /// compositor delivers a keystroke to whatever holds the keyboard, and that
    /// is almost never the shell: a shortcut reaches
    /// [`DesktopShell::handle_hotkey`](crate::DesktopShell::handle_hotkey) only
    /// for a key that landed on one of the shell's own surfaces. Every shortcut
    /// would otherwise be dead the moment the user clicked into an application —
    /// Alt+Tab, whose entire purpose is to be pressed from inside another
    /// window, most of all.
    ///
    /// [Conditional](HotkeyAction::is_conditional) bindings are excluded: a grab
    /// is not conditional, and holding Escape permanently would break it
    /// everywhere on the desktop. See [`conditional_chords`](Self::conditional_chords).
    ///
    /// Derived from the bindings rather than hand-listed beside them, which is
    /// what this used to be: a chord in one list and not the other is either a
    /// shortcut nobody can press or a key taken from every application for
    /// nothing, and neither shows up as a build failure.
    #[must_use]
    pub fn global_chords(&self) -> Vec<(Key, Modifiers)> {
        self.chords_where(|action| !action.is_conditional())
    }

    /// The chords to hold only while one of the shell's surfaces is open.
    #[must_use]
    pub fn conditional_chords(&self) -> Vec<(Key, Modifiers)> {
        self.chords_where(HotkeyAction::is_conditional)
    }

    /// The chords of every binding whose action satisfies `want`.
    fn chords_where(&self, want: impl Fn(&HotkeyAction) -> bool) -> Vec<(Key, Modifiers)> {
        self.bindings
            .iter()
            .filter(|(_, action)| want(action))
            .flat_map(|(hotkey, _)| hotkey.chords())
            .collect()
    }

    /// Iterate over all registered bindings in sorted order.
    pub fn all_bindings(&self) -> impl Iterator<Item = (&Hotkey, &HotkeyAction)> {
        self.bindings.iter()
    }

    /// Return the number of registered bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Check whether a specific hotkey is registered.
    pub fn is_registered(&self, hotkey: &Hotkey) -> bool {
        self.bindings
            .contains_key(&Hotkey::normalized(hotkey.key, hotkey.modifiers()))
    }

    /// If the hotkey conflicts with an existing binding, return the existing
    /// action. Returns `None` if the hotkey is free.
    pub fn conflicts_with(&self, hotkey: &Hotkey) -> Option<&HotkeyAction> {
        self.bindings
            .get(&Hotkey::normalized(hotkey.key, hotkey.modifiers()))
    }

    /// Remove all bindings.
    pub fn clear(&mut self) {
        self.bindings.clear();
    }

    /// Remove all bindings and re-populate with the standard defaults.
    pub fn reset_defaults(&mut self) {
        self.bindings.clear();
        register_defaults(self);
    }
}

impl Default for HotkeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Default bindings
// ============================================================================

/// Helper: construct a Modifiers with specific flags set.
const fn mods(ctrl: bool, alt: bool, shift: bool, super_key: bool) -> Modifiers {
    Modifiers {
        ctrl,
        alt,
        shift,
        super_key,
    }
}

/// Super alone — by far the most common modifier set in the table below.
const fn sup() -> Modifiers {
    mods(false, false, false, true)
}

/// Populate a registry with the standard default shortcut bindings.
///
/// This is the desktop's shipped keyboard, and it is the union of the two tables
/// that used to disagree: everything the shell already ran, plus everything this
/// module already promised. See `design-decisions.md` §571 for why the union
/// rather than either side.
fn register_defaults(reg: &mut HotkeyRegistry) {
    let defaults: &[(Hotkey, HotkeyAction)] = &[
        // ---- the focused window ------------------------------------------
        (
            Hotkey::new(Key::F4, Modifiers::alt()),
            HotkeyAction::CloseWindow,
        ),
        (Hotkey::new(Key::D, sup()), HotkeyAction::ShowDesktop),
        (Hotkey::new(Key::Left, sup()), HotkeyAction::SnapLeft),
        (Hotkey::new(Key::Right, sup()), HotkeyAction::SnapRight),
        (Hotkey::new(Key::Up, sup()), HotkeyAction::MaximizeWindow),
        (
            Hotkey::new(Key::Down, sup()),
            HotkeyAction::RestoreOrMinimize,
        ),
        // Super+Z, as in "zones". Super plus an arrow is already taken by the
        // four one-press placements above, and the chooser needs a key that is
        // not one of them.
        (Hotkey::new(Key::Z, sup()), HotkeyAction::ToggleZoneOverlay),
        // `HotkeyAction::MinimizeWindow` deliberately has no default chord:
        // Super+Down already minimizes an unmaximized window, and a second key
        // for the same job would be spent for nothing.

        // ---- moving between windows --------------------------------------
        (
            Hotkey::new(Key::Tab, Modifiers::alt()),
            HotkeyAction::CycleWindows,
        ),
        (
            Hotkey::new(Key::Tab, mods(false, true, true, false)),
            HotkeyAction::CycleWindowsBackwards,
        ),
        // Super+Tab, which is the chord every other desktop uses for this and is
        // not one of the four above. Alt+Tab is deliberately left alone: the two
        // are complements, not alternatives.
        (Hotkey::new(Key::Tab, sup()), HotkeyAction::ToggleOverview),
        // ---- virtual desktops ---------------------------------------------
        (
            Hotkey::new(Key::Left, mods(true, false, false, true)),
            HotkeyAction::PreviousDesktop,
        ),
        (
            Hotkey::new(Key::Right, mods(true, false, false, true)),
            HotkeyAction::NextDesktop,
        ),
        // ---- the shell's own surfaces --------------------------------------
        // The Super key on its own, both of them. `Hotkey::normalized` drops the
        // Super bit from the press of the Super key itself, so one entry answers
        // a driver that sets the bit and a driver that does not.
        (Hotkey::bare(Key::LeftSuper), HotkeyAction::ToggleStartMenu),
        (Hotkey::bare(Key::RightSuper), HotkeyAction::ToggleStartMenu),
        // Super+R, as in "run" — the chord Windows uses for the same box, and
        // free here. The run-dialog module's own doc offers "Ctrl+R or Super+R";
        // Ctrl+R is not taken by the desktop but *is* taken by roughly every
        // application that has a reload command, and a global grab on it would
        // break all of them.
        (Hotkey::new(Key::R, sup()), HotkeyAction::ToggleRunDialog),
        // Super+N, as in "notifications" — the chord Windows uses for the same
        // panel, and free here.
        (
            Hotkey::new(Key::N, sup()),
            HotkeyAction::ToggleNotifications,
        ),
        // Super+/ — what macOS and most editors use for "show me the
        // shortcuts", and free here. Written as the `Slash` key rather than as
        // `Shift+Slash`-for-a-question-mark on purpose: the binding is on a
        // *key*, not on a character, so it does not move when the user is
        // typing on a layout where `/` sits somewhere else.
        (
            Hotkey::new(Key::Slash, sup()),
            HotkeyAction::ToggleShortcutCard,
        ),
        // Bare Escape, claimed *conditionally*: with nothing open the press is
        // not consumed and reaches the focused window. See
        // `HotkeyAction::is_conditional`, which is what keeps this out of
        // `global_chords` — a permanent Escape grab would break the key in every
        // dialog on the desktop.
        (Hotkey::bare(Key::Escape), HotkeyAction::DismissPopup),
        // ---- sound -----------------------------------------------------------
        // Bare: hardware media keys are keys of their own and no modifier is
        // involved. These were `Key::Unknown(0xAF)` and its neighbours — Windows
        // virtual key codes, which nothing in this system emits — so the three
        // volume bindings could never fire. They are named variants now
        // (`gui/compositor/src/keymap.rs` translates the scan codes that do
        // arrive), and `Hotkey::normalized` is what makes a press with Shift
        // held still find them.
        (Hotkey::bare(Key::VolumeUp), HotkeyAction::VolumeUp),
        (Hotkey::bare(Key::VolumeDown), HotkeyAction::VolumeDown),
        (Hotkey::bare(Key::VolumeMute), HotkeyAction::VolumeMute),
        // `HotkeyAction::BrightnessUp`/`BrightnessDown` deliberately have no
        // default binding. A laptop's brightness pair sends no scancode at all
        // — the firmware answers it over ACPI or vendor WMI — so there is no
        // key to bind, and the two bindings that used to be here bound codes
        // that never arrive. The actions stay so that a user can put them on a
        // chord of their own. See known-issues.md →
        // `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.

        // ---- starting a program ---------------------------------------------
        (Hotkey::new(Key::I, sup()), HotkeyAction::SystemSettings),
        (
            Hotkey::new(Key::E, sup()),
            // The path the start menu's "File Explorer" entry uses, not the bare
            // word "explorer": `HotkeyOutcome::launches` carries command lines,
            // and whoever executes one is not obliged to search a path.
            HotkeyAction::LaunchApp("/usr/bin/explorer".to_string()),
        ),
        (Hotkey::new(Key::L, sup()), HotkeyAction::ScreenLock),
        (
            Hotkey::new(Key::Delete, mods(true, true, false, false)),
            HotkeyAction::ShowTaskManager,
        ),
        (Hotkey::bare(Key::PrintScreen), HotkeyAction::Screenshot),
        (
            Hotkey::new(Key::S, mods(false, false, true, true)),
            HotkeyAction::ScreenshotRegion,
        ),
    ];

    for (hotkey, action) in defaults {
        // A failure here is a chord claimed twice in the list above, which is a
        // bug in this function rather than something a caller can cause. It is
        // not swallowed: `no_default_binding_collides_with_another` fails on it,
        // and reporting it at runtime would mean a shell that refuses to start
        // over a shortcut.
        let _ = reg.register(*hotkey, action.clone());
    }
}

// ============================================================================
// HotkeyConfig — text-based persistence
// ============================================================================

/// Configuration wrapper for loading/saving hotkey bindings as text.
///
/// File format (one binding per line):
/// ```text
/// # Comment lines start with '#'
/// Alt+F4=close_window
/// Super+D=show_desktop
/// Super+E=launch:/usr/bin/explorer
/// Ctrl+Alt+Delete=show_task_manager
/// ```
pub struct HotkeyConfig {
    /// Parsed bindings.
    bindings: Vec<(Hotkey, HotkeyAction)>,
}

impl HotkeyConfig {
    /// Build a config snapshot from an existing registry.
    pub fn from_registry(registry: &HotkeyRegistry) -> Self {
        let bindings: Vec<(Hotkey, HotkeyAction)> = registry
            .all_bindings()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        Self { bindings }
    }

    /// Parse hotkey configuration from text. Lines starting with '#' are
    /// comments. Blank lines are skipped.
    pub fn load(text: &str) -> Result<Self, HotkeyError> {
        let mut bindings = Vec::new();

        for (line_idx, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // `split_once` is `find` plus both slices in one step, so the
            // separator's position never has to be carried in a variable and
            // re-applied — which is where an off-by-one or a mid-character
            // split would come from if the separator were ever multi-byte.
            // One-based for the human reading the error; saturating so the
            // number reported for an implausibly long file is wrong by one
            // rather than wrapping to zero.
            let line_number = line_idx.saturating_add(1);
            let (key_part, value_part) =
                line.split_once('=')
                    .ok_or_else(|| HotkeyError::ParseError {
                        line_number,
                        message: "expected '=' separator".to_string(),
                    })?;
            let key_part = key_part.trim();
            let value_part = value_part.trim();

            let hotkey = parse_hotkey_string(key_part).map_err(|e| HotkeyError::ParseError {
                line_number,
                message: format!("{}", e),
            })?;

            let action = HotkeyAction::from_config_value(value_part).map_err(|e| {
                HotkeyError::ParseError {
                    line_number,
                    message: format!("{}", e),
                }
            })?;

            bindings.push((hotkey, action));
        }

        Ok(Self { bindings })
    }

    /// Serialize the configuration to text.
    pub fn save(&self) -> String {
        let mut output = String::from("# Keyboard shortcut configuration\n");
        output.push_str("# Format: Modifier+Key=action\n\n");

        for (hotkey, action) in &self.bindings {
            output.push_str(&hotkey.display_name());
            output.push('=');
            output.push_str(&action.to_config_value());
            output.push('\n');
        }

        output
    }

    /// Convert this configuration into a populated registry.
    ///
    /// If duplicate hotkeys exist in the config, the last one wins (no error).
    pub fn into_registry(self) -> HotkeyRegistry {
        let mut registry = HotkeyRegistry::new();
        for (hotkey, action) in self.bindings {
            // Overwrite any previous binding for the same hotkey. Normalised on
            // the way in like every other entry point, so a config file that
            // writes `Ctrl+VolumeUp` binds the same thing a press of the volume
            // key finds rather than an entry nothing can reach.
            registry
                .bindings
                .insert(Hotkey::normalized(hotkey.key, hotkey.modifiers()), action);
        }
        registry
    }

    /// Create a config with the standard defaults.
    pub fn reset_defaults() -> Self {
        Self::from_registry(&HotkeyRegistry::defaults())
    }

    /// Return the number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check whether there are no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ============================================================================
// Key name parsing and display
// ============================================================================

/// Return a human-readable name for a `Key` variant.
fn key_display_name(key: Key) -> &'static str {
    match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Escape => "Escape",
        Key::Space => "Space",
        Key::LeftShift => "LeftShift",
        Key::RightShift => "RightShift",
        Key::LeftCtrl => "LeftCtrl",
        Key::RightCtrl => "RightCtrl",
        Key::LeftAlt => "LeftAlt",
        Key::RightAlt => "RightAlt",
        Key::LeftSuper => "LeftSuper",
        Key::RightSuper => "RightSuper",
        Key::PrintScreen => "PrintScreen",
        Key::ScrollLock => "ScrollLock",
        Key::Pause => "Pause",
        Key::CapsLock => "CapsLock",
        Key::NumLock => "NumLock",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Semicolon => "Semicolon",
        Key::Colon => "Colon",
        Key::Slash => "Slash",
        Key::Backslash => "Backslash",
        Key::LeftBracket => "LeftBracket",
        Key::RightBracket => "RightBracket",
        Key::Minus => "Minus",
        Key::Equals => "Equals",
        Key::Apostrophe => "Apostrophe",
        Key::Grave => "Grave",
        Key::VolumeUp => "VolumeUp",
        Key::VolumeDown => "VolumeDown",
        Key::VolumeMute => "VolumeMute",
        Key::MediaPlayPause => "MediaPlayPause",
        Key::MediaNextTrack => "MediaNextTrack",
        Key::MediaPrevTrack => "MediaPrevTrack",
        Key::MediaStop => "MediaStop",
        // The media keys used to be named here, by matching `Unknown` against
        // 0xAF/0xAE/0xAD/0xE0/0xE1 — *Windows virtual key codes*, which nothing
        // in this system produces. What does arrive is scan code set 1 with the
        // extended prefix folded into the high byte (0xE030 and friends), so
        // the arms matched nothing and the names were unreachable for the whole
        // life of the module. They are real `Key` variants now; see
        // `gui/compositor/src/keymap.rs`. Brightness has no arm at all, because
        // it has no scancode to arrive as — see
        // known-issues.md → `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.
        Key::Unknown(_) => "Unknown",
    }
}

/// Stable sort key for hotkeys (used by Ord impl).
fn key_sort_name(key: Key) -> &'static str {
    key_display_name(key)
}

/// Parse a key name string (case-insensitive) into a `Key`.
fn parse_key_name(name: &str) -> Result<Key, HotkeyError> {
    match name.to_ascii_lowercase().as_str() {
        "a" => Ok(Key::A),
        "b" => Ok(Key::B),
        "c" => Ok(Key::C),
        "d" => Ok(Key::D),
        "e" => Ok(Key::E),
        "f" => Ok(Key::F),
        "g" => Ok(Key::G),
        "h" => Ok(Key::H),
        "i" => Ok(Key::I),
        "j" => Ok(Key::J),
        "k" => Ok(Key::K),
        "l" => Ok(Key::L),
        "m" => Ok(Key::M),
        "n" => Ok(Key::N),
        "o" => Ok(Key::O),
        "p" => Ok(Key::P),
        "q" => Ok(Key::Q),
        "r" => Ok(Key::R),
        "s" => Ok(Key::S),
        "t" => Ok(Key::T),
        "u" => Ok(Key::U),
        "v" => Ok(Key::V),
        "w" => Ok(Key::W),
        "x" => Ok(Key::X),
        "y" => Ok(Key::Y),
        "z" => Ok(Key::Z),
        "0" => Ok(Key::Num0),
        "1" => Ok(Key::Num1),
        "2" => Ok(Key::Num2),
        "3" => Ok(Key::Num3),
        "4" => Ok(Key::Num4),
        "5" => Ok(Key::Num5),
        "6" => Ok(Key::Num6),
        "7" => Ok(Key::Num7),
        "8" => Ok(Key::Num8),
        "9" => Ok(Key::Num9),
        "f1" => Ok(Key::F1),
        "f2" => Ok(Key::F2),
        "f3" => Ok(Key::F3),
        "f4" => Ok(Key::F4),
        "f5" => Ok(Key::F5),
        "f6" => Ok(Key::F6),
        "f7" => Ok(Key::F7),
        "f8" => Ok(Key::F8),
        "f9" => Ok(Key::F9),
        "f10" => Ok(Key::F10),
        "f11" => Ok(Key::F11),
        "f12" => Ok(Key::F12),
        "left" => Ok(Key::Left),
        "right" => Ok(Key::Right),
        "up" => Ok(Key::Up),
        "down" => Ok(Key::Down),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        "backspace" => Ok(Key::Backspace),
        "delete" => Ok(Key::Delete),
        "insert" => Ok(Key::Insert),
        "enter" | "return" => Ok(Key::Enter),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "space" => Ok(Key::Space),
        "printscreen" | "print" | "prtsc" => Ok(Key::PrintScreen),
        "scrolllock" => Ok(Key::ScrollLock),
        "pause" | "break" => Ok(Key::Pause),
        "capslock" => Ok(Key::CapsLock),
        "numlock" => Ok(Key::NumLock),
        "comma" => Ok(Key::Comma),
        "period" => Ok(Key::Period),
        "semicolon" => Ok(Key::Semicolon),
        "colon" => Ok(Key::Colon),
        "slash" => Ok(Key::Slash),
        "backslash" => Ok(Key::Backslash),
        "leftbracket" => Ok(Key::LeftBracket),
        "rightbracket" => Ok(Key::RightBracket),
        "minus" => Ok(Key::Minus),
        "equals" => Ok(Key::Equals),
        "apostrophe" => Ok(Key::Apostrophe),
        "grave" | "backtick" => Ok(Key::Grave),
        "super" | "leftsuper" => Ok(Key::LeftSuper),
        "rightsuper" => Ok(Key::RightSuper),
        "leftshift" => Ok(Key::LeftShift),
        "rightshift" => Ok(Key::RightShift),
        "leftctrl" => Ok(Key::LeftCtrl),
        "rightctrl" => Ok(Key::RightCtrl),
        "leftalt" => Ok(Key::LeftAlt),
        "rightalt" => Ok(Key::RightAlt),
        "volumeup" => Ok(Key::VolumeUp),
        "volumedown" => Ok(Key::VolumeDown),
        "volumemute" => Ok(Key::VolumeMute),
        "mediaplaypause" => Ok(Key::MediaPlayPause),
        "medianexttrack" => Ok(Key::MediaNextTrack),
        "mediaprevtrack" => Ok(Key::MediaPrevTrack),
        "mediastop" => Ok(Key::MediaStop),
        // "brightnessup"/"brightnessdown" are gone rather than remapped: they
        // parsed to Windows virtual key codes this system never emits, and
        // there is no scancode to point them at instead. A config file naming
        // one now fails loudly, which is the honest answer — the binding it
        // asked for could not have worked. See known-issues.md →
        // `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.
        _ => Err(HotkeyError::UnknownKey(name.to_string())),
    }
}

/// Parse a combined hotkey string like "Ctrl+Alt+Delete" into a `Hotkey`.
fn parse_hotkey_string(s: &str) -> Result<Hotkey, HotkeyError> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err(HotkeyError::UnknownKey(s.to_string()));
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut super_key = false;
    let mut principal_key: Option<Key> = None;

    for part in &parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "super" | "win" | "meta" | "cmd" => super_key = true,
            _ => {
                // This should be the principal key (last non-modifier token).
                principal_key = Some(parse_key_name(part)?);
            }
        }
    }

    let key = principal_key.ok_or_else(|| HotkeyError::UnknownKey(s.to_string()))?;

    Ok(Hotkey {
        key,
        ctrl,
        alt,
        shift,
        super_key,
    })
}

// ============================================================================
// Settings panel rendering
// ============================================================================

/// Where every row of the card goes, worked out once.
///
/// The card grows *sideways* rather than downwards. One column of one row per
/// binding is the obvious layout and it does not fit: the twenty-five default
/// bindings alone come to 1,010 units, and the moment a user adds three of
/// their own the card is taller than a 1080-line display — at which point the
/// last rows are drawn off the bottom edge, with nothing on screen to say that
/// anything is missing. A card whose *end* is invisible is worse than no card,
/// because the user reads it, does not find the shortcut, and concludes there
/// isn't one.
///
/// Columns rather than scrolling because the card is read, not operated: it has
/// no input handling at all (`DesktopShell` toggles a `bool` and nothing else),
/// and a surface with a hidden region and no way to reach it has the same defect
/// as the overflowing single column. Printed shortcut references are laid out in
/// columns for the same reason.
struct PanelLayout {
    /// How many rows each column holds. At least one.
    rows_per_column: usize,
    /// Total width, which is one `PANEL_WIDTH` per column.
    width: f32,
    /// Total height: the header, the tallest column, and the bottom padding.
    height: f32,
}

/// Work out [`PanelLayout`] for `registry` given the height it has to fit in.
///
/// `max_height` is what the *caller* can spare, not what the card wants. A card
/// that fits gets one column; one that does not gets as many as it needs, each
/// no taller than `max_height`.
fn panel_layout(registry: &HotkeyRegistry, max_height: f32) -> PanelLayout {
    // Floored at one row: a `max_height` smaller than a single row would
    // otherwise ask for a column per binding and a card wider than any screen.
    // One row that overflows by a few units is the better failure — the card
    // still reads top-down.
    let usable = (max_height - HEADER_HEIGHT - PADDING).max(ROW_HEIGHT);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`usable` is at least ROW_HEIGHT so the quotient is at least \
                  1.0 and never negative, and it is bounded above by a display \
                  height in the same units — truncation is the floor that is \
                  wanted here"
    )]
    let rows_that_fit = ((usable / ROW_HEIGHT) as usize).max(1);
    // `max(1)` again on the count: an empty registry still draws its header, and
    // a zero-column card would be a zero-width rectangle rather than an empty
    // one with a title on it.
    let count = registry.len().max(1);
    let columns = count.div_ceil(rows_that_fit).max(1);
    // Recomputed from the column count rather than left at `rows_that_fit`, so
    // that twenty-six bindings over two columns come out thirteen and thirteen
    // rather than twenty-five and one.
    let rows_per_column = count.div_ceil(columns);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a binding count large enough to lose precision in an f32 is \
                  a card several million rows tall; the cast is not what is \
                  wrong in that case"
    )]
    let (columns_f, rows_f) = (columns as f32, rows_per_column as f32);
    PanelLayout {
        rows_per_column,
        width: PANEL_WIDTH * columns_f,
        height: HEADER_HEIGHT + rows_f * ROW_HEIGHT + PADDING,
    }
}

/// How large the card will be, before it is drawn.
///
/// A caller that wants to centre the panel has to know its size, and the size
/// depends on how many bindings there are and on how much room they have — so
/// it cannot be a constant, and it must not be a second calculation: a caller
/// that computed the height itself would centre a rectangle of one size around a
/// card of another the moment a user added a binding. [`render_settings_panel`]
/// asks this same function.
///
/// `max_height` is the room the caller has; see [`panel_layout`] for what
/// happens when the bindings need more than that. Pass the caller the *same*
/// number here and to [`render_settings_panel`], or the two will disagree about
/// how many columns there are.
///
/// Returned as `(width, height)` in the caller's own units, which are the
/// units the panel is drawn in.
#[must_use]
pub fn settings_panel_size(registry: &HotkeyRegistry, max_height: f32) -> (f32, f32) {
    let layout = panel_layout(registry, max_height);
    (layout.width, layout.height)
}

/// Render a hotkey settings panel showing all bindings.
///
/// Produces a self-contained list of `RenderCommand`s that can be composited
/// on top of the desktop. The panel is positioned at `(panel_x, panel_y)`.
///
/// `selected_index` optionally highlights one row (for keyboard navigation).
///
/// `max_height` is how tall the caller can let the card be; past that it grows
/// into a second column rather than off the bottom of the screen. See
/// [`panel_layout`]. Whatever is passed here must also be passed to
/// [`settings_panel_size`], or the placement and the drawing will disagree.
///
/// `p` supplies every colour drawn; see this module's `# Colour` section for
/// the four judgements that decide which role goes where.
pub fn render_settings_panel(
    registry: &HotkeyRegistry,
    p: &Palette,
    panel_x: f32,
    panel_y: f32,
    selected_index: Option<usize>,
    max_height: f32,
) -> Vec<RenderCommand> {
    let binding_count = registry.len();
    let layout = panel_layout(registry, max_height);
    let (panel_width, panel_height) = (layout.width, layout.height);
    let radii = CornerRadii::all(PANEL_RADIUS);

    let mut cmds: Vec<RenderCommand> =
        Vec::with_capacity(binding_count.saturating_mul(6).saturating_add(8));

    // Shadow.
    cmds.push(RenderCommand::BoxShadow {
        x: panel_x,
        y: panel_y,
        width: panel_width,
        height: panel_height,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 6.0,
        // Black in both modes, which is why it does not flip with the theme:
        // a shadow is an absence of light rather than a colour.
        color: p.shadow(),
        corner_radii: radii,
    });

    // Background.
    cmds.push(RenderCommand::FillRect {
        x: panel_x,
        y: panel_y,
        width: panel_width,
        height: panel_height,
        // Judgement 1: the transparency setting, not a baked-in alpha.
        color: p.panel_bg(),
        corner_radii: radii,
    });

    // Border.
    cmds.push(RenderCommand::StrokeRect {
        x: panel_x,
        y: panel_y,
        width: panel_width,
        height: panel_height,
        color: p.surface2,
        line_width: 1.0,
        corner_radii: radii,
    });

    // Clip to panel bounds.
    cmds.push(RenderCommand::PushClip {
        x: panel_x,
        y: panel_y,
        width: panel_width,
        height: panel_height,
    });

    // Header.
    cmds.push(RenderCommand::Text {
        x: panel_x + PADDING,
        y: panel_y + PADDING,
        text: "Keyboard Shortcuts".to_string(),
        color: p.text,
        font_size: HEADER_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Separator line below header.
    cmds.push(RenderCommand::Line {
        x1: panel_x + PADDING,
        y1: panel_y + HEADER_HEIGHT,
        x2: panel_x + panel_width - PADDING,
        y2: panel_y + HEADER_HEIGHT,
        color: p.surface1,
        width: 1.0,
    });

    // Rows, filled down each column and then across — the order a reference is
    // read. `content_width` stays *one column* wide, because every measurement
    // inside the loop is relative to the column its row is in and not to the
    // card as a whole; `panel_x` is shadowed below for the same reason.
    let content_width = PANEL_WIDTH - PADDING * 2.0;
    // Stepped rather than divided out of `i`, so there is no division by a
    // count a future edit could let reach zero.
    let mut row_in_column: usize = 0;
    let mut column_x = panel_x;
    for (i, (hotkey, action)) in registry.all_bindings().enumerate() {
        if row_in_column == layout.rows_per_column {
            row_in_column = 0;
            column_x += PANEL_WIDTH;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a row index within one column is bounded by the rows that \
                      fit on a display; see `panel_layout`"
        )]
        let row_y = panel_y + HEADER_HEIGHT + row_in_column as f32 * ROW_HEIGHT;
        row_in_column = row_in_column.saturating_add(1);
        // Shadowing the parameter is deliberate: everything below places itself
        // against the left edge of *its column*, and a stray `panel_x` reaching
        // past this point would silently draw a second column's row on top of
        // the first.
        let panel_x = column_x;
        let is_selected = selected_index == Some(i);

        // Selection highlight.
        if is_selected {
            cmds.push(RenderCommand::FillRect {
                x: panel_x + PADDING / 2.0,
                y: row_y + 2.0,
                width: content_width + PADDING,
                height: ROW_HEIGHT - 4.0,
                // Judgement 2: one rung up from the panel, not the accent.
                // The selected row is where the user is *looking*; nothing on
                // this card is in force.
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Action label on the left.
        let label = action.display_label();
        // Judgement 3: the selection is said twice. The fill above is the
        // other saying, and neither is allowed to carry it alone.
        let label_color = if is_selected { p.text } else { p.subtext1 };
        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y: row_y + (ROW_HEIGHT - LABEL_FONT_SIZE) / 2.0,
            text: label.to_string(),
            color: label_color,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width * 0.45),
            overflow: TextOverflow::Ellipsis,
        });

        // Anything that starts a program says which program, beside the label.
        // Asked of `command` rather than matched on here, so the card cannot
        // name one thing and the shortcut start another: they read the same
        // field. `LaunchApp` is the row that needs it — its label is only
        // "Launch application" — but the fixed-command actions get it too,
        // because "Task manager" is a name and `/usr/bin/procexplorer` is the
        // answer to *which* task manager.
        let extra_text = action.command();
        if let Some(detail) = extra_text {
            let detail_x = panel_x + PADDING + content_width * 0.2;
            cmds.push(RenderCommand::Text {
                x: detail_x,
                y: row_y + (ROW_HEIGHT - KEY_FONT_SIZE) / 2.0 + 1.0,
                text: detail.to_string(),
                // Dimmer than either label branch: the app name is an
                // argument to the action beside it, not a second action.
                color: p.overlay0,
                font_size: KEY_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width * 0.25),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Key badges on the right.
        let display = hotkey.display_name();
        let badge_parts: Vec<&str> = display.split('+').collect();
        let mut badge_x = panel_x + PANEL_WIDTH - PADDING;

        // Render badges right-to-left so they align to the right edge.
        for part in badge_parts.iter().rev() {
            let text_width = text::padded_width(part, 6.0, KEY_FONT_SIZE, FontWeightHint::Regular);
            badge_x -= text_width + 4.0;

            let badge_y = row_y + (ROW_HEIGHT - KEY_BADGE_HEIGHT) / 2.0;

            // Badge background.
            cmds.push(RenderCommand::FillRect {
                x: badge_x,
                y: badge_y,
                width: text_width,
                height: KEY_BADGE_HEIGHT,
                // Judgement 4: the rung *behind* the panel, so a badge reads
                // as a recess cut into the card in either mode.
                color: p.mantle,
                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),
            });

            // Badge border.
            cmds.push(RenderCommand::StrokeRect {
                x: badge_x,
                y: badge_y,
                width: text_width,
                height: KEY_BADGE_HEIGHT,
                color: p.surface1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),
            });

            // Badge text.
            cmds.push(RenderCommand::Text {
                x: badge_x + 6.0,
                y: badge_y + (KEY_BADGE_HEIGHT - KEY_FONT_SIZE) / 2.0,
                text: (*part).to_string(),
                // Judgement 4: dimmer than the header. A badge quotes a key;
                // it is not a heading.
                color: p.subtext0,
                font_size: KEY_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    cmds.push(RenderCommand::PopClip);

    cmds
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
    // The colour tests select a rectangle by the exact literal dimensions the
    // code under test was handed — a fill 24 units high is *a key badge*, and
    // nothing else on this panel is. That is the assertion meant: a tolerance
    // would let a rectangle that has been resized pass as one that has not,
    // and geometry is the only handle this panel offers on which of its
    // unnamed boxes is which.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use guitk::color::Color;

    /// More room than any card will ever ask for, so the layout stays at one
    /// column.
    ///
    /// Passed by every test that is about something other than columns — the
    /// colour tests, the badge tests, the selection tests — because those were
    /// all written against a single column and a single column is still what
    /// they are asserting about. The tests that *are* about the fold pass a real
    /// display height instead.
    const ROOMY: f32 = 100_000.0;

    /// No default binding names a key by its raw code.
    ///
    /// The defect this pins is the one the media bindings had for the whole
    /// life of this module: three of them were `Key::Unknown(0xAF)` and its
    /// neighbours — *Windows* virtual key codes — and this system's keyboard
    /// path emits scan code set 1 with the extended prefix in the high byte, so
    /// the codes never arrived and the bindings never fired. Nothing said so,
    /// because a binding that matches nothing looks exactly like a binding
    /// nobody has pressed.
    ///
    /// A `Key::Unknown` in the *defaults* is always this mistake. A user is
    /// welcome to bind a raw code — that is what `Unknown` carries it for, and
    /// what a remapper wants — but a default has to be a key the system is
    /// known to produce, and the named variants are the ones the keymap
    /// promises.
    #[test]
    fn no_default_binding_names_a_key_by_its_raw_code() {
        for (hotkey, action) in HotkeyRegistry::defaults().all_bindings() {
            assert!(
                !matches!(hotkey.key, Key::Unknown(_)),
                "default binding for {action:?} is a raw code: {:?}",
                hotkey.key
            );
        }
    }

    /// The volume keys are bound bare, and to the named variants the keymap
    /// actually produces.
    #[test]
    fn the_volume_keys_have_working_defaults() {
        let reg = HotkeyRegistry::defaults();
        for (key, expected) in [
            (Key::VolumeUp, HotkeyAction::VolumeUp),
            (Key::VolumeDown, HotkeyAction::VolumeDown),
            (Key::VolumeMute, HotkeyAction::VolumeMute),
        ] {
            assert_eq!(
                reg.lookup(key, &Modifiers::NONE),
                Some(&expected),
                "{key:?} should be bound bare"
            );
        }
    }

    /// Brightness keeps its *actions* and loses its *bindings*.
    ///
    /// There is no scancode for a laptop's brightness pair — the firmware
    /// answers it over ACPI or vendor WMI — so the two default bindings that
    /// used to exist could not fire, and a binding that cannot fire is worse
    /// than none: it tells a settings panel the key is spoken for. The actions
    /// stay, because a user may put them on a chord of their own.
    #[test]
    fn brightness_has_no_default_binding_because_it_has_no_key() {
        let bound: Vec<_> = HotkeyRegistry::defaults()
            .all_bindings()
            .filter(|(_, a)| matches!(a, HotkeyAction::BrightnessUp | HotkeyAction::BrightnessDown))
            .map(|(h, _)| h.key)
            .collect();
        assert!(
            bound.is_empty(),
            "brightness should be unbound, got {bound:?}"
        );
        // Still nameable, so a config file can ask for it.
        assert_eq!(
            HotkeyAction::from_config_value("brightness_up").ok(),
            Some(HotkeyAction::BrightnessUp)
        );
    }

    /// Every key `key_display_name` can name round-trips through the parser.
    ///
    /// The two tables are written out by hand and face each other, so the
    /// failure they invite is a name that displays one way and parses another
    /// — or, as with the media keys, displays a name the parser then rejects.
    #[test]
    fn every_media_key_name_survives_display_and_parse() {
        for key in [
            Key::VolumeUp,
            Key::VolumeDown,
            Key::VolumeMute,
            Key::MediaPlayPause,
            Key::MediaNextTrack,
            Key::MediaPrevTrack,
            Key::MediaStop,
        ] {
            let name = key_display_name(key);
            assert_eq!(parse_key_name(name).ok(), Some(key), "name {name:?}");
        }
    }

    /// The config line splits at its *first* `=`, and the position of that
    /// separator is never carried in a variable that a later expression has to
    /// re-apply with an offset.
    #[test]
    fn a_config_line_splits_at_its_first_separator_only() {
        // A line with no separator is reported against the line it was on —
        // comments and blanks are skipped but still counted.
        assert!(matches!(
            HotkeyConfig::load("# comment\n\nCtrl+Q\n"),
            Err(HotkeyError::ParseError { line_number: 3, .. })
        ));

        // Whitespace around the separator belongs to neither side.
        match (
            HotkeyConfig::load("  Ctrl+Q  =  close_window  \n"),
            HotkeyConfig::load("Ctrl+Q=close_window\n"),
        ) {
            (Ok(spaced), Ok(tight)) => {
                assert_eq!(spaced.bindings.len(), 1);
                assert_eq!(spaced.save(), tight.save());
            }
            _ => panic!("a well-formed line failed to parse"),
        }

        // The value may itself contain a separator: only the first one splits.
        match HotkeyConfig::load("Super+E=launch:a=b\n") {
            Ok(cfg) => assert_eq!(
                cfg.bindings.first().map(|(_, a)| a.to_config_value()),
                Some("launch:a=b".to_owned()),
            ),
            Err(e) => panic!("{e}"),
        }
    }

    // --- key badge sizing ---

    #[test]
    fn a_key_badge_fits_its_key_name() {
        // A hotkey display name is split on '+' and each part gets its own
        // badge. Sized at 0.6 em per byte, "Backspace" fit but "Entf" (German
        // Delete) or any accented key name did not.
        for part in ["Ctrl", "Shift", "Backspace", "F11", "Entf", "→"] {
            let w = guitk::text::padded_width(part, 6.0, KEY_FONT_SIZE, FontWeightHint::Regular);
            let drawn = guitk::text::measure(part, KEY_FONT_SIZE, FontWeightHint::Regular);
            assert!(drawn + 12.0 <= w + 0.01, "{part:?} overflows its badge");
        }
    }
    use guitk::event::{Key, Modifiers};

    // ====================================================================
    // Hotkey construction and matching
    // ====================================================================

    #[test]
    fn test_hotkey_new() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        assert_eq!(hk.key, Key::F4);
        assert!(hk.alt);
        assert!(!hk.ctrl);
        assert!(!hk.shift);
        assert!(!hk.super_key);
    }

    #[test]
    fn test_hotkey_bare() {
        let hk = Hotkey::bare(Key::PrintScreen);
        assert_eq!(hk.key, Key::PrintScreen);
        assert!(!hk.alt);
        assert!(!hk.ctrl);
        assert!(!hk.shift);
        assert!(!hk.super_key);
    }

    #[test]
    fn test_hotkey_matches_positive() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(hk.matches(Key::F4, &m));
    }

    #[test]
    fn test_hotkey_matches_negative_wrong_key() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(!hk.matches(Key::F5, &m));
    }

    #[test]
    fn test_hotkey_matches_negative_wrong_modifier() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(!hk.matches(Key::F4, &m));
    }

    /// Rule one: a dedicated key means the same thing whatever is held.
    ///
    /// Volume Up with Shift down is Volume Up. The alternative — exact matching
    /// here too — is a volume key that stops working for a reason the user
    /// cannot see and could not describe: they were holding a modifier for
    /// something else at the time.
    ///
    /// Applied on the way *in* as well as out, so a registry cannot end up
    /// holding a `Ctrl+VolumeUp` binding that no press can ever reach. Asking
    /// for one is reported as the collision it is.
    #[test]
    fn a_dedicated_key_ignores_the_modifiers_that_happen_to_be_down() {
        let reg = HotkeyRegistry::defaults();
        for modifiers in ALL_MODIFIER_SETS {
            assert_eq!(
                reg.lookup(Key::VolumeUp, &modifiers),
                Some(&HotkeyAction::VolumeUp),
                "the volume key stopped working with {modifiers:?} held"
            );
        }

        let mut reg = HotkeyRegistry::new();
        reg.register(Hotkey::bare(Key::VolumeUp), HotkeyAction::VolumeUp)
            .expect("first claim");
        assert!(
            reg.register(
                Hotkey::new(Key::VolumeUp, Modifiers::ctrl()),
                HotkeyAction::VolumeDown
            )
            .is_err(),
            "Ctrl+VolumeUp was accepted as a separate binding, so it is a \
             binding no keystroke can reach"
        );
        assert_eq!(reg.len(), 1);
    }

    /// Rule two: a chord never carries the modifier its own key *is*.
    ///
    /// Whether the Super bit is set on the press of the Super key itself is the
    /// keyboard driver's business, and both answers are defensible. Normalising
    /// it away means the start menu opens either way, rather than depending on
    /// which driver is loaded.
    #[test]
    fn a_modifier_key_pressed_alone_does_not_carry_its_own_bit() {
        let reg = HotkeyRegistry::defaults();
        for key in [Key::LeftSuper, Key::RightSuper] {
            for modifiers in [Modifiers::NONE, sup()] {
                assert_eq!(
                    reg.lookup(key, &modifiers),
                    Some(&HotkeyAction::ToggleStartMenu),
                    "{key:?} with {modifiers:?} did not open the start menu"
                );
            }
        }
    }

    /// A grab names one exact chord, so a binding that answers several presses
    /// has to be claimed several times over.
    #[test]
    fn a_binding_is_grabbed_under_every_press_that_reaches_it() {
        let reg = HotkeyRegistry::defaults();
        let global = reg.global_chords();
        for modifiers in ALL_MODIFIER_SETS {
            assert!(
                global.contains(&(Key::VolumeUp, modifiers)),
                "the volume key is not claimed with {modifiers:?} held, so the \
                 press goes to the focused window instead"
            );
        }
        for modifiers in [Modifiers::NONE, sup()] {
            assert!(
                global.contains(&(Key::LeftSuper, modifiers)),
                "the Super key is not claimed with {modifiers:?}, so a driver \
                 that reports it that way cannot open the start menu"
            );
        }
        // And the ordinary case stays one chord: a shortcut that claimed the
        // sixteen near-misses of Alt+F4 would take Ctrl+Alt+F4 from every
        // application for nothing.
        assert_eq!(
            global
                .iter()
                .filter(|(key, _)| *key == Key::F4)
                .collect::<Vec<_>>(),
            vec![&(Key::F4, Modifiers::alt())]
        );
    }

    /// The card that lists the shortcuts is itself reachable by a shortcut, and
    /// that shortcut is on the card.
    ///
    /// Both halves matter. Without a binding the card is a surface nothing can
    /// open — which is what `TD-C-THE-SHORTCUT-CARD-HAS-NO-DOOR` recorded — and
    /// without the binding being *in the registry* the card would be the one
    /// shortcut the card does not mention, which is the shortcut a user who has
    /// closed it most needs to know.
    #[test]
    fn the_card_that_lists_the_shortcuts_lists_the_shortcut_that_opens_it() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Slash, &sup()),
            Some(&HotkeyAction::ToggleShortcutCard),
            "Super+/ does not open the shortcut card"
        );
        assert!(
            reg.all_bindings()
                .any(|(_, a)| *a == HotkeyAction::ToggleShortcutCard),
            "the card does not list its own chord"
        );
        assert!(
            reg.global_chords().contains(&(Key::Slash, sup())),
            "the card's chord is never grabbed, so the press reaches the \
             focused window instead"
        );
    }

    /// The card never runs off the bottom of the screen it is drawn on.
    ///
    /// The defect this pins is the one the single-column layout had the moment
    /// the shortcut card got a chord: twenty-five bindings at 38 units a row is
    /// 1,010 units of list, and on a 1080-line display three user bindings put
    /// the last rows below the edge — silently, with nothing on the card to say
    /// so. A user reads to the bottom of what they can see, does not find the
    /// shortcut, and concludes it does not exist.
    #[test]
    fn the_card_folds_into_columns_rather_than_running_off_the_screen() {
        let reg = HotkeyRegistry::defaults();
        for screen_h in [1080.0_f32, 800.0, 600.0, 480.0] {
            let (width, height) = settings_panel_size(&reg, screen_h);
            assert!(
                height <= screen_h,
                "on a {screen_h}-line display the card is {height} tall, so its \
                 last rows are drawn off the edge"
            );
            // And it is *drawn* the size it was measured, not merely measured
            // small: a fold that only the size function knows about would put
            // every row past the first column back in one long line.
            let cmds =
                render_settings_panel(&reg, &Palette::for_mode(false), 0.0, 0.0, None, screen_h);
            let lowest = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { y, .. } => Some(*y),
                    RenderCommand::FillRect { y, height, .. } => Some(y + height),
                    _ => None,
                })
                .fold(0.0_f32, f32::max);
            assert!(
                lowest <= height,
                "something is drawn at {lowest}, below the card's own {height}"
            );
            // Every row is still on the card: nothing is dropped to make it fit.
            let labels = cmds
                .iter()
                .filter(|c| matches!(c, RenderCommand::Text { .. }))
                .count();
            assert!(
                labels > reg.len(),
                "{labels} pieces of text for {} bindings plus a header — rows \
                 have gone missing",
                reg.len()
            );
            assert!(
                width >= PANEL_WIDTH,
                "the card got narrower instead of wider"
            );
        }
    }

    /// A card that fits is left alone: the fold is for the case that needs it.
    #[test]
    fn a_card_that_fits_stays_one_column_wide() {
        let (width, _) = settings_panel_size(&HotkeyRegistry::defaults(), ROOMY);
        assert_eq!(
            width, PANEL_WIDTH,
            "the card grew a second column with the whole screen to itself"
        );
    }

    /// The card is measured before it is drawn, and the two answers agree.
    ///
    /// `render_shortcut_card` centres the panel using [`settings_panel_size`];
    /// if that reported a height the drawing did not fill, the card would sit
    /// off-centre by half the error, and the gap would grow with every binding
    /// a user added.
    #[test]
    fn the_card_is_as_tall_as_it_was_measured_to_be() {
        let reg = HotkeyRegistry::defaults();
        let (width, height) = settings_panel_size(&reg, ROOMY);
        let cmds = render_settings_panel(&reg, &Palette::for_mode(false), 0.0, 0.0, None, ROOMY);
        let Some(RenderCommand::FillRect {
            width: bg_width,
            height: bg_height,
            ..
        }) = cmds
            .iter()
            .find(|c| matches!(c, RenderCommand::FillRect { .. }))
        else {
            panic!("the card has no background, so it has no size to agree about");
        };
        assert!(
            (*bg_width - width).abs() < f32::EPSILON,
            "measured {width} wide, drawn {bg_width}"
        );
        assert!(
            (*bg_height - height).abs() < f32::EPSILON,
            "measured {height} tall, drawn {bg_height}"
        );
        // And the height tracks the contents rather than being a constant that
        // happens to match today's defaults.
        let mut bigger = HotkeyRegistry::defaults();
        bigger
            .register(
                Hotkey::new(Key::F9, mods(true, false, false, false)),
                HotkeyAction::ShowDesktop,
            )
            .expect("Ctrl+F9 is unbound in the defaults");
        let (_, taller) = settings_panel_size(&bigger, ROOMY);
        assert!(
            taller > height,
            "one more binding did not make the card any taller ({taller} vs {height})"
        );
    }

    #[test]
    fn test_hotkey_matches_extra_modifier_fails() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        };
        // Extra shift should NOT match — exact modifier match required.
        assert!(!hk.matches(Key::F4, &m));
    }

    #[test]
    fn test_hotkey_display_name_simple() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        assert_eq!(hk.display_name(), "Alt+F4");
    }

    #[test]
    fn test_hotkey_display_name_multi_modifier() {
        let hk = Hotkey::new(Key::Delete, mods(true, true, false, false));
        assert_eq!(hk.display_name(), "Ctrl+Alt+Delete");
    }

    #[test]
    fn test_hotkey_display_name_bare() {
        let hk = Hotkey::bare(Key::PrintScreen);
        assert_eq!(hk.display_name(), "PrintScreen");
    }

    #[test]
    fn test_hotkey_display_name_bare_super() {
        // LeftSuper as a principal key (not modifier) must serialize distinctly.
        let hk = Hotkey::bare(Key::LeftSuper);
        assert_eq!(hk.display_name(), "LeftSuper");
    }

    #[test]
    fn test_hotkey_modifiers_roundtrip() {
        let m = mods(true, false, true, false);
        let hk = Hotkey::new(Key::A, m);
        let m2 = hk.modifiers();
        assert_eq!(m2.ctrl, true);
        assert_eq!(m2.alt, false);
        assert_eq!(m2.shift, true);
        assert_eq!(m2.super_key, false);
    }

    // ====================================================================
    // Registry: register / unregister / lookup
    // ====================================================================

    #[test]
    fn test_registry_empty() {
        let reg = HotkeyRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn test_registry_register_and_lookup() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        assert_eq!(reg.len(), 1);

        let found = reg.lookup(Key::F4, &Modifiers::alt());
        assert_eq!(found, Some(&HotkeyAction::CloseWindow));
    }

    #[test]
    fn test_registry_lookup_miss() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();

        let found = reg.lookup(Key::F5, &Modifiers::alt());
        assert!(found.is_none());
    }

    #[test]
    fn test_registry_conflict_error() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();

        let result = reg.register(hk, HotkeyAction::MinimizeWindow);
        assert!(result.is_err());
        match result {
            Err(HotkeyError::Conflict { existing, .. }) => {
                assert_eq!(existing, HotkeyAction::CloseWindow);
            }
            _ => panic!("expected Conflict error"),
        }
    }

    #[test]
    fn test_registry_idempotent_register() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        // Re-registering the same action should succeed.
        let result = reg.register(hk, HotkeyAction::CloseWindow);
        assert!(result.is_ok());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_unregister() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        assert!(reg.unregister(&hk));
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_unregister_missing() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::bare(Key::A);
        assert!(!reg.unregister(&hk));
    }

    #[test]
    fn test_registry_is_registered() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::bare(Key::PrintScreen);
        assert!(!reg.is_registered(&hk));
        reg.register(hk, HotkeyAction::Screenshot).ok();
        assert!(reg.is_registered(&hk));
    }

    #[test]
    fn test_registry_conflicts_with() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::D, mods(false, false, false, true));
        reg.register(hk, HotkeyAction::ShowDesktop).ok();

        assert_eq!(reg.conflicts_with(&hk), Some(&HotkeyAction::ShowDesktop));

        let free = Hotkey::bare(Key::A);
        assert!(reg.conflicts_with(&free).is_none());
    }

    #[test]
    fn test_registry_all_bindings_iteration() {
        let mut reg = HotkeyRegistry::new();
        reg.register(Hotkey::bare(Key::A), HotkeyAction::VolumeUp)
            .ok();
        reg.register(Hotkey::bare(Key::B), HotkeyAction::VolumeDown)
            .ok();
        let bindings: Vec<_> = reg.all_bindings().collect();
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn test_registry_clear() {
        let mut reg = HotkeyRegistry::defaults();
        assert!(!reg.is_empty());
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_reset_defaults() {
        let mut reg = HotkeyRegistry::new();
        reg.register(
            Hotkey::bare(Key::Z),
            HotkeyAction::LaunchApp("/usr/bin/test".into()),
        )
        .ok();
        reg.reset_defaults();
        // Should no longer have the user's binding.
        assert!(reg.lookup(Key::Z, &Modifiers::NONE).is_none());
        // Should have the standard bindings.
        assert!(reg.lookup(Key::F4, &Modifiers::alt()).is_some());
    }

    // ====================================================================
    // Defaults
    // ====================================================================

    /// No two entries in the default table claim the same chord.
    ///
    /// `install_defaults` registers with `let _ =`, because a shell that refused
    /// to start over a duplicated shortcut would be worse than one that dropped
    /// the second entry — so a collision is silent at runtime and this is the
    /// only thing that reports it. What it would look like in use: a shortcut
    /// listed on the reference card that simply does something else, with
    /// nothing anywhere saying why.
    ///
    /// Counted rather than compared, because `register` refuses the *second*
    /// claim: the registry ends up one binding short of the list that built it.
    #[test]
    fn no_default_binding_collides_with_another() {
        let mut reg = HotkeyRegistry::new();
        let mut expected = 0usize;
        let mut collisions = Vec::new();
        for (hotkey, action) in HotkeyRegistry::defaults().all_bindings() {
            expected += 1;
            if let Err(e) = reg.register(*hotkey, action.clone()) {
                collisions.push(format!("{e}"));
            }
        }
        assert!(
            collisions.is_empty(),
            "the default table claims a chord twice: {collisions:?}"
        );
        assert_eq!(reg.len(), expected);
    }

    /// Every fixed command a shortcut can start names a program the shell knows
    /// about.
    ///
    /// These five paths are string constants, so nothing but this test connects
    /// them to the programs that exist. A typo in one is a shortcut that grabs
    /// its chord from the whole desktop and then asks for a binary that is not
    /// there — and the user sees a key that does nothing, with no error to
    /// search for. The start menu's own database is the authority, because it is
    /// what launches these same programs when they are clicked instead of typed.
    #[test]
    fn every_command_a_shortcut_names_is_a_program_the_shell_knows_about() {
        let known: Vec<String> = crate::launcher::builtin_app_database()
            .iter()
            .map(|app| app.executable_path.clone())
            .collect();
        // The fixed-command actions, plus whatever the *default* table puts on
        // `LaunchApp` — the one action whose command is not a constant. A
        // `LaunchApp` the user typed is not checked and cannot be: it is the
        // variant that exists to name a program this shell has never heard of.
        let defaults = HotkeyRegistry::defaults();
        let checked = every_action()
            .into_iter()
            .filter(|a| !matches!(a, HotkeyAction::LaunchApp(_)))
            .chain(defaults.all_bindings().map(|(_, a)| a.clone()));
        for action in checked {
            let Some(command) = action.command() else {
                continue;
            };
            // The database stores the program, not the invocation, so a command
            // with flags is checked on its program word. The flags themselves
            // are pinned by the constants' own doc against the tool's `main`.
            let program = command.split_whitespace().next().unwrap_or(command);
            assert!(
                known.iter().any(|c| c == program),
                "{action:?} starts {program:?}, which no start-menu entry names \
                 — so either the path is a typo or the program does not exist"
            );
        }
    }

    #[test]
    fn test_defaults_contains_alt_f4() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::F4, &Modifiers::alt());
        assert_eq!(action, Some(&HotkeyAction::CloseWindow));
    }

    #[test]
    fn test_defaults_contains_super_d() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::D, &mods(false, false, false, true));
        assert_eq!(action, Some(&HotkeyAction::ShowDesktop));
    }

    #[test]
    fn test_defaults_contains_ctrl_alt_delete() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::Delete, &mods(true, true, false, false));
        assert_eq!(action, Some(&HotkeyAction::ShowTaskManager));
    }

    #[test]
    fn test_defaults_contains_printscreen() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::PrintScreen, &Modifiers::NONE);
        assert_eq!(action, Some(&HotkeyAction::Screenshot));
    }

    #[test]
    fn test_defaults_contains_super_left_right() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Left, &mods(false, false, false, true)),
            Some(&HotkeyAction::SnapLeft)
        );
        assert_eq!(
            reg.lookup(Key::Right, &mods(false, false, false, true)),
            Some(&HotkeyAction::SnapRight)
        );
    }

    /// The bug that whole-modifier-set matching exists to make impossible.
    ///
    /// The live bindings were once a chain of `if`s that each tested only the
    /// modifiers it cared about, so `Super+Left` — tested first — answered a
    /// `Ctrl+Super+Left` press too, and the virtual-desktop shortcuts below had
    /// never once fired. Keyed on the exact modifier set, the near-miss is a
    /// different key in the map and cannot shadow anything.
    #[test]
    fn a_shortcut_does_not_answer_for_a_chord_with_one_more_modifier() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Left, &mods(true, false, false, true)),
            Some(&HotkeyAction::PreviousDesktop),
            "Ctrl+Super+Left is the previous desktop, not a snap"
        );
        assert_eq!(
            reg.lookup(Key::Right, &mods(true, false, false, true)),
            Some(&HotkeyAction::NextDesktop),
            "Ctrl+Super+Right is the next desktop, not a snap"
        );
    }

    #[test]
    fn test_defaults_contains_super_l_lock() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::L, &mods(false, false, false, true)),
            Some(&HotkeyAction::ScreenLock)
        );
    }

    #[test]
    fn test_defaults_contains_alt_tab() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Tab, &Modifiers::alt()),
            Some(&HotkeyAction::CycleWindows)
        );
    }

    #[test]
    fn test_defaults_contains_super_shift_s() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::S, &mods(false, false, true, true)),
            Some(&HotkeyAction::ScreenshotRegion)
        );
    }

    #[test]
    fn test_defaults_contains_super_r() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::R, &mods(false, false, false, true)),
            Some(&HotkeyAction::ToggleRunDialog)
        );
    }

    #[test]
    fn test_defaults_contains_super_i() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::I, &mods(false, false, false, true)),
            Some(&HotkeyAction::SystemSettings)
        );
    }

    #[test]
    fn test_defaults_contains_super_e() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::E, &mods(false, false, false, true)),
            Some(&HotkeyAction::LaunchApp("/usr/bin/explorer".to_string()))
        );
    }

    // ====================================================================
    // Key parsing
    // ====================================================================

    #[test]
    fn test_parse_key_name_letters() {
        assert_eq!(parse_key_name("A").ok(), Some(Key::A));
        assert_eq!(parse_key_name("z").ok(), Some(Key::Z));
    }

    #[test]
    fn test_parse_key_name_function_keys() {
        assert_eq!(parse_key_name("F1").ok(), Some(Key::F1));
        assert_eq!(parse_key_name("f12").ok(), Some(Key::F12));
    }

    #[test]
    fn test_parse_key_name_navigation() {
        assert_eq!(parse_key_name("Left").ok(), Some(Key::Left));
        assert_eq!(parse_key_name("pageup").ok(), Some(Key::PageUp));
        assert_eq!(parse_key_name("Home").ok(), Some(Key::Home));
    }

    #[test]
    fn test_parse_key_name_special() {
        assert_eq!(parse_key_name("Space").ok(), Some(Key::Space));
        assert_eq!(parse_key_name("Enter").ok(), Some(Key::Enter));
        assert_eq!(parse_key_name("return").ok(), Some(Key::Enter));
        assert_eq!(parse_key_name("Esc").ok(), Some(Key::Escape));
    }

    #[test]
    fn test_parse_key_name_unknown() {
        assert!(parse_key_name("Nosuchkey").is_err());
    }

    #[test]
    fn test_parse_hotkey_string_simple() {
        let hk = parse_hotkey_string("Alt+F4").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::F4);
        assert!(hk.alt);
        assert!(!hk.ctrl);
    }

    #[test]
    fn test_parse_hotkey_string_multi_modifier() {
        let hk = parse_hotkey_string("Ctrl+Alt+Delete").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::Delete);
        assert!(hk.ctrl);
        assert!(hk.alt);
    }

    #[test]
    fn test_parse_hotkey_string_bare() {
        let hk = parse_hotkey_string("PrintScreen").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::PrintScreen);
        assert!(!hk.ctrl);
        assert!(!hk.alt);
    }

    // ====================================================================
    // Config persistence
    // ====================================================================

    #[test]
    fn test_config_save_load_roundtrip() {
        let reg = HotkeyRegistry::defaults();
        let config = HotkeyConfig::from_registry(&reg);
        let text = config.save();

        let loaded = HotkeyConfig::load(&text);
        assert!(loaded.is_ok());
        let restored = loaded.ok().map(|c| c.into_registry());
        assert!(restored.is_some());
        let restored = restored.unwrap();

        // Every original binding should be present in the restored registry.
        for (hk, action) in reg.all_bindings() {
            let found = restored.lookup(hk.key, &hk.modifiers());
            assert_eq!(
                found,
                Some(action),
                "binding {} not restored",
                hk.display_name()
            );
        }
    }

    #[test]
    fn test_config_load_comments_and_blanks() {
        let text = "# comment\n\nAlt+F4=close_window\n# another comment\n";
        let config = HotkeyConfig::load(text);
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.len(), 1);
    }

    #[test]
    fn test_config_load_parse_error_no_equals() {
        let text = "Alt+F4 close_window\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_unknown_action() {
        let text = "Alt+F4=nonexistent_action\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_unknown_key() {
        let text = "Alt+Bogus=close_window\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_launch_action() {
        let text = "Super+E=launch:/usr/bin/explorer\n";
        let config = HotkeyConfig::load(text).ok();
        assert!(config.is_some());
        let reg = config.unwrap().into_registry();
        assert_eq!(
            reg.lookup(Key::E, &mods(false, false, false, true)),
            Some(&HotkeyAction::LaunchApp("/usr/bin/explorer".to_string()))
        );
    }

    #[test]
    fn test_config_load_switch_desktop() {
        let text = "Ctrl+Super+Left=switch_desktop:0\n";
        let config = HotkeyConfig::load(text).ok();
        assert!(config.is_some());
        let reg = config.unwrap().into_registry();
        assert_eq!(
            reg.lookup(Key::Left, &mods(true, false, false, true)),
            Some(&HotkeyAction::SwitchDesktop(0))
        );
    }

    /// `custom:whatever` used to parse, and named a string nobody read.
    ///
    /// A binding on it grabbed the chord from every application on the desktop
    /// and then did nothing with it — the worst of both, since the key was gone
    /// *and* the shortcut was dead. Now the line is rejected, which is the
    /// honest answer: the shell cannot perform an action it has no name for.
    #[test]
    fn a_config_line_naming_an_action_the_shell_cannot_perform_is_refused() {
        assert!(matches!(
            HotkeyConfig::load("Ctrl+Shift+X=custom:my_action\n"),
            Err(HotkeyError::ParseError { .. })
        ));
    }

    #[test]
    fn test_config_reset_defaults() {
        let config = HotkeyConfig::reset_defaults();
        assert!(!config.is_empty());
        let reg = config.into_registry();
        assert_eq!(
            reg.lookup(Key::F4, &Modifiers::alt()),
            Some(&HotkeyAction::CloseWindow)
        );
    }

    // ====================================================================
    // Action serialization
    // ====================================================================

    /// Every action there is, listed once.
    ///
    /// Spelled out rather than derived, because the point of the list is to be
    /// the *second* opinion about what the enum contains: a `strum`-style
    /// iterator built from the same enum would agree with a mistake. Adding a
    /// variant and forgetting this list costs one failing test; adding a variant
    /// and forgetting `to_config_value` costs a user their settings file.
    fn every_action() -> Vec<HotkeyAction> {
        vec![
            HotkeyAction::CloseWindow,
            HotkeyAction::MinimizeWindow,
            HotkeyAction::MaximizeWindow,
            HotkeyAction::RestoreOrMinimize,
            HotkeyAction::SnapLeft,
            HotkeyAction::SnapRight,
            HotkeyAction::ToggleZoneOverlay,
            HotkeyAction::ShowDesktop,
            HotkeyAction::CycleWindows,
            HotkeyAction::CycleWindowsBackwards,
            HotkeyAction::ToggleOverview,
            HotkeyAction::PreviousDesktop,
            HotkeyAction::NextDesktop,
            HotkeyAction::SwitchDesktop(3),
            HotkeyAction::ToggleStartMenu,
            HotkeyAction::ToggleRunDialog,
            HotkeyAction::ToggleNotifications,
            HotkeyAction::ToggleShortcutCard,
            HotkeyAction::DismissPopup,
            HotkeyAction::VolumeUp,
            HotkeyAction::VolumeDown,
            HotkeyAction::VolumeMute,
            HotkeyAction::BrightnessUp,
            HotkeyAction::BrightnessDown,
            HotkeyAction::LaunchApp("/usr/bin/my_app".to_string()),
            HotkeyAction::ShowTaskManager,
            HotkeyAction::SystemSettings,
            HotkeyAction::ScreenLock,
            HotkeyAction::Screenshot,
            HotkeyAction::ScreenshotRegion,
        ]
    }

    #[test]
    fn test_action_config_roundtrip() {
        let actions = every_action();

        for action in &actions {
            let serialized = action.to_config_value();
            let parsed = HotkeyAction::from_config_value(&serialized);
            assert!(
                parsed.is_ok(),
                "failed to parse '{}' for {:?}",
                serialized,
                action
            );
            assert_eq!(
                parsed.ok().as_ref(),
                Some(action),
                "roundtrip mismatch for {:?}",
                action
            );
        }
    }

    // ====================================================================
    // Display label
    // ====================================================================

    #[test]
    fn test_action_display_labels() {
        assert_eq!(HotkeyAction::CloseWindow.display_label(), "Close Window");
        assert_eq!(
            HotkeyAction::ToggleOverview.display_label(),
            "Window Overview"
        );
        assert_eq!(
            HotkeyAction::LaunchApp("x".into()).display_label(),
            "Launch App"
        );
    }

    /// No action shares a label with another.
    ///
    /// The reference card is a column of labels, and two rows reading the same
    /// thing under different chords is a card that cannot be used for what it is
    /// for. The risk is real because the labels are written by hand next to each
    /// other and several are near-synonyms — "Snap Left" and "Move Window Left"
    /// were the same action under two names before these tables were merged.
    #[test]
    fn no_two_actions_read_the_same_on_the_reference_card() {
        let mut seen: std::collections::BTreeMap<String, HotkeyAction> =
            std::collections::BTreeMap::new();
        for action in every_action() {
            let label = action.display_label().to_owned();
            if let Some(other) = seen.get(&label) {
                panic!("{action:?} and {other:?} both read {label:?}");
            }
            seen.insert(label, action);
        }
    }

    // ====================================================================
    // Error display
    // ====================================================================

    #[test]
    fn test_error_display_conflict() {
        let err = HotkeyError::Conflict {
            hotkey: Hotkey::new(Key::F4, Modifiers::alt()),
            existing: HotkeyAction::CloseWindow,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Alt+F4"));
        assert!(msg.contains("CloseWindow"));
    }

    #[test]
    fn test_error_display_parse() {
        let err = HotkeyError::ParseError {
            line_number: 5,
            message: "bad line".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("line 5"));
        assert!(msg.contains("bad line"));
    }

    // ====================================================================
    // Hotkey ordering (for BTreeMap determinism)
    // ====================================================================

    #[test]
    fn test_hotkey_ord_stable() {
        let a = Hotkey::bare(Key::A);
        let b = Hotkey::bare(Key::B);
        assert!(a < b);

        let ctrl_a = Hotkey::new(Key::A, Modifiers::ctrl());
        // Modifiers sort before bare keys (ctrl=true > ctrl=false).
        assert!(a < ctrl_a);
    }

    // ====================================================================
    // Settings panel rendering
    // ====================================================================

    #[test]
    fn test_render_settings_panel_nonempty() {
        let reg = HotkeyRegistry::defaults();
        let cmds = render_settings_panel(&reg, &accented(false), 100.0, 100.0, None, ROOMY);
        // Should produce a non-trivial number of render commands:
        // shadow + bg + border + clip + header text + separator + rows.
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_settings_panel_empty_registry() {
        let reg = HotkeyRegistry::new();
        let cmds = render_settings_panel(&reg, &accented(false), 0.0, 0.0, None, ROOMY);
        // Should still render header (shadow + bg + border + clip + header + sep + popclip).
        assert!(cmds.len() >= 6);
    }

    #[test]
    fn test_render_settings_panel_with_selection() {
        let reg = HotkeyRegistry::defaults();
        let cmds = render_settings_panel(&reg, &accented(false), 50.0, 50.0, Some(0), ROOMY);
        // Should have at least one extra FillRect for the selection highlight.
        let fill_rects = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        // 1 (shadow is BoxShadow) + 1 bg + 1 selection + N badges.
        assert!(fill_rects >= 3);
    }

    // ====================================================================
    // Colour — part 2 of
    // TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE
    // ====================================================================

    /// A palette whose accent belongs to no palette.
    ///
    /// Judgement 2 is a claim that *nothing* in this panel is accented, and at
    /// the shipped theme that claim cannot fail: the stock accent **is**
    /// `blue`, so a site wrongly repainted with the accent would be
    /// indistinguishable from one correctly drawing a role. Substituting a
    /// colour outside the palette makes the count of accented commands
    /// meaningful; the loop proves the substitute really is outside it, rather
    /// than colliding with a role some other test then reads by accident.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            if name == "accent" {
                continue;
            }
            assert_ne!(
                (role.r, role.g, role.b),
                (p.accent.r, p.accent.g, p.accent.b),
                "the substitute accent collides with {name}, so an accent \
                 assertion would be reading that role instead"
            );
        }
        p
    }

    /// Every colour any command in `cmds` will put on the screen.
    fn color_of(cmd: &RenderCommand) -> Option<Color> {
        match cmd {
            RenderCommand::FillRect { color, .. }
            | RenderCommand::StrokeRect { color, .. }
            | RenderCommand::Text { color, .. }
            | RenderCommand::Line { color, .. }
            | RenderCommand::BoxShadow { color, .. } => Some(*color),
            _ => None,
        }
    }

    /// The command lines the default set draws beside its labels.
    ///
    /// Asked of the registry rather than written out, because the badge
    /// assertions below work by *elimination* — badge ink is the small text
    /// that is not a detail line — and a list that fell behind the defaults
    /// would quietly reclassify a command line as a key badge and assert the
    /// wrong colour on it. There were six of these the day this was written and
    /// exactly one before the two shortcut tables were merged, so the list
    /// falling behind is not hypothetical.
    fn detail_lines() -> Vec<String> {
        HotkeyRegistry::defaults()
            .all_bindings()
            .filter_map(|(_, action)| action.command().map(str::to_owned))
            .collect()
    }

    /// The colour of the one `Text` command reading exactly `s`.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "expected exactly one Text reading {s:?}");
        hits[0]
    }

    /// Perceived brightness, the same weighting [`appearance::readable_on`]
    /// uses — so "dimmer than" means the same thing here as it does there.
    fn luma(c: Color) -> f32 {
        0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b)
    }

    /// The whole-panel sweep: nothing is drawn that the palette cannot
    /// account for, in either mode, with and without a selection, and with an
    /// empty registry as well as the default one.
    ///
    /// The light render is the one that matters. Every constant this
    /// conversion deleted was a Catppuccin **Mocha** value, so a substitution
    /// that was missed is invisible in dark mode — it draws exactly what it
    /// always drew — and names itself the moment the light palette is handed
    /// in.
    #[test]
    fn every_colour_this_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for empty in [false, true] {
                let reg = if empty {
                    HotkeyRegistry::new()
                } else {
                    HotkeyRegistry::defaults()
                };
                let last = reg.len().saturating_sub(1);
                for sel in [None, Some(0), Some(last)] {
                    let cmds = render_settings_panel(&reg, &p, 40.0, 60.0, sel, ROOMY);
                    assert_drawn_from(&p, &cmds, &[], "hotkeys settings panel");
                }
            }
        }
        // A sweep over a panel that drew nothing would pass vacuously.
        let p = accented(true);
        let cmds = render_settings_panel(&HotkeyRegistry::defaults(), &p, 0.0, 0.0, Some(3), ROOMY);
        assert!(cmds.len() > 40, "the fixture rendered almost nothing");
    }

    /// None of the ten deleted constants is still drawn.
    ///
    /// The sweep above catches a leftover by *membership*; this catches it by
    /// *name*, so a failure says which constant survived rather than only that
    /// some colour is foreign. The two overlap deliberately — the overlap is
    /// what turns "a colour is wrong" into "`MANTLE` is still there".
    #[test]
    fn none_of_the_ten_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 10] = [
            ("BASE", 0x001E_1E2E),
            ("MANTLE", 0x0018_1825),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("SURFACE2", 0x0058_5B70),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("SUBTEXT1", 0x00BA_C2DE),
            ("OVERLAY0", 0x006C_7086),
            ("SHADOW", 0x0000_0000),
        ];
        let p = accented(true);
        let reg = HotkeyRegistry::defaults();
        let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, Some(0), ROOMY);
        for cmd in &cmds {
            let Some(c) = color_of(cmd) else { continue };
            // `SHADOW` was black, and black stays black in both modes on
            // purpose — a shadow is an absence of light. It is in the list
            // above only so the list is visibly the whole `mod theme`, and it
            // is the one entry that cannot be checked this way.
            if c.r == 0 && c.g == 0 && c.b == 0 {
                continue;
            }
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, hex) in DELETED {
                assert_ne!(
                    rgb, hex,
                    "the light render still draws Mocha {name}, so that \
                     constant survived the conversion"
                );
            }
        }
    }

    /// Every site draws the role it claims — pinned one site at a time.
    ///
    /// A membership sweep cannot see a *permutation*: swap the badge fill and
    /// the separator and every colour drawn is still a role of the palette, so
    /// the sweep stays silent while the panel is visibly wrong. Only a table
    /// indexed by site catches that, which is why this exists beside the sweep
    /// rather than instead of it.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let reg = HotkeyRegistry::defaults();
            let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, Some(0), ROOMY);

            // Panel chrome, identified by the panel's own full width.
            let shadow = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::BoxShadow { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("no shadow");
            assert_eq!((shadow.r, shadow.g, shadow.b), (0, 0, 0), "shadow");
            assert_eq!(shadow, p.shadow(), "shadow");

            let bg = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::FillRect { width, color, .. } if *width == PANEL_WIDTH => {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("no panel background");
            assert_eq!(bg, p.panel_bg(), "panel background");

            let border = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::StrokeRect { width, color, .. } if *width == PANEL_WIDTH => {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("no panel border");
            assert_eq!(border, p.surface2, "panel border");

            assert_eq!(
                text_color(&cmds, "Keyboard Shortcuts"),
                p.text,
                "header ink"
            );

            let sep = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Line { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("no separator");
            assert_eq!(sep, p.surface1, "header separator");

            // The selection highlight: the only fill of the row's height.
            let hl: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { height, color, .. }
                        if *height == ROW_HEIGHT - 4.0 =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(hl, vec![p.surface0], "selection highlight");

            // Badge chrome: every fill and stroke of the badge height.
            let badge_fills: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { height, color, .. }
                        if *height == KEY_BADGE_HEIGHT =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert!(badge_fills.len() > 10, "too few badges to be a check");
            assert!(
                badge_fills.iter().all(|c| *c == p.mantle),
                "badge background"
            );

            let badge_borders: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { height, color, .. }
                        if *height == KEY_BADGE_HEIGHT =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(badge_borders.len(), badge_fills.len(), "badge border count");
            assert!(
                badge_borders.iter().all(|c| *c == p.surface1),
                "badge border"
            );

            // Badge ink: the `KEY_FONT_SIZE` text that is not one of the
            // command lines the default set draws beside its labels.
            let details = detail_lines();
            let badge_ink: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        font_size,
                        text,
                        color,
                        ..
                    } if *font_size == KEY_FONT_SIZE && !details.contains(text) => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(badge_ink.len(), badge_fills.len(), "badge ink count");
            assert!(badge_ink.iter().all(|c| *c == p.subtext0), "badge ink");

            for detail in &details {
                assert_eq!(text_color(&cmds, detail), p.overlay0, "detail ink");
            }

            // Both branches of the label, in one render: row 0 is selected
            // and every other row is not. A fixture that rendered only one
            // branch would leave the other unchecked.
            let labels: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        font_size, color, ..
                    } if *font_size == LABEL_FONT_SIZE => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(labels.len(), reg.len(), "one label per row");
            assert_eq!(labels[0], p.text, "selected label ink");
            assert!(
                labels[1..].iter().all(|c| *c == p.subtext1),
                "unselected label ink"
            );
        }
    }

    /// Judgement 2, stated as a count: this panel accents nothing.
    ///
    /// Stated as a count rather than as "the selection is `surface0`" because
    /// the interesting failure is not the selection changing role — the pin
    /// table catches that — but some *other* site quietly acquiring the
    /// accent. Only a count over the whole panel can see that, and only an
    /// accent that is outside the palette makes the count mean anything.
    #[test]
    fn nothing_in_this_panel_is_accented() {
        for light in [false, true] {
            let p = accented(light);
            for empty in [false, true] {
                let reg = if empty {
                    HotkeyRegistry::new()
                } else {
                    HotkeyRegistry::defaults()
                };
                let last = reg.len().saturating_sub(1);
                for sel in [None, Some(0), Some(last)] {
                    let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, sel, ROOMY);
                    let accented_count = cmds
                        .iter()
                        .filter_map(color_of)
                        .filter(|c| (c.r, c.g, c.b) == (p.accent.r, p.accent.g, p.accent.b))
                        .count();
                    assert_eq!(
                        accented_count, 0,
                        "a hotkey card is read, not operated: nothing on it \
                         is in force, so nothing on it should wear the \
                         accent (light={light}, empty={empty}, sel={sel:?})"
                    );
                }
            }
        }
    }

    /// Judgement 1: the panel is as transparent as the user asked.
    ///
    /// The alpha used to be `240`, frozen into a constant, so this panel
    /// ignored the transparency setting in both directions at once — visible
    /// to a user who had turned transparency *off*, and more solid than every
    /// neighbouring popup for a user who had turned it up. The shadow is
    /// checked alongside it precisely because it must *not* move: it is black
    /// at a fixed alpha in both modes, and a fix that made everything follow
    /// `panel_alpha` would be as wrong as the constant was.
    #[test]
    fn the_panel_is_as_transparent_as_the_user_asked() {
        let reg = HotkeyRegistry::defaults();
        for light in [false, true] {
            let mut seen: Vec<u8> = Vec::new();
            for alpha in [255_u8, 200, 160] {
                let mut p = accented(light);
                p.panel_alpha = alpha;
                let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, None, ROOMY);

                let bg = cmds
                    .iter()
                    .find_map(|c| match c {
                        RenderCommand::FillRect { width, color, .. } if *width == PANEL_WIDTH => {
                            Some(*color)
                        }
                        _ => None,
                    })
                    .expect("no panel background");
                assert_eq!(
                    bg.a, alpha,
                    "the panel background ignored panel_alpha={alpha}"
                );
                assert_eq!((bg.r, bg.g, bg.b), (p.base.r, p.base.g, p.base.b));
                seen.push(bg.a);

                let shadow = cmds
                    .iter()
                    .find_map(|c| match c {
                        RenderCommand::BoxShadow { color, .. } => Some(*color),
                        _ => None,
                    })
                    .expect("no shadow");
                assert_eq!(
                    shadow,
                    p.shadow(),
                    "the shadow followed the transparency setting, but a \
                     shadow is an absence of light and does not thin out \
                     when a panel does"
                );
            }
            assert_eq!(seen, vec![255, 200, 160], "the three levels ran");
        }
    }

    /// Judgement 3: a selected row is marked twice, and moving the selection
    /// moves both marks together.
    ///
    /// Indexed by row rather than counted, because a count cannot tell a
    /// selection that landed on the *wrong* row from one that landed on the
    /// right one — and an off-by-one between the highlight's `y` and the
    /// label's is exactly the bug a list like this grows.
    #[test]
    fn a_selected_row_is_said_twice() {
        let reg = HotkeyRegistry::defaults();
        let rows = reg.len();
        assert!(rows > 3, "the default set is too small to index into");
        let p = accented(true);

        // With no selection at all, neither saying appears anywhere.
        let none = render_settings_panel(&reg, &p, 0.0, 0.0, None, ROOMY);
        assert!(
            !none.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { height, .. } if *height == ROW_HEIGHT - 4.0
            )),
            "an unselected panel drew a selection highlight"
        );
        assert!(
            none.iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        font_size, color, ..
                    } if *font_size == LABEL_FONT_SIZE => Some(*color),
                    _ => None,
                })
                .all(|c| c == p.subtext1),
            "an unselected panel brightened a label"
        );

        for sel in [0, 1, rows / 2, rows - 1] {
            let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, Some(sel), ROOMY);

            // Saying one: exactly one highlight, at the selected row's y.
            let highlights: Vec<(f32, Color)> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        y, height, color, ..
                    } if *height == ROW_HEIGHT - 4.0 => Some((*y, *color)),
                    _ => None,
                })
                .collect();
            let want_y = HEADER_HEIGHT + sel as f32 * ROW_HEIGHT + 2.0;
            assert_eq!(highlights, vec![(want_y, p.surface0)], "highlight at {sel}");

            // Saying two: exactly one brightened label, and it is that row's.
            let labels: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        font_size, color, ..
                    } if *font_size == LABEL_FONT_SIZE => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(labels.len(), rows);
            for (i, c) in labels.iter().enumerate() {
                let want = if i == sel { p.text } else { p.subtext1 };
                assert_eq!(*c, want, "label ink at row {i} with row {sel} selected");
            }
        }

        // An out-of-range selection selects nothing rather than the last row.
        let over = render_settings_panel(&reg, &p, 0.0, 0.0, Some(rows), ROOMY);
        assert!(
            !over.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { height, .. } if *height == ROW_HEIGHT - 4.0
            )),
            "a selection past the end still highlighted a row"
        );
    }

    /// Judgement 4, as relations rather than literals.
    ///
    /// "The badge is `#181825`" is true in Mocha and false in Latte, so it is
    /// not the claim worth testing. What must hold in both modes is that a
    /// badge is *visibly a badge*: a different colour from the card it sits
    /// on, outlined in something different again, and lettered more quietly
    /// than the heading above it. Those survive the mode flip; the hex does
    /// not, which is the whole reason the constants had to go.
    #[test]
    fn a_key_badge_stands_off_the_panel_it_sits_on() {
        let reg = HotkeyRegistry::defaults();
        for light in [false, true] {
            let p = accented(light);
            let cmds = render_settings_panel(&reg, &p, 0.0, 0.0, None, ROOMY);
            let mode = if light { "light" } else { "dark" };

            let bg = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::FillRect { width, color, .. } if *width == PANEL_WIDTH => {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("no panel background");
            let badge = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::FillRect { height, color, .. }
                        if *height == KEY_BADGE_HEIGHT =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("no badge");
            let edge = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::StrokeRect { height, color, .. }
                        if *height == KEY_BADGE_HEIGHT =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("no badge border");

            assert_ne!(
                (badge.r, badge.g, badge.b),
                (bg.r, bg.g, bg.b),
                "in {mode} mode a key badge is the same colour as the card \
                 it sits on, so it does not read as a badge at all"
            );
            assert_ne!(
                (edge.r, edge.g, edge.b),
                (badge.r, badge.g, badge.b),
                "in {mode} mode a badge's border is the same colour as its \
                 fill, so the badge has no outline"
            );

            let header = text_color(&cmds, "Keyboard Shortcuts");
            let details = detail_lines();
            let ink = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text {
                        font_size,
                        text,
                        color,
                        ..
                    } if *font_size == KEY_FONT_SIZE && !details.contains(text) => Some(*color),
                    _ => None,
                })
                .expect("no badge ink");
            let detail = text_color(&cmds, details.first().expect("no detail line"));
            let quieter = |a: Color, b: Color| {
                // "Quieter" is distance from the panel, not absolute
                // darkness: in Latte the dimmer ink is the *lighter* one.
                (luma(a) - luma(bg)).abs() < (luma(b) - luma(bg)).abs()
            };
            assert!(
                quieter(ink, header),
                "in {mode} mode a key badge is lettered as loudly as the \
                 heading; a badge quotes a key, it is not a heading"
            );
            assert!(
                quieter(detail, ink),
                "in {mode} mode the app name beside an action is as loud as \
                 the key badges; it is an argument, not an action"
            );
        }
    }
}
