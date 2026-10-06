//! `theme-preview` -- draw a picture of a theme.
//!
//! ```text
//! theme-preview [--light | --dark] [--size WIDTHxHEIGHT] THEME PICTURE.png
//! ```
//!
//! Draws the standard scene ([`themepreview::Scene`]) in the installed theme
//! `THEME` -- worn on every axis it covers, over the default settings so two
//! pictures differ by their themes alone -- and writes it as a PNG. For a
//! theme's author before they share it, for a theme repository's CI, and for
//! a theme browser to show a theme that came with no screenshot.
//!
//! Exits 0 when the picture was written, 1 when it could not be, and 2 for a
//! command line this does not understand.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use appearance::themes::{self, ThemeDirs};
use appearance::{AppearanceSettings, ThemeMode};
use themepreview::DEFAULT_SIZE;

const USAGE: &str = "\
usage: theme-preview [--light | --dark] [--size WIDTHxHEIGHT] THEME PICTURE.png

Draw the desktop in the installed theme THEME -- its wallpaper and icons, the
taskbar with the start menu open, a window of controls in the theme's frame,
a notification -- and write the picture to PICTURE.png.

  --light, --dark        the mode to draw in (dark unless asked; a theme with
                         one mode's colours is drawn in that one)
  --size WIDTHxHEIGHT    the picture's size: 1280x800 unless asked, from
                         1024x640 to 3840x2160
  -h, --help             show this and stop

Exit status: 0 when the picture was written, 1 when it could not be, 2 for a
command line this does not understand.
";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    light: bool,
    size: (u32, u32),
    theme: OsString,
    picture: PathBuf,
}

/// The command line after the program's name: what it asks for, `None` for
/// a request for the usage, or why it cannot be understood.
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut light: Option<bool> = None;
    let mut size: Option<(u32, u32)> = None;
    let mut words: Vec<OsString> = Vec::new();
    let mut only_words = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if only_words {
            words.push(arg);
        } else if arg == "--" {
            only_words = true;
        } else if arg == "-h" || arg == "--help" {
            return Ok(None);
        } else if arg == "--light" || arg == "--dark" {
            if light.replace(arg == "--light").is_some() {
                return Err("the mode is given twice".to_owned());
            }
        } else if arg == "--size" {
            let Some(value) = args.next() else {
                return Err("`--size` needs WIDTHxHEIGHT".to_owned());
            };
            if size.replace(size_written(&value)?).is_some() {
                return Err("`--size` is given twice".to_owned());
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
    match (words.next(), words.next(), words.next()) {
        (Some(theme), Some(picture), None) => Ok(Some(Options {
            light: light.unwrap_or(false),
            size: size.unwrap_or(DEFAULT_SIZE),
            theme,
            picture: PathBuf::from(picture),
        })),
        (_, _, Some(_)) => Err("one theme and one picture, no more".to_owned()),
        _ => Err("a theme and the picture to write are wanted".to_owned()),
    }
}

/// `WIDTHxHEIGHT`, as two numbers.
fn size_written(arg: &std::ffi::OsStr) -> Result<(u32, u32), String> {
    let text = arg.to_str().unwrap_or_default();
    let numbers = text
        .split_once(['x', 'X'])
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));
    numbers.ok_or_else(|| {
        format!(
            "`{}` is not a size: write it WIDTHxHEIGHT, as 1280x800",
            pathcodec::display_os(arg)
        )
    })
}

fn main() -> ExitCode {
    let options = match parse(std::env::args_os().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprint!("theme-preview: {why}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match draw(&options, &ThemeDirs::standard()) {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("theme-preview: {why}");
            ExitCode::from(1)
        }
    }
}

/// Draw what `options` ask for, with the themes in `dirs`, answering what was
/// done -- or why it could not be.
fn draw(options: &Options, dirs: &ThemeDirs) -> Result<String, String> {
    let shown = pathcodec::display_os(&options.theme);
    let Some(info) = themes::available_in(dirs)
        .into_iter()
        .find(|info| info.id == options.theme)
    else {
        return Err(format!("no theme called {shown} is installed"));
    };
    if let Some(problem) = &info.problem {
        return Err(format!("{shown} {problem}"));
    }
    let mut settings = AppearanceSettings {
        theme_mode: if options.light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        ..AppearanceSettings::default()
    };
    let worn = settings.wear_theme(&info, dirs);
    let (width, height) = options.size;
    let picture = themepreview::render(&settings, width, height).map_err(|err| err.to_string())?;
    let png = picture.png().map_err(|err| err.to_string())?;
    settingsfile::write_atomic(&options.picture, &png).map_err(|err| {
        format!(
            "{} could not be written ({err})",
            pathcodec::display_path(&options.picture)
        )
    })?;
    Ok(format!(
        "drew {shown}, {} mode, its {} worn, into {} ({width}x{height})",
        if settings.is_light() { "light" } else { "dark" },
        worn.join(", "),
        pathcodec::display_path(&options.picture)
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_options_theme_and_picture_are_read() {
        assert_eq!(
            parse(args(&["nord", "nord.png"])),
            Ok(Some(Options {
                light: false,
                size: DEFAULT_SIZE,
                theme: "nord".into(),
                picture: PathBuf::from("nord.png"),
            }))
        );
        assert_eq!(
            parse(args(&[
                "--light",
                "--size",
                "1600x900",
                "nord",
                "out/n.png"
            ])),
            Ok(Some(Options {
                light: true,
                size: (1600, 900),
                theme: "nord".into(),
                picture: PathBuf::from("out/n.png"),
            }))
        );
        assert_eq!(parse(args(&["--help"])), Ok(None));
        assert_eq!(
            parse(args(&["--", "-odd", "-odd.png"]))
                .unwrap()
                .unwrap()
                .theme,
            "-odd"
        );
    }

    #[test]
    fn what_is_not_understood_is_said() {
        let refused = |list: &[&str]| parse(args(list)).unwrap_err();
        assert!(refused(&["nord"]).contains("are wanted"));
        assert!(refused(&["a", "b", "c"]).contains("no more"));
        assert!(refused(&["--size", "big", "a", "b"]).contains("is not a size"));
        assert!(refused(&["--size"]).contains("needs"));
        assert!(refused(&["--light", "--dark", "a", "b"]).contains("twice"));
        assert!(refused(&["--loud", "a", "b"]).contains("`--loud`"));
    }

    /// A theme that is not installed is said, and nothing is written.
    #[test]
    fn a_theme_that_is_not_there_is_said() {
        let scratch = scratchdir::ScratchDir::new("theme-preview-missing");
        let dirs = ThemeDirs {
            user: Some(scratch.dir().join("user")),
            system: scratch.dir().join("system"),
        };
        let out = scratch.dir().join("out.png");
        let options = Options {
            light: false,
            size: DEFAULT_SIZE,
            theme: "gone".into(),
            picture: out.clone(),
        };
        let why = draw(&options, &dirs).unwrap_err();
        assert!(why.contains("no theme called gone"), "{why}");
        assert!(!out.exists());
    }
}
