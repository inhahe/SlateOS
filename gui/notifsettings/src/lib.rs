//! Which programs may interrupt you, and how.
//!
//! The desktop shell decides whether to show a notification; the Settings
//! application decides what the user wants. This crate is the sentence they
//! both read, and it exists for the reason `inputsettings` does: a setting
//! edited in one process and obeyed in another needs one definition, or the
//! two ends agree on every field name and still disagree about the answer.
//!
//! # Why the rules are keyed by the program's name
//!
//! A notification carries an `app_name` and nothing else that identifies who
//! sent it — no pid, no path, no handle. So the name is not a convenient key,
//! it is the *only* key, and a rule filed under anything else could never be
//! matched to an arriving message. It is also the key a person would choose:
//! "stop Mail interrupting me" is a sentence about a name.
//!
//! The cost is stated rather than hidden: two programs calling themselves
//! "Mail" share one rule, and a program that renames itself loses its rule and
//! starts from the default. Both are visible to the user and recoverable by
//! the user, which a pid-keyed rule would not be — see the note on
//! [`AppRule::app_name`].
//!
//! # What is deliberately not here
//!
//! **Do Not Disturb itself.** The *mode* the user is in is
//! `desktop::focus_assist`'s, along with the rule that compares a program's
//! [`Importance`] against it. This crate carries what the user chose, not
//! what the desktop does with it -- the same division `inputsettings` keeps
//! with the compositor, and the reason it can be read by a settings
//! application that has no business knowing what focus mode is on.
//!
//! **The notifications themselves.** A message is not a setting. They live and
//! die within one session and belong to whoever is holding them.
//!
//! # Layout
//!
//! `notifications.yaml` in the user's configuration directory, per
//! [`settingsfile`]:
//!
//! ```yaml
//! apps:
//!   Mail:
//!     importance: priority
//!     sound: false
//!     banner: true
//! ```
//!
//! An app with no entry uses [`AppRule::new`], so a fresh install has an
//! empty file and every program behaves the same way.

#![deny(clippy::all, clippy::pedantic)]

use daywindow::{DailyWindow, TimeOfDay};
use settingsfile::yaml_enum;
use yamldoc::Document;

// ============================================================================
// Priority
// ============================================================================

/// How far a program's notifications get when the user is trying to focus.
///
/// **This is the scale `desktop::focus_assist` already uses**, variant for
/// variant, because that is the code with a consumer:
/// `should_show_notification` compares an app's level against the current
/// focus mode, and the shell's `notify` calls it on every message. A settings
/// crate that invented its own scale would be a fourth model of a thing this
/// tree is trying to get down to one of.
///
/// It is deliberately **not** the same scale as a notification's own urgency,
/// which is a property of one *message* (`notif_pane::NotifPriority`, the
/// coloured badge). How loud a message is and how much a program is trusted
/// to interrupt are different questions, and giving them one enum is how the
/// answer to one silently becomes the answer to the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Importance {
    /// Never shown while any focus mode is on.
    Silent,
    /// Follows the focus mode's ordinary rules.
    #[default]
    Normal,
    /// Still shown in "priority only".
    Priority,
    /// Always shown: alarms, security alerts.
    Critical,
}

impl Importance {
    /// The name shown to the user.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Silent => "Silent",
            Self::Normal => "Normal",
            Self::Priority => "Priority",
            Self::Critical => "Critical",
        }
    }

    /// Every level, quietest first, in the order a chooser should offer them.
    pub const ALL: [Self; 4] = [Self::Silent, Self::Normal, Self::Priority, Self::Critical];

    // `Normal` is the default at both ends, deliberately: it is what
    // `focus_assist::app_priority` already answers for a program with no
    // override, so a program the user has never configured behaves the same
    // whether or not this file exists.
}

yaml_enum!(Importance {
    Silent => "silent",
    Normal => "normal",
    Priority => "priority",
    Critical => "critical",
});

// ============================================================================
// One program's rule
// ============================================================================

