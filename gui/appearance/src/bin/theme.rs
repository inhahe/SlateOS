//! `theme` -- make, change, install, share and remove themes from a
//! terminal.
//!
//! ```text
//! theme list
//! theme derive THEME NAME [--as FOLDER]
//! theme compose NAME [--as FOLDER]
//! theme install PATH [--as FOLDER]
//! theme export THEME FOLDER
//! theme remove THEME
//! theme get THEME [SECTION]
//! theme set THEME SECTION ROLE COLOUR
//! theme unset THEME SECTION ROLE
//! theme describe THEME FIELD [VALUE]
//! theme tags THEME [TAG...]
//! ```
//!
//! Each command is one call of [`appearance::themes::authoring`] -- the
//! model a theme editor in Settings works through -- so the terminal and the
//! editor cannot do a thing two ways. A theme is named by its folder, as
//! `appearance.yaml` chooses it; every byte of a name or a path is taken as
//! given (`args_os`), since a name may hold any.
//!
//! Exits 0 when the command was done, 1 when it could not be, and 2 for a
//! command line this does not understand.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use appearance::AppearanceSettings;
use appearance::themecheck::{Finding, Report, Severity};
use appearance::themes::authoring::{self, AuthoringError, ColorSection, MetaText, ThemeDraft};
use appearance::themes::{self, Origin, ThemeDirs, ThemeFile, ThemeInfo};
use guitk::color::Color;

const USAGE: &str = "\
usage: theme COMMAND [ARGUMENT...]

Make, change, install, share and remove themes. Your own are kept in
~/.local/share/slateos/themes; one installed with the system, or the
built-in one (aero), is changed by deriving a copy of it.

  theme list
      every installed theme: its folder, its name, whose it is, what it covers
  theme derive THEME NAME [--as FOLDER]
      a copy of THEME called NAME, yours to change; its folder is named
      for NAME unless --as names it
  theme compose NAME [--as FOLDER]
      one theme called NAME made of your look as it is: each part taken
      from the theme you chose for it -- to keep, or to export and share
  theme install PATH [--as FOLDER]
      install the theme folder, or the lone theme file, at PATH -- once a
      copy of it passes the checks themecheck makes
  theme export THEME FOLDER
      copy THEME to the new folder FOLDER, to share, and check the copy
  theme remove THEME
      remove one of your own themes
  theme get THEME [SECTION]
      the colours THEME sets, as the desktop reads them
  theme set THEME SECTION ROLE COLOUR
      set a colour of one of your themes; COLOUR is #rrggbb (the # may be
      left off, which a shell that reads # as a comment needs)
  theme unset THEME SECTION ROLE
      leave a colour of one of your themes to the built-in theme
  theme describe THEME FIELD [VALUE]
      set what describes one of your themes -- FIELD is name, author,
      version or license -- or with no VALUE, remove it
  theme tags THEME [TAG...]
      set the words one of your themes is searched by; none removes them

SECTION is colors, colors-light, terminal, terminal-light, syntax or
syntax-light.

  -h, --help   show this and stop
  --           what follows is an argument, even if it begins with -

Exit status: 0 when it was done, 1 when it could not be, 2 for a command
line this does not understand.
";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    List,
    Derive {
        from: OsString,
        name: String,
        folder: Option<OsString>,
    },
    Compose {
        name: String,
        folder: Option<OsString>,
    },
    Install {
        from: PathBuf,
        folder: Option<OsString>,
    },
    Export {
        theme: OsString,
        to: PathBuf,
    },
    Remove {
        theme: OsString,
    },
    Get {
        theme: OsString,
        section: Option<ColorSection>,
    },
    Set {
        theme: OsString,
        section: ColorSection,
        role: String,
        color: Color,
    },
    Unset {
        theme: OsString,
        section: ColorSection,
        role: String,
    },
    Describe {
        theme: OsString,
        field: MetaText,
        value: String,
    },
    Tags {
        theme: OsString,
        tags: Vec<String>,
    },
}

