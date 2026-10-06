//! What the picker remembers between openings, and from one login to the
//! next: the recent picks and the skin tone -- [`Remembered`].
//!
//! They are kept in `charpicker.yaml` in the user's settings directory
//! (`settingsfile`'s layout): one file every host of the picker reads, so a
//! character picked in one program is recent in the next, and the tone chosen
//! in one is the tone in all.
//!
//! ```yaml
//! tone: medium
//! recent:
//!   - 👋
//!   - é
//! ```
//!
//! A file written by hand is read as far as it makes sense: a tone this does
//! not know is no tone, and a recent pick the picker cannot show is dropped
//! when the picker takes the list ([`CharPicker::with_recent`]). Saving
//! rewrites only these two keys, so a user's comments in the file stay where
//! they were.

use std::io;

use yamldoc::Document;

use crate::{CharPicker, MAX_RECENT, SkinTone};

/// The settings group's name: the file is `charpicker.yaml`.
pub const CONFIG_NAME: &str = "charpicker";

/// What the file calls no tone.
const NO_TONE: &str = "none";

/// What the picker remembers: the recent picks and the skin tone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    /// The recent picks, most recent first, untoned.
    pub recent: Vec<String>,
    /// The skin tone emoji are drawn in; `None` for none.
    pub tone: Option<SkinTone>,
}

impl Remembered {
    /// What `charpicker.yaml` says: nothing remembered where there is no
    /// file, or no user to have one.
    #[must_use]
    pub fn load() -> Self {
        Self::from_doc(&settingsfile::load(CONFIG_NAME))
    }

    /// What `doc` says. At most [`MAX_RECENT`] picks are taken, the most
    /// recent first, as the file lists them.
    #[must_use]
    pub fn from_doc(doc: &Document) -> Self {
        let tone = doc
            .get_str(&["tone"])
            .and_then(|name| SkinTone::named(name.trim()));
        let mut recent = doc.get_seq(&["recent"]).unwrap_or_default();
        recent.truncate(MAX_RECENT);
        Self { recent, tone }
    }

    /// Write this into `doc`: its `tone` and `recent` keys, and nothing else.
    pub fn write_into(&self, doc: &mut Document) {
        doc.set_str(&["tone"], self.tone.map_or(NO_TONE, SkinTone::name));
        let recent: Vec<&str> = self.recent.iter().map(String::as_str).collect();
        doc.set_seq(&["recent"], &recent);
    }

    /// Write this to `charpicker.yaml`, keeping whatever else the file
    /// holds.
    ///
    /// # Errors
    ///
    /// The write's own error -- or that there is no user's settings
    /// directory to write in. What was remembered still holds for the
    /// session that remembered it.
    pub fn save(&self) -> io::Result<()> {
        let mut doc = settingsfile::load(CONFIG_NAME);
        self.write_into(&mut doc);
        settingsfile::store(CONFIG_NAME, &doc)
    }
}

impl CharPicker {
    /// What this picker would remember now: its recent picks and its tone.
    #[must_use]
    pub fn remembered(&self) -> Remembered {
        Remembered {
            recent: self.recent.clone(),
            tone: self.tone,
        }
    }

    /// Start from what was remembered -- [`with_recent`](Self::with_recent)
    /// and [`with_tone`](Self::with_tone) at once.
    #[must_use]
    pub fn with_remembered(self, remembered: Remembered) -> Self {
        self.with_recent(remembered.recent)
            .with_tone(remembered.tone)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// **The file's tone and picks are read, and an unknown tone is none.**
    #[test]
    fn the_file_is_read() {
        let doc = Document::parse("tone: medium-dark\nrecent:\n  - \"\u{1F44B}\"\n  - \u{E9}\n");
        let remembered = Remembered::from_doc(&doc);
        assert_eq!(remembered.tone, Some(SkinTone::MediumDark));
        assert_eq!(remembered.recent, ["\u{1F44B}", "\u{E9}"]);
        let doc = Document::parse("tone: purple\n");
        assert_eq!(Remembered::from_doc(&doc), Remembered::default());
        assert_eq!(
            Remembered::from_doc(&Document::parse("")),
            Remembered::default()
        );
    }

    /// **No more than [`MAX_RECENT`] picks are taken from a file.**
    #[test]
    fn a_long_list_is_cut() {
        let mut text = String::from("recent:\n");
        for c in ('a'..='z').chain('A'..='Z') {
            text.push_str(&format!("  - {c}\n"));
        }
        let remembered = Remembered::from_doc(&Document::parse(&text));
        assert_eq!(remembered.recent.len(), MAX_RECENT);
        assert_eq!(remembered.recent[0], "a");
    }

    /// **What is written is read back, and the rest of the file -- a user's
    /// comment, a key of their own -- is kept.**
    #[test]
    fn what_is_written_is_read_back() {
        let mut doc = Document::parse("# mine\ncolour: blue\ntone: light\n");
        let remembered = Remembered {
            recent: vec!["\u{2605}".to_string(), "#".to_string(), "a: b".to_string()],
            tone: Some(SkinTone::Dark),
        };
        remembered.write_into(&mut doc);
        let text = doc.to_text();
        assert!(text.contains("# mine"), "{text}");
        assert!(text.contains("colour: blue"), "{text}");
        let again = Remembered::from_doc(&Document::parse(&text));
        assert_eq!(again, remembered, "{text}");
        let none = Remembered::default();
        none.write_into(&mut doc);
        assert_eq!(Remembered::from_doc(&doc), none);
    }

    /// **A picker starts from what was remembered, and says what it
    /// remembers.**
    #[test]
    fn a_picker_starts_from_what_was_remembered() {
        let remembered = Remembered {
            recent: vec!["\u{E9}".to_string()],
            tone: Some(SkinTone::Light),
        };
        let picker = CharPicker::new().with_remembered(remembered.clone());
        assert_eq!(picker.remembered(), remembered);
        assert_eq!(picker.category(), crate::Category::Recent);
    }

    /// **The file is saved and loaded where the user's settings are** -- a
    /// scratch directory here, never the developer's own.
    #[test]
    fn the_file_is_saved_and_loaded() {
        settingsfile::testing::with_scratch_config("charpicker-remembered", |_root| {
            assert_eq!(Remembered::load(), Remembered::default(), "no file yet");
            let remembered = Remembered {
                recent: vec!["\u{1F44B}".to_string()],
                tone: Some(SkinTone::Medium),
            };
            remembered.save().expect("saved");
            assert_eq!(Remembered::load(), remembered);
        });
    }
}