/// What one program is allowed to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRule {
    /// The program's own name for itself, as it appears on its notifications.
    ///
    /// **The only identity available.** A notification carries this string and
    /// nothing else that says who sent it, so a rule filed under a pid or a
    /// path could never be matched to an arriving message. Two programs that
    /// call themselves the same thing therefore share a rule, and a program
    /// that renames itself starts from the default again.
    ///
    /// That is a real cost and it is the right one: both failures are visible
    /// to the user in this list and fixable by them. A rule keyed on something
    /// they cannot see would fail invisibly instead.
    pub app_name: String,
    /// How far this program's notifications get while the user is focusing.
    ///
    /// `Silent` is how a program is switched off entirely, which is why there
    /// is no separate `enabled` flag: `desktop::focus_assist` already reads
    /// exactly this one value to make exactly this decision, and a second
    /// boolean beside it would be a second way to say "no" that the consumer
    /// does not read.
    pub importance: Importance,
    /// Whether its notifications make a sound.
    pub sound: bool,
    /// Whether they appear as a banner, rather than only in the list.
    pub banner: bool,
}

impl AppRule {
    /// The rule a program with no entry gets.
    ///
    /// Matches what `focus_assist::app_priority` answers today for an
    /// unconfigured program, so that adding this file changes nothing for
    /// anyone who has not opened the settings page. Permissive, because the
    /// alternative is a desktop where a newly installed program is silently
    /// muted and the user has no reason to suspect it.
    #[must_use]
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            importance: Importance::Normal,
            sound: true,
            banner: true,
        }
    }

    /// This rule with a different importance.
    ///
    /// Builders rather than field assignment for the two settings a caller
    /// usually changes one of, because a rule is most often written as a
    /// single expression in a table of defaults.
    #[must_use]
    pub fn with_importance(mut self, importance: Importance) -> Self {
        self.importance = importance;
        self
    }

    /// This rule with sound and banner set.
    #[must_use]
    pub fn with_alerts(mut self, sound: bool, banner: bool) -> Self {
        self.sound = sound;
        self.banner = banner;
        self
    }
}

// ============================================================================
// Quiet hours
// ============================================================================

/// Hours in which nothing ordinary may interrupt, whoever sent it.
///
/// The per-program rules above answer "may *this* program interrupt me". This
/// answers "may anything, *now*", and the two are independent: a program you
/// marked `Critical` still gets through, which is the point of having marked
/// it.
///
/// **The window is a [`DailyWindow`] rather than four numbers**, because that
/// type exists for this: its own documentation lists do-not-disturb quiet
/// hours as one of three features that each grew four unvalidated `u8`s, and
/// one of the three shipped a start of `25:00` that compared as an overnight
/// window and then never opened. A schedule that silently stops happening is
/// the worst failure this feature has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuietHours {
    /// Whether the schedule is in force at all.
    ///
    /// Separate from the window for the reason `night_light` is separate from
    /// its strength: a switch that forgets the hours you set is a switch
    /// nobody sets twice.
    pub enabled: bool,
    /// When it runs, which may wrap past midnight -- and usually does.
    pub window: DailyWindow,
    /// Which days it runs on, Sunday first.
    ///
    /// An array rather than a list of day numbers, so "is today one of them"
    /// is an index rather than a search, and so a file naming Tuesday twice
    /// cannot mean anything different from naming it once.
    pub days: [bool; 7],
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            // Ten at night until seven in the morning, every day: the hours
            // this feature is named after, so that switching it on without
            // touching anything else does the thing people mean by it.
            window: DailyWindow::from_hm(22, 0, 7, 0).unwrap_or_default(),
            days: [true; 7],
        }
    }
}

