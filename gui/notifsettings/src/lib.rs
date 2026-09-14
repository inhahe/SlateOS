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
}

// ============================================================================
// The settings
// ============================================================================

/// Every program-specific rule the user has set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifSettings {
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
        Self { apps }
    }

    /// Fold the settings back into the document they came from.
    ///
    /// Removes the entry for a program the user has deleted, so that a rule
    /// taken out of the list leaves the file rather than lingering as a key
    /// that nothing reads and the next reader restores.
    pub fn write_into(&self, doc: &mut Document) {
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
