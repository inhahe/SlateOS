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
//! **Do Not Disturb.** That is `desktop::focus_assist`, it is already wired,
//! and it is a *mode the user is in* rather than a rule about a program. A
//! second copy of it here would be a second answer to "may this interrupt
//! me?", which is the shape of defect this crate is meant to remove.
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
//!     enabled: true
//!     priority: high
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

/// How much of the user's attention a notification asks for.
///
/// Shared with the message type in the shell rather than duplicated there: a
/// rule that says "only High and above from this program" has to be comparing
/// the same four values the message carries, and two enums with the same four
/// names are two enums.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Worth recording, not worth a banner.
    Low,
    #[default]
    Normal,
    High,
    /// Interrupts even a focused full-screen program.
    Urgent,
}

impl Priority {
    /// The name shown to the user.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }

    /// Every level, lowest first, in the order a chooser should offer them.
    pub const ALL: [Self; 4] = [Self::Low, Self::Normal, Self::High, Self::Urgent];

    // `Priority::default()` is `Normal` and `AppRule::new`'s floor is `Low`,
    // which reads like a contradiction and is not: the two answer different
    // questions. A *message* with no stated priority is an ordinary one, so
    // `Normal`. A *rule* with no stated floor should admit everything, so
    // `Low`. Giving both the same value would break one of them -- a default
    // floor of `Normal` would silently drop every `Low` notification from a
    // program the user had never configured.
}

yaml_enum!(Priority {
    Low => "low",
    Normal => "normal",
    High => "high",
    Urgent => "urgent",
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
    /// Whether this program may notify at all.
    pub enabled: bool,
    /// The lowest priority from this program that is still shown.
    pub priority: Priority,
    /// Whether its notifications make a sound.
    pub sound: bool,
    /// Whether they appear as a banner, rather than only in the list.
    pub banner: bool,
}

impl AppRule {
    /// The rule a program with no entry gets: everything on.
    ///
    /// Permissive by default because the alternative is a desktop where a
    /// newly installed program is silently muted and the user has no reason
    /// to suspect it. A notification nobody asked to suppress should arrive.
    #[must_use]
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            enabled: true,
            priority: Priority::Low,
            sound: true,
            banner: true,
        }
    }

    /// Whether a notification of `priority` from this program should be shown.
    #[must_use]
    pub fn allows(&self, priority: Priority) -> bool {
        self.enabled && priority >= self.priority
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

    /// Whether a notification from `app_name` at `priority` should be shown.
    #[must_use]
    pub fn allows(&self, app_name: &str, priority: Priority) -> bool {
        self.rule_for(app_name).allows(priority)
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
            if let Some(v) = doc.get_bool(&["apps", &name, "enabled"]) {
                rule.enabled = v;
            }
            if let Some(v) = doc
                .get_str(&["apps", &name, "priority"])
                .and_then(|s| Priority::from_yaml_name(&s))
            {
                rule.priority = v;
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
            doc.set_bool(&["apps", name, "enabled"], rule.enabled);
            doc.set_str(&["apps", name, "priority"], rule.priority.yaml_name());
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

    #[test]
    fn a_program_with_no_rule_may_notify() {
        let s = NotifSettings::default();
        assert!(s.allows("Mail", Priority::Low));
        assert!(s.allows("Mail", Priority::Urgent));
    }

    #[test]
    fn a_disabled_program_is_silenced_at_every_priority() {
        let mut s = NotifSettings::default();
        let mut rule = AppRule::new("Mail");
        rule.enabled = false;
        s.set_rule(rule);
        for p in Priority::ALL {
            assert!(!s.allows("Mail", p), "{p:?} got through a disabled program");
        }
    }

    #[test]
    fn a_priority_floor_admits_that_level_and_above() {
        let mut s = NotifSettings::default();
        let mut rule = AppRule::new("Mail");
        rule.priority = Priority::High;
        s.set_rule(rule);
        assert!(!s.allows("Mail", Priority::Low));
        assert!(!s.allows("Mail", Priority::Normal));
        assert!(s.allows("Mail", Priority::High), "the floor itself is in");
        assert!(s.allows("Mail", Priority::Urgent));
    }

    #[test]
    fn a_rule_names_one_program_and_not_its_neighbours() {
        let mut s = NotifSettings::default();
        let mut rule = AppRule::new("Mail");
        rule.enabled = false;
        s.set_rule(rule);
        assert!(!s.allows("Mail", Priority::Urgent));
        assert!(s.allows("Chat", Priority::Low), "Chat took Mail's rule");
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
        mail.enabled = false;
        mail.priority = Priority::Urgent;
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
        let original = "# my rules\napps:\n  Mail:\n    enabled: false\n";
        let mut file = NotifFile::from_document(Document::parse(original));
        assert!(!file.settings.rule_for("Mail").enabled);

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
        let original = "apps:\n  Mail:\n    enabled: true\n    loudness: 11\n";
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
        let original = "apps:\n  Mail:\n    enabled: false\n  Chat:\n    enabled: false\n";
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
    fn an_unknown_priority_falls_back_rather_than_refusing_the_file() {
        let doc = Document::parse("apps:\n  Mail:\n    priority: deafening\n");
        let s = NotifSettings::read_from(&doc);
        assert_eq!(s.apps.len(), 1, "the file was refused");
        assert_eq!(s.rule_for("Mail").priority, AppRule::new("Mail").priority);
    }

    #[test]
    fn every_priority_has_a_spelling_that_round_trips() {
        for p in Priority::ALL {
            assert_eq!(Priority::from_yaml_name(p.yaml_name()), Some(p));
            assert!(!p.label().is_empty());
        }
    }
}