impl QuietHours {
    /// Whether quiet hours are in force at this local time.
    ///
    /// `weekday` is 0 for Sunday, matching [`days`](Self::days).
    ///
    /// **The day is the one the window *starts* on.** An overnight window
    /// asked about at one in the morning is the previous evening's window
    /// still running, so a Friday-night rule has to still be quiet at 1 a.m.
    /// on Saturday even if Saturday is not selected. Getting this the obvious
    /// way round would make every overnight schedule end at midnight without
    /// saying so.
    #[must_use]
    pub fn active_at(&self, hour: u8, minute: u8, weekday: u8) -> bool {
        let Some(now) = TimeOfDay::new(hour, minute) else {
            // Not a time of day. Refusing to be quiet is the safe answer: a
            // notification shown when it need not have been is a nuisance, and
            // one held back for ever is a message the user never sees.
            return false;
        };
        if !self.enabled || !self.window.contains_hm(hour, minute) {
            return false;
        }
        // Normalised before anything is added to it, which is what makes the
        // step back to yesterday safe: after this, `weekday` is at most 6.
        let weekday = weekday % 7;
        let overnight = self.window.start() > self.window.end();
        let day = if overnight && now < self.window.start() {
            // Before the start on an overnight rule: this is yesterday's
            // window, still running.
            weekday.saturating_add(6) % 7
        } else {
            weekday
        };
        self.days.get(usize::from(day)).copied().unwrap_or(false)
    }
}

// ============================================================================
// The settings
// ============================================================================

/// Every program-specific rule the user has set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifSettings {
    /// Hours in which nothing ordinary interrupts, whoever sent it.
    pub quiet_hours: QuietHours,
    /// One rule per program the user has an opinion about, in file order.
    ///
    /// A `Vec` rather than a map because the order is the user's: this list is
    /// rendered as rows, and a map would reorder them on every save according
    /// to something no one chose.
    pub apps: Vec<AppRule>,
}

impl NotifSettings {
    /// The rule for `app_name`, or the default if the user has not set one.
    ///
    /// Returns a value rather than an `Option` because every caller wants the
    /// same thing for a program with no entry, and the one place that decision
    /// belongs is here. A caller doing `unwrap_or_default()` would be choosing
    /// it again, and could choose differently.
    #[must_use]
    pub fn rule_for(&self, app_name: &str) -> AppRule {
        self.apps
            .iter()
            .find(|r| r.app_name == app_name)
            .cloned()
            .unwrap_or_else(|| AppRule::new(app_name))
    }

    /// Add or replace the rule for `rule.app_name`, keeping list order.
    ///
    /// Replacing in place rather than removing and pushing, so that editing a
    /// rule does not move its row to the bottom of the user's list under their
    /// pointer.
    pub fn set_rule(&mut self, rule: AppRule) {
        match self.apps.iter_mut().find(|r| r.app_name == rule.app_name) {
            Some(existing) => *existing = rule,
            None => self.apps.push(rule),
        }
    }

    /// Read the settings out of a configuration document.
    ///
    /// Anything missing or unparseable takes its default. A settings file is
    /// read on login and a user cannot fix one that refuses to load, so every
    /// field degrades on its own rather than failing the file.
    #[must_use]
    pub fn read_from(doc: &Document) -> Self {
        let mut quiet_hours = QuietHours::default();
        if let Some(v) = doc.get_bool(&["quiet_hours", "enabled"]) {
            quiet_hours.enabled = v;
        }
        // Both ends or neither. A file giving a start and no end would
        // otherwise pair a chosen time with a default one and produce a window
        // the user never asked for -- and an overnight one at that, since the
        // default end is 07:00.
        if let (Some(start), Some(end)) = (
            doc.get_str(&["quiet_hours", "start"])
                .and_then(|v| parse_hm(&v)),
            doc.get_str(&["quiet_hours", "end"])
                .and_then(|v| parse_hm(&v)),
        ) {
            quiet_hours.window = DailyWindow::new(start, end);
        }
        // Block style only: `yamldoc::get_seq` does not read a flow list
        // (`days: [mon, tue]`), which someone editing by hand might well
        // write and would then find silently ignored. That is the shared
        // YAML layer's limit, and every settings list in the tree has it.
        if let Some(days) = doc.get_seq(&["quiet_hours", "days"]) {
            let mut on = [false; 7];
            for name in &days {
                if let Some(i) = weekday_index(name)
                    && let Some(slot) = on.get_mut(i)
                {
                    *slot = true;
                }
            }
            // An empty or wholly unreadable list is a schedule that runs on no
            // day, which is a switch that is on and does nothing. Read as "the
            // user did not say", which is what the default already means.
            if on.iter().any(|d| *d) {
                quiet_hours.days = on;
            }
        }

        let mut apps = Vec::new();
        for name in doc.keys(&["apps"]) {
            let mut rule = AppRule::new(&name);
            if let Some(v) = doc
                .get_str(&["apps", &name, "importance"])
                .and_then(|s| Importance::from_yaml_name(&s))
            {
                rule.importance = v;
            }
            if let Some(v) = doc.get_bool(&["apps", &name, "sound"]) {
                rule.sound = v;
            }
            if let Some(v) = doc.get_bool(&["apps", &name, "banner"]) {
                rule.banner = v;
            }
            apps.push(rule);
        }
        Self { quiet_hours, apps }
    }

