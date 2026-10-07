//! Which service menus the user turned on or off: `context-menus.yaml`.
//!
//! ```yaml
//! # Menus put in the user's own directory, turned on -- off until they are.
//! enabled: [my-menu.desktop]
//! # Menus the system installed, turned off -- on until they are.
//! disabled: [compress.desktop]
//! ```
//!
//! By file name, the name a menu is known by, written whatever bytes it holds
//! (`pathcodec`). The Settings application's page for context menus writes
//! it; every file menu reads it.

use crate::{Origin, ServiceMenu};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use yamldoc::Document;

/// The settings file the choices live in -- `context-menus.yaml`.
pub const CONFIG_NAME: &str = "context-menus";

/// The user's choices. See the module docs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    /// Menus of the user's own, turned on.
    enabled: BTreeSet<OsString>,
    /// Menus of the system's, turned off.
    disabled: BTreeSet<OsString>,
}

impl Choices {
    /// Whether `menu` is on: a system menu unless turned off, a user's own
    /// only once turned on.
    #[must_use]
    pub fn is_on(&self, menu: &ServiceMenu) -> bool {
        match menu.origin {
            Origin::System => !self.disabled.contains(&menu.id),
            Origin::User => self.enabled.contains(&menu.id),
        }
    }

    /// Turn `menu` on or off.
    pub fn set(&mut self, menu: &ServiceMenu, on: bool) {
        let id = menu.id.clone();
        match (menu.origin, on) {
            (Origin::System, true) => {
                self.disabled.remove(&id);
            }
            (Origin::System, false) => {
                self.disabled.insert(id);
            }
            (Origin::User, true) => {
                self.enabled.insert(id);
            }
            (Origin::User, false) => {
                self.enabled.remove(&id);
            }
        }
    }

    /// The choices `doc` holds. What is not there is the default: every
    /// system menu on, every user menu off.
    ///
    /// A name is taken as written, blanks and all: a file's name may begin
    /// or end with a space, and trimming it would lose the user's choice
    /// about that file.
    #[must_use]
    pub fn read_from(doc: &Document) -> Self {
        let names = |key: &str| -> BTreeSet<OsString> {
            doc.get_seq(&[key])
                .unwrap_or_default()
                .iter()
                .map(|name| pathcodec::decode_path(name).into_os_string())
                .collect()
        };
        Self {
            enabled: names("enabled"),
            disabled: names("disabled"),
        }
    }

    /// Write the choices into `doc`, leaving everything else in it as it was.
    pub fn write_into(&self, doc: &mut Document) {
        let encode = |set: &BTreeSet<OsString>| -> Vec<String> {
            set.iter()
                .map(|name| pathcodec::encode_path(Path::new(name)))
                .collect()
        };
        let enabled = encode(&self.enabled);
        let disabled = encode(&self.disabled);
        doc.set_seq(
            &["enabled"],
            &enabled.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        doc.set_seq(
            &["disabled"],
            &disabled.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
}

/// The user's choices together with the document they came from, so a save
/// keeps the file's comments and anything a later version wrote.
#[derive(Clone, Debug, Default)]
pub struct ChoicesFile {
    /// The choices being edited.
    pub choices: Choices,
    /// The file as read, kept whole.
    doc: Document,
}

impl ChoicesFile {
    /// The defaults, backed by an empty document -- no filesystem touched.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read `context-menus.yaml`. A missing or unreadable file is the
    /// defaults: the ordinary state for someone who never changed one.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(settingsfile::load(CONFIG_NAME))
    }

    /// Open on an already-read document.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            choices: Choices::read_from(&doc),
            doc,
        }
    }

    /// The file's text with the current choices in it, nothing written.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut doc = self.doc.clone();
        self.choices.write_into(&mut doc);
        doc.to_text()
    }

    /// Write the choices to `context-menus.yaml`.
    ///
    /// # Errors
    ///
    /// As `settingsfile::store`: no configuration directory, or the write
    /// failed.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.choices.write_into(&mut self.doc);
        settingsfile::store(CONFIG_NAME, &self.doc)
    }
}