/// The command line after the program's name: what it asks for, `None` for
/// a request for the usage, or why it cannot be understood.
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Command>, String> {
    let mut words: Vec<OsString> = Vec::new();
    let mut folder: Option<OsString> = None;
    let mut only_words = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if only_words {
            words.push(arg);
        } else if arg == "--" {
            only_words = true;
        } else if arg == "--help" || arg == "-h" {
            return Ok(None);
        } else if arg == "--as" {
            let Some(name) = args.next() else {
                return Err("`--as` needs the folder to name the theme".to_owned());
            };
            if folder.replace(name).is_some() {
                return Err("`--as` is given twice".to_owned());
            }
        } else if arg.len() > 1 && arg.as_encoded_bytes().first() == Some(&b'-') {
            return Err(format!(
                "`{}` is not an option this understands",
                pathcodec::display_os(&arg)
            ));
        } else {
            words.push(arg);
        }
    }
    let mut words = words.into_iter();
    let Some(verb) = words.next() else {
        return Err("no command was given".to_owned());
    };
    let rest: Vec<OsString> = words.collect();
    let verb = verb.to_str().unwrap_or_default().to_owned();
    if folder.is_some() && !matches!(verb.as_str(), "derive" | "compose" | "install") {
        return Err(format!(
            "`--as` names the folder for derive, compose and install, not {verb}"
        ));
    }
    let command = match (verb.as_str(), rest.as_slice()) {
        ("list", []) => Command::List,
        ("derive", [from, name]) => Command::Derive {
            from: from.clone(),
            name: text(name, "NAME")?,
            folder,
        },
        ("compose", [name]) => Command::Compose {
            name: text(name, "NAME")?,
            folder,
        },
        ("install", [from]) => Command::Install {
            from: PathBuf::from(from),
            folder,
        },
        ("export", [theme, to]) => Command::Export {
            theme: theme.clone(),
            to: PathBuf::from(to),
        },
        ("remove", [theme]) => Command::Remove {
            theme: theme.clone(),
        },
        ("get", [theme]) => Command::Get {
            theme: theme.clone(),
            section: None,
        },
        ("get", [theme, section]) => Command::Get {
            theme: theme.clone(),
            section: Some(section_named(section)?),
        },
        ("set", [theme, section, role, color]) => Command::Set {
            theme: theme.clone(),
            section: section_named(section)?,
            role: text(role, "ROLE")?,
            color: color_written(color)?,
        },
        ("unset", [theme, section, role]) => Command::Unset {
            theme: theme.clone(),
            section: section_named(section)?,
            role: text(role, "ROLE")?,
        },
        ("describe", [theme, field]) => Command::Describe {
            theme: theme.clone(),
            field: field_named(field)?,
            value: String::new(),
        },
        ("describe", [theme, field, value]) => Command::Describe {
            theme: theme.clone(),
            field: field_named(field)?,
            value: text(value, "VALUE")?,
        },
        ("tags", [theme, tags @ ..]) => Command::Tags {
            theme: theme.clone(),
            tags: tags
                .iter()
                .map(|tag| text(tag, "TAG"))
                .collect::<Result<_, _>>()?,
        },
        (
            "list" | "derive" | "compose" | "install" | "export" | "remove" | "get" | "set"
            | "unset" | "describe" | "tags",
            _,
        ) => {
            return Err(format!(
                "`{verb}` takes other arguments than these -- see its line below"
            ));
        }
        _ => {
            return Err(format!(
                "`{}` is not a command this has",
                pathcodec::display_os(OsStr::new(&verb))
            ));
        }
    };
    Ok(Some(command))
}

/// An argument that must be text: a name, a role, a value.
fn text(arg: &OsStr, what: &str) -> Result<String, String> {
    arg.to_str().map(str::to_owned).ok_or_else(|| {
        format!(
            "{what} must be text, and `{}` is not",
            pathcodec::display_os(arg)
        )
    })
}

fn section_named(arg: &OsStr) -> Result<ColorSection, String> {
    arg.to_str()
        .and_then(ColorSection::from_key)
        .ok_or_else(|| {
            let keys: Vec<&str> = ColorSection::ALL
                .iter()
                .map(|section| section.key())
                .collect();
            format!(
                "`{}` is not a section: it is one of {}",
                pathcodec::display_os(arg),
                keys.join(", ")
            )
        })
}

fn field_named(arg: &OsStr) -> Result<MetaText, String> {
    arg.to_str().and_then(MetaText::from_key).ok_or_else(|| {
        let keys: Vec<&str> = MetaText::ALL.iter().map(|field| field.key()).collect();
        format!(
            "`{}` is not something that describes a theme: it is one of {}",
            pathcodec::display_os(arg),
            keys.join(", ")
        )
    })
}