    /// Fold the settings back into the document they came from.
    ///
    /// Removes the entry for a program the user has deleted, so that a rule
    /// taken out of the list leaves the file rather than lingering as a key
    /// that nothing reads and the next reader restores.
    pub fn write_into(&self, doc: &mut Document) {
        doc.set_bool(&["quiet_hours", "enabled"], self.quiet_hours.enabled);
        doc.set_str(
            &["quiet_hours", "start"],
            &format_hm(self.quiet_hours.window.start()),
        );
        doc.set_str(
            &["quiet_hours", "end"],
            &format_hm(self.quiet_hours.window.end()),
        );
        let days: Vec<&str> = WEEKDAYS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.quiet_hours.days.get(*i).copied().unwrap_or(false))
            .map(|(_, name)| *name)
            .collect();
        doc.set_seq(&["quiet_hours", "days"], &days);
        let kept: Vec<&str> = self.apps.iter().map(|r| r.app_name.as_str()).collect();
        for name in doc.keys(&["apps"]) {
            if !kept.contains(&name.as_str()) {
                doc.remove(&["apps", &name]);
            }
        }
        for rule in &self.apps {
            let name = rule.app_name.as_str();
            doc.set_str(&["apps", name, "importance"], rule.importance.yaml_name());
            doc.set_bool(&["apps", name, "sound"], rule.sound);
            doc.set_bool(&["apps", name, "banner"], rule.banner);
        }
    }
}

/// The days, Sunday first, as they are spelled in the file.
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// The index of a day's spelling, or `None`.
///
/// Case-insensitive and prefix-based, so `Mon`, `monday` and `MONDAY` all
/// work: this is a file people edit by hand, and refusing `Monday` because the
/// writer spells it `mon` would be a rule with no purpose behind it.
fn weekday_index(name: &str) -> Option<usize> {
    let lower = name.trim().to_ascii_lowercase();
    WEEKDAYS.iter().position(|d| lower.starts_with(d))
}

/// `HH:MM` to a time of day, or `None` for anything else.
fn parse_hm(text: &str) -> Option<TimeOfDay> {
    let (h, m) = text.trim().split_once(':')?;
    TimeOfDay::new(h.trim().parse().ok()?, m.trim().parse().ok()?)
}

/// A time of day as `HH:MM`, zero-padded.
fn format_hm(t: TimeOfDay) -> String {
    format!("{:02}:{:02}", t.hour(), t.minute())
}

// ============================================================================
// The file
// ============================================================================

/// The settings group these preferences live in — `notifications.yaml`.
///
/// The name is as much a part of the shared contract as the schema: two
/// processes that agree on every key but disagree about which file holds them
/// have simply written two files.
pub const CONFIG_NAME: &str = "notifications";

