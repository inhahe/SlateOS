//! What a menu of programs shows, and where.
//!
//! The Desktop Menu Specification's *main categories*, folded into the
//! folders a start menu shows: every program lands in exactly one, the first
//! main category its entry names, so that a program listing
//! `Audio;Video;AudioVideo` appears once under Multimedia rather than three
//! times.

use crate::{App, Kind};

/// The name SlateOS answers to in `OnlyShowIn` and `NotShowIn`, and in
/// `XDG_CURRENT_DESKTOP`: an entry meant for one desktop can say so.
pub const DESKTOP_NAME: &str = "SlateOS";

/// A folder of the applications tree.
///
/// Declared in the order a menu lists them: alphabetical by their English
/// names, with Other -- programs that name no main category -- last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// `Utility`: small tools.
    Accessories,
    /// `Development`.
    Development,
    /// `Education`.
    Education,
    /// `Game`.
    Games,
    /// `Graphics`.
    Graphics,
    /// `Network`.
    Internet,
    /// `AudioVideo`, `Audio`, `Video`.
    Multimedia,
    /// `Office`.
    Office,
    /// `Science`.
    Science,
    /// `Settings`.
    Settings,
    /// `System`.
    System,
    /// No main category at all -- and the folder of anything that has not
    /// said.
    #[default]
    Other,
}

impl Category {
    /// Every folder, in menu order.
    pub const ALL: [Self; 12] = [
        Self::Accessories,
        Self::Development,
        Self::Education,
        Self::Games,
        Self::Graphics,
        Self::Internet,
        Self::Multimedia,
        Self::Office,
        Self::Science,
        Self::Settings,
        Self::System,
        Self::Other,
    ];

    /// The folder for an entry's `Categories`: the first main category it
    /// names, in the entry's own order -- the author's first answer to "what
    /// is this". Additional categories (`Calculator`, `TextEditor`) are
    /// refinements and choose nothing on their own.
    #[must_use]
    pub fn of(categories: &[String]) -> Self {
        categories
            .iter()
            .find_map(|c| Self::main(c))
            .unwrap_or(Self::Other)
    }

    /// The folder a main category belongs in, or `None` for a category that
    /// is not a main one.
    #[must_use]
    pub fn main(category: &str) -> Option<Self> {
        Some(match category {
            "Utility" => Self::Accessories,
            "Development" => Self::Development,
            "Education" => Self::Education,
            "Game" => Self::Games,
            "Graphics" => Self::Graphics,
            "Network" => Self::Internet,
            "AudioVideo" | "Audio" | "Video" => Self::Multimedia,
            "Office" => Self::Office,
            "Science" => Self::Science,
            "Settings" => Self::Settings,
            "System" => Self::System,
            _ => return None,
        })
    }

    /// The folder's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Accessories => "Accessories",
            Self::Development => "Development",
            Self::Education => "Education",
            Self::Games => "Games",
            Self::Graphics => "Graphics",
            Self::Internet => "Internet",
            Self::Multimedia => "Multimedia",
            Self::Office => "Office",
            Self::Science => "Science",
            Self::Settings => "Settings",
            Self::System => "System",
            Self::Other => "Other",
        }
    }

    /// The folder's picture: the Icon Naming Specification's name for it,
    /// which icon themes draw.
    #[must_use]
    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Accessories => "applications-accessories",
            Self::Development => "applications-development",
            Self::Education => "applications-education",
            Self::Games => "applications-games",
            Self::Graphics => "applications-graphics",
            Self::Internet => "applications-internet",
            Self::Multimedia => "applications-multimedia",
            Self::Office => "applications-office",
            Self::Science => "applications-science",
            Self::Settings => "preferences-desktop",
            Self::System => "applications-system",
            Self::Other => "applications-other",
        }
    }
}

/// Whether a menu on the desktops named `desktops` lists `app`.
///
/// Programs only (`Type=Application`), and not those the entry keeps out of
/// menus: `NoDisplay` (installed, but a helper -- a file handler, a settings
/// panel another program opens), `Hidden` (deleted, as far as the user is
/// concerned), `OnlyShowIn` another desktop, `NotShowIn` this one.
///
/// `TryExec` is not checked here, because it needs the filesystem; see
/// [`crate::scan::program_exists`].
#[must_use]
pub fn shows_in_menu(app: &App, desktops: &[&str]) -> bool {
    let named = |list: &[String]| list.iter().any(|d| desktops.contains(&d.as_str()));
    app.kind == Kind::Application
        && !app.no_display
        && !app.hidden
        && (app.only_show_in.is_empty() || named(&app.only_show_in))
        && !named(&app.not_show_in)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{DesktopEntry, Locale};

    fn app(extra: &str) -> App {
        let text = format!("[Desktop Entry]\nType=Application\nName=x\nExec=x\n{extra}\n");
        App::from_entry(
            &DesktopEntry::parse(text.as_bytes()).expect("parses"),
            "x.desktop",
            None::<&Locale>,
        )
        .expect("valid")
    }

    fn cats(list: &str) -> Vec<String> {
        list.split(';')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// **A program lands in the first main category it names**, whatever
    /// additional categories come before it; none is Other.
    #[test]
    fn a_program_lands_in_its_first_main_category() {
        assert_eq!(
            Category::of(&cats("Utility;Calculator;")),
            Category::Accessories
        );
        assert_eq!(
            Category::of(&cats("Calculator;Utility;")),
            Category::Accessories
        );
        assert_eq!(
            Category::of(&cats("Audio;Video;AudioVideo;")),
            Category::Multimedia
        );
        assert_eq!(
            Category::of(&cats("Network;WebBrowser;")),
            Category::Internet
        );
        assert_eq!(Category::of(&cats("Game;LogicGame;")), Category::Games);
        assert_eq!(Category::of(&cats("Office;Development;")), Category::Office);
        assert_eq!(Category::of(&cats("X-Mine;")), Category::Other);
        assert_eq!(Category::of(&[]), Category::Other);
    }

    /// The folders are in menu order, Other last, and each has a name and
    /// a picture.
    #[test]
    fn every_folder_has_a_place_a_name_and_a_picture() {
        let mut sorted = Category::ALL;
        sorted.sort();
        assert_eq!(sorted, Category::ALL);
        assert_eq!(Category::ALL.last(), Some(&Category::Other));
        for category in Category::ALL {
            assert!(!category.label().is_empty());
            assert!(!category.icon_name().is_empty());
        }
    }

    /// **What the entry keeps out of menus stays out**, and what it keeps
    /// for another desktop stays there.
    #[test]
    fn what_the_entry_keeps_out_of_menus_stays_out() {
        let here = [DESKTOP_NAME];
        assert!(shows_in_menu(&app(""), &here));
        assert!(!shows_in_menu(&app("NoDisplay=true"), &here));
        assert!(!shows_in_menu(&app("Hidden=true"), &here));
        assert!(!shows_in_menu(&app("OnlyShowIn=GNOME;KDE;"), &here));
        assert!(shows_in_menu(&app("OnlyShowIn=GNOME;SlateOS;"), &here));
        assert!(!shows_in_menu(&app("NotShowIn=SlateOS;"), &here));
        assert!(shows_in_menu(&app("NotShowIn=GNOME;"), &here));
        let link = App::from_entry(
            &DesktopEntry::parse(b"[Desktop Entry]\nType=Link\nName=x\nURL=https://x/\n")
                .expect("parses"),
            "x.desktop",
            None,
        )
        .expect("valid");
        assert!(!shows_in_menu(&link, &here), "a link is not a program");
    }
}