/// A colour as typed: `#rrggbb`, or the same without its `#`.
fn color_written(arg: &OsStr) -> Result<Color, String> {
    let typed = arg.to_str().unwrap_or_default();
    let hex = if typed.starts_with('#') {
        typed.to_owned()
    } else {
        format!("#{typed}")
    };
    Color::from_hex_text(&hex).ok_or_else(|| {
        format!(
            "`{}` is not a colour: write it #rrggbb",
            pathcodec::display_os(arg)
        )
    })
}

fn main() -> ExitCode {
    let command = match parse(std::env::args_os().skip(1)) {
        Ok(Some(command)) => command,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprint!("theme: {why}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let dirs = ThemeDirs::standard();
    // The user's settings, read only for the one command that asks what
    // they choose.
    let settings = || appearance::AppearanceFile::load().settings;
    let mut out = io::stdout().lock();
    match run(&command, &dirs, settings, &mut out) {
        Ok(Done::Yes) => ExitCode::SUCCESS,
        Ok(Done::No(why)) => {
            eprintln!("theme: {why}");
            ExitCode::from(1)
        }
        Err(err) => {
            // Nowhere left to say anything -- a closed pipe -- so say on the
            // other stream why the rest was not said.
            eprintln!("theme: {err}");
            ExitCode::from(1)
        }
    }
}

/// Whether a command was done, and why not when it was not.
#[derive(Debug, PartialEq, Eq)]
enum Done {
    Yes,
    No(String),
}

impl From<AuthoringError> for Done {
    fn from(err: AuthoringError) -> Self {
        Self::No(err.to_string())
    }
}

/// Do `command` on the themes in `dirs`, saying what was done on `out` --
/// `settings` giving the user's appearance settings, for the command that
/// asks. The `Err` is only for `out` itself failing.
fn run(
    command: &Command,
    dirs: &ThemeDirs,
    settings: impl FnOnce() -> AppearanceSettings,
    out: &mut impl Write,
) -> io::Result<Done> {
    match command {
        Command::List => {
            for info in themes::available_in(dirs) {
                writeln!(out, "{}", listed(&info))?;
            }
            Ok(Done::Yes)
        }
        Command::Derive { from, name, folder } => {
            let id = folder
                .clone()
                .unwrap_or_else(|| authoring::id_for(dirs, name));
            match authoring::derive(dirs, from, &id, name) {
                Ok((draft, left_out)) => {
                    writeln!(
                        out,
                        "made {} from {}, in {}",
                        shown(draft.id()),
                        shown(from),
                        pathcodec::display_path(draft.dir())
                    )?;
                    show_findings(out, &left_out)?;
                    Ok(Done::Yes)
                }
                Err(err) => Ok(err.into()),
            }
        }
        Command::Compose { name, folder } => {
            let id = folder
                .clone()
                .unwrap_or_else(|| authoring::id_for(dirs, name));
            match authoring::compose(dirs, &settings(), &id, name) {
                Ok((draft, said)) => {
                    writeln!(
                        out,
                        "made {} of your look, in {}",
                        shown(draft.id()),
                        pathcodec::display_path(draft.dir())
                    )?;
                    show_findings(out, &said)?;
                    Ok(Done::Yes)
                }
                Err(err) => Ok(err.into()),
            }
        }
        Command::Install { from, folder } => {
            let id = match folder {
                Some(folder) => folder.clone(),
                None => match install_name(from) {
                    Some(name) => authoring::id_for(dirs, &name),
                    None => return Ok(Done::No("PATH names no file or folder".to_owned())),
                },
            };
            match authoring::install(dirs, from, &id) {
                Ok(report) => {
                    writeln!(out, "installed {}", shown(&id))?;
                    show_findings(out, &report.findings)?;
                    Ok(Done::Yes)
                }
                Err(AuthoringError::Refused(report)) => {
                    show_findings(out, &report.findings)?;
                    Ok(Done::No(format!(
                        "{} was not installed: {}",
                        shown(&id),
                        AuthoringError::Refused(report)
                    )))
                }
                Err(err) => Ok(err.into()),
            }
        }
        Command::Export { theme, to } => match authoring::export(dirs, theme, to) {
            Ok(report) => {
                writeln!(
                    out,
                    "copied {} to {}",
                    shown(theme),
                    pathcodec::display_path(to)
                )?;
                show_findings(out, &report.findings)?;
                writeln!(out, "= {}", summary(&report))?;
                Ok(Done::Yes)
            }
            Err(err) => Ok(err.into()),
        },
        Command::Remove { theme } => match authoring::remove(dirs, theme) {
            Ok(()) => {
                writeln!(out, "removed {}", shown(theme))?;
                Ok(Done::Yes)
            }
            Err(err) => Ok(err.into()),
        },
        Command::Get { theme, section } => match authoring::read_theme(dirs, theme) {
            Ok(file) => {
                show_colors(out, &file, *section)?;
                Ok(Done::Yes)
            }
            Err(err) => Ok(err.into()),
        },
        Command::Set {
            theme,
            section,
            role,
            color,
        } => edit(dirs, theme, out, |draft| {
            draft.set_color(*section, role, *color)?;
            Ok(format!("{}.{role} is {}", section.key(), color.hex_text()))
        }),
        Command::Unset {
            theme,
            section,
            role,
        } => edit(dirs, theme, out, |draft| {
            Ok(if draft.clear_color(*section, role) {
                format!("{}.{role} is left to the built-in theme", section.key())
            } else {
                format!("{}.{role} was not set", section.key())
            })
        }),
        Command::Describe {
            theme,
            field,
            value,
        } => edit(dirs, theme, out, |draft| {
            draft.set_meta(*field, value);
            Ok(if value.trim().is_empty() {
                format!("{} removed", field.key())
            } else {
                format!("{} is {}", field.key(), value.trim())
            })
        }),
        Command::Tags { theme, tags } => edit(dirs, theme, out, |draft| {
            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            draft.set_tags(&tags);
            Ok(format!("tags are [{}]", draft.file().meta.tags.join(", ")))
        }),
    }
}

/// Open the user's theme `theme`, change it with `change` -- which says what
/// it did -- and save it if that changed anything, then say what the desktop
/// now ignores in it.
fn edit(
    dirs: &ThemeDirs,
    theme: &OsStr,
    out: &mut impl Write,
    change: impl FnOnce(&mut ThemeDraft) -> Result<String, AuthoringError>,
) -> io::Result<Done> {
    let mut draft = match ThemeDraft::open(dirs, theme) {
        Ok(draft) => draft,
        Err(err) => return Ok(err.into()),
    };
    let did = match change(&mut draft) {
        Ok(did) => did,
        Err(err) => return Ok(err.into()),
    };
    if draft.is_changed() {
        if let Err(err) = draft.save() {
            return Ok(err.into());
        }
        writeln!(out, "{}: {did}", shown(theme))?;
    } else {
        writeln!(out, "{}: {did}; nothing changed", shown(theme))?;
    }
    for warning in draft.file().warnings {
        writeln!(out, "  ignored: {warning}")?;
    }
    Ok(Done::Yes)
}

/// What a theme installed from `from` is called when `--as` does not say: a
/// folder's name; a lone file's without its extension -- or, for a file
/// called `theme.yaml`, the name of the folder it is in, as a theme's own
/// file is called. Shown as text: a name that is not is made one, which
/// [`authoring::id_for`] then makes a folder's name of.
fn install_name(from: &Path) -> Option<String> {
    let name = if from.is_dir() {
        from.file_name()?
    } else if from.file_name() == Some(OsStr::new(themes::FILE_NAME)) {
        from.parent()?.file_name()?
    } else {
        from.file_stem()?
    };
    Some(pathcodec::display_os(name))
}

/// A theme's name as printed.
fn shown(id: &OsStr) -> String {
    pathcodec::display_os(id)
}

/// One theme as `list` prints it: its folder's name, its name, whose it is,
/// what it covers -- or why it cannot be used.
fn listed(info: &ThemeInfo) -> String {
    let whose = match info.origin {
        Origin::BuiltIn => "built in",
        Origin::System => "the system's",
        Origin::User => "yours",
    };
    let mut covers: Vec<String> = Vec::new();
    if info.provides_colors() {
        let modes = match (info.has_dark, info.has_light) {
            (true, true) => "dark and light",
            (true, false) => "dark",
            _ => "light",
        };
        covers.push(format!("colours ({modes})"));
    }
    for (provides, axis) in [
        (info.provides_icons(), "icons"),
        (info.provides_widget_style(), "controls"),
        (info.provides_animation(), "animation"),
        (info.provides_decorations(), "window frames"),
        (info.provides_panel(), "taskbar"),
        (info.provides_wallpapers(), "wallpapers"),
        (info.provides_fonts(), "fonts"),
    ] {
        if provides {
            covers.push(axis.to_owned());
        }
    }
    let what = match &info.problem {
        Some(problem) => format!("cannot be used: it {problem}"),
        None if covers.is_empty() => "covers nothing".to_owned(),
        None => format!("covers {}", covers.join(", ")),
    };
    format!("{}  \"{}\", {whose}, {what}", shown(&info.id), info.name)
}

/// The colours `file` sets -- every section, or `only` -- a line each in the
/// palette's order, then what in the file the desktop ignores.
fn show_colors(
    out: &mut impl Write,
    file: &ThemeFile,
    only: Option<ColorSection>,
) -> io::Result<()> {
    let sections: Vec<ColorSection> = match only {
        Some(section) => vec![section],
        None => ColorSection::ALL.to_vec(),
    };
    for section in sections {
        let set = section.of(&file.colors);
        if set.is_empty() && only.is_none() {
            continue;
        }
        writeln!(out, "{}:", section.key())?;
        if set.is_empty() {
            writeln!(out, "  (none: every colour is the built-in theme's)")?;
        }
        for role in section.roles() {
            if let Some(color) = set.get(role) {
                writeln!(out, "  {role} {}", color.hex_text())?;
            }
        }
    }
    for warning in &file.warnings {
        writeln!(out, "ignored: {warning}")?;
    }
    Ok(())
}

/// Findings a line each, as `themecheck` prints them.
fn show_findings(out: &mut impl Write, findings: &[Finding]) -> io::Result<()> {
    for finding in findings {
        writeln!(out, "  {finding}")?;
    }
    Ok(())
}

/// A report's counts, as `themecheck` sums one up.
fn summary(report: &Report) -> String {
    let counted = |n: usize, what: &str| {
        if n == 1 {
            format!("1 {what}")
        } else {
            format!("{n} {what}s")
        }
    };
    format!(
        "{}, {}, {} -- covers {}",
        counted(report.count(Severity::Error), "error"),
        counted(report.count(Severity::Warning), "warning"),
        counted(report.count(Severity::Note), "note"),
        if report.covers.is_empty() {
            "nothing".to_owned()
        } else {
            report.covers.join(", ")
        }
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;
    use std::fs;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn parsed(list: &[&str]) -> Command {
        parse(args(list)).unwrap().unwrap()
    }

    fn refused(list: &[&str]) -> String {
        parse(args(list)).unwrap_err()
    }

    #[test]
    fn each_command_is_read_with_its_arguments() {
        assert_eq!(parsed(&["list"]), Command::List);
        assert_eq!(
            parsed(&["derive", "nord", "Nord, warmer"]),
            Command::Derive {
                from: "nord".into(),
                name: "Nord, warmer".to_owned(),
                folder: None,
            }
        );
        assert_eq!(
            parsed(&["derive", "--as", "warm", "nord", "Nord, warmer"]),
            Command::Derive {
                from: "nord".into(),
                name: "Nord, warmer".to_owned(),
                folder: Some("warm".into()),
            }
        );
        assert_eq!(
            parsed(&["install", "a/ocean", "--as", "sea"]),
            Command::Install {
                from: PathBuf::from("a/ocean"),
                folder: Some("sea".into()),
            }
        );
        assert_eq!(
            parsed(&["export", "nord", "out/nord"]),
            Command::Export {
                theme: "nord".into(),
                to: PathBuf::from("out/nord"),
            }
        );
        assert_eq!(
            parsed(&["remove", "mine"]),
            Command::Remove {
                theme: "mine".into()
            }
        );
        assert_eq!(
            parsed(&["get", "mine"]),
            Command::Get {
                theme: "mine".into(),
                section: None,
            }
        );
        assert_eq!(
            parsed(&["get", "mine", "terminal-light"]),
            Command::Get {
                theme: "mine".into(),
                section: Some(ColorSection::TerminalLight),
            }
        );
        assert_eq!(
            parsed(&["set", "mine", "colors", "base", "#102030"]),
            Command::Set {
                theme: "mine".into(),
                section: ColorSection::Dark,
                role: "base".to_owned(),
                color: Color::from_hex(0x10_2030),
            }
        );
        assert_eq!(
            parsed(&["unset", "mine", "syntax", "keyword"]),
            Command::Unset {
                theme: "mine".into(),
                section: ColorSection::SyntaxDark,
                role: "keyword".to_owned(),
            }
        );
        assert_eq!(
            parsed(&["describe", "mine", "author", "Me"]),
            Command::Describe {
                theme: "mine".into(),
                field: MetaText::Author,
                value: "Me".to_owned(),
            }
        );
        assert_eq!(
            parsed(&["describe", "mine", "license"]),
            Command::Describe {
                theme: "mine".into(),
                field: MetaText::License,
                value: String::new(),
            }
        );
        assert_eq!(
            parsed(&["tags", "mine", "warm", "evening"]),
            Command::Tags {
                theme: "mine".into(),
                tags: vec!["warm".to_owned(), "evening".to_owned()],
            }
        );
        assert_eq!(
            parsed(&["tags", "mine"]),
            Command::Tags {
                theme: "mine".into(),
                tags: Vec::new(),
            }
        );
    }

    /// A colour may be typed without its `#`, which a shell reading `#` as a
    /// comment needs.
    #[test]
    fn a_colour_is_read_with_or_without_its_hash() {
        let Command::Set { color, .. } = parsed(&["set", "m", "colors", "base", "2e3440"]) else {
            panic!("not a set");
        };
        assert_eq!(color, Color::from_hex(0x2E_3440));
        let bad = refused(&["set", "m", "colors", "base", "#2e34"]);
        assert!(bad.contains("is not a colour"), "{bad}");
        let bad = refused(&["set", "m", "colors", "base", "+9b4fa"]);
        assert!(bad.contains("is not a colour"), "{bad}");
    }

    /// After `--`, an argument may begin with a dash.
    #[test]
    fn after_two_dashes_everything_is_an_argument() {
        assert_eq!(
            parsed(&["--", "remove", "-dashed"]),
            Command::Remove {
                theme: "-dashed".into()
            }
        );
        assert_eq!(
            parsed(&["remove", "-"]),
            Command::Remove { theme: "-".into() }
        );
    }

    #[test]
    fn help_is_asked_for_by_either_spelling() {
        assert_eq!(parse(args(&["--help"])), Ok(None));
        assert_eq!(parse(args(&["set", "-h"])), Ok(None));
    }

    #[test]
    fn what_is_not_understood_is_said() {
        assert!(refused(&[]).contains("no command"));
        assert!(refused(&["paint"]).contains("`paint` is not a command"));
        assert!(refused(&["list", "extra"]).contains("takes other arguments"));
        assert!(refused(&["set", "m", "colors", "base"]).contains("takes other arguments"));
        assert!(refused(&["--loud", "list"]).contains("`--loud`"));
        let section = refused(&["get", "m", "colours"]);
        assert!(section.contains("`colours` is not a section"), "{section}");
        assert!(section.contains("colors-light"), "{section}");
        let field = refused(&["describe", "m", "tags", "x"]);
        assert!(field.contains("`tags` is not something"), "{field}");
        assert!(refused(&["--as"]).contains("needs the folder"));
        assert!(refused(&["derive", "--as", "a", "--as", "b", "n", "N"]).contains("twice"));
        assert!(refused(&["remove", "--as", "a", "n"]).contains("not remove"));
    }

    /// Text arguments must be text; a theme's name and a path need not be.
    #[cfg(unix)]
    #[test]
    fn a_name_need_not_be_text_but_a_colours_role_must() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(vec![b'n', 0xFF]);
        let command = parse(vec!["remove".into(), odd.clone()]).unwrap().unwrap();
        assert_eq!(command, Command::Remove { theme: odd.clone() });
        let role = parse(vec!["unset".into(), "m".into(), "colors".into(), odd]).unwrap_err();
        assert!(role.contains("ROLE must be text"), "{role}");
    }

    /// Theme directories in a scratch directory.
    fn scratch(tag: &str) -> (ScratchDir, ThemeDirs) {
        let scratch = ScratchDir::new(&format!("slateos-theme-cli-{tag}"));
        let dirs = ThemeDirs {
            user: Some(scratch.dir().join("user")),
            system: scratch.dir().join("system"),
        };
        (scratch, dirs)
    }

    /// Run `list` as the program would, and what it printed.
    fn ran(list: &[&str], dirs: &ThemeDirs) -> (Done, String) {
        let command = parsed(list);
        let mut out = Vec::new();
        let done = run(&command, dirs, AppearanceSettings::default, &mut out).unwrap();
        (done, String::from_utf8(out).unwrap())
    }

    const NORD: &str = "\
meta:
  name: Nord
colors:
  base: \"#2e3440\"
  text: \"#eceff4\"
colors-light:
  base: \"#eceff4\"
";

    /// A theme made, changed line by line, read back and removed, as a
    /// person at a terminal would.
    #[test]
    fn a_theme_is_made_changed_read_and_removed() {
        let (_scratch, dirs) = scratch("round");
        let system = dirs.system.join("nord");
        fs::create_dir_all(&system).unwrap();
        fs::write(system.join(themes::FILE_NAME), NORD).unwrap();

        let (done, said) = ran(&["derive", "nord", "Nord, warmer"], &dirs);
        assert_eq!(done, Done::Yes);
        assert!(
            said.starts_with("made Nord, warmer from nord, in "),
            "{said}"
        );

        let id = "Nord, warmer";
        let (done, said) = ran(&["set", id, "colors", "base", "3b4252"], &dirs);
        assert_eq!(done, Done::Yes);
        assert_eq!(said, "Nord, warmer: colors.base is #3b4252\n");
        let (_, said) = ran(&["set", id, "colors", "base", "3b4252"], &dirs);
        assert_eq!(
            said,
            "Nord, warmer: colors.base is #3b4252; nothing changed\n"
        );
        let (done, said) = ran(&["unset", id, "colors-light", "base"], &dirs);
        assert_eq!(done, Done::Yes);
        assert_eq!(
            said,
            "Nord, warmer: colors-light.base is left to the built-in theme\n"
        );
        let (_, said) = ran(&["unset", id, "colors-light", "base"], &dirs);
        assert_eq!(
            said,
            "Nord, warmer: colors-light.base was not set; nothing changed\n"
        );
        let (_, said) = ran(&["describe", id, "author", " Me "], &dirs);
        assert_eq!(said, "Nord, warmer: author is Me\n");
        let (_, said) = ran(&["tags", id, "warm", "evening"], &dirs);
        assert_eq!(said, "Nord, warmer: tags are [warm, evening]\n");

        let (done, said) = ran(&["get", id], &dirs);
        assert_eq!(done, Done::Yes);
        assert_eq!(said, "colors:\n  base #3b4252\n  text #eceff4\n");
        let (_, said) = ran(&["get", id, "syntax"], &dirs);
        assert_eq!(
            said,
            "syntax:\n  (none: every colour is the built-in theme's)\n"
        );

        let (_, said) = ran(&["list"], &dirs);
        assert!(
            said.contains("aero  \"Aero\", built in, covers colours (dark and light)"),
            "{said}"
        );
        assert!(
            said.contains("Nord, warmer  \"Nord, warmer\", yours, covers colours (dark)"),
            "{said}"
        );
        assert!(
            said.contains("nord  \"Nord\", the system's, covers colours (dark and light)"),
            "{said}"
        );

        let (done, said) = ran(&["remove", id], &dirs);
        assert_eq!(done, Done::Yes);
        assert_eq!(said, "removed Nord, warmer\n");
        assert!(!dirs.user.as_ref().unwrap().join(id).exists());
    }

    #[test]
    fn what_cannot_be_done_is_said_and_fails() {
        let (_scratch, dirs) = scratch("refused");
        let system = dirs.system.join("nord");
        fs::create_dir_all(&system).unwrap();
        fs::write(system.join(themes::FILE_NAME), NORD).unwrap();

        let (done, said) = ran(&["set", "nord", "colors", "base", "000000"], &dirs);
        assert_eq!(done, AuthoringError::NotTheUsers.into());
        assert_eq!(said, "");
        let (done, _) = ran(&["remove", "missing"], &dirs);
        assert_eq!(done, AuthoringError::NotInstalled.into());
        let (done, _) = ran(&["get", "missing"], &dirs);
        assert_eq!(done, AuthoringError::NotInstalled.into());
        let (done, _) = ran(&["derive", "--as", "aero", "nord", "X"], &dirs);
        assert_eq!(done, AuthoringError::InvalidName.into());
        ran(&["derive", "nord", "Mine"], &dirs);
        let (done, said) = ran(&["set", "Mine", "colors", "accent", "ff0000"], &dirs);
        assert_eq!(
            done,
            AuthoringError::NoSuchRole(ColorSection::Dark, "accent".to_owned()).into()
        );
        assert_eq!(said, "");
    }

    #[test]
    fn a_theme_is_installed_and_shared_with_what_its_check_found() {
        let (scratch, dirs) = scratch("install");
        let from = scratch.dir().join("downloads").join("ocean");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join(themes::FILE_NAME), NORD).unwrap();

        let from_text = from.to_str().unwrap();
        let (done, said) = ran(&["install", from_text], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("installed ocean\n"), "{said}");
        assert!(dirs.user.as_ref().unwrap().join("ocean").is_dir());
        // Again: a free name is found for the second.
        let (done, said) = ran(&["install", from_text], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("installed ocean-2\n"), "{said}");

        // A theme's own file is named for its folder.
        let lone = from.join(themes::FILE_NAME);
        let (done, said) = ran(&["install", lone.to_str().unwrap()], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("installed ocean-3\n"), "{said}");

        // One that fails its check is not installed, and the findings say why.
        let bad = scratch.dir().join("downloads").join("bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join(themes::FILE_NAME), NORD).unwrap();
        fs::write(bad.join("run.sh"), "#!/bin/sh\n").unwrap();
        let (done, said) = ran(&["install", bad.to_str().unwrap()], &dirs);
        let Done::No(why) = done else {
            panic!("a theme holding a script was installed");
        };
        assert!(
            why.starts_with("bad was not installed: the theme did not pass"),
            "{why}"
        );
        assert!(said.contains("error: run.sh:"), "{said}");
        assert!(!dirs.user.as_ref().unwrap().join("bad").exists());

        let to = scratch.dir().join("shared");
        let (done, said) = ran(&["export", "ocean", to.to_str().unwrap()], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("copied ocean to "), "{said}");
        assert!(said.contains("\n= 0 errors, "), "{said}");
        assert_eq!(
            fs::read_to_string(to.join(themes::FILE_NAME)).unwrap(),
            NORD
        );
    }

    /// `compose` makes one theme of the look the settings choose, named as
    /// asked, its folder named for the name unless `--as` says.
    #[test]
    fn a_theme_is_composed_of_the_look_chosen() {
        assert_eq!(
            parsed(&["compose", "My look"]),
            Command::Compose {
                name: "My look".to_owned(),
                folder: None,
            }
        );
        assert_eq!(
            parsed(&["compose", "My look", "--as", "look"]),
            Command::Compose {
                name: "My look".to_owned(),
                folder: Some("look".into()),
            }
        );
        assert!(refused(&["compose"]).contains("takes other arguments"));

        let (_scratch, dirs) = scratch("compose");
        let (done, said) = ran(&["compose", "My look"], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("made My look of your look, in "), "{said}");
        let file = dirs
            .user
            .as_ref()
            .unwrap()
            .join("My look")
            .join(themes::FILE_NAME);
        let text = fs::read_to_string(file).unwrap();
        assert!(text.contains("# The colours: Aero's.\n"), "{text}");
        // Again under the name: a free folder is found.
        let (done, said) = ran(&["compose", "My look"], &dirs);
        assert_eq!(done, Done::Yes, "{said}");
        assert!(said.starts_with("made My look-2 of your look"), "{said}");
        let (done, _) = ran(&["compose", "Mine", "--as", "aero"], &dirs);
        assert_eq!(done, AuthoringError::InvalidName.into());
    }

    #[test]
    fn a_theme_installed_from_a_lone_file_is_named_for_it() {
        assert_eq!(
            install_name(Path::new("downloads/nord.yaml")).as_deref(),
            Some("nord")
        );
        assert_eq!(
            install_name(Path::new("downloads/nord/theme.yaml")).as_deref(),
            Some("nord")
        );
        assert_eq!(install_name(Path::new("/")), None);
    }

    #[test]
    fn a_reports_counts_are_summed_up() {
        let finding = |severity| Finding {
            severity,
            place: String::new(),
            message: String::new(),
        };
        let report = Report {
            findings: vec![
                finding(Severity::Warning),
                finding(Severity::Note),
                finding(Severity::Note),
            ],
            covers: vec!["colors", "icons"],
            files: 2,
            bytes: 3,
        };
        assert_eq!(
            summary(&report),
            "0 errors, 1 warning, 2 notes -- covers colors, icons"
        );
        assert_eq!(
            summary(&Report::default()),
            "0 errors, 0 warnings, 0 notes -- covers nothing"
        );
    }
}