/// The user's notification settings together with the document they came from.
///
/// The pair is a type rather than two fields for the reason `InputFile` gives:
/// a save splices the changed values into the document that was read, because
/// that document carries the user's comments, blank lines, key order, and any
/// setting belonging to a different version of the desktop. Rebuilding the
/// file from [`NotifSettings`] alone silently deletes all of it.
pub struct NotifFile {
    /// The settings being edited. Public because the front ends bind controls
    /// straight to the fields.
    pub settings: NotifSettings,
    /// The file as read, kept whole. See the type's documentation.
    doc: Document,
}

impl Default for NotifFile {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifFile {
    /// The defaults, backed by an empty document.
    ///
    /// Deliberately does not read the filesystem: a constructor that consulted
    /// `$HOME` would make every caller's tests depend on the machine running
    /// them. [`load`](Self::load) does the I/O.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: NotifSettings::default(),
            doc: Document::new(),
        }
    }

    /// Read the user's saved settings from `notifications.yaml`.
    ///
    /// A missing or unreadable file yields the defaults — the ordinary state
    /// on a fresh install, not an error to report to someone who has simply
    /// never changed a setting.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(settingsfile::load(CONFIG_NAME))
    }

    /// Open on an already-read document, so the format can be exercised
    /// without a filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            settings: NotifSettings::read_from(&doc),
            doc,
        }
    }

    /// Fold the current settings into the document and return its text,
    /// without touching the filesystem.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut doc = self.doc.clone();
        self.settings.write_into(&mut doc);
        doc.to_text()
    }

    /// Write the settings back to `notifications.yaml`.
    ///
    /// # Errors
    ///
    /// As [`settingsfile::store`]: no configuration directory, or the write
    /// failed.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.settings.write_into(&mut self.doc);
        settingsfile::store(CONFIG_NAME, &self.doc)
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production
    // code are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// Off by default, so adding this feature changes nobody's desktop.
    #[test]
    fn quiet_hours_are_off_until_asked_for() {
        let q = QuietHours::default();
        assert!(!q.enabled);
        // At three in the morning, which is inside the default window.
        assert!(!q.active_at(3, 0, 3), "a switch that is off was in force");
    }

    /// The ordinary case: ten at night to seven, every day.
    #[test]
    fn quiet_hours_hold_between_their_ends() {
        let q = QuietHours {
            enabled: true,
            ..QuietHours::default()
        };
        assert!(
            q.active_at(23, 0, 3),
            "eleven at night is inside 22:00-07:00"
        );
        assert!(q.active_at(3, 0, 4), "three in the morning is inside it");
        assert!(!q.active_at(12, 0, 3), "midday is not");
        assert!(!q.active_at(21, 59, 3), "a minute before the start is not");
        assert!(q.active_at(22, 0, 3), "the start itself is");
    }

    /// **The case that goes wrong silently.** An overnight window asked about
    /// after midnight belongs to the day it *started* on.
    ///
    /// A Friday-night rule has to still be quiet at one o'clock on Saturday
    /// morning. Attributing that hour to Saturday -- the obvious reading --
    /// would make every overnight schedule stop at midnight, and it would stop
    /// without saying anything: the user set 22:00 to 07:00 on Friday and gets
    /// two hours of it.
    #[test]
    fn an_overnight_window_belongs_to_the_day_it_started_on() {
        let mut q = QuietHours {
            enabled: true,
            ..QuietHours::default()
        };
        // Friday only. 0 = Sunday, so Friday is 5 and Saturday is 6.
        q.days = [false, false, false, false, false, true, false];

        assert!(q.active_at(23, 0, 5), "Friday night is in force");
        assert!(
            q.active_at(1, 0, 6),
            "one o'clock on Saturday morning is still Friday's window"
        );
        assert!(
            !q.active_at(23, 0, 6),
            "Saturday night is not, because Saturday was not selected"
        );
        assert!(
            !q.active_at(1, 0, 0),
            "one o'clock on Sunday morning would be Saturday's window"
        );
    }

    /// A window that does not cross midnight is attributed to today.
    #[test]
    fn a_daytime_window_belongs_to_today() {
        let mut q = QuietHours {
            enabled: true,
            ..QuietHours::default()
        };
        q.window = DailyWindow::from_hm(9, 0, 17, 0).expect("a real window");
        q.days = [false, true, false, false, false, false, false]; // Monday

        assert!(q.active_at(12, 0, 1), "Monday lunchtime is in force");
        assert!(!q.active_at(12, 0, 2), "Tuesday lunchtime is not");
        assert!(
            !q.active_at(3, 0, 1),
            "Monday at three in the morning is not"
        );
    }

    #[test]
    fn quiet_hours_survive_a_round_trip_through_the_file() {
        let mut before = NotifSettings::default();
        before.quiet_hours.enabled = true;
        before.quiet_hours.window = DailyWindow::from_hm(21, 30, 6, 45).expect("real");
        before.quiet_hours.days = [true, false, true, false, true, false, false];

        let mut doc = Document::new();
        before.write_into(&mut doc);
        let after = NotifSettings::read_from(&Document::parse(&doc.to_text()));

        assert_eq!(after.quiet_hours, before.quiet_hours);
    }

    /// The spellings a person would actually type are accepted.
    #[test]
    fn a_hand_written_day_list_is_read_generously() {
        // Block style, which is what `yamldoc` reads and what `set_seq`
        // writes. A flow list -- `days: [mon, tue]` -- is ignored by
        // `get_seq`; that is the shared YAML layer's limit rather than
        // this crate's, and it is noted where the days are read.
        let doc = Document::parse(
            "quiet_hours:\n  enabled: true\n  days:\n    - Monday\n    - TUE\n    - fri\n",
        );
        let q = NotifSettings::read_from(&doc).quiet_hours;
        assert_eq!(q.days, [false, true, true, false, false, true, false]);
    }

    /// Half a window is no window.
    ///
    /// A file with a start and no end would otherwise pair the user's time
    /// with the default 07:00 and produce an overnight schedule they never
    /// asked for.
    #[test]
    fn a_start_without_an_end_leaves_the_window_alone() {
        let doc = Document::parse("quiet_hours:\n  enabled: true\n  start: \"01:00\"\n");
        let q = NotifSettings::read_from(&doc).quiet_hours;
        assert_eq!(q.window, QuietHours::default().window);
    }

    /// An unreadable time is not a window either.
    #[test]
    fn a_nonsense_time_leaves_the_window_alone() {
        for text in ["25:00", "noon", "7", "07:61", ""] {
            let doc = Document::parse(&format!(
                "quiet_hours:\n  start: \"{text}\"\n  end: \"08:00\"\n"
            ));
            let q = NotifSettings::read_from(&doc).quiet_hours;
            assert_eq!(
                q.window,
                QuietHours::default().window,
                "{text} was read as a time of day"
            );
        }
    }

    /// A program the user has never configured behaves as it did before this
    /// file existed.
    ///
    /// The point of matching `focus_assist::app_priority`'s own default:
    /// adding a settings file must not change anyone's desktop until they
    /// open the page and change something.
    #[test]
    fn an_unconfigured_program_gets_the_same_answer_focus_assist_already_gave() {
        let s = NotifSettings::default();
        assert_eq!(s.rule_for("Mail").importance, Importance::Normal);
        assert!(s.rule_for("Mail").sound);
        assert!(s.rule_for("Mail").banner);
    }

    #[test]
    fn a_rule_names_one_program_and_not_its_neighbours() {
        let mut s = NotifSettings::default();
        let mut rule = AppRule::new("Mail");
        rule.importance = Importance::Silent;
        s.set_rule(rule);
        assert_eq!(s.rule_for("Mail").importance, Importance::Silent);
        assert_eq!(
            s.rule_for("Chat").importance,
            Importance::Normal,
            "Chat took Mail's rule"
        );
    }

    /// The scale is ordered, because the consumer compares with it.
    ///
    /// `focus_assist::should_show_notification` asks `priority >= Priority`
    /// and `>= Critical`. If these variants were declared in another order
    /// those comparisons would silently mean something else, so the ordering
    /// is asserted here rather than left to the order somebody typed them in.
    #[test]
    fn importance_is_ordered_quietest_first() {
        assert!(Importance::Silent < Importance::Normal);
        assert!(Importance::Normal < Importance::Priority);
        assert!(Importance::Priority < Importance::Critical);
        assert_eq!(Importance::ALL[0], Importance::Silent);
        assert_eq!(Importance::ALL[3], Importance::Critical);
    }

    #[test]
    fn editing_a_rule_leaves_it_where_the_user_put_it() {
        let mut s = NotifSettings::default();
        for name in ["Mail", "Chat", "Updates"] {
            s.set_rule(AppRule::new(name));
        }
        let mut edited = AppRule::new("Mail");
        edited.sound = false;
        s.set_rule(edited);

        let order: Vec<&str> = s.apps.iter().map(|r| r.app_name.as_str()).collect();
        assert_eq!(order, vec!["Mail", "Chat", "Updates"]);
        assert!(!s.rule_for("Mail").sound);
    }

    #[test]
    fn settings_survive_a_round_trip_through_the_file() {
        let mut before = NotifSettings::default();
        let mut mail = AppRule::new("Mail");
        mail.importance = Importance::Critical;
        mail.sound = false;
        before.set_rule(mail);
        before.set_rule(AppRule::new("Chat"));

        let mut doc = Document::new();
        before.write_into(&mut doc);
        let after = NotifSettings::read_from(&Document::parse(&doc.to_text()));

        assert_eq!(after, before);
    }

    /// A comment the user wrote comes back.
    ///
    /// The reason `NotifFile` keeps the document rather than the settings
    /// alone: rebuilding the file from the model deletes everything the model
    /// does not carry.
    #[test]
    fn a_users_comment_survives_a_save() {
        let original = "# my rules\napps:\n  Mail:\n    importance: silent\n";
        let mut file = NotifFile::from_document(Document::parse(original));
        assert_eq!(
            file.settings.rule_for("Mail").importance,
            Importance::Silent
        );

        file.settings.set_rule(AppRule::new("Chat"));
        let text = file.to_text();

        assert!(text.contains("# my rules"), "the comment was lost: {text}");
        assert!(
            text.contains("Chat"),
            "the new rule was not written: {text}"
        );
    }

    /// A setting this build does not know is not deleted by saving.
    #[test]
    fn a_key_from_a_newer_desktop_survives_a_save() {
        let original = "apps:\n  Mail:\n    importance: normal\n    loudness: 11\n";
        let mut file = NotifFile::from_document(Document::parse(original));
        file.settings.set_rule(AppRule::new("Chat"));

        let text = file.to_text();

        assert!(
            text.contains("loudness"),
            "an unknown key was dropped: {text}"
        );
    }

    /// A rule the user removed leaves the file.
    #[test]
    fn a_removed_rule_does_not_come_back() {
        let original = "apps:\n  Mail:\n    importance: silent\n  Chat:\n    importance: silent\n";
        let mut file = NotifFile::from_document(Document::parse(original));
        assert_eq!(file.settings.apps.len(), 2);

        file.settings.apps.retain(|r| r.app_name != "Mail");
        let text = file.to_text();

        assert!(
            !text.contains("Mail"),
            "a removed rule was rewritten: {text}"
        );
        assert!(text.contains("Chat"), "the wrong rule was removed: {text}");
    }

    /// An unreadable spelling degrades to the default rather than failing.
    #[test]
    fn an_unknown_importance_falls_back_rather_than_refusing_the_file() {
        let doc = Document::parse("apps:\n  Mail:\n    importance: deafening\n");
        let s = NotifSettings::read_from(&doc);
        assert_eq!(s.apps.len(), 1, "the file was refused");
        assert_eq!(
            s.rule_for("Mail").importance,
            AppRule::new("Mail").importance
        );
    }

    #[test]
    fn every_importance_has_a_spelling_that_round_trips() {
        for p in Importance::ALL {
            assert_eq!(Importance::from_yaml_name(p.yaml_name()), Some(p));
            assert!(!p.label().is_empty());
        }
    }
}
